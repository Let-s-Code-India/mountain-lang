//! Channels (Phase 13 M3; Document 12 §6, §11.13; Document 14 §3, §9.6): `channel::<T>(n)`,
//! `channelUnbounded::<T>()`, and the methods of `Sender<T>` / `Receiver<T>`.
//!
//! A handle is ONE POINTER to the runtime's channel object (null = moved out); its drop glue counts the handle
//! out of the channel. A message is MOVED into the queue slot: the runtime gets a pointer to a stack copy, and
//! on success the copy is the queue's. When a send fails (every receiver gone) the value stays with the caller,
//! who hands it back inside `Err(v)`, so a `Drop` payload is dropped exactly once on every path.
use super::*;

impl<'ctx, 'a> Codegen<'ctx, 'a> {
    /// Drop glue of a handle: if the pointer is not null (not moved out), count it out of the channel.
    pub(super) fn chan_handle_glue(&mut self, f: FunctionValue<'ctx>, p: PointerValue<'ctx>, ty: &Ty) -> R<()> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let h = self.builder.build_load(ptrt, p, "chan")?.into_pointer_value();
        let (rel, end) = (self.glue_block(f, "release"), self.glue_block(f, "end"));
        let nn = self.builder.build_is_not_null(h, "live")?;
        self.builder.build_conditional_branch(nn, rel, end)?;
        self.builder.position_at_end(rel);
        let name = if crate::channels::is_sender(ty) { "__mtn_chan_sender_drop" } else { "__mtn_chan_receiver_drop" };
        let r = self.rt_decl(name, None, &[ptrt.into()]);
        self.builder.build_call(r, &[h.into()], "")?;
        self.builder.build_unconditional_branch(end)?;
        self.builder.position_at_end(end);
        Ok(())
    }

    /// The element's drop function for the runtime (null when the type owns nothing).
    fn chan_elem_drop(&mut self, elem: &Ty) -> R<PointerValue<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        if self.needs_drop(elem) && *elem != Ty::Never {
            Ok(self.drop_glue(elem)?.as_global_value().as_pointer_value())
        } else {
            Ok(ptrt.const_null())
        }
    }

    /// `channel::<T>()`, `channel::<T>(n)`, `channelUnbounded::<T>()`: a `(Sender<T>, Receiver<T>)`.
    pub(super) fn chan_new(&mut self, whole: &Expr, unbounded: bool, args: &[Arg]) -> R<V<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let i64t = self.context.i64_type();
        let tuple_ty = self.ty_of(whole)?;
        let Ty::Tuple(parts) = &tuple_ty else { return Err(CgErr("internal: `channel` without a handle pair type".into())) };
        let elem = crate::channels::elem_of(&parts[0]).ok_or_else(|| CgErr("internal: `channel` without a message type".into()))?;
        let (esize, ealign) = if is_unit_like(&elem) || elem == Ty::Never {
            (0, 1)
        } else {
            let lt = self.llty(&elem)?;
            (self.td.get_abi_size(&lt), self.td.get_abi_alignment(&lt) as u64)
        };
        let drop = self.chan_elem_drop(&elem)?;
        let cap: V<'ctx> = if unbounded {
            i64t.const_zero().into()
        } else if let Some(a) = args.first() {
            let aty = self.ty_of(&a.value)?;
            let v = self.expr_as(&a.value, &aty)?.into_int_value();
            let signed = matches!(aty, Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::I128 | Ty::Isize);
            let bits = v.get_type().get_bit_width();
            let v64 = if bits == 64 {
                v
            } else if bits > 64 {
                self.builder.build_int_truncate(v, i64t, "cap")?
            } else if signed {
                self.builder.build_int_s_extend(v, i64t, "cap")?
            } else {
                self.builder.build_int_z_extend(v, i64t, "cap")?
            };
            // `n == 0` (or negative) is a program error: a channel holds at least one message
            let bad = self.builder.build_int_compare(IntPredicate::SLE, v64, i64t.const_zero(), "cap_bad")?;
            self.panic_if(bad, "channel capacity must be at least 1")?;
            v64.into()
        } else {
            i64t.const_int(1024, false).into()
        };
        let f = self.rt_decl("__mtn_chan_new", Some(ptrt.into()), &[i64t.into(), i64t.into(), ptrt.into(), i64t.into()]);
        let c = self.builder.build_call(f, &[i64t.const_int(esize, false).into(), i64t.const_int(ealign, false).into(), drop.into(), cap.into()], "chan")?;
        let ValueKind::Basic(h) = c.try_as_basic_value() else { return Err(CgErr("internal: chan_new returns nothing".into())) };
        // the first handle of each side is the creation itself; the runtime starts with one of each
        let lt = self.llty(&tuple_ty)?.into_struct_type();
        let mut agg = lt.get_undef();
        agg = self.builder.build_insert_value(agg, h, 0, "tx")?.into_struct_value();
        agg = self.builder.build_insert_value(agg, h, 1, "rx")?.into_struct_value();
        Ok(agg.into())
    }

    /// The address of the handle behind `receiver` (peeling references, `Box`, `Rc`, `Arc`) and its handle type.
    fn chan_handle_value(&mut self, receiver: &Expr, rty: &Ty) -> R<(PointerValue<'ctx>, Ty)> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let mut t = rty.clone();
        let mut boxed = false;
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
        let handle_ty = t;
        // `addr` is the address of the handle slot (the pointer to the channel lives there)
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
        let h = self.builder.build_load(ptrt, addr, "handle")?.into_pointer_value();
        Ok((h, handle_ty))
    }

    /// `tx.send(v)`, `tx.trySend(v)`, `rx.recv()`, `rx.tryRecv()`, `tx.clone()`, `rx.clone()` (blocking forms outside
    /// `async fn`; the `await` forms are `chan_await`).
    pub(super) fn chan_method(&mut self, receiver: &Expr, rty: &Ty, name: &str, args: &[Arg], call_ty: &Ty) -> R<V<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let i32t = self.context.i32_type();
        let (h, hty) = self.chan_handle_value(receiver, rty)?;
        let elem = crate::channels::elem_of(&hty).ok_or_else(|| CgErr("internal: channel method without a message type".into()))?;
        match name {
            "clone" => {
                let f = if crate::channels::is_sender(&hty) { "__mtn_chan_sender_clone" } else { "__mtn_chan_receiver_clone" };
                let fv = self.rt_decl(f, Some(ptrt.into()), &[ptrt.into()]);
                let c = self.builder.build_call(fv, &[h.into()], "cl")?;
                let ValueKind::Basic(v) = c.try_as_basic_value() else { return Err(CgErr("internal: clone returns nothing".into())) };
                Ok(v)
            }
            "send" | "trySend" => {
                let rt = if name == "send" { "__mtn_chan_send" } else { "__mtn_chan_try_send" };
                let slot = self.chan_msg_slot(&args[0].value, &elem)?;
                let fv = self.rt_decl(rt, Some(i32t.into()), &[ptrt.into(), ptrt.into()]);
                let c = self.builder.build_call(fv, &[h.into(), slot.into()], "sent")?;
                let ValueKind::Basic(r) = c.try_as_basic_value() else { return Err(CgErr("internal: send returns nothing".into())) };
                self.send_result(r.into_int_value(), slot, &elem, call_ty)
            }
            "recv" | "tryRecv" => {
                let rt = if name == "recv" { "__mtn_chan_recv" } else { "__mtn_chan_try_recv" };
                let out = self.chan_out_slot(&elem)?;
                let fv = self.rt_decl(rt, Some(i32t.into()), &[ptrt.into(), ptrt.into()]);
                let c = self.builder.build_call(fv, &[h.into(), out.into()], "got")?;
                let ValueKind::Basic(r) = c.try_as_basic_value() else { return Err(CgErr("internal: recv returns nothing".into())) };
                self.recv_result(r.into_int_value(), out, &elem, call_ty)
            }
            other => Err(CgErr(format!("internal: channel method `{}`", other))),
        }
    }

    /// Evaluates a message into a fresh stack slot and returns the slot's address.
    fn chan_msg_slot(&mut self, e: &Expr, elem: &Ty) -> R<PointerValue<'ctx>> {
        let v = self.expr_as(e, elem)?;
        let slot = self.alloca_ty(elem, "msg")?;
        if !(is_unit_like(elem) || *elem == Ty::Never) {
            self.builder.build_store(slot, v)?;
        }
        Ok(slot)
    }

    /// A stack slot a received message is written to.
    fn chan_out_slot(&mut self, elem: &Ty) -> R<PointerValue<'ctx>> {
        self.alloca_ty(elem, "recvd")
    }

    /// `Ok(())` when the runtime took the message (0), else `Err(v)` with the value still in `slot`.
    fn send_result(&mut self, r: IntValue<'ctx>, slot: PointerValue<'ctx>, elem: &Ty, call_ty: &Ty) -> R<V<'ctx>> {
        let i32t = self.context.i32_type();
        let ok = self.builder.build_int_compare(IntPredicate::EQ, r, i32t.const_zero(), "was_sent")?;
        let ok_idx = self.variant_index("Ok", call_ty).ok_or_else(|| CgErr("internal: send result is not a Result".into()))?;
        let err_idx = self.variant_index("Err", call_ty).ok_or_else(|| CgErr("internal: send result is not a Result".into()))?;
        let unit = is_unit_like(elem) || *elem == Ty::Never;
        let back = if unit { self.unit() } else { self.load(slot, elem)? };
        let okv = self.make_enum(call_ty, ok_idx, vec![])?;
        let errv = self.make_enum(call_ty, err_idx, vec![back])?;
        Ok(self.builder.build_select(ok, okv, errv, "send_res")?)
    }

    /// `Some(v)` when the runtime wrote a message (1), else `None`.
    fn recv_result(&mut self, r: IntValue<'ctx>, out: PointerValue<'ctx>, elem: &Ty, call_ty: &Ty) -> R<V<'ctx>> {
        let i32t = self.context.i32_type();
        let got = self.builder.build_int_compare(IntPredicate::NE, r, i32t.const_zero(), "got_msg")?;
        let some_idx = self.variant_index("Some", call_ty).ok_or_else(|| CgErr("internal: recv result is not an Option".into()))?;
        let none_idx = self.variant_index("None", call_ty).ok_or_else(|| CgErr("internal: recv result is not an Option".into()))?;
        let unit = is_unit_like(elem) || *elem == Ty::Never;
        let v = if unit { self.unit() } else { self.load(out, elem)? };
        let somev = self.make_enum(call_ty, some_idx, vec![v])?;
        let nonev = self.make_enum(call_ty, none_idx, vec![])?;
        Ok(self.builder.build_select(got, somev, nonev, "recv_res")?)
    }

    /// `await tx.send(v)` / `await rx.recv()` inside an `async fn`: the frame suspends (and holds no worker)
    /// until the runtime can complete the operation.
    pub(super) fn chan_await(&mut self, receiver: &Expr, name: &str, args: &[Arg], call_ty: &Ty) -> R<V<'ctx>> {
        self.require_async("`await` of a channel operation")?;
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let i32t = self.context.i32_type();
        let rty = self.ty_of(receiver)?;
        let (h, hty) = self.chan_handle_value(receiver, &rty)?;
        let elem = crate::channels::elem_of(&hty).ok_or_else(|| CgErr("internal: channel await without a message type".into()))?;
        // the channel pointer must survive suspension: keep it in the frame
        let hslot = self.alloca(ptrt.into(), "chan_h")?;
        self.builder.build_store(hslot, h)?;
        let is_send = name == "send";
        let slot = if is_send { self.chan_msg_slot(&args[0].value, &elem)? } else { self.chan_out_slot(&elem)? };
        let (head, pend, done) = (self.new_block("chan_head"), self.new_block("chan_pending"), self.new_block("chan_done"));
        self.builder.build_unconditional_branch(head)?;
        self.position(head);
        let hv = self.builder.build_load(ptrt, hslot, "h")?;
        let rt = if is_send { "__mtn_chan_send_poll" } else { "__mtn_chan_recv_poll" };
        let fv = self.rt_decl(rt, Some(i32t.into()), &[ptrt.into(), ptrt.into()]);
        let c = self.builder.build_call(fv, &[hv.into(), slot.into()], "r")?;
        let ValueKind::Basic(r) = c.try_as_basic_value() else { return Err(CgErr("internal: chan poll returns nothing".into())) };
        let r = r.into_int_value();
        let ready = self.builder.build_int_compare(IntPredicate::NE, r, i32t.const_int(2, false), "ready")?;
        self.builder.build_conditional_branch(ready, done, pend)?;
        self.position(pend);
        self.gen_suspend(false, true)?;
        self.builder.build_unconditional_branch(head)?;
        self.position(done);
        if is_send {
            self.send_result(r, slot, &elem, call_ty)
        } else {
            self.recv_result(r, slot, &elem, call_ty)
        }
    }
}
