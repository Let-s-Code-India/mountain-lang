//! Drop glue, drop frames and moves (Phase 11c C3; Document 6, Document 13 §1-§2).
//!
//! Ownership model of the generated code:
//!  * every value of a type that `needs_drop` is owned by exactly one place;
//!  * an owning slot (a local, a temporary, a field) is always either a VALID value or ALL-ZERO
//!    ("moved out"): slots are zero-initialised in the function's entry block, a move out of a
//!    place stores zero back, and every drop glue is a no-op on the all-zero value (it null-checks
//!    every heap pointer). This is what makes conditional moves, partial moves, loops and early
//!    exits free of double frees and leaks without a separate flag per variable;
//!  * a drop frame lists the owning slots of one lexical scope (or one statement, for temporaries);
//!    leaving the scope normally, or by `return` / `break` / `continue` / `?` / a tail call, runs the
//!    glue of every live frame entry in reverse order and re-zeroes the slot.

use super::*;
use inkwell::values::BasicValue;

pub(super) struct DropEntry<'ctx> {
    pub ptr: PointerValue<'ctx>,
    pub ty: Ty,
}

pub(super) struct DropFrame<'ctx> {
    /// `true` for a lexical scope (locals register here), `false` for a statement's temporaries.
    pub scope: bool,
    pub entries: Vec<DropEntry<'ctx>>,
}

impl<'ctx, 'a> Codegen<'ctx, 'a> {
    /// Whether dropping a value of `ty` does anything (it owns heap memory somewhere).
    pub(super) fn needs_drop(&self, ty: &Ty) -> bool {
        self.needs_drop_d(ty, 0)
    }

    fn needs_drop_d(&self, ty: &Ty, depth: u32) -> bool {
        if depth > 48 {
            return true;
        }
        match ty {
            Ty::StringTy | Ty::Array(_) | Ty::Fn(..) => true,
            Ty::Generic(n, _) if n == "ActorRef" => false,
            // a lock drops what it protects; a guard releases its lock (Phase 13 M2)
            Ty::Generic(n, a) if crate::locks::is_lock_generic(n) => match a.first() {
                Some(crate::types::GenericArg::Type(t)) => self.needs_drop_d(t, depth + 1),
                _ => false,
            },
            Ty::Generic(n, _) if crate::locks::is_guard_generic(n) => true,
            Ty::Named(n) if n == "SpinlockGuard" && !self.structs.contains_key(n) => true,
            Ty::Generic(n, _) => crate::types::is_heap_generic(n),
            Ty::Tuple(ts) => ts.iter().any(|t| self.needs_drop_d(t, depth + 1)),
            Ty::Fixed(t, _) => self.needs_drop_d(t, depth + 1),
            // a type with a `Drop` impl always has something to do (Document 6 §9.9)
            Ty::Named(n) if self.drop_impl_key(n).is_some() => true,
            Ty::Named(n) => {
                if let Some((fields, _)) = self.structs.get(n) {
                    fields.iter().any(|(_, t)| self.needs_drop_d(t, depth + 1))
                } else if let Some(vs) = self.enums.get(n) {
                    vs.iter().any(|(_, fs)| fs.iter().any(|t| self.needs_drop_d(t, depth + 1)))
                } else {
                    false
                }
            }
            Ty::OptionTy(_) | Ty::ResultTy(..) => self.variants_of(ty).map(|vs| vs.iter().any(|(_, fs)| fs.iter().any(|t| self.needs_drop_d(t, depth + 1)))).unwrap_or(false),
            _ => false,
        }
    }

    // ---- frames -----------------------------------------------------------------

    pub(super) fn push_frame(&mut self, scope: bool) {
        self.drop_frames.push(DropFrame { scope, entries: Vec::new() });
    }

    /// Pops the top frame; with `emit`, first runs its drops (when the current block is live).
    pub(super) fn pop_frame(&mut self, emit: bool) -> R<()> {
        if emit && !self.dead {
            self.emit_top_frame()?;
        }
        self.drop_frames.pop();
        Ok(())
    }

    pub(super) fn push_scope(&mut self) {
        self.locals.push(HashMap::new());
        self.push_frame(true);
    }

    pub(super) fn pop_scope(&mut self, emit: bool) -> R<()> {
        let r = self.pop_frame(emit);
        self.locals.pop();
        r
    }

    /// Drops (and re-zeroes) every entry of the top frame, newest first, without popping it.
    pub(super) fn emit_top_frame(&mut self) -> R<()> {
        let n = self.drop_frames.len();
        if n == 0 {
            return Ok(());
        }
        self.emit_frames_range(n - 1)
    }

    /// Drops the entries of all frames at index `>= from` (newest first): the scope-exit code of
    /// `return` (`from == 0`), `break` / `continue` (the loop's frame depth), `?` and tail calls.
    pub(super) fn emit_frames_from(&mut self, from: usize) -> R<()> {
        if self.dead {
            return Ok(());
        }
        self.emit_frames_range(from)
    }

