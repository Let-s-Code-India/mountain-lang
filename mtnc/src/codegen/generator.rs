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
