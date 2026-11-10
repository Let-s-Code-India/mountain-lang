//! Closures and function values (Phase 12 M1; Document 10 §4, §5, §9.5).
//!
//! A function value is `{ code, env }` (`fat_ty`): `code` is a function taking the environment
//! pointer first and then the declared parameters; a named function has a null `env` and goes
//! through a small thunk that drops the environment argument.
//!
//! A closure environment is `{ drop_fn, called, captures.. }`:
//!  * `drop_fn` is null for an environment on the creating function's frame (a closure that
//!    only borrows: it owns nothing, copying the value is harmless) and otherwise the function
//!    that drops the moved-in captures and frees the heap block (an owning closure is move-only);
//!  * `called` guards a closure whose body moves a captured value out: a second call panics;
//!  * a `Ref`/`MutRef` capture is a pointer to the variable, a `Move` capture is the value itself.
//! Dropping a function value runs `drop_fn` when there is one; the all-zero value (a moved-out
//! slot) is a no-op, like every other drop glue (see `drops.rs`).

use super::*;
use crate::borrow::CapMode;
use inkwell::types::FunctionType;
use inkwell::values::AsValueRef;

pub(super) struct PendingClosure<'ctx> {
    pub f: FunctionValue<'ctx>,
    pub c: *const ClosureExpr,
    pub env_ty: StructType<'ctx>,
    pub caps: Vec<(String, CapMode, Ty)>,
    pub ptys: Vec<Ty>,
    pub ret: Ty,
    pub once: bool,
    pub cur_self: Option<Ty>,
    pub owner: String,
}

