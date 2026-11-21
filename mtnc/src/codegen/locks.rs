//! Locks (Phase 13 M2; Document 14 §6, §7, §9.1, §9.5): `Spinlock`, `Mutex<T>`, `RwLock<T>` and their guards.
//!
//! Layout. `Mutex<T>` and `RwLock<T>` are `{ [8 x i32] state, T value }`: 32 bytes of zero-initialised lock
//! words (the runtime's `mtn_lk`, `runtime/mtn_rt.c`) followed by the protected value. A `Spinlock` is one `i32`.
//! A lock is therefore a plain value: it can be a `static`, a struct field, or live inside an `Arc`.
//!
//! A guard is ONE POINTER to the lock it came from (null = moved out). Its drop glue (drops.rs) calls the
//! runtime's unlock; it is internal glue, not a user `Drop` impl (a guard has no field to hold a hidden
//! "live" byte, and a null pointer already says "moved"). The guard's value is at field 1 of the lock.
use super::*;

impl<'ctx, 'a> Codegen<'ctx, 'a> {
    /// The `{ [8 x i32], T }` object of a `Mutex<T>` / `RwLock<T>`.
    pub(super) fn lock_obj_ty(&mut self, elem: &Ty) -> R<StructType<'ctx>> {
        let state = self.int_ty(32).array_type(8);
        let v = self.llty(elem)?;
        Ok(self.context.struct_type(&[state.into(), v], false))
    }

    /// Pointer to the protected value, given the pointer a guard holds.
    pub(super) fn guard_value_ptr(&mut self, guard_ty: &Ty, p: PointerValue<'ctx>) -> R<PointerValue<'ctx>> {
        let Some(elem) = crate::locks::elem_of(guard_ty) else { return Err(CgErr(format!("internal: `{}` is not a guard with a value", guard_ty))) };
        let obj = self.lock_obj_ty(&elem)?;
        Ok(self.builder.build_struct_gep(obj, p, 1, "lockval")?)
    }

    /// The runtime entry that releases the lock a guard of type `guard_ty` holds.
    pub(super) fn guard_release_fn(&mut self, guard_ty: &Ty) -> FunctionValue<'ctx> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let name = match guard_ty {
            Ty::Generic(n, _) if n == "MutexGuard" => "__mtn_lock_mutex_unlock",
            Ty::Generic(n, _) if n == "RwLockReadGuard" => "__mtn_lock_rw_read_unlock",
            Ty::Generic(n, _) if n == "RwLockWriteGuard" => "__mtn_lock_rw_write_unlock",
            _ => "__mtn_lock_spin_release",
        };
        self.rt_decl(name, None, &[ptrt.into()])
    }

    /// Drop glue of a guard: if the pointer is not null (not moved out), release the lock.
    pub(super) fn guard_glue(&mut self, f: FunctionValue<'ctx>, p: PointerValue<'ctx>, ty: &Ty) -> R<()> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let lock = self.builder.build_load(ptrt, p, "lock")?.into_pointer_value();
        let (rel, end) = (self.glue_block(f, "release"), self.glue_block(f, "end"));
        let nn = self.builder.build_is_not_null(lock, "held")?;
        self.builder.build_conditional_branch(nn, rel, end)?;
        self.builder.position_at_end(rel);
        let r = self.guard_release_fn(ty);
        self.builder.build_call(r, &[lock.into()], "")?;
        self.builder.build_unconditional_branch(end)?;
        self.builder.position_at_end(end);
        Ok(())
    }

    /// `Mutex::new(v)`, `RwLock::new(v)`, `Spinlock::new()`.
    pub(super) fn lock_new(&mut self, ty: &Ty, args: &[Arg]) -> R<V<'ctx>> {
        match ty {
            Ty::Named(_) => Ok(self.int_ty(32).const_zero().into()),
            _ => {
                let elem = crate::locks::elem_of(ty).ok_or_else(|| CgErr("internal: lock without an element type".into()))?;
                let obj = self.lock_obj_ty(&elem)?;
                let v = self.expr_as(&args[0].value, &elem)?;
                let state = self.int_ty(32).array_type(8).const_zero();
                let mut agg = obj.get_undef();
                agg = self.builder.build_insert_value(agg, state, 0, "state")?.into_struct_value();
                agg = self.builder.build_insert_value(agg, v, 1, "value")?.into_struct_value();
                Ok(agg.into())
            }
        }
    }

    /// `m.lock()`, `m.tryLock()`, `l.read()`, `l.write()`, `l.tryRead()`, `l.tryWrite()`, `s.acquire()`.
    /// `lock` is the (peeled) type of the lock, `call_ty` the type of the whole call.
    pub(super) fn lock_method(&mut self, receiver: &Expr, rty: &Ty, name: &str, call_ty: &Ty) -> R<V<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        // the address of the lock: through references, `Box`, `Rc`, `Arc`
        let mut boxed = false;
        let mut t = rty.clone();
        loop {
            match t {
                Ty::Ref(_, i) => t = *i,
                Ty::Generic(ref n, ref a) if matches!(n.as_str(), "Box" | "Rc" | "Arc") && !a.is_empty() => {
                    boxed = true;
                    match &a[0] {
                        crate::types::GenericArg::Type(x) => t = x.clone(),
                        _ => break,
                    }
                }
                _ => break,
            }
        }
        let addr: PointerValue<'ctx> = if boxed {
            let (p, t0) = self.place_or_spill(receiver)?;
            let (mut p, mut t0) = (p, t0);
            while let Ty::Ref(_, i) = &t0 {
                p = self.builder.build_load(ptrt, p, "deref")?.into_pointer_value();
                t0 = (**i).clone();
            }
            self.peel_box_place(p, t0)?.0
        } else if matches!(rty, Ty::Ref(..)) {
            let mut v = self.expr(receiver)?;
            let mut cur = rty.clone();
            while let Ty::Ref(_, i) = &cur {
                if matches!(**i, Ty::Ref(..)) {
                    v = self.builder.build_load(ptrt, v.into_pointer_value(), "deref")?;
                    cur = (**i).clone();
                } else {
                    break;
                }
            }
            v.into_pointer_value()
        } else {
            self.place_or_spill(receiver)?.0
        };
        let (blocking, try_, rt) = match name {
            "lock" => (true, false, "__mtn_lock_mutex_lock"),
            "tryLock" => (false, true, "__mtn_lock_mutex_trylock"),
            "read" => (true, false, "__mtn_lock_rw_read"),
            "write" => (true, false, "__mtn_lock_rw_write"),
            "tryRead" => (false, true, "__mtn_lock_rw_try_read"),
            "tryWrite" => (false, true, "__mtn_lock_rw_try_write"),
            "acquire" => (true, false, "__mtn_lock_spin_acquire"),
            other => return Err(CgErr(format!("internal: lock method `{}`", other))),
        };
        let _ = blocking;
        if !try_ {
            let f = self.rt_decl(rt, None, &[ptrt.into()]);
            self.builder.build_call(f, &[addr.into()], "")?;
            // the guard IS the address of the lock
            return Ok(addr.into());
        }
        let i32t = self.int_ty(32);
        let f = self.rt_decl(rt, Some(i32t.into()), &[ptrt.into()]);
        let ValueKind::Basic(r) = self.builder.build_call(f, &[addr.into()], "trylock")?.try_as_basic_value() else { return Err(CgErr("internal: trylock returns nothing".into())) };
        let r = r.into_int_value();
        let ok = self.builder.build_int_compare(IntPredicate::NE, r, i32t.const_zero(), "got")?;
        let some_idx = self.variant_index("Some", call_ty).ok_or_else(|| CgErr("internal: tryLock result is not an Option".into()))?;
        let none_idx = self.variant_index("None", call_ty).ok_or_else(|| CgErr("internal: tryLock result is not an Option".into()))?;
        let somev = self.make_enum(call_ty, some_idx, vec![addr.into()])?;
        let nonev = self.make_enum(call_ty, none_idx, vec![])?;
        Ok(self.builder.build_select(ok, somev, nonev, "try_res")?)
    }
}