    fn emit_frames_range(&mut self, from: usize) -> R<()> {
        let mut todo: Vec<(PointerValue<'ctx>, Ty)> = Vec::new();
        for f in self.drop_frames[from..].iter().rev() {
            for e in f.entries.iter().rev() {
                todo.push((e.ptr, e.ty.clone()));
            }
        }
        for (p, t) in todo {
            self.drop_in_place(p, &t)?;
        }
        Ok(())
    }

    /// Registers an owning slot with the innermost lexical scope.
    pub(super) fn register_local_drop(&mut self, ptr: PointerValue<'ctx>, ty: &Ty) {
        if !self.needs_drop(ty) {
            return;
        }
        if let Some(f) = self.drop_frames.iter_mut().rev().find(|f| f.scope) {
            f.entries.push(DropEntry { ptr, ty: ty.clone() });
        }
    }

    /// Registers an owning temporary with the innermost frame (statement or scope).
    fn register_temp_drop(&mut self, ptr: PointerValue<'ctx>, ty: &Ty) {
        if let Some(f) = self.drop_frames.last_mut() {
            f.entries.push(DropEntry { ptr, ty: ty.clone() });
        }
    }

    /// An entry-block alloca that starts out all-zero when `ty` owns heap memory.
    pub(super) fn alloca_owning(&mut self, ty: &Ty, name: &str) -> R<PointerValue<'ctx>> {
        let lt = self.llty(ty)?;
        let p = self.alloca(lt, name)?;
        if self.needs_drop(ty) {
            self.alloca_builder.build_store(p, lt.const_zero())?;
        }
        Ok(p)
    }

    /// Spills an owned value to a fresh slot that is dropped with the innermost frame.
    pub(super) fn spill_owned(&mut self, v: V<'ctx>, ty: &Ty) -> R<PointerValue<'ctx>> {
        let p = self.alloca_owning(ty, "tmp")?;
        self.builder.build_store(p, v)?;
        if self.needs_drop(ty) {
            self.register_temp_drop(p, ty);
        }
        Ok(p)
    }

    /// Marks the value at `p` as moved out (stores the all-zero value).
    pub(super) fn mark_moved(&mut self, p: PointerValue<'ctx>, ty: &Ty) -> R<()> {
        if self.needs_drop(ty) {
            let lt = self.llty(ty)?;
            self.builder.build_store(p, lt.const_zero())?;
        }
        Ok(())
    }

    /// Runs the glue of the value at `p` and leaves the slot all-zero.
    pub(super) fn drop_in_place(&mut self, p: PointerValue<'ctx>, ty: &Ty) -> R<()> {
        if !self.needs_drop(ty) {
            return Ok(());
        }
        let g = self.drop_glue(ty)?;
        self.builder.build_call(g, &[p.into()], "")?;
        self.mark_moved(p, ty)
    }

    // ---- glue -------------------------------------------------------------------

    fn glue_name(ty: &Ty) -> String {
        let s: String = ty.to_string().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        format!("__mtn_drop_{}", s)
    }

    /// The drop-glue function `void(ptr)` of `ty`, generated once per type.
    pub(super) fn drop_glue(&mut self, ty: &Ty) -> R<FunctionValue<'ctx>> {
        let key = ty.to_string();
        if let Some(f) = self.drop_glues.get(&key) {
            return Ok(*f);
        }
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let mut name = Self::glue_name(ty);
        if self.module.get_function(&name).is_some() {
            name = format!("{}_{}", name, self.drop_glues.len());
        }
        let f = self.module.add_function(&name, self.context.void_type().fn_type(&[ptrt.into()], false), None);
        self.drop_glues.insert(key, f);
        let saved = self.builder.get_insert_block();
        let saved_dead = self.dead;
        let entry = self.context.append_basic_block(f, "entry");
        self.builder.position_at_end(entry);
        let p = f.get_nth_param(0).unwrap().into_pointer_value();
        let r = self.glue_body(f, p, ty);
        if r.is_ok() {
            self.builder.build_return(None)?;
        }
        if let Some(bb) = saved {
            self.builder.position_at_end(bb);
        }
        self.dead = saved_dead;
        r?;
        Ok(f)
    }

