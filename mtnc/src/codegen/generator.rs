//! Generators (Phase 12 M2; Document 9 §4, §8.5; Document 13 §8.1).
//!
//! `fn g(..) -> Generator<T>` containing `yield` is compiled to an LLVM *coroutine*
//! (switched-resume ABI). Calling `g` runs only the ramp: it allocates the frame on the heap
//! (through `__mtn_alloc`), copies the parameters into it and suspends before the body
//! (lazy start). A `Generator<T>` value is the frame pointer (the coroutine handle): the frame
//! never moves, every parameter and local lives in it as an ordinary owning slot (zero-slot
//! discipline, `drops.rs`), and the coroutine passes (`coro-split`) turn every stack slot that
//! is live across a `yield` into a frame field.
//!
//! * `yield v` stores `v` in the *promise* (a slot of the frame) and suspends.
//! * `g.next()` / `for x in g` resume the frame and move the promise out: `Some(v)`, or `None`
//!   once the body has finished (a finished generator stays finished).
//! * Dropping a generator calls `coro.destroy`: the code after every suspension point's
//!   *destroy* edge drops exactly the locals that are live there (the same drop frames a
//!   `return` runs), then frees the frame. A body that ran to its end has dropped everything
//!   already; destroying it only frees the frame.
//!
//! Document 9 §4's "enum of resume points plus captured locals" is exactly what `coro-split`
//! builds; using LLVM's implementation keeps every SSA value and every pattern-binding pointer
//! correct across a suspension (the same machinery serves `async fn` in M4).

use super::*;
use inkwell::llvm_sys::core::*;
use inkwell::llvm_sys::prelude::LLVMValueRef;
use inkwell::context::AsContextRef;
use inkwell::values::AsValueRef;
use std::ffi::CString;

pub(super) struct GenCtx<'ctx> {
    pub hdl: PointerValue<'ctx>,
    pub promise: PointerValue<'ctx>,
    pub item: Ty,
    /// the final suspension point (after the body finished or `return;`)
    pub final_bb: BasicBlock<'ctx>,
    /// frees the frame
    pub cleanup_bb: BasicBlock<'ctx>,
    /// `coro.end` + `ret hdl`
    pub susp_bb: BasicBlock<'ctx>,
}

const FRAME_ALIGN: u64 = 16;

impl<'ctx, 'a> Codegen<'ctx, 'a> {
    /// Calls the LLVM intrinsic `name` (token-typed values are raw, inkwell has no wrapper).
    fn coro_call(&mut self, name: &str, overload: &[BasicTypeEnum<'ctx>], args: &[LLVMValueRef]) -> R<LLVMValueRef> {
        let intr = Intrinsic::find(name).ok_or_else(|| CgErr(format!("internal: missing LLVM intrinsic {}", name)))?;
        let decl = intr.get_declaration(&self.module, overload).ok_or_else(|| CgErr(format!("internal: cannot declare {}", name)))?;
        let cname = CString::new("").unwrap();
        // SAFETY: `decl` is the declaration of the intrinsic and `args` match its signature (the
        // callers below pass exactly the operands LLVM 18's LangRef documents for each intrinsic).
        let r = unsafe {
            let fty = LLVMGlobalGetValueType(decl.as_value_ref());
            LLVMBuildCall2(self.builder.as_mut_ptr(), fty, decl.as_value_ref(), args.as_ptr() as *mut LLVMValueRef, args.len() as u32, cname.as_ptr())
        };
        Ok(r)
    }

    fn token_none(&self) -> LLVMValueRef {
        // SAFETY: plain constant creation in this context.
        unsafe { LLVMConstNull(LLVMTokenTypeInContext(self.context.as_ctx_ref())) }
    }

    fn raw_ptr(&self, r: LLVMValueRef) -> PointerValue<'ctx> {
        // SAFETY: `r` is a pointer-typed value just built by `coro_call`.
        unsafe { PointerValue::new(r) }
    }

