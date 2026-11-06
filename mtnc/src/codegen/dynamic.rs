//! `dyn Trait` (Phase 11c C5; Document 13 §1 point 4, Document 7): fat pointers and vtables.
//!
//! `Box<dyn Trait>` and `&dyn Trait` are a fat pointer `{ data: ptr, vtable: ptr }`. The vtable of
//! a (concrete type, trait) pair is one constant `{ drop: ptr, size: usize, align: usize, m0: ptr,
//! m1: ptr, .. }` where `drop` is the concrete type's drop glue (null when it owns nothing),
//! `size`/`align` let the box free the allocation without knowing the type, and `m0..` are the
//! implementing functions in the trait's declaration order (null for methods that cannot be
//! called through a trait object: by-value `self`, no `self`, own generics).

use super::*;

pub(super) struct TraitMethod {
    pub name: String,
    pub self_kind: Option<PMode>,
    pub params: Vec<(String, Ty, PMode)>,
    pub ret: Ty,
    pub generic: bool,
}

/// First vtable slot used by a trait method.
const FIRST_METHOD_SLOT: u32 = 3;

impl<'ctx, 'a> Codegen<'ctx, 'a> {
    pub(super) fn fat_ty(&self) -> StructType<'ctx> {
        let p = self.context.ptr_type(AddressSpace::default());
        self.context.struct_type(&[p.into(), p.into()], false)
    }

    fn vtable_ty(&self, methods: usize) -> StructType<'ctx> {
        let p = self.context.ptr_type(AddressSpace::default());
        let u = self.usize_ty();
        let mut fs: Vec<BasicTypeEnum<'ctx>> = vec![p.into(), u.into(), u.into()];
        for _ in 0..methods {
            fs.push(p.into());
        }
        self.context.struct_type(&fs, false)
    }

    fn build_fat(&mut self, data: V<'ctx>, vt: PointerValue<'ctx>) -> R<V<'ctx>> {
        let st = self.fat_ty();
        let mut agg = st.get_undef();
        agg = self.builder.build_insert_value(agg, data, 0, "data")?.into_struct_value();
        agg = self.builder.build_insert_value(agg, vt, 1, "vtable")?.into_struct_value();
        Ok(agg.into())
    }

    /// The (memoised) vtable global of `conc` implementing `tr`.
    pub(super) fn vtable_for(&mut self, conc: &Ty, tr: &str) -> R<PointerValue<'ctx>> {
        let key = format!("{}::{}", conc, tr);
        if let Some(v) = self.vtables.get(&key) {
            return Ok(*v);
        }
        let Some(methods) = self.trait_defs.get(tr) else {
            return Err(CgErr(format!("unknown trait `{}` in `dyn {}`", tr, tr)));
        };
        let names: Vec<(String, Option<PMode>, bool)> = methods.iter().map(|m| (m.name.clone(), m.self_kind, m.generic)).collect();
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let ut = self.usize_ty();
        let lt = self.llty(conc)?;
        let size = self.td.get_abi_size(&lt);
        let align = (self.td.get_abi_alignment(&lt) as u64).max(1);
        let drop_ptr: inkwell::values::BasicValueEnum<'ctx> = if self.needs_drop(conc) {
            self.drop_glue(conc)?.as_global_value().as_pointer_value().into()
        } else {
            ptrt.const_null().into()
        };
        let mut fields: Vec<inkwell::values::BasicValueEnum<'ctx>> = vec![drop_ptr, ut.const_int(size, false).into(), ut.const_int(align, false).into()];
        let tystr = conc.to_string();
        for (name, self_kind, generic) in &names {
            let slot: inkwell::values::BasicValueEnum<'ctx> = match (self_kind, generic, self.trait_impl_keys.get(&(tystr.clone(), tr.to_string(), name.clone()))) {
                (Some(PMode::Ref(_)), false, Some(k)) => self.fns[k].val.as_global_value().as_pointer_value().into(),
                (_, _, None) if matches!(self_kind, Some(PMode::Ref(_))) && !generic => {
                    return Err(CgErr(format!("`{}` does not implement the method `{}` of trait `{}` (needed for `dyn {}`)", conc, name, tr, tr)));
                }
                _ => ptrt.const_null().into(),
            };
            fields.push(slot);
        }
        let vt_ty = self.vtable_ty(names.len());
        let init = self.context.const_struct(&fields, false);
        let gname = format!("__mtn_vtable_{}_{}", Self::sanitize(&tystr), tr);
        let g = self.module.add_global(vt_ty, None, &gname);
        g.set_constant(true);
        g.set_initializer(&init);
        let p = g.as_pointer_value();
        self.vtables.insert(key, p);
        Ok(p)
    }

    fn sanitize(s: &str) -> String {
        s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
    }

    /// Converts an already evaluated `src` value to the trait-object type `dst` the checker chose.
    pub(super) fn coerce_to_dyn(&mut self, v: V<'ctx>, src: &Ty, dst: &Ty) -> R<V<'ctx>> {
        match (src, dst) {
            (Ty::Ref(_, a), Ty::Ref(_, d)) => {
                let Ty::DynTrait(tr) = &**d else { return Err(CgErr("internal: bad trait-object coercion".into())) };
                if let Ty::Generic(n, ga) = &**a {
                    if n == "Box" && matches!(ga.first(), Some(crate::types::GenericArg::Type(Ty::DynTrait(_)))) {
                        // `&Box<dyn Trait>` -> `&dyn Trait`: the box's own fat pointer
                        let fat = self.fat_ty();
                        return Ok(self.builder.build_load(fat, v.into_pointer_value(), "fat")?);
                    }
                }
                let vt = self.vtable_for(a, tr)?;
                self.build_fat(v, vt)
            }
            (Ty::Generic(_, sa), Ty::Generic(_, da)) => {
                let (Some(crate::types::GenericArg::Type(c)), Some(crate::types::GenericArg::Type(Ty::DynTrait(tr)))) = (sa.first(), da.first()) else {
                    return Err(CgErr("internal: bad trait-object coercion".into()));
                };
                let (c, tr) = (c.clone(), tr.clone());
                let vt = self.vtable_for(&c, &tr)?;
                self.build_fat(v, vt)
            }
            _ => Err(CgErr(format!("internal: cannot coerce `{}` to `{}`", src, dst))),
        }
    }

    /// Drop glue of `Box<dyn Trait>`: run the concrete drop glue (from the vtable), then free the
    /// allocation with the size and alignment stored in the vtable.
    pub(super) fn dyn_box_glue(&mut self, f: FunctionValue<'ctx>, p: PointerValue<'ctx>) -> R<()> {
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let ut = self.usize_ty();
        let fat = self.fat_ty();
        let dp = self.builder.build_struct_gep(fat, p, 0, "dp")?;
        let vp = self.builder.build_struct_gep(fat, p, 1, "vp")?;
        let data = self.builder.build_load(ptrt, dp, "data")?.into_pointer_value();
        let vt = self.builder.build_load(ptrt, vp, "vt")?.into_pointer_value();
        let (live, call_bb, after, end) = (
            self.context.append_basic_block(f, "live"),
            self.context.append_basic_block(f, "call_drop"),
            self.context.append_basic_block(f, "free"),
            self.context.append_basic_block(f, "end"),
        );
        let nn = self.builder.build_is_not_null(data, "nn")?;
        self.builder.build_conditional_branch(nn, live, end)?;
        self.builder.position_at_end(live);
        let vt_ty = self.vtable_ty(0);
        let dfp = self.builder.build_struct_gep(vt_ty, vt, 0, "dropp")?;
        let dfn = self.builder.build_load(ptrt, dfp, "dropfn")?.into_pointer_value();
        let has_drop = self.builder.build_is_not_null(dfn, "has_drop")?;
        self.builder.build_conditional_branch(has_drop, call_bb, after)?;
        self.builder.position_at_end(call_bb);
        let ft = self.context.void_type().fn_type(&[ptrt.into()], false);
        self.builder.build_indirect_call(ft, dfn, &[data.into()], "")?;
        self.builder.build_unconditional_branch(after)?;
        self.builder.position_at_end(after);
        let sp = self.builder.build_struct_gep(vt_ty, vt, 1, "sizep")?;
        let ap = self.builder.build_struct_gep(vt_ty, vt, 2, "alignp")?;
        let size = self.builder.build_load(ut, sp, "size")?.into_int_value();
        let align = self.builder.build_load(ut, ap, "align")?.into_int_value();
        // a zero-sized value was allocated with 1 byte
        let (_, fd) = self.alloc_fns();
        self.builder.build_call(fd, &[data.into(), size.into(), align.into()], "")?;
        self.builder.build_unconditional_branch(end)?;
        self.builder.position_at_end(end);
        Ok(())
    }

    /// `x.method(args)` where `x` is a `Box<dyn Trait>`, `&dyn Trait` or `&Box<dyn Trait>`.
    pub(super) fn dyn_method_call(&mut self, receiver: &Expr, rty: &Ty, tr: &str, name: &str, args: &[Arg]) -> R<V<'ctx>> {
        let fat = self.fat_ty();
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let Some(methods) = self.trait_defs.get(tr) else {
            return Err(CgErr(format!("unknown trait `{}`", tr)));
        };
        let Some(idx) = methods.iter().position(|m| m.name == name) else {
            return Err(CgErr(format!("trait `{}` has no method `{}`", tr, name)));
        };
        let m = &methods[idx];
        let (self_kind, generic, params, ret) = (m.self_kind, m.generic, m.params.clone(), m.ret.clone());
        if generic {
            return Err(CgErr(format!("`{}::{}` has its own generic parameters, so it cannot be called through `dyn {}` (not object safe)", tr, name, tr)));
        }
        if !matches!(self_kind, Some(PMode::Ref(_))) {
            return Err(CgErr(format!("`{}::{}` takes `self` by value or has no `self`, so it cannot be called through `dyn {}` (declare it `borrow self` or `borrow mut self`)", tr, name, tr)));
        }
        // the fat pointer of the receiver (lent, never moved)
        let fatv = match rty {
            Ty::Ref(_, i) if matches!(**i, Ty::DynTrait(_)) => self.expr(receiver)?,
            Ty::Ref(_, i) if matches!(&**i, Ty::Generic(n, _) if n == "Box") => {
                let slot = self.expr(receiver)?.into_pointer_value();
                self.builder.build_load(fat, slot, "fat")?
            }
            _ => {
                let (slot, _) = self.place_or_spill(receiver)?;
                self.builder.build_load(fat, slot, "fat")?
            }
        };
        let sv = fatv.into_struct_value();
        let data = self.builder.build_extract_value(sv, 0, "data")?.into_pointer_value();
        let vt = self.builder.build_extract_value(sv, 1, "vt")?.into_pointer_value();
        let vt_ty = self.vtable_ty(methods_len(self, tr));
        let fp = self.builder.build_struct_gep(vt_ty, vt, FIRST_METHOD_SLOT + idx as u32, "mp")?;
        let fnp = self.builder.build_load(ptrt, fp, "method")?.into_pointer_value();
        // arguments (`borrow` parameters are pointers, `borrow dyn` ones fat pointers)
        let infos: Vec<ParamInfo<'a>> = params.iter().map(|(n, t, mode)| ParamInfo { name: n.clone(), ty: t.clone(), mode: *mode, default: None }).collect();
        let display = format!("dyn {}::{}", tr, name);
        let vals = self.resolve_args(&display, &infos, args)?;
        let mut lls: Vec<BasicMetadataTypeEnum<'ctx>> = vec![ptrt.into()];
        let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> = vec![data.into()];
        for ((_, t, mode), v) in params.iter().zip(vals.iter()) {
            lls.push(match mode {
                PMode::Value => self.llty(t)?.into(),
                PMode::Ref(_) if matches!(t, Ty::DynTrait(_)) => fat.into(),
                PMode::Ref(_) => ptrt.into(),
                PMode::Variadic => self.str_ty().into(),
            });
            call_args.push((*v).into());
        }
        let ft = if is_unit_like(&ret) { self.context.void_type().fn_type(&lls, false) } else { self.llty(&ret)?.fn_type(&lls, false) };
        let call = self.builder.build_indirect_call(ft, fnp, &call_args, "dyncall")?;
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
}

fn methods_len<'ctx, 'a>(cg: &Codegen<'ctx, 'a>, tr: &str) -> usize {
    cg.trait_defs.get(tr).map(|m| m.len()).unwrap_or(0)
}