    pub(super) fn glue_block(&self, f: FunctionValue<'ctx>, name: &str) -> BasicBlock<'ctx> {
        self.context.append_basic_block(f, name)
    }

    pub(super) fn call_dealloc(&mut self, p: PointerValue<'ctx>, size: IntValue<'ctx>, align: IntValue<'ctx>) -> R<()> {
        let (_, fd) = self.alloc_fns();
        self.builder.build_call(fd, &[p.into(), size.into(), align.into()], "")?;
        Ok(())
    }

    fn glue_body(&mut self, f: FunctionValue<'ctx>, p: PointerValue<'ctx>, ty: &Ty) -> R<()> {
        let ut = self.usize_ty();
        let ptrt = self.context.ptr_type(AddressSpace::default());
        match ty {
            Ty::Fn(..) => self.fn_value_glue(f, p)?,
            Ty::StringTy => {
                let ht = self.handle_ty();
                let dp = self.builder.build_struct_gep(ht, p, 0, "dp")?;
                let cp = self.builder.build_struct_gep(ht, p, 2, "cp")?;
                let data = self.builder.build_load(ptrt, dp, "data")?.into_pointer_value();
                let cap = self.builder.build_load(ut, cp, "cap")?.into_int_value();
                let (free_bb, end) = (self.glue_block(f, "free"), self.glue_block(f, "end"));
                let nn = self.builder.build_is_not_null(data, "nn")?;
                self.builder.build_conditional_branch(nn, free_bb, end)?;
                self.builder.position_at_end(free_bb);
                self.call_dealloc(data, cap, ut.const_int(1, false))?;
                self.builder.build_unconditional_branch(end)?;
                self.builder.position_at_end(end);
            }
            Ty::Array(elem) => {
                let elem = (**elem).clone();
                let elt = self.llty(&elem)?;
                let ht = self.handle_ty();
                let dp = self.builder.build_struct_gep(ht, p, 0, "dp")?;
                let lp = self.builder.build_struct_gep(ht, p, 1, "lp")?;
                let cp = self.builder.build_struct_gep(ht, p, 2, "cp")?;
                let data = self.builder.build_load(ptrt, dp, "data")?.into_pointer_value();
                let len = self.builder.build_load(ut, lp, "len")?.into_int_value();
                let cap = self.builder.build_load(ut, cp, "cap")?.into_int_value();
                let (some_bb, end) = (self.glue_block(f, "some"), self.glue_block(f, "end"));
                let nn = self.builder.build_is_not_null(data, "nn")?;
                self.builder.build_conditional_branch(nn, some_bb, end)?;
                self.builder.position_at_end(some_bb);
                if self.needs_drop(&elem) {
                    let g = self.drop_glue(&elem)?;
                    let (head, body, done) = (self.glue_block(f, "head"), self.glue_block(f, "body"), self.glue_block(f, "done"));
                    let pre = self.builder.get_insert_block().unwrap();
                    self.builder.build_unconditional_branch(head)?;
                    self.builder.position_at_end(head);
                    let i = self.builder.build_phi(ut, "i")?;
                    i.add_incoming(&[(&ut.const_zero(), pre)]);
                    let iv = i.as_basic_value().into_int_value();
                    let more = self.builder.build_int_compare(IntPredicate::ULT, iv, len, "more")?;
                    self.builder.build_conditional_branch(more, body, done)?;
                    self.builder.position_at_end(body);
                    // SAFETY: `iv < len <= cap`, inside the allocation.
                    let ep = unsafe { self.builder.build_in_bounds_gep(elt, data, &[iv], "ep")? };
                    self.builder.build_call(g, &[ep.into()], "")?;
                    let next = self.builder.build_int_add(iv, ut.const_int(1, false), "next")?;
                    let body_end = self.builder.get_insert_block().unwrap();
                    i.add_incoming(&[(&next, body_end)]);
                    self.builder.build_unconditional_branch(head)?;
                    self.builder.position_at_end(done);
                }
                let esize = self.td.get_abi_size(&elt).max(1);
                let ealign = (self.td.get_abi_alignment(&elt) as u64).max(1);
                let bytes = self.builder.build_int_mul(cap, ut.const_int(esize, false), "bytes")?;
                self.call_dealloc(data, bytes, ut.const_int(ealign, false))?;
                self.builder.build_unconditional_branch(end)?;
                self.builder.position_at_end(end);
            }
            Ty::Generic(n, args) if n == "Box" && matches!(args.first(), Some(crate::types::GenericArg::Type(Ty::DynTrait(..)))) => {
                let a = box_alloc_arg(ty);
                self.dyn_box_glue(f, p, a)?;
            }
            Ty::Generic(n, args) if n == "Box" => {
                let Some(crate::types::GenericArg::Type(inner)) = args.first() else {
                    return Err(CgErr("internal: `Box` without a type argument".into()));
                };
                let inner = inner.clone();
                let boxed = self.builder.build_load(ptrt, p, "box")?.into_pointer_value();
                let (free_bb, end) = (self.glue_block(f, "free"), self.glue_block(f, "end"));
                let nn = self.builder.build_is_not_null(boxed, "nn")?;
                self.builder.build_conditional_branch(nn, free_bb, end)?;
                self.builder.position_at_end(free_bb);
                if self.needs_drop(&inner) {
                    let g = self.drop_glue(&inner)?;
                    self.builder.build_call(g, &[boxed.into()], "")?;
                }
                let lt = self.llty(&inner)?;
                let size = self.td.get_abi_size(&lt).max(1);
                let align = (self.td.get_abi_alignment(&lt) as u64).max(1);
                if let Some(a) = box_alloc_arg(ty) {
                    // `Box<T, A>` frees through ITS allocator (Document 13 §3.2)
                    let bt = self.llty(ty)?;
                    let ap = self.builder.build_struct_gep(bt, p, 1, "allocp")?;
                    let alloc = self.builder.build_load(ptrt, ap, "alloc")?.into_pointer_value();
                    self.call_allocator_dealloc(&a, alloc, boxed, ut.const_int(size, false))?;
                } else {
                    self.call_dealloc(boxed, ut.const_int(size, false), ut.const_int(align, false))?;
                }
                self.builder.build_unconditional_branch(end)?;
                self.builder.position_at_end(end);
            }
            // a guard releases its lock; a moved-out guard is a null pointer and does nothing
            Ty::Generic(n, _) if crate::locks::is_guard_generic(n) => self.guard_glue(f, p, ty)?,
            Ty::Named(n) if n == "SpinlockGuard" && !self.structs.contains_key(n) => self.guard_glue(f, p, ty)?,
            // a lock drops the value it protects (the lock words need no clean-up)
            Ty::Generic(n, a) if crate::locks::is_lock_generic(n) => {
                let Some(crate::types::GenericArg::Type(inner)) = a.first() else { return Err(CgErr("internal: lock without a type argument".into())) };
                let inner = inner.clone();
                if self.needs_drop(&inner) {
                    let obj = self.lock_obj_ty(&inner)?;
                    let vp = self.builder.build_struct_gep(obj, p, 1, "lockval")?;
                    let g = self.drop_glue(&inner)?;
                    self.builder.build_call(g, &[vp.into()], "")?;
                }
            }
            Ty::Generic(n, args) if n == "Rc" || n == "Arc" => {
                let Some(crate::types::GenericArg::Type(inner)) = args.first() else {
                    return Err(CgErr(format!("internal: `{}` without a type argument", n)));
                };
                let inner = inner.clone();
                let atomic = n == "Arc";
                let objt = self.rc_obj_ty(&inner)?;
                let is_dyn = matches!(inner, Ty::DynTrait(..));
                let obj = self.builder.build_load(ptrt, p, "rc")?.into_pointer_value();
                // `Rc<dyn T>`: the vtable is the second word of the handle
                let dyn_vt = if is_dyn {
                    let fat = self.fat_ty();
                    let vp = self.builder.build_struct_gep(fat, p, 1, "vtp")?;
                    Some(self.builder.build_load(ptrt, vp, "vt")?.into_pointer_value())
                } else {
                    None
                };
                let (live, last, end) = (self.glue_block(f, "live"), self.glue_block(f, "last"), self.glue_block(f, "end"));
                let nn = self.builder.build_is_not_null(obj, "nn")?;
                self.builder.build_conditional_branch(nn, live, end)?;
                self.builder.position_at_end(live);
                let strong = self.builder.build_struct_gep(objt, obj, 0, "strong")?;
                let old = self.rc_dec(strong, atomic)?;
                let was_last = self.builder.build_int_compare(IntPredicate::EQ, old, ut.const_int(1, false), "was_last")?;
                self.builder.build_conditional_branch(was_last, last, end)?;
                self.builder.position_at_end(last);
                // the last strong owner drops the value, then releases the implicit weak reference
                if let Some(vt) = dyn_vt {
                    // the concrete drop glue comes from the vtable (null when the value owns nothing)
                    let vt_ty = self.vtable_ty_pub(0);
                    let dfp = self.builder.build_struct_gep(vt_ty, vt, 0, "dropp")?;
                    let dfn = self.builder.build_load(ptrt, dfp, "dropfn")?.into_pointer_value();
                    let has = self.builder.build_is_not_null(dfn, "has_drop")?;
                    let (call_bb, after) = (self.glue_block(f, "dyn_drop"), self.glue_block(f, "dyn_after"));
                    self.builder.build_conditional_branch(has, call_bb, after)?;
                    self.builder.position_at_end(call_bb);
                    let vp = self.dyn_rc_value_ptr(obj, vt)?;
                    let ft = self.context.void_type().fn_type(&[ptrt.into()], false);
                    self.builder.build_indirect_call(ft, dfn, &[vp.into()], "")?;
                    self.builder.build_unconditional_branch(after)?;
                    self.builder.position_at_end(after);
                } else if self.needs_drop(&inner) {
                    let g = self.drop_glue(&inner)?;
                    let vp = self.builder.build_struct_gep(objt, obj, 2, "value")?;
                    self.builder.build_call(g, &[vp.into()], "")?;
                }
                self.rc_release_weak(f, objt, obj, atomic, end, dyn_vt)?;
                self.builder.position_at_end(end);
            }
            Ty::Generic(n, args) if n == "Weak" => {
                let Some(crate::types::GenericArg::Type(inner)) = args.first() else {
                    return Err(CgErr("internal: `Weak` without a type argument".into()));
                };
                let inner = inner.clone();
                let atomic = Self::weak_is_arc(args);
                let objt = self.rc_obj_ty(&inner)?;
                let obj = self.builder.build_load(ptrt, p, "weak")?.into_pointer_value();
                let dyn_vt = if matches!(inner, Ty::DynTrait(..)) {
                    let fat = self.fat_ty();
                    let vp = self.builder.build_struct_gep(fat, p, 1, "vtp")?;
                    Some(self.builder.build_load(ptrt, vp, "vt")?.into_pointer_value())
                } else {
                    None
                };
                let (live, end) = (self.glue_block(f, "live"), self.glue_block(f, "end"));
                let nn = self.builder.build_is_not_null(obj, "nn")?;
                self.builder.build_conditional_branch(nn, live, end)?;
                self.builder.position_at_end(live);
                self.rc_release_weak(f, objt, obj, atomic, end, dyn_vt)?;
                self.builder.position_at_end(end);
            }
            Ty::Generic(n, _) if n == "Generator" || n == "Future" => self.generator_glue(f, p)?,
            Ty::Generic(n, _) if n == "JoinHandle" => self.join_handle_glue(p)?,
            Ty::Generic(n, _) => {
                return Err(CgErr(format!("internal error: no drop glue is defined for the type `{}`", n)));
            }
            Ty::Tuple(ts) => {
                let st = self.llty(ty)?;
                for (i, t) in ts.clone().iter().enumerate() {
                    if self.needs_drop(t) {
                        let g = self.drop_glue(t)?;
                        let fp = self.builder.build_struct_gep(st, p, i as u32, "tp")?;
                        self.builder.build_call(g, &[fp.into()], "")?;
                    }
                }
            }
            Ty::Fixed(t, n) => {
                let lt = self.llty(ty)?;
                let g = self.drop_glue(t)?;
                for i in 0..*n {
                    // SAFETY: `i < n`, the array length.
                    let ep = unsafe { self.builder.build_in_bounds_gep(lt, p, &[ut.const_zero(), ut.const_int(i, false)], "ep")? };
                    self.builder.build_call(g, &[ep.into()], "")?;
                }
            }
            Ty::Named(n) if self.structs.contains_key(n) => {
                let (fields, _) = self.structs[n].clone();
                let st = self.llty(ty)?;
                let end = self.glue_block(f, "end");
                if self.drop_impl_key(n).is_some() {
                    self.user_drop_prologue(f, p, ty, st.into_struct_type(), fields.len() as u32, end)?;
                }
                for (i, (_, ft)) in fields.iter().enumerate() {
                    if self.needs_drop(ft) {
                        let g = self.drop_glue(ft)?;
                        let fp = self.builder.build_struct_gep(st, p, i as u32, "fp")?;
                        self.builder.build_call(g, &[fp.into()], "")?;
                    }
                }
                self.builder.build_unconditional_branch(end)?;
                self.builder.position_at_end(end);
            }
            Ty::Named(_) | Ty::OptionTy(_) | Ty::ResultTy(..) => {
                let variants = self.variants_of(ty).ok_or_else(|| CgErr(format!("internal: no drop glue for `{}`", ty)))?;
                let layout = self.enum_layout(ty)?;
                let end = self.glue_block(f, "end");
                if let Ty::Named(n) = ty {
                    if self.drop_impl_key(n).is_some() {
                        self.user_drop_prologue(f, p, ty, layout.llty, 2, end)?;
                    }
                }
                let tag = self.enum_tag(ty, p)?;
                let mut cases: Vec<(IntValue<'ctx>, BasicBlock<'ctx>)> = Vec::new();
                let mut bodies: Vec<(usize, BasicBlock<'ctx>)> = Vec::new();
                for (i, (_, fs)) in variants.iter().enumerate() {
                    if fs.iter().any(|t| self.needs_drop(t)) {
                        let bb = self.glue_block(f, "variant");
                        cases.push((self.int_ty(32).const_int(i as u64, false), bb));
                        bodies.push((i, bb));
                    }
                }
                self.builder.build_switch(tag, end, &cases)?;
                for (i, bb) in bodies {
                    self.builder.position_at_end(bb);
                    let payload = self.builder.build_struct_gep(layout.llty, p, 1, "payload")?;
                    for (j, ft) in variants[i].1.clone().iter().enumerate() {
                        if self.needs_drop(ft) {
                            let g = self.drop_glue(ft)?;
                            let fp = self.builder.build_struct_gep(layout.variants[i], payload, j as u32, "pf")?;
                            self.builder.build_call(g, &[fp.into()], "")?;
                        }
                    }
                    self.builder.build_unconditional_branch(end)?;
                }
                self.builder.position_at_end(end);
            }
            other => return Err(CgErr(format!("internal: no drop glue for `{}`", other))),
        }
        Ok(())
    }

    /// Start of the glue of a type with a `Drop` impl (Document 6 §9.9 rule 2): when the hidden live
    /// byte is 0 (a moved-out or never-built slot) go straight to `end`; otherwise clear the byte
    /// and run the user's `drop(borrow mut self)` BEFORE the fields are dropped (the caller emits the
    /// field drops after this and ends in `end`).
    fn user_drop_prologue(&mut self, f: FunctionValue<'ctx>, p: PointerValue<'ctx>, ty: &Ty, st: StructType<'ctx>, flag_idx: u32, end: BasicBlock<'ctx>) -> R<()> {
        let Ty::Named(n) = ty else { return Err(CgErr("internal: Drop on a non-named type".into())) };
        let key = self.drop_impl_key(n).ok_or_else(|| CgErr(format!("internal: no `Drop` impl for `{}`", n)))?;
        let user = self.fns.get(&key).ok_or_else(|| CgErr(format!("internal: the `drop` method of `{}` was not declared", n)))?.val;
        let i8t = self.context.i8_type();
        let fp = self.builder.build_struct_gep(st, p, flag_idx, "livep")?;
        let live = self.builder.build_load(i8t, fp, "live")?.into_int_value();
        let is_live = self.builder.build_int_compare(IntPredicate::NE, live, i8t.const_zero(), "is_live")?;
        let run = self.glue_block(f, "user_drop");
        self.builder.build_conditional_branch(is_live, run, end)?;
        self.builder.position_at_end(run);
        // at most once: the byte is cleared before the call, so a second run of the glue is a no-op
        self.builder.build_store(fp, i8t.const_zero())?;
        self.builder.build_call(user, &[p.into()], "")?;
        Ok(())
    }

    // ---- Rc / Arc / Weak (Document 6 §6) ----------------------------------------
    //
    // Heap object: `{ strong: usize, weak: usize, value: T }`. `Rc<T>`, `Arc<T>` and `Weak<T>` are
    // all a pointer to it (null = moved out). `weak` counts the `Weak` handles PLUS ONE implicit
    // reference held collectively by the strong owners; the object is freed by whoever brings
    // `weak` to zero, so a racing last `Arc` and last `Weak` can never both free it.

    pub(super) fn rc_obj_ty(&mut self, inner: &Ty) -> R<StructType<'ctx>> {
        let ut = self.usize_ty();
        if matches!(inner, Ty::DynTrait(..)) {
            // `Rc<dyn T>`: only the header `{strong, weak}` is statically known; the value follows it at an
            // offset that depends on the concrete type's alignment (read from the vtable).
            return Ok(self.context.struct_type(&[ut.into(), ut.into()], false));
        }
        let lt = self.llty(inner)?;
        Ok(self.context.struct_type(&[ut.into(), ut.into(), lt], false))
    }

    pub(super) fn weak_is_arc(args: &[crate::types::GenericArg]) -> bool {
        matches!(args.get(1), Some(crate::types::GenericArg::Type(Ty::Named(k))) if k == "Arc")
    }

    /// `counter -= 1` (atomically for `Arc`); returns the OLD value.
    fn rc_dec(&mut self, counter: PointerValue<'ctx>, atomic: bool) -> R<IntValue<'ctx>> {
        let ut = self.usize_ty();
        if atomic {
            Ok(self.builder.build_atomicrmw(inkwell::AtomicRMWBinOp::Sub, counter, ut.const_int(1, false), inkwell::AtomicOrdering::AcquireRelease)?)
        } else {
            let old = self.builder.build_load(ut, counter, "old")?.into_int_value();
            let new = self.builder.build_int_sub(old, ut.const_int(1, false), "new")?;
            self.builder.build_store(counter, new)?;
            Ok(old)
        }
    }

    /// `counter += 1` (atomically for `Arc`).
    pub(super) fn rc_inc(&mut self, counter: PointerValue<'ctx>, atomic: bool) -> R<()> {
        let ut = self.usize_ty();
        if atomic {
            self.builder.build_atomicrmw(inkwell::AtomicRMWBinOp::Add, counter, ut.const_int(1, false), inkwell::AtomicOrdering::Monotonic)?;
        } else {
            let old = self.builder.build_load(ut, counter, "old")?.into_int_value();
            let new = self.builder.build_int_add(old, ut.const_int(1, false), "new")?;
            self.builder.build_store(counter, new)?;
        }
        Ok(())
    }

    /// `(offset of the value, whole object size, object alignment)` of an `Rc<dyn T>` object whose concrete
    /// type has the size/alignment recorded in vtable `vt` (the same layout `{usize, usize, T}` has for `Rc<T>`).
    fn dyn_rc_layout(&mut self, vt: PointerValue<'ctx>) -> R<(IntValue<'ctx>, IntValue<'ctx>, IntValue<'ctx>)> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let _ = ptrt;
        let ut = self.usize_ty();
        let vt_ty = self.vtable_ty_pub(0);
        let sp = self.builder.build_struct_gep(vt_ty, vt, 1, "vsize")?;
        let ap = self.builder.build_struct_gep(vt_ty, vt, 2, "valign")?;
        let size = self.builder.build_load(ut, sp, "size")?.into_int_value();
        let align = self.builder.build_load(ut, ap, "align")?.into_int_value();
        let hdr = ut.const_int(self.td.get_abi_size(&self.context.struct_type(&[ut.into(), ut.into()], false)), false);
        let one = ut.const_int(1, false);
        let am1 = self.builder.build_int_sub(align, one, "am1")?;
        // off = (hdr + align - 1) & !(align - 1)
        let sum = self.builder.build_int_add(hdr, am1, "sum")?;
        let mask = self.builder.build_not(am1, "mask")?;
        let off = self.builder.build_and(sum, mask, "off")?;
        // object alignment = max(align, alignof(usize))
        let ualign = ut.const_int(self.td.get_abi_alignment(&ut) as u64, false);
        let bigger = self.builder.build_int_compare(IntPredicate::UGT, align, ualign, "bigger")?;
        let oalign = self.builder.build_select(bigger, align, ualign, "oalign")?.into_int_value();
        // total = round_up(off + size, oalign)
        let raw = self.builder.build_int_add(off, size, "raw")?;
        let om1 = self.builder.build_int_sub(oalign, one, "om1")?;
        let t1 = self.builder.build_int_add(raw, om1, "t1")?;
        let omask = self.builder.build_not(om1, "omask")?;
        let total = self.builder.build_and(t1, omask, "total")?;
        Ok((off, total, oalign))
    }

    /// Pointer to the value inside an `Rc<dyn T>` object (`obj + offset`).
    pub(super) fn dyn_rc_value_ptr(&mut self, obj: PointerValue<'ctx>, vt: PointerValue<'ctx>) -> R<PointerValue<'ctx>> {
        let (off, _, _) = self.dyn_rc_layout(vt)?;
        let i8t = self.context.i8_type();
        // SAFETY: `off` is within the allocation made for this object
        Ok(unsafe { self.builder.build_gep(i8t, obj, &[off], "dynval")? })
    }