    fn raw_int(&self, r: LLVMValueRef) -> IntValue<'ctx> {
        // SAFETY: `r` is an integer-typed value just built by `coro_call`.
        unsafe { IntValue::new(r) }
    }

    /// Start of a generator function: promise, `coro.id`, frame allocation, `coro.begin`, and
    /// the shared exit blocks. Leaves the builder in the ramp, after `coro.begin`.
    pub(super) fn gen_prologue(&mut self, item: &Ty) -> R<()> {
        let f = self.cur_fn.unwrap();
        let kind = Attribute::get_named_enum_kind_id("presplitcoroutine");
        f.add_attribute(AttributeLoc::Function, self.context.create_enum_attribute(kind, 0));
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let i32t = self.int_ty(32);
        let i64t = self.int_ty(64);
        let promise = {
            let lt = self.llty(item)?;
            self.alloca(lt, "promise")?
        };
        let null = ptrt.const_null();
        let id = self.coro_call("llvm.coro.id", &[], &[i32t.const_int(0, false).as_value_ref(), promise.as_value_ref(), null.as_value_ref(), null.as_value_ref()])?;
        let size = self.coro_call("llvm.coro.size", &[i64t.into()], &[])?;
        let (fa, _) = self.alloc_fns();
        let mem = self.builder.build_call(fa, &[self.raw_int(size).into(), self.int_ty(self.ptr_bits()).const_int(FRAME_ALIGN, false).into()], "frame")?;
        let ValueKind::Basic(mem) = mem.try_as_basic_value() else { return Err(CgErr("internal: __mtn_alloc returns a value".into())) };
        let hdl_raw = self.coro_call("llvm.coro.begin", &[], &[id, mem.as_value_ref()])?;
        let hdl = self.raw_ptr(hdl_raw);
        let (final_bb, cleanup_bb, susp_bb) = (self.new_block("gen_final"), self.new_block("gen_cleanup"), self.new_block("gen_suspend"));
        let resume_from = self.builder.get_insert_block().unwrap();
        // susp: coro.end; ret hdl
        self.builder.position_at_end(susp_bb);
        let none = self.token_none();
        self.coro_call("llvm.coro.end", &[], &[hdl.as_value_ref(), self.context.bool_type().const_int(0, false).as_value_ref(), none])?;
        self.builder.build_return(Some(&hdl))?;
        // cleanup: free the frame (null when the frame was elided onto a caller's stack)
        self.builder.position_at_end(cleanup_bb);
        let fm = self.coro_call("llvm.coro.free", &[], &[id, hdl.as_value_ref()])?;
        let fm = self.raw_ptr(fm);
        let size2 = self.coro_call("llvm.coro.size", &[i64t.into()], &[])?;
        let (free_bb, done_bb) = (self.new_block("gen_free"), self.new_block("gen_freed"));
        let nn = self.builder.build_is_not_null(fm, "nn")?;
        self.builder.build_conditional_branch(nn, free_bb, done_bb)?;
        self.builder.position_at_end(free_bb);
        self.call_dealloc(fm, self.raw_int(size2), self.int_ty(self.ptr_bits()).const_int(FRAME_ALIGN, false))?;
        self.builder.build_unconditional_branch(done_bb)?;
        self.builder.position_at_end(done_bb);
        self.builder.build_unconditional_branch(susp_bb)?;
        self.builder.position_at_end(resume_from);
        self.gen_ctx = Some(GenCtx { hdl, promise, item: item.clone(), final_bb, cleanup_bb, susp_bb });
        Ok(())
    }

    /// One suspension point. `drop_live`: the destroy edge first drops what is live here.
    /// After a non-final suspension the builder is in the resume block.
    pub(super) fn gen_suspend(&mut self, is_final: bool, drop_live: bool) -> R<()> {
        let (hdl, susp_bb, cleanup_bb) = {
            let g = self.gen_ctx.as_ref().ok_or_else(|| CgErr("internal: generator state missing".into()))?;
            (g.hdl, g.susp_bb, g.cleanup_bb)
        };
        let save = self.coro_call("llvm.coro.save", &[], &[hdl.as_value_ref()])?;
        let fin = self.context.bool_type().const_int(is_final as u64, false);
        let s = self.coro_call("llvm.coro.suspend", &[], &[save, fin.as_value_ref()])?;
        let s = self.raw_int(s);
        let (resume_bb, destroy_bb) = (self.new_block("gen_resume"), self.new_block("gen_destroy"));
        let i8t = self.int_ty(8);
        self.builder.build_switch(s, susp_bb, &[(i8t.const_int(0, false), resume_bb), (i8t.const_int(1, false), destroy_bb)])?;
        self.position(destroy_bb);
        if drop_live {
            self.emit_frames_from(0)?;
        }
        self.builder.build_unconditional_branch(cleanup_bb)?;
        self.position(resume_bb);
        if is_final {
            // resuming a finished coroutine is undefined; `next` checks `coro.done` first
            self.builder.build_unreachable()?;
        }
        Ok(())
    }

    /// End of the body (or `return;`): drop everything live, then the final suspension.
    pub(super) fn gen_finish_body(&mut self) -> R<()> {
        self.emit_frames_from(0)?;
        let fb = self.gen_ctx.as_ref().unwrap().final_bb;
        self.builder.build_unconditional_branch(fb)?;
        self.terminate();
        Ok(())
    }

    /// Fills the final-suspension block (after the whole body was generated).
    pub(super) fn gen_close(&mut self) -> R<()> {
        let fb = self.gen_ctx.as_ref().unwrap().final_bb;
        self.position(fb);
        self.gen_suspend(true, false)?;
        self.gen_ctx = None;
        Ok(())
    }

    /// `yield e;`
    pub(super) fn gen_yield(&mut self, e: &Expr) -> R<()> {
        let (item, promise) = {
            let g = self.gen_ctx.as_ref().ok_or_else(|| CgErr("`yield` outside a generator function".into()))?;
            (g.item.clone(), g.promise)
        };
        let v = self.expr_as(e, &item)?;
        if self.dead {
            return Ok(());
        }
        self.builder.build_store(promise, v)?;
        self.gen_suspend(false, true)
    }

    /// Emits "is there a next item": resumes the frame in `hdl`. Returns `(has, promise_ptr)`;
    /// `promise_ptr` is meaningful only when `has` is true.
    pub(super) fn gen_poll(&mut self, hdl: PointerValue<'ctx>, item: &Ty) -> R<(IntValue<'ctx>, PointerValue<'ctx>)> {
        let (res_bb, none_bb, join) = (self.new_block("gen_poll"), self.new_block("gen_none"), self.new_block("gen_polled"));
        let d0 = self.coro_call("llvm.coro.done", &[], &[hdl.as_value_ref()])?;
        self.builder.build_conditional_branch(self.raw_int(d0), none_bb, res_bb)?;
        self.position(res_bb);
        self.coro_call("llvm.coro.resume", &[], &[hdl.as_value_ref()])?;
        let d1 = self.coro_call("llvm.coro.done", &[], &[hdl.as_value_ref()])?;
        let res_end = self.builder.get_insert_block().unwrap();
        self.builder.build_conditional_branch(self.raw_int(d1), none_bb, join)?;
        self.position(none_bb);
        self.builder.build_unconditional_branch(join)?;
        self.position(join);
        let bt = self.context.bool_type();
        let phi = self.builder.build_phi(bt, "has")?;
        phi.add_incoming(&[(&bt.const_int(0, false), none_bb), (&bt.const_int(1, false), res_end)]);
        let lt = self.llty(item)?;
        let align = self.td.get_abi_alignment(&lt) as u64;
        let pp = self.coro_call("llvm.coro.promise", &[], &[hdl.as_value_ref(), self.int_ty(32).const_int(align, false).as_value_ref(), bt.const_int(0, false).as_value_ref()])?;
        Ok((phi.as_basic_value().into_int_value(), self.raw_ptr(pp)))
    }

    /// `g.next()` -> `Option<T>`; the receiver is lent, never moved.
    pub(super) fn generator_next(&mut self, receiver: &Expr, item: &Ty, whole_ty: &Ty) -> R<V<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let (mut slot, mut t) = self.place_or_spill(receiver)?;
        while let Ty::Ref(_, i) = &t {
            slot = self.builder.build_load(ptrt, slot, "deref")?.into_pointer_value();
            t = (**i).clone();
        }
        let hdl = self.builder.build_load(ptrt, slot, "hdl")?.into_pointer_value();
        let (has, pp) = self.gen_poll(hdl, item)?;
        let some_idx = self.variant_index("Some", whole_ty).ok_or_else(|| CgErr("internal: `next` result is not an Option".into()))?;
        let none_idx = self.variant_index("None", whole_ty).ok_or_else(|| CgErr("internal: `next` result is not an Option".into()))?;
        let out = self.alloca_ty(whole_ty, "next")?;
        let (some_bb, none_bb, merge) = (self.new_block("next_some"), self.new_block("next_none"), self.new_block("next_end"));
        self.builder.build_conditional_branch(has, some_bb, none_bb)?;
        self.position(some_bb);
        let v = self.load(pp, item)?;
        let sv = self.make_enum(whole_ty, some_idx, vec![v])?;
        self.builder.build_store(out, sv)?;
        self.builder.build_unconditional_branch(merge)?;
        self.position(none_bb);
        let nv = self.make_enum(whole_ty, none_idx, vec![])?;
        self.builder.build_store(out, nv)?;
        self.builder.build_unconditional_branch(merge)?;
        self.position(merge);
        self.load(out, whole_ty)
    }

    /// Drop glue of a `Generator<T>` slot: destroy the frame (drops what is live, frees it).
    pub(super) fn generator_glue(&mut self, f: FunctionValue<'ctx>, p: PointerValue<'ctx>) -> R<()> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let hdl = self.builder.build_load(ptrt, p, "hdl")?.into_pointer_value();
        let (go, end) = (self.glue_block(f, "destroy"), self.glue_block(f, "end"));
        let nn = self.builder.build_is_not_null(hdl, "nn")?;
        self.builder.build_conditional_branch(nn, go, end)?;
        self.builder.position_at_end(go);
        self.coro_call("llvm.coro.destroy", &[], &[hdl.as_value_ref()])?;
        self.builder.build_unconditional_branch(end)?;
        self.builder.position_at_end(end);
        Ok(())
    }
}