impl<'ctx, 'a> Codegen<'ctx, 'a> {
    /// The LLVM type of a function taking the environment pointer plus `ptys`.
    pub(super) fn fn_llty(&mut self, ptys: &[Ty], ret: &Ty) -> R<FunctionType<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let mut ps: Vec<BasicMetadataTypeEnum> = vec![ptrt.into()];
        for t in ptys {
            ps.push(self.llty(t)?.into());
        }
        Ok(if is_unit_like(ret) { self.context.void_type().fn_type(&ps, false) } else { self.llty(ret)?.fn_type(&ps, false) })
    }

    fn make_fn_value(&mut self, code: PointerValue<'ctx>, env: PointerValue<'ctx>) -> R<V<'ctx>> {
        let ft = self.fat_ty();
        let mut agg = ft.get_undef();
        agg = self.builder.build_insert_value(agg, code, 0, "code")?.into_struct_value();
        agg = self.builder.build_insert_value(agg, env, 1, "env")?.into_struct_value();
        Ok(agg.into())
    }

    // ---- creation -----------------------------------------------------------------

    pub(super) fn gen_closure(&mut self, e: &Expr, c: &ClosureExpr) -> R<V<'ctx>> {
        let ty = self.ty_of(e)?;
        let Ty::Fn(ptys, ret) = &ty else { return Err(CgErr(format!("internal: closure typed `{}`", ty))) };
        let (ptys, ret) = (ptys.clone(), (**ret).clone());
        let info = self
            .closure_infos
            .get(&(e as *const Expr as usize))
            .cloned()
            .ok_or_else(|| CgErr("internal: no capture analysis for a closure".into()))?;
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let mut fields: Vec<BasicTypeEnum<'ctx>> = vec![ptrt.into(), self.int_ty(64).into()];
        let mut caps: Vec<(String, CapMode, Ty, PointerValue<'ctx>)> = Vec::new();
        for (name, mode) in &info.captures {
            let Some((lp, lty)) = self.lookup_local(name) else {
                return Err(CgErr(format!("internal: captured variable `{}` is not a local here", name)));
            };
            fields.push(if *mode == CapMode::Move { self.llty(&lty)? } else { ptrt.into() });
            caps.push((name.clone(), *mode, lty, lp));
        }
        let env_ty = self.context.struct_type(&fields, false);
        self.closure_counter += 1;
        let n = self.closure_counter;
        let fty = self.fn_llty(&ptys, &ret)?;
        let f = self.module.add_function(&format!("__mtn_closure_{}", n), fty, None);
        if caps.is_empty() {
            // nothing captured: no environment at all (like a named function)
            self.pending_closures.push(PendingClosure {
                f,
                c: c as *const ClosureExpr,
                env_ty,
                caps: Vec::new(),
                ptys,
                ret,
                once: false,
                cur_self: self.cur_self.clone(),
                owner: if self.cur_key == "<closure>" { self.closure_owner.clone() } else { self.cur_key.clone() },
            });
            return self.make_fn_value(f.as_global_value().as_pointer_value(), ptrt.const_null());
        }
        let heap = info.owning;
        let drop_fn = if heap { Some(self.closure_drop_fn(n, env_ty, &caps)?) } else { None };
        let env = if heap {
            let ut = self.usize_ty();
            let size = self.td.get_abi_size(&env_ty).max(1);
            let align = (self.td.get_abi_alignment(&env_ty) as u64).max(8);
            let (fa, _) = self.alloc_fns();
            let r = self.builder.build_call(fa, &[ut.const_int(size, false).into(), ut.const_int(align, false).into()], "env")?;
            let ValueKind::Basic(p) = r.try_as_basic_value() else { return Err(CgErr("internal: alloc result".into())) };
            p.into_pointer_value()
        } else {
            self.alloca(env_ty.into(), "env")?
        };
        let hp = self.builder.build_struct_gep(env_ty, env, 0, "drop_fn")?;
        let dv = match drop_fn {
            Some(d) => d.as_global_value().as_pointer_value(),
            None => ptrt.const_null(),
        };
        self.builder.build_store(hp, dv)?;
        let cp = self.builder.build_struct_gep(env_ty, env, 1, "called")?;
        self.builder.build_store(cp, self.int_ty(64).const_zero())?;
        for (i, (name, mode, lty, lp)) in caps.iter().enumerate() {
            let fp = self.builder.build_struct_gep(env_ty, env, 2 + i as u32, "cap")?;
            match mode {
                CapMode::Ref | CapMode::MutRef => {
                    self.builder.build_store(fp, *lp)?;
                }
                CapMode::Move => {
                    let v = self.load(*lp, lty)?;
                    if self.needs_drop(lty) && !matches!(lty, Ty::Fn(..)) {
                        if self.is_ref_param_local(*lp, lty) || self.ref_captured.contains(&(lp.as_value_ref() as usize)) {
                            return Err(CgErr(format!("cannot move `{}` into a closure: it is borrowed here, not owned", name)));
                        }
                        self.mark_moved(*lp, lty)?;
                    } else if matches!(lty, Ty::Fn(..)) {
                        self.fn_value_moved(*lp)?;
                    }
                    self.builder.build_store(fp, v)?;
                }
            }
        }
        self.pending_closures.push(PendingClosure {
            f,
            c: c as *const ClosureExpr,
            env_ty,
            caps: caps.iter().map(|(n, m, t, _)| (n.clone(), *m, t.clone())).collect(),
            ptys,
            ret,
            once: info.once,
            cur_self: self.cur_self.clone(),
            owner: if self.cur_key == "<closure>" { self.closure_owner.clone() } else { self.cur_key.clone() },
        });
        self.make_fn_value(f.as_global_value().as_pointer_value(), env)
    }

    /// `void drop(env)`: drops the moved-in captures, then frees the heap block.
    fn closure_drop_fn(&mut self, n: usize, env_ty: StructType<'ctx>, caps: &[(String, CapMode, Ty, PointerValue<'ctx>)]) -> R<FunctionValue<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let fd = self.module.add_function(&format!("__mtn_closure_{}_drop", n), self.context.void_type().fn_type(&[ptrt.into()], false), None);
        let saved = self.builder.get_insert_block();
        let saved_dead = self.dead;
        let entry = self.context.append_basic_block(fd, "entry");
        self.builder.position_at_end(entry);
        let env = fd.get_nth_param(0).unwrap().into_pointer_value();
        let r = (|| -> R<()> {
            for (i, (_, mode, ty, _)) in caps.iter().enumerate() {
                if *mode == CapMode::Move && self.needs_drop(ty) {
                    let fp = self.builder.build_struct_gep(env_ty, env, 2 + i as u32, "cap")?;
                    self.drop_in_place(fp, ty)?;
                }
            }
            let ut = self.usize_ty();
            let size = self.td.get_abi_size(&env_ty).max(1);
            let align = (self.td.get_abi_alignment(&env_ty) as u64).max(8);
            self.call_dealloc(env, ut.const_int(size, false), ut.const_int(align, false))?;
            self.builder.build_return(None)?;
            Ok(())
        })();
        if let Some(bb) = saved {
            self.builder.position_at_end(bb);
        }
        self.dead = saved_dead;
        r?;
        Ok(fd)
    }

    // ---- bodies -------------------------------------------------------------------

    pub(super) fn gen_closure_body(&mut self, pc: PendingClosure<'ctx>) -> R<()> {
        // SAFETY: the program outlives code generation (`generate(&'a Program)`).
        let c: &ClosureExpr = unsafe { &*pc.c };
        let f = pc.f;
        self.cur_fn = Some(f);
        self.cur_self = pc.cur_self.clone();
        self.cur_key = "<closure>".to_string();
        self.closure_owner = pc.owner.clone();
        self.tail_calls.clear();
        self.tail_params = Vec::new();
        self.tail_loop = None;
        self.ret_ty = pc.ret.clone();
        self.locals = vec![HashMap::new()];
        self.drop_frames = vec![drops::DropFrame { scope: true, entries: Vec::new() }];
        self.loops.clear();
        self.ref_captured.clear();
        self.dead = false;
        let alloca_bb = self.context.append_basic_block(f, "entry");
        self.alloca_bb = Some(alloca_bb);
        let body_bb = self.context.append_basic_block(f, "body");
        self.position(body_bb);
        let env = f.get_nth_param(0).unwrap().into_pointer_value();
        if pc.once {
            let fl = self.builder.build_struct_gep(pc.env_ty, env, 1, "called")?;
            let i64t = self.int_ty(64);
            let old = self.builder.build_load(i64t, fl, "was")?.into_int_value();
            let was = self.builder.build_int_compare(IntPredicate::NE, old, i64t.const_zero(), "was_called")?;
            self.panic_if(was, "a closure that moves a captured value out can be called only once")?;
            self.builder.build_store(fl, i64t.const_int(1, false))?;
        }
        let ptrt = self.context.ptr_type(AddressSpace::default());
        for (i, (name, mode, ty)) in pc.caps.iter().enumerate() {
            let fp = self.builder.build_struct_gep(pc.env_ty, env, 2 + i as u32, "cap")?;
            let slot = match mode {
                CapMode::Ref | CapMode::MutRef => {
                    let p = self.builder.build_load(ptrt, fp, name)?.into_pointer_value();
                    self.ref_captured.insert(p.as_value_ref() as usize);
                    p
                }
                CapMode::Move => fp,
            };
            self.locals[0].insert(name.clone(), (slot, ty.clone()));
        }
        for (i, ((pat, _), pty)) in c.params.iter().zip(pc.ptys.iter()).enumerate() {
            let pv = f.get_nth_param(1 + i as u32).unwrap();
            match pat {
                Pattern::Ident(n) | Pattern::Mut(n) => {
                    let slot = self.declare_local(n, pty)?;
                    self.builder.build_store(slot, pv)?;
                }
                Pattern::Wildcard => {
                    self.spill_owned(pv, pty)?;
                }
                other => {
                    let tmp = self.spill_owned(pv, pty)?;
                    let mut binds = Vec::new();
                    self.pat_test(other, tmp, pty, &mut binds, false)?;
                    let pending = self.materialize(binds)?;
                    self.commit_binds(pending)?;
                }
            }
        }
        let ret = pc.ret.clone();
        let (v, bty) = match &c.body {
            ClosureBody::Block(b) => self.gen_block(b)?,
            ClosureBody::Expr(e) => {
                self.push_frame(false);
                let ty = self.ty_of(e)?;
                let r = self.expr_as(e, &ret);
                let ok = r.is_ok();
                self.pop_frame(ok)?;
                let v = r?;
                (v, if self.dead || ty == Ty::Never { Ty::Never } else { ty })
            }
        };
        if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
            if self.dead || bty == Ty::Never {
                self.builder.build_unreachable()?;
            } else if is_unit_like(&ret) {
                self.emit_frames_from(0)?;
                self.builder.build_return(None)?;
            } else {
                self.emit_frames_from(0)?;
                self.builder.build_return(Some(&v))?;
            }
        }
        self.alloca_builder.position_at_end(alloca_bb);
        self.alloca_builder.build_unconditional_branch(body_bb)?;
        self.cur_fn = None;
        self.cur_self = None;
        self.cur_key = String::new();
        Ok(())
    }

    // ---- calls --------------------------------------------------------------------

    /// `callee(args)` where `callee` is a value of function type (a variable, field, element,
    /// parenthesized closure or the result of another call).
    pub(super) fn call_value(&mut self, callee: &Expr, args: &[Arg]) -> R<V<'ctx>> {
        if args.iter().any(|a| a.name.is_some()) {
            return Err(CgErr("named arguments are only allowed when calling a declared function, not a function value (Document 10 §2.2)".into()));
        }
        let cty = self.ty_of(callee)?;
        let Ty::Fn(ptys, ret) = &cty else {
            return Err(CgErr(format!("cannot call a value of type `{}`: only function values can be called", cty)));
        };
        let (ptys, ret) = (ptys.clone(), (**ret).clone());
        if args.len() != ptys.len() {
            return Err(CgErr(format!("this function value takes {} argument(s), found {}", ptys.len(), args.len())));
        }
        let mut c = callee;
        while let Expr::Paren(i) = c {
            c = i;
        }
        let fv = if self.is_place_expr(c) {
            let (p, t) = self.place(c)?;
            self.load(p, &t)?
        } else {
            let v = self.expr(c)?;
            let p = self.spill_owned(v, &cty)?;
            self.load(p, &cty)?
        };
        let sv = fv.into_struct_value();
        let code = self.builder.build_extract_value(sv, 0, "code")?.into_pointer_value();
        let env = self.builder.build_extract_value(sv, 1, "env")?;
        let mut vals: Vec<BasicMetadataValueEnum> = vec![env.into()];
        for (a, t) in args.iter().zip(ptys.iter()) {
            vals.push(self.expr_as(&a.value, t)?.into());
        }
        let fty = self.fn_llty(&ptys, &ret)?;
        let call = self.builder.build_indirect_call(fty, code, &vals, "")?;
        if ret == Ty::Never {
            self.builder.build_unreachable()?;
            self.terminate();
            return Ok(self.unit());
        }
        match call.try_as_basic_value() {
            ValueKind::Basic(v) => Ok(v),
            ValueKind::Instruction(_) => Ok(self.unit()),
        }
    }

    // ---- named functions as values ---------------------------------------------------

    pub(super) fn fn_item_value(&mut self, name: &str) -> R<V<'ctx>> {
        let thunk = self.fn_thunk(name)?;
        let ptrt = self.context.ptr_type(AddressSpace::default());
        self.make_fn_value(thunk.as_global_value().as_pointer_value(), ptrt.const_null())
    }

    fn fn_thunk(&mut self, name: &str) -> R<FunctionValue<'ctx>> {
        if let Some(t) = self.fn_thunks.get(name) {
            return Ok(*t);
        }
        let (val, params, ret, self_kind) = {
            let i = &self.fns[name];
            (i.val, i.params.clone(), i.ret.clone(), i.self_kind)
        };
        if self_kind.is_some() || params.iter().any(|p| p.mode != PMode::Value) {
            return unsupported(&format!("using the function `{}` (it has `borrow`/variadic parameters) as a value", name), 12);
        }
        let ptys: Vec<Ty> = params.iter().map(|p| p.ty.clone()).collect();
        let fty = self.fn_llty(&ptys, &ret)?;
        let t = self.module.add_function(&format!("__mtn_thunk_{}", Self::fn_symbol(name)), fty, None);
        self.fn_thunks.insert(name.to_string(), t);
        let saved = self.builder.get_insert_block();
        let entry = self.context.append_basic_block(t, "entry");
        self.builder.position_at_end(entry);
        let args: Vec<BasicMetadataValueEnum> = (0..ptys.len()).map(|i| t.get_nth_param(1 + i as u32).unwrap().into()).collect();
        let call = self.builder.build_call(val, &args, "")?;
        if ret == Ty::Never {
            self.builder.build_unreachable()?;
        } else {
            match call.try_as_basic_value() {
                ValueKind::Basic(v) if !is_unit_like(&ret) => {
                    self.builder.build_return(Some(&v))?;
                }
                _ => {
                    self.builder.build_return(None)?;
                }
            }
        }
        if let Some(bb) = saved {
            self.builder.position_at_end(bb);
        }
        Ok(t)
    }

    // ---- drop / move of function values --------------------------------------------------

    /// Reading a function value by value moves it out only when its environment is owned
    /// (heap, with a drop function); a borrowing or named function is freely copyable.
    pub(super) fn fn_value_moved(&mut self, p: PointerValue<'ctx>) -> R<()> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let ft = self.fat_ty();
        let ep = self.builder.build_struct_gep(ft, p, 1, "envp")?;
        let env = self.builder.build_load(ptrt, ep, "env")?.into_pointer_value();
        let (ld, zero, end) = (self.new_block("fv_ld"), self.new_block("fv_zero"), self.new_block("fv_end"));
        let nn = self.builder.build_is_not_null(env, "nn")?;
        self.builder.build_conditional_branch(nn, ld, end)?;
        self.position(ld);
        let df = self.builder.build_load(ptrt, env, "dropfn")?.into_pointer_value();
        let has = self.builder.build_is_not_null(df, "has_drop")?;
        self.builder.build_conditional_branch(has, zero, end)?;
        self.position(zero);
        self.builder.build_store(p, ft.const_zero())?;
        self.builder.build_unconditional_branch(end)?;
        self.position(end);
        Ok(())
    }

    /// Drop glue body of a function value stored at `p`.
    pub(super) fn fn_value_glue(&mut self, f: FunctionValue<'ctx>, p: PointerValue<'ctx>) -> R<()> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let ft = self.fat_ty();
        let ep = self.builder.build_struct_gep(ft, p, 1, "envp")?;
        let env = self.builder.build_load(ptrt, ep, "env")?.into_pointer_value();
        let (ld, run, end) = (self.glue_block(f, "ld"), self.glue_block(f, "run"), self.glue_block(f, "end"));
        let nn = self.builder.build_is_not_null(env, "nn")?;
        self.builder.build_conditional_branch(nn, ld, end)?;
        self.builder.position_at_end(ld);
        let df = self.builder.build_load(ptrt, env, "dropfn")?.into_pointer_value();
        let has = self.builder.build_is_not_null(df, "has_drop")?;
        self.builder.build_conditional_branch(has, run, end)?;
        self.builder.position_at_end(run);
        let dt = self.context.void_type().fn_type(&[ptrt.into()], false);
        self.builder.build_indirect_call(dt, df, &[env.into()], "")?;
        self.builder.build_unconditional_branch(end)?;
        self.builder.position_at_end(end);
        Ok(())
    }
}