    /// Releases one weak reference of `obj`; frees the object when it was the last. Ends in `end`.
    /// `dyn_vt`: the vtable when the object is an `Rc<dyn T>` (its size then comes from the vtable).
    fn rc_release_weak(&mut self, f: FunctionValue<'ctx>, objt: StructType<'ctx>, obj: PointerValue<'ctx>, atomic: bool, end: BasicBlock<'ctx>, dyn_vt: Option<PointerValue<'ctx>>) -> R<()> {
        let ut = self.usize_ty();
        let weak = self.builder.build_struct_gep(objt, obj, 1, "weak")?;
        let old = self.rc_dec(weak, atomic)?;
        let free = self.glue_block(f, "free_obj");
        let was_last = self.builder.build_int_compare(IntPredicate::EQ, old, ut.const_int(1, false), "wl")?;
        self.builder.build_conditional_branch(was_last, free, end)?;
        self.builder.position_at_end(free);
        if let Some(vt) = dyn_vt {
            let (_, total, oalign) = self.dyn_rc_layout(vt)?;
            self.call_dealloc(obj, total, oalign)?;
        } else {
            let size = self.td.get_abi_size(&objt).max(1);
            let align = (self.td.get_abi_alignment(&objt) as u64).max(1);
            self.call_dealloc(obj, ut.const_int(size, false), ut.const_int(align, false))?;
        }
        self.builder.build_unconditional_branch(end)?;
        Ok(())
    }

