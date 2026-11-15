//! Actors (Phase 12 M5; Document 12 §8, §11.5).
//!
//! The front end (`desugar::desugar_actors`) has already turned an `actor` into ordinary items: a
//! private state struct, one handler method per `on`, a message enum named like the actor and a
//! dispatch function `__actor_dispatch_A(borrow mut state, msg)`. What is left for code generation:
//!
//!  * `__actor_new(state, __actor_dispatch_A)` (the body of `A::spawn()`): asks the runtime for an
//!    actor (`__mtn_rt_actor_new`, which owns the state block and the mailbox), stores the evaluated
//!    initial state into the state block and returns the runtime's actor pointer, which IS the
//!    `ActorRef<A>` (a plain, copyable pointer: actors live until the process exits).
//!  * `r.send(msg)`: the message value is moved into a stack slot and copied into the mailbox by
//!    `__mtn_rt_actor_send`; the runtime later calls the per-actor *thunk* generated here
//!    (`void thunk(ptr state, ptr msg)`: load the message, call the dispatch function, which owns it).
//!  * The state's drop glue is handed to the runtime, which runs it once at shutdown.

use super::*;

impl<'ctx, 'a> Codegen<'ctx, 'a> {
    /// `void thunk(ptr state, ptr msg)` for actor `actor` (message type `msg_ty`).
    fn actor_thunk(&mut self, actor: &str, dispatch: &str, msg_ty: &Ty) -> R<FunctionValue<'ctx>> {
        let key = format!("actor:{}", actor);
        if let Some(t) = self.task_thunks.get(&key) {
            return Ok(*t);
        }
        let Some(dv) = self.fns.get(dispatch).map(|f| f.val) else {
            return Err(CgErr(format!("internal: the dispatch function `{}` of actor `{}` is missing", dispatch, actor)));
        };
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let fd = self.module.add_function(&format!("__mtn_actor_thunk_{}", actor), self.context.void_type().fn_type(&[ptrt.into(), ptrt.into()], false), None);
        self.task_thunks.insert(key, fd);
        let saved = self.builder.get_insert_block();
        let saved_dead = self.dead;
        let entry = self.context.append_basic_block(fd, "entry");
        self.builder.position_at_end(entry);
        let (st, msg) = (fd.get_nth_param(0).unwrap().into_pointer_value(), fd.get_nth_param(1).unwrap().into_pointer_value());
        let r = (|| -> R<()> {
            let m = self.load(msg, msg_ty)?;
            self.builder.build_call(dv, &[st.into(), m.into()], "")?;
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

    /// `__actor_new(state, __actor_dispatch_A)` -> `ActorRef<A>`.
    pub(super) fn gen_actor_new(&mut self, args: &[Arg]) -> R<V<'ctx>> {
        if args.len() != 2 {
            return Err(CgErr("internal: `__actor_new` takes the state and the dispatch function".into()));
        }
        let st_ty = self.ty_of(&args[0].value)?;
        let Ty::Named(sn) = &st_ty else { return Err(CgErr("internal: `__actor_new` needs an actor state".into())) };
        let Some(actor) = sn.strip_prefix("__ActorState_").map(|s| s.to_string()) else {
            return Err(CgErr("internal: `__actor_new` needs an actor state".into()));
        };
        let Expr::Ident(dispatch) = &args[1].value else { return Err(CgErr("internal: `__actor_new` needs the dispatch function by name".into())) };
        let msg_ty = Ty::Named(actor.clone());
        let thunk = self.actor_thunk(&actor, dispatch, &msg_ty)?;
        let ut = self.usize_ty();
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let st_llt = self.llty(&st_ty)?;
        let msg_llt = self.llty(&msg_ty)?;
        let (st_size, msg_size) = (self.td.get_abi_size(&st_llt), self.td.get_abi_size(&msg_llt));
        let sdrop = if self.needs_drop(&st_ty) { self.drop_glue(&st_ty)?.as_global_value().as_pointer_value() } else { ptrt.const_null() };
        let init = self.expr_as(&args[0].value, &st_ty)?;
        let f = self.rt_decl("__mtn_rt_actor_new", Some(ptrt.into()), &[ptrt.into(), ptrt.into(), ut.into(), ut.into()]);
        let a = self
            .builder
            .build_call(f, &[thunk.as_global_value().as_pointer_value().into(), sdrop.into(), ut.const_int(st_size.max(1), false).into(), ut.const_int(msg_size.max(1), false).into()], "actor")?;
        let ValueKind::Basic(a) = a.try_as_basic_value() else { return Err(CgErr("internal: actor_new result".into())) };
        let g = self.rt_decl("__mtn_rt_actor_state", Some(ptrt.into()), &[ptrt.into()]);
        let sp = self.builder.build_call(g, &[a.into()], "state")?;
        let ValueKind::Basic(sp) = sp.try_as_basic_value() else { return Err(CgErr("internal: actor_state result".into())) };
        self.builder.build_store(sp.into_pointer_value(), init)?;
        Ok(a)
    }

    /// `r.send(msg)`: moves the message into the actor's mailbox; never blocks.
    pub(super) fn gen_actor_send(&mut self, receiver: &Expr, arg: &Expr, msg_ty: &Ty) -> R<V<'ctx>> {
        let mut rt = self.ty_of(receiver)?;
        let mut r = self.expr(receiver)?;
        while let Ty::Ref(_, inner) = rt {
            r = self.load(r.into_pointer_value(), &inner)?;
            rt = *inner;
        }
        let m = self.expr_as(arg, msg_ty)?;
        let slot = self.alloca_ty(msg_ty, "msg")?;
        self.builder.build_store(slot, m)?;
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let f = self.rt_decl("__mtn_rt_actor_send", None, &[ptrt.into(), ptrt.into()]);
        self.builder.build_call(f, &[r.into(), slot.into()], "")?;
        Ok(self.unit())
    }
}