// ------------------------------------------------------------------------------------------
// `async fn` (Phase 12 M4; Document 12 §3, §11.4)
//
// An `async fn` is compiled exactly like a generator (same coroutine ramp, heap frame that never
// moves, lazy start, drop-on-destroy edge for every live local), with two differences: its promise
// holds the function's *result*, and `await` — instead of `yield` — is how the body suspends.
//
//   await <future>   : loop { if sub is done break; resume sub; if sub is done break; suspend }
//   await <handle>   : loop { if the task is done take its result; else register as waiter; suspend }
//
// "suspend" returns to whoever resumed this frame: the parent's await loop, or the executor
// (`run_task` in the C runtime), which marks the task idle until the runtime wakes it. The frame
// of the sub-future / the join handle is held in a hidden slot that is part of the drop frames, so a
// frame destroyed while suspended drops it too.
// ------------------------------------------------------------------------------------------
impl<'ctx, 'a> Codegen<'ctx, 'a> {
    /// `return v` / the body's tail in an `async fn`: the value is the future's result.
    pub(super) fn async_return(&mut self, v: Option<BasicValueEnum<'ctx>>) -> R<()> {
        if let Some(v) = v {
            let promise = self.gen_ctx.as_ref().ok_or_else(|| CgErr("internal: async return without a frame".into()))?.promise;
            self.builder.build_store(promise, v)?;
        }
        self.gen_finish_body()
    }