    /// `Rc::new(v)` / `Arc::new(v)`: strong = 1, weak = 1 (the implicit reference).
    pub(super) fn rc_new(&mut self, inner: &Ty, v: V<'ctx>) -> R<V<'ctx>> {
        let ut = self.usize_ty();
        let objt = self.rc_obj_ty(inner)?;
        let size = self.td.get_abi_size(&objt).max(1);
        let align = (self.td.get_abi_alignment(&objt) as u64).max(1);
        let (fa, _) = self.alloc_fns();
        let call = self.builder.build_call(fa, &[ut.const_int(size, false).into(), ut.const_int(align, false).into()], "rc")?;
        let ValueKind::Basic(p) = call.try_as_basic_value() else { return Err(CgErr("internal: alloc result".into())) };
        let p = p.into_pointer_value();
        let sp = self.builder.build_struct_gep(objt, p, 0, "strong")?;
        self.builder.build_store(sp, ut.const_int(1, false))?;
        let wp = self.builder.build_struct_gep(objt, p, 1, "weak")?;
        self.builder.build_store(wp, ut.const_int(1, false))?;
        let vp = self.builder.build_struct_gep(objt, p, 2, "value")?;
        self.builder.build_store(vp, v)?;
        Ok(p.into())
    }

    /// `x.clone()` / `x.downgrade()` / `x.upgrade()` on an `Rc` / `Arc` / `Weak` receiver. The
    /// receiver is lent, never moved.
    pub(super) fn rc_method(&mut self, receiver: &Expr, gname: &str, gargs: &[crate::types::GenericArg], name: &str, whole_ty: &Ty) -> R<V<'ctx>> {
        let Some(crate::types::GenericArg::Type(inner)) = gargs.first() else {
            return Err(CgErr(format!("internal: `{}` without a type argument", gname)));
        };
        let inner = inner.clone();
        let ptrt = self.context.ptr_type(AddressSpace::default());
        // address of the slot that holds the object pointer
        let rty = self.ty_of(receiver)?;
        let (mut slot, mut t) = self.place_or_spill(receiver)?;
        while let Ty::Ref(_, i) = &t {
            slot = self.builder.build_load(ptrt, slot, "deref")?.into_pointer_value();
            t = (**i).clone();
        }
        let _ = rty;
        let is_dyn = matches!(inner, Ty::DynTrait(..));
        // a handle is the object pointer, or `{ object pointer, vtable }` for `Rc<dyn T>`
        let handle: V<'ctx> = if is_dyn { self.builder.build_load(self.fat_ty(), slot, "handle")? } else { self.builder.build_load(ptrt, slot, "obj")? };
        let obj = if is_dyn { self.builder.build_extract_value(handle.into_struct_value(), 0, "obj")?.into_pointer_value() } else { handle.into_pointer_value() };
        let objt = self.rc_obj_ty(&inner)?;
        let arc_kind = gname == "Arc" || (gname == "Weak" && Self::weak_is_arc(gargs));
        match (gname, name) {
            ("Rc" | "Arc", "clone") => {
                let sp = self.builder.build_struct_gep(objt, obj, 0, "strong")?;
                self.rc_inc(sp, arc_kind)?;
                Ok(handle)
            }
            ("Rc" | "Arc", "downgrade") | ("Weak", "clone") => {
                let wp = self.builder.build_struct_gep(objt, obj, 1, "weak")?;
                self.rc_inc(wp, arc_kind)?;
                Ok(handle)
            }
            ("Weak", "upgrade") => {
                let ut = self.usize_ty();
                let some_idx = self.variant_index("Some", whole_ty).ok_or_else(|| CgErr("internal: `upgrade` result is not an Option".into()))?;
                let none_idx = self.variant_index("None", whole_ty).ok_or_else(|| CgErr("internal: `upgrade` result is not an Option".into()))?;
                let out = self.alloca_ty(whole_ty, "upgraded")?;
                let (some_bb, none_bb, merge) = (self.new_block("up_some"), self.new_block("up_none"), self.new_block("up_end"));
                let sp = self.builder.build_struct_gep(objt, obj, 0, "strong")?;
                if arc_kind {
                    let head = self.new_block("up_try");
                    self.builder.build_unconditional_branch(head)?;
                    self.position(head);
                    let cur = self.builder.build_load(ut, sp, "cur")?;
                    if let Some(i) = cur.as_instruction_value() {
                        i.set_atomic_ordering(inkwell::AtomicOrdering::Acquire).map_err(|e| CgErr(format!("internal: {}", e)))?;
                    }
                    let cur = cur.into_int_value();
                    let dead = self.builder.build_int_compare(IntPredicate::EQ, cur, ut.const_zero(), "dead")?;
                    let try_bb = self.new_block("up_cas");
                    self.builder.build_conditional_branch(dead, none_bb, try_bb)?;
                    self.position(try_bb);
                    let next = self.builder.build_int_add(cur, ut.const_int(1, false), "next")?;
                    let r = self.builder.build_cmpxchg(sp, cur, next, inkwell::AtomicOrdering::AcquireRelease, inkwell::AtomicOrdering::Monotonic)?;
                    let won = self.builder.build_extract_value(r, 1, "won")?.into_int_value();
                    self.builder.build_conditional_branch(won, some_bb, head)?;
                } else {
                    let cur = self.builder.build_load(ut, sp, "cur")?.into_int_value();
                    let dead = self.builder.build_int_compare(IntPredicate::EQ, cur, ut.const_zero(), "dead")?;
                    let inc = self.new_block("up_inc");
                    self.builder.build_conditional_branch(dead, none_bb, inc)?;
                    self.position(inc);
                    let next = self.builder.build_int_add(cur, ut.const_int(1, false), "next")?;
                    self.builder.build_store(sp, next)?;
                    self.builder.build_unconditional_branch(some_bb)?;
                }
                self.position(some_bb);
                let v = self.make_enum(whole_ty, some_idx, vec![handle])?;
                self.builder.build_store(out, v)?;
                self.builder.build_unconditional_branch(merge)?;
                self.position(none_bb);
                let v = self.make_enum(whole_ty, none_idx, Vec::new())?;
                self.builder.build_store(out, v)?;
                self.builder.build_unconditional_branch(merge)?;
                self.position(merge);
                self.load(out, whole_ty)
            }
            _ => Err(CgErr(format!("`{}` has no method `{}`", gname, name))),
        }
    }

