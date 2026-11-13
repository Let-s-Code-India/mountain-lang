//! Tasks and threads (Phase 12 M3; Document 12 §11.1-11.3, §11.9; Document 17 §11.6).
//!
//! `spawn <closure>` / `thread::spawnOS(<closure>)` evaluate the closure to an ordinary M1
//! function value `{ code, env }` (so capture analysis, heap environments and their drop glue are
//! the M1 machinery) and hand it to the C runtime (`runtime/mtn_rt.c`) together with a *thunk*
//! generated per result type: `void thunk(code, env, out)` calls the closure, stores the result in
//! `out` and then drops the closure's environment (exactly once, on the thread that ran the task).
//! A `JoinHandle<T>` is the runtime's task pointer; `join` moves the result out and frees the task,
//! dropping an unjoined handle detaches it.

use super::*;

impl<'ctx, 'a> Codegen<'ctx, 'a> {
    fn rt_decl(&mut self, name: &str, ret: Option<BasicTypeEnum<'ctx>>, params: &[BasicMetadataTypeEnum<'ctx>]) -> FunctionValue<'ctx> {
        self.get_extern(name, ret, params, false)
    }

    /// `void thunk(ptr code, ptr env, ptr out)` for a task whose closure returns `ret`.
    fn task_thunk(&mut self, ret: &Ty) -> R<FunctionValue<'ctx>> {
        let key = ret.to_string();
        if let Some(t) = self.task_thunks.get(&key) {
            return Ok(*t);
        }
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let n = self.task_thunks.len();
        let fd = self.module.add_function(&format!("__mtn_task_thunk_{}", n), self.context.void_type().fn_type(&[ptrt.into(), ptrt.into(), ptrt.into()], false), None);
        self.task_thunks.insert(key, fd);
        let saved = self.builder.get_insert_block();
        let saved_dead = self.dead;
        let entry = self.context.append_basic_block(fd, "entry");
        self.builder.position_at_end(entry);
        let (code, env, out) = (
            fd.get_nth_param(0).unwrap().into_pointer_value(),
            fd.get_nth_param(1).unwrap().into_pointer_value(),
            fd.get_nth_param(2).unwrap().into_pointer_value(),
        );
        let r = (|| -> R<()> {
            let fty = self.fn_llty(&[], ret)?;
            let call = self.builder.build_indirect_call(fty, code, &[env.into()], "")?;
            if *ret == Ty::Never {
                self.builder.build_unreachable()?;
                return Ok(());
            }
            if let ValueKind::Basic(v) = call.try_as_basic_value() {
                if !is_unit_like(ret) {
                    self.builder.build_store(out, v)?;
                }
            }
            // drop the closure value: run the environment's drop function, if it has one
            let (ld, run, end) = (self.glue_block(fd, "ld"), self.glue_block(fd, "run"), self.glue_block(fd, "end"));
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

    /// `spawn <closure>` (`os == false`) and `thread::spawnOS(<closure>)` (`os == true`) -> `JoinHandle<T>`.
    pub(super) fn gen_spawn(&mut self, closure: &Expr, os: bool) -> R<V<'ctx>> {
        let cty = self.ty_of(closure)?;
        let Ty::Fn(_, ret) = &cty else {
            return Err(CgErr(format!("internal: a task closure is typed `{}`", cty)));
        };
        let ret = (**ret).clone();
        let fv = self.expr(closure)?.into_struct_value();
        let code = self.builder.build_extract_value(fv, 0, "code")?;
        let env = self.builder.build_extract_value(fv, 1, "env")?;
        let thunk = self.task_thunk(&ret)?;
        let ut = self.usize_ty();
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let unit = is_unit_like(&ret) || ret == Ty::Never;
        let size = if unit {
            0
        } else {
            let lt = self.llty(&ret)?;
            self.td.get_abi_size(&lt)
        };
        let rdrop = if !unit && self.needs_drop(&ret) { self.drop_glue(&ret)?.as_global_value().as_pointer_value() } else { ptrt.const_null() };
        let name = if os { "__mtn_rt_spawn_os" } else { "__mtn_rt_spawn" };
        let f = self.rt_decl(name, Some(ptrt.into()), &[ptrt.into(), ptrt.into(), ptrt.into(), ut.into(), ptrt.into()]);
        let r = self.builder.build_call(
            f,
            &[thunk.as_global_value().as_pointer_value().into(), code.into(), env.into(), ut.const_int(size, false).into(), rdrop.into()],
            "task",
        )?;
        let ValueKind::Basic(h) = r.try_as_basic_value() else { return Err(CgErr("internal: spawn result".into())) };
        Ok(h)
    }

    /// `h.join()`: consumes the handle (the type checker/borrow checker enforce the move) and returns the task's value.
    pub(super) fn gen_join(&mut self, receiver: &Expr, item: &Ty) -> R<V<'ctx>> {
        let h = self.expr(receiver)?;
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let unit = is_unit_like(item) || *item == Ty::Never;
        let out = if unit { ptrt.const_null() } else { self.alloca_ty(item, "joined")? };
        let f = self.rt_decl("__mtn_rt_join", None, &[ptrt.into(), ptrt.into()]);
        self.builder.build_call(f, &[h.into(), out.into()], "")?;
        if unit {
            return Ok(self.unit());
        }
        self.load(out, item)
    }

    /// Drop glue of a `JoinHandle<T>` slot: an unjoined handle detaches the task.
    pub(super) fn join_handle_glue(&mut self, p: PointerValue<'ctx>) -> R<()> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let h = self.builder.build_load(ptrt, p, "handle")?;
        let f = self.rt_decl("__mtn_rt_handle_drop", None, &[ptrt.into()]);
        self.builder.build_call(f, &[h.into()], "")?;
        Ok(())
    }

    /// `pinToCore(n)`.
    pub(super) fn gen_pin_to_core(&mut self, arg: &Expr) -> R<V<'ctx>> {
        let ut = self.usize_ty();
        let v = self.expr_as(arg, &Ty::Usize)?;
        let f = self.rt_decl("__mtn_rt_pin_to_core", None, &[ut.into()]);
        self.builder.build_call(f, &[v.into()], "")?;
        Ok(self.unit())
    }
}