    /// Returns `v` from the function being generated (a plain return, or the result of an `async fn`).
    pub(super) fn return_value(&mut self, v: BasicValueEnum<'ctx>) -> R<()> {
        if self.async_item.is_some() {
            // the caller goes on emitting into a reachable block (`?`'s success path): the block
            // `gen_finish_body` left behind is unreachable
            self.async_return(Some(v))?;
            self.builder.build_unreachable()?;
            return Ok(());
        }
        self.emit_frames_from(0)?;
        self.builder.build_return(Some(&v))?;
        Ok(())
    }

    fn require_async(&self, what: &str) -> R<()> {
        if self.gen_ctx.is_none() || self.async_item.is_none() {
            return Err(CgErr(format!("{} outside an `async fn`", what)));
        }
        Ok(())
    }

    /// Runs `body` in a fresh helper function `void/i32 name(ptr, ...)` without disturbing the
    /// function being generated.
    fn with_helper<F>(&mut self, name: &str, fty: inkwell::types::FunctionType<'ctx>, body: F) -> R<FunctionValue<'ctx>>
    where
        F: FnOnce(&mut Self, FunctionValue<'ctx>) -> R<()>,
    {
        if let Some(f) = self.module.get_function(name) {
            return Ok(f);
        }
        let fd = self.module.add_function(name, fty, None);
        let saved = self.builder.get_insert_block();
        let saved_dead = self.dead;
        let entry = self.context.append_basic_block(fd, "entry");
        self.builder.position_at_end(entry);
        let r = body(self, fd);
        if let Some(bb) = saved {
            self.builder.position_at_end(bb);
        }
        self.dead = saved_dead;
        r?;
        Ok(fd)
    }

    /// `i32 __mtn_fut_poll(ptr frame)`: resumes the frame unless it is done; returns 1 when it is done.
    pub(super) fn fut_poll_fn(&mut self) -> R<FunctionValue<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let i32t = self.context.i32_type();
        self.with_helper("__mtn_fut_poll", i32t.fn_type(&[ptrt.into()], false), |cg, fd| {
            let h = fd.get_nth_param(0).unwrap().into_pointer_value();
            let (res_bb, ret1, ret_end) = (cg.context.append_basic_block(fd, "resume"), cg.context.append_basic_block(fd, "done"), cg.context.append_basic_block(fd, "end"));
            let d0 = cg.coro_call("llvm.coro.done", &[], &[h.as_value_ref()])?;
            cg.builder.build_conditional_branch(cg.raw_int(d0), ret1, res_bb)?;
            cg.builder.position_at_end(res_bb);
            cg.coro_call("llvm.coro.resume", &[], &[h.as_value_ref()])?;
            let d1 = cg.coro_call("llvm.coro.done", &[], &[h.as_value_ref()])?;
            let d1 = cg.builder.build_int_z_extend(cg.raw_int(d1), i32t, "d1")?;
            let res_last = cg.builder.get_insert_block().unwrap();
            cg.builder.build_unconditional_branch(ret_end)?;
            cg.builder.position_at_end(ret1);
            cg.builder.build_unconditional_branch(ret_end)?;
            cg.builder.position_at_end(ret_end);
            let phi = cg.builder.build_phi(i32t, "r")?;
            phi.add_incoming(&[(&d1, res_last), (&i32t.const_int(1, false), ret1)]);
            cg.builder.build_return(Some(&phi.as_basic_value()))?;
            Ok(())
        })
    }

    /// `void __mtn_fut_destroy(ptr frame)`: frees a finished frame.
    pub(super) fn fut_destroy_fn(&mut self) -> R<FunctionValue<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        self.with_helper("__mtn_fut_destroy", self.context.void_type().fn_type(&[ptrt.into()], false), |cg, fd| {
            let h = fd.get_nth_param(0).unwrap().into_pointer_value();
            cg.coro_call("llvm.coro.destroy", &[], &[h.as_value_ref()])?;
            cg.builder.build_return(None)?;
            Ok(())
        })
    }