    /// `Rc::strongCount(borrow r)` / `Rc::weakCount(borrow r)` (also for `Arc`).
    pub(super) fn rc_count(&mut self, arg: &Expr, which: &str) -> R<V<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let ut = self.usize_ty();
        let aty = self.ty_of(arg)?;
        let Ty::Ref(_, pointee) = &aty else { return Err(CgErr("internal: count of a non-reference".into())) };
        let Ty::Generic(_, ga) = &**pointee else { return Err(CgErr("internal: count of a non-Rc".into())) };
        let Some(crate::types::GenericArg::Type(inner)) = ga.first() else { return Err(CgErr("internal: Rc without a type argument".into())) };
        let inner = inner.clone();
        let slot = self.expr(arg)?.into_pointer_value();
        let obj = self.builder.build_load(ptrt, slot, "obj")?.into_pointer_value();
        let objt = self.rc_obj_ty(&inner)?;
        let idx = if which == "strongCount" { 0 } else { 1 };
        let cp = self.builder.build_struct_gep(objt, obj, idx, "count")?;
        let c = self.builder.build_load(ut, cp, "c")?.into_int_value();
        if idx == 1 {
            // the implicit reference is not a `Weak` handle
            return Ok(self.builder.build_int_sub(c, ut.const_int(1, false), "weak")?.into());
        }
        Ok(c.into())
    }

    // ---- Box --------------------------------------------------------------------

    /// `Box::new(value)`: allocate through the one allocation interface and move the value in.
    pub(super) fn box_new(&mut self, inner: &Ty, v: V<'ctx>) -> R<V<'ctx>> {
        let ut = self.usize_ty();
        let lt = self.llty(inner)?;
        let size = self.td.get_abi_size(&lt).max(1);
        let align = (self.td.get_abi_alignment(&lt) as u64).max(1);
        let (fa, _) = self.alloc_fns();
        let call = self.builder.build_call(fa, &[ut.const_int(size, false).into(), ut.const_int(align, false).into()], "box")?;
        let ValueKind::Basic(p) = call.try_as_basic_value() else { return Err(CgErr("internal: alloc result".into())) };
        self.builder.build_store(p.into_pointer_value(), v)?;
        Ok(p)
    }

    /// `A::dealloc(alloc, data, size)` of a user `impl Allocator for A` (the trait method, called statically).
    pub(super) fn call_allocator_dealloc(&mut self, a: &Ty, alloc: PointerValue<'ctx>, data: PointerValue<'ctx>, size: IntValue<'ctx>) -> R<()> {
        let key = self.trait_impl_keys.get(&(a.to_string(), "Allocator".to_string(), "dealloc".to_string())).cloned();
        let Some(k) = key else { return Err(CgErr(format!("`{}` does not implement `Allocator`", a))) };
        let f = self.fns[&k].val;
        self.builder.build_call(f, &[alloc.into(), data.into(), size.into()], "")?;
        Ok(())
    }

    /// `A::alloc(alloc, size, align) -> *mut u8` of a user `impl Allocator for A`; a null result aborts.
    pub(super) fn call_allocator_alloc(&mut self, a: &Ty, alloc: PointerValue<'ctx>, size: IntValue<'ctx>, align: IntValue<'ctx>) -> R<PointerValue<'ctx>> {
        let key = self.trait_impl_keys.get(&(a.to_string(), "Allocator".to_string(), "alloc".to_string())).cloned();
        let Some(k) = key else { return Err(CgErr(format!("`{}` does not implement `Allocator`", a))) };
        let f = self.fns[&k].val;
        let call = self.builder.build_call(f, &[alloc.into(), size.into(), align.into()], "")?;
        let ValueKind::Basic(p) = call.try_as_basic_value() else { return Err(CgErr("internal: allocator result".into())) };
        let p = p.into_pointer_value();
        let isnull = self.builder.build_is_null(p, "oom")?;
        self.panic_if(isnull, "allocator returned a null pointer (out of memory)")?;
        Ok(p)
    }

    /// `String::from(&str)`: a heap copy of the bytes.
    pub(super) fn string_from_str(&mut self, v: V<'ctx>) -> R<V<'ctx>> {
        let ut = self.usize_ty();
        let sv = v.into_struct_value();
        let src = self.builder.build_extract_value(sv, 0, "src")?.into_pointer_value();
        let n = self.builder.build_extract_value(sv, 1, "n")?.into_int_value();
        let is0 = self.builder.build_int_compare(IntPredicate::EQ, n, ut.const_zero(), "is0")?;
        let alloc_n = self.builder.build_select(is0, ut.const_int(1, false), n, "alloc_n")?.into_int_value();
        let (fa, _) = self.alloc_fns();
        let call = self.builder.build_call(fa, &[alloc_n.into(), ut.const_int(1, false).into()], "str")?;
        let ValueKind::Basic(p) = call.try_as_basic_value() else { return Err(CgErr("internal: alloc result".into())) };
        let p = p.into_pointer_value();
        self.builder.build_memcpy(p, 1, src, 1, n)?;
        self.build_handle(p, n, alloc_n)
    }
}

/// The allocator type `A` of `Box<T, A>` (None for a plain `Box<T>`).
pub(super) fn box_alloc_arg(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::Generic(n, a) if n == "Box" && a.len() == 2 => match &a[1] {
            crate::types::GenericArg::Type(t) => Some(t.clone()),
            _ => None,
        },
        _ => None,
    }
}