    /// `void __mtn_fut_take_<T>(ptr frame, ptr out)`: moves the finished frame's result out of its promise.
    pub(super) fn fut_take_fn(&mut self, item: &Ty) -> R<FunctionValue<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let name = format!("__mtn_fut_take_{}", item.to_string().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect::<String>());
        let item = item.clone();
        self.with_helper(&name, self.context.void_type().fn_type(&[ptrt.into(), ptrt.into()], false), move |cg, fd| {
            let h = fd.get_nth_param(0).unwrap().into_pointer_value();
            let out = fd.get_nth_param(1).unwrap().into_pointer_value();
            if !is_unit_like(&item) && item != Ty::Never {
                let lt = cg.llty(&item)?;
                let align = cg.td.get_abi_alignment(&lt) as u64;
                let bt = cg.context.bool_type();
                let pp = cg.coro_call("llvm.coro.promise", &[], &[h.as_value_ref(), cg.int_ty(32).const_int(align, false).as_value_ref(), bt.const_int(0, false).as_value_ref()])?;
                let pp = cg.raw_ptr(pp);
                let v = cg.builder.build_load(lt, pp, "result")?;
                cg.builder.build_store(out, v)?;
            }
            cg.builder.build_return(None)?;
            Ok(())
        })
    }

    /// `spawn <future>` -> `JoinHandle<T>`: the frame becomes a task on the pool.
    pub(super) fn gen_spawn_future(&mut self, e: &Expr, item: &Ty) -> R<V<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let hdl = self.expr(e)?;
        let (poll, take, destroy) = (self.fut_poll_fn()?, self.fut_take_fn(item)?, self.fut_destroy_fn()?);
        let (size, rdrop) = self.task_result_info(item)?;
        let ut = self.usize_ty();
        let f = self.rt_decl("__mtn_rt_spawn_future", Some(ptrt.into()), &[ptrt.into(), ptrt.into(), ptrt.into(), ptrt.into(), ut.into(), ptrt.into()]);
        let r = self.builder.build_call(
            f,
            &[
                hdl.into(),
                poll.as_global_value().as_pointer_value().into(),
                take.as_global_value().as_pointer_value().into(),
                destroy.as_global_value().as_pointer_value().into(),
                ut.const_int(size, false).into(),
                rdrop.into(),
            ],
            "task",
        )?;
        let ValueKind::Basic(h) = r.try_as_basic_value() else { return Err(CgErr("internal: spawn result".into())) };
        Ok(h)
    }

    /// (size of the result slot, drop glue for an untaken result or null)
    pub(super) fn task_result_info(&mut self, item: &Ty) -> R<(u64, PointerValue<'ctx>)> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let unit = is_unit_like(item) || *item == Ty::Never;
        let size = if unit {
            0
        } else {
            let lt = self.llty(item)?;
            self.td.get_abi_size(&lt)
        };
        let rdrop = if !unit && self.needs_drop(item) { self.drop_glue(item)?.as_global_value().as_pointer_value() } else { ptrt.const_null() };
        Ok((size, rdrop))
    }

    /// `await e` (Doc 12 §3, §11.4).
    pub(super) fn gen_await(&mut self, inner: &Expr) -> R<V<'ctx>> {
        self.require_async("`await`")?;
        let ity = self.ty_of(inner)?;
        if let Some(u) = crate::types::future_item(&ity) {
            self.await_future(inner, &ity, &u)
        } else if let Some(u) = crate::types::join_handle_item(&ity) {
            self.await_handle(inner, &ity, &u)
        } else {
            Err(CgErr(format!("internal: `await` of a value of type `{}`", ity)))
        }
    }

    fn await_future(&mut self, inner: &Expr, ity: &Ty, item: &Ty) -> R<V<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        self.push_scope();
        let v = self.expr(inner)?;
        let slot = self.declare_local("$await", ity)?;
        self.builder.build_store(slot, v)?;
        let (head, resume, pend, ready) = (self.new_block("await_head"), self.new_block("await_resume"), self.new_block("await_pending"), self.new_block("await_ready"));
        self.builder.build_unconditional_branch(head)?;
        self.position(head);
        let hdl = self.builder.build_load(ptrt, slot, "sub")?.into_pointer_value();
        let d0 = self.coro_call("llvm.coro.done", &[], &[hdl.as_value_ref()])?;
        self.builder.build_conditional_branch(self.raw_int(d0), ready, resume)?;
        self.position(resume);
        self.coro_call("llvm.coro.resume", &[], &[hdl.as_value_ref()])?;
        let d1 = self.coro_call("llvm.coro.done", &[], &[hdl.as_value_ref()])?;
        self.builder.build_conditional_branch(self.raw_int(d1), ready, pend)?;
        // the sub-future is not done: this frame suspends too; when resumed it polls the sub-future again
        self.position(pend);
        self.gen_suspend(false, true)?;
        self.builder.build_unconditional_branch(head)?;
        self.position(ready);
        let out = if is_unit_like(item) || *item == Ty::Never {
            self.unit()
        } else {
            let lt = self.llty(item)?;
            let align = self.td.get_abi_alignment(&lt) as u64;
            let bt = self.context.bool_type();
            let pp = self.coro_call("llvm.coro.promise", &[], &[hdl.as_value_ref(), self.int_ty(32).const_int(align, false).as_value_ref(), bt.const_int(0, false).as_value_ref()])?;
            let pp = self.raw_ptr(pp);
            self.builder.build_load(lt, pp, "awaited")?
        };
        // the finished frame only has to be freed; the hidden slot is cleared so scope exit drops nothing
        self.coro_call("llvm.coro.destroy", &[], &[hdl.as_value_ref()])?;
        self.builder.build_store(slot, ptrt.const_null())?;
        self.pop_scope(true)?;
        Ok(out)
    }

    fn await_handle(&mut self, inner: &Expr, ity: &Ty, item: &Ty) -> R<V<'ctx>> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let i32t = self.context.i32_type();
        self.push_scope();
        let v = self.expr(inner)?;
        let slot = self.declare_local("$await", ity)?;
        self.builder.build_store(slot, v)?;
        let unit = is_unit_like(item) || *item == Ty::Never;
        let out = if unit { ptrt.const_null() } else { self.alloca_ty(item, "awaited")? };
        let (head, pend, ready) = (self.new_block("awaitj_head"), self.new_block("awaitj_pending"), self.new_block("awaitj_ready"));
        self.builder.build_unconditional_branch(head)?;
        self.position(head);
        let h = self.builder.build_load(ptrt, slot, "task")?;
        let f = self.rt_decl("__mtn_rt_await_join", Some(i32t.into()), &[ptrt.into(), ptrt.into()]);
        let r = self.builder.build_call(f, &[h.into(), out.into()], "r")?;
        let ValueKind::Basic(r) = r.try_as_basic_value() else { return Err(CgErr("internal: await_join".into())) };
        let done = self.builder.build_int_compare(inkwell::IntPredicate::NE, r.into_int_value(), i32t.const_zero(), "done")?;
        self.builder.build_conditional_branch(done, ready, pend)?;
        self.position(pend);
        self.gen_suspend(false, true)?;
        self.builder.build_unconditional_branch(head)?;
        self.position(ready);
        // the runtime released the task: nothing is left for the hidden slot to drop
        self.builder.build_store(slot, ptrt.const_null())?;
        let res = if unit { self.unit() } else { self.load(out, item)? };
        self.pop_scope(true)?;
        Ok(res)
    }

    /// `__await_yield()` (the body of the prelude `yieldNow`): requeue this task behind the others.
    pub(super) fn gen_await_yield(&mut self) -> R<V<'ctx>> {
        self.require_async("`yieldNow`")?;
        let f = self.rt_decl("__mtn_rt_yield", None, &[]);
        self.builder.build_call(f, &[], "")?;
        self.gen_suspend(false, true)?;
        Ok(self.unit())
    }

    /// `__await_sleep(ms)` (the body of the prelude `sleepMs`): the task is woken by the runtime's timer.
    pub(super) fn gen_await_sleep(&mut self, arg: &Expr) -> R<V<'ctx>> {
        self.require_async("`sleepMs`")?;
        let i64t = self.context.i64_type();
        let i32t = self.context.i32_type();
        let i8t = self.context.i8_type();
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let ms = self.expr_as(arg, &Ty::U64)?;
        let dl = self.alloca(i64t.into(), "deadline")?;
        let reg = self.alloca(i8t.into(), "registered")?;
        let fd = self.rt_decl("__mtn_rt_deadline_ns", Some(i64t.into()), &[i64t.into()]);
        let d = self.builder.build_call(fd, &[ms.into()], "dl")?;
        let ValueKind::Basic(d) = d.try_as_basic_value() else { return Err(CgErr("internal: deadline".into())) };
        self.builder.build_store(dl, d)?;
        self.builder.build_store(reg, i8t.const_zero())?;
        let (head, pend, done) = (self.new_block("sleep_head"), self.new_block("sleep_pending"), self.new_block("sleep_done"));
        self.builder.build_unconditional_branch(head)?;
        self.position(head);
        let dv = self.builder.build_load(i64t, dl, "d")?;
        let f = self.rt_decl("__mtn_rt_sleep_wait", Some(i32t.into()), &[i64t.into(), ptrt.into()]);
        let r = self.builder.build_call(f, &[dv.into(), reg.into()], "r")?;
        let ValueKind::Basic(r) = r.try_as_basic_value() else { return Err(CgErr("internal: sleep_wait".into())) };
        let elapsed = self.builder.build_int_compare(inkwell::IntPredicate::NE, r.into_int_value(), i32t.const_zero(), "elapsed")?;
        self.builder.build_conditional_branch(elapsed, done, pend)?;
        self.position(pend);
        self.gen_suspend(false, true)?;
        self.builder.build_unconditional_branch(head)?;
        self.position(done);
        Ok(self.unit())
    }
}
