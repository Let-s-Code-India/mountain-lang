//! Statements, expressions, patterns and control flow for `codegen.rs`.

use super::*;

enum SwKind {
    Const(u64),
    CatchAll,
}

fn flatten_patterns(ps: &[Pattern]) -> Vec<&Pattern> {
    let mut out = Vec::new();
    for p in ps {
        match p {
            Pattern::Or(alts) => out.extend(flatten_patterns(alts)),
            other => out.push(other),
        }
    }
    out
}

fn pattern_binds(p: &Pattern) -> bool {
    match p {
        Pattern::Mut(_) => true,
        Pattern::Ident(n) => !n.contains("::"), // refined by callers that know the type
        Pattern::Tuple(s) | Pattern::TupleStruct(_, s) | Pattern::Or(s) => s.iter().any(pattern_binds),
        _ => false,
    }
}

impl<'ctx, 'a> Codegen<'ctx, 'a> {
    // ------------------------------------------------------------------
    // Blocks and statements
    // ------------------------------------------------------------------

    pub(super) fn gen_block(&mut self, b: &Block) -> R<(V<'ctx>, Ty)> {
        self.locals.push(HashMap::new());
        let mut result: R<(V<'ctx>, Ty)> = Ok((self.unit(), Ty::Unit));
        for s in &b.stmts {
            if let Err(e) = self.gen_stmt(s) {
                result = Err(e);
                break;
            }
        }
        if result.is_ok() {
            result = match &b.tail {
                Some(t) => (|| {
                    let ty = self.ty_of(t)?;
                    let v = self.expr(t)?;
                    Ok((v, ty))
                })(),
                None => Ok((self.unit(), Ty::Unit)),
            };
        }
        self.locals.pop();
        let (v, ty) = result?;
        Ok((v, if self.dead { Ty::Never } else { ty }))
    }

    fn gen_stmt(&mut self, s: &Stmt) -> R<()> {
        match s {
            Stmt::Let { pattern, ty, value, .. } => {
                let annotated = ty.as_ref().map(|t| self.rty(t));
                match value {
                    Some(v) => {
                        let vty = self.ty_of(v)?;
                        let lty = annotated.clone().unwrap_or_else(|| vty.clone());
                        let val = self.expr(v)?;
                        match pattern {
                            Pattern::Ident(n) | Pattern::Mut(n) => {
                                let slot = self.declare_local(n, &lty)?;
                                if vty != Ty::Never {
                                    self.builder.build_store(slot, val)?;
                                }
                            }
                            Pattern::Wildcard => {}
                            other => {
                                let tmp = self.spill(val, &lty)?;
                                let mut binds = Vec::new();
                                self.pat_test(other, tmp, &lty, &mut binds, false)?;
                                self.materialize(binds)?;
                            }
                        }
                    }
                    None => {
                        let (Pattern::Ident(n) | Pattern::Mut(n)) = pattern else {
                            return Err(CgErr("`let` with a destructuring pattern needs an initializer".into()));
                        };
                        let Some(t) = annotated else {
                            return Err(CgErr(format!("cannot infer the type of `{}`: no annotation and no initializer", n)));
                        };
                        self.declare_local(n, &t)?;
                    }
                }
                Ok(())
            }
            Stmt::Expr(e) => {
                self.expr(e)?;
                Ok(())
            }
            Stmt::Return(v) => self.gen_return(v.as_ref()),
            Stmt::Break { label, value } => self.gen_break(label, value.as_ref()),
            Stmt::Continue { label } => {
                let idx = self.find_loop(label)?;
                let bb = self.loops[idx].continue_bb;
                self.builder.build_unconditional_branch(bb)?;
                self.terminate();
                Ok(())
            }
            Stmt::Yield(_) => unsupported("generators (`yield`)", 12),
            // Nested items were hoisted and registered with the module-level items.
            Stmt::Item(_) => Ok(()),
            Stmt::TargetBlock(kind, b) => {
                if matches!(kind, TargetKind::Native | TargetKind::All) {
                    self.gen_block(b)?;
                }
                Ok(())
            }
        }
    }

    fn find_loop(&self, label: &Option<String>) -> R<usize> {
        match label {
            Some(l) => self
                .loops
                .iter()
                .rposition(|c| c.label.as_deref() == Some(l.as_str()))
                .ok_or_else(|| CgErr(format!("label `'{}` does not name an enclosing loop", l))),
            None => {
                if self.loops.is_empty() {
                    Err(CgErr("`break`/`continue` outside of a loop".into()))
                } else {
                    Ok(self.loops.len() - 1)
                }
            }
        }
    }

    fn gen_break(&mut self, label: &Option<String>, value: Option<&Expr>) -> R<()> {
        let idx = self.find_loop(label)?;
        if let Some(e) = value {
            let ety = self.ty_of(e)?;
            let v = self.expr(e)?;
            if let Some((slot, _)) = self.loops[idx].result.clone() {
                if ety != Ty::Never {
                    self.builder.build_store(slot, v)?;
                }
            }
        }
        let bb = self.loops[idx].break_bb;
        self.builder.build_unconditional_branch(bb)?;
        self.terminate();
        Ok(())
    }

    fn gen_return(&mut self, v: Option<&Expr>) -> R<()> {
        let ret = self.ret_ty.clone();
        let val = match v {
            Some(e) => Some(self.expr_as(e, &ret)?),
            None => None,
        };
        if is_unit_like(&ret) {
            self.builder.build_return(None)?;
        } else {
            let v = match val {
                Some(v) => v,
                None => return Err(CgErr(format!("`return;` in a function returning `{}`", ret))),
            };
            self.builder.build_return(Some(&v))?;
        }
        self.terminate();
        Ok(())
    }

    fn spill(&mut self, v: V<'ctx>, ty: &Ty) -> R<PointerValue<'ctx>> {
        let lt = self.llty(ty)?;
        let p = self.alloca(lt, "tmp")?;
        self.builder.build_store(p, v)?;
        Ok(p)
    }

    fn load(&mut self, p: PointerValue<'ctx>, ty: &Ty) -> R<V<'ctx>> {
        let lt = self.llty(ty)?;
        Ok(self.builder.build_load(lt, p, "")?)
    }

    /// Evaluates `e`; a diverging (`!`) expression stands in as zero of `want`.
    pub(super) fn expr_as(&mut self, e: &Expr, want: &Ty) -> R<V<'ctx>> {
        let t = self.ty_of(e)?;
        let v = self.expr(e)?;
        if t == Ty::Never && *want != Ty::Never {
            return self.zero_of(want);
        }
        Ok(v)
    }

    fn materialize(&mut self, binds: Vec<Bind<'ctx>>) -> R<()> {
        for b in binds {
            let v = self.load(b.ptr, &b.ty)?;
            let slot = self.declare_local(&b.name, &b.ty)?;
            self.builder.build_store(slot, v)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    pub(super) fn expr(&mut self, e: &Expr) -> R<V<'ctx>> {
        match e {
            Expr::Literal(l) => {
                let ty = self.ty_of(e)?;
                self.literal(l, &ty, false)
            }
            Expr::Paren(inner) => self.expr(inner),
            Expr::Ident(name) => self.ident(e, name),
            Expr::Path(path) => {
                let ty = self.ty_of(e)?;
                if let [_, variant] = path.as_slice() {
                    if let Some(idx) = self.variant_index(variant, &ty) {
                        return self.make_enum(&ty, idx, Vec::new());
                    }
                }
                unsupported("module/associated paths", 14)
            }
            Expr::Tuple(items) => {
                if items.is_empty() {
                    return Ok(self.unit());
                }
                let ty = self.ty_of(e)?;
                let Ty::Tuple(tys) = &ty else { return Err(CgErr(format!("internal: tuple typed `{}`", ty))) };
                let tys = tys.clone();
                let mut vals = Vec::new();
                for (it, t) in items.iter().zip(tys.iter()) {
                    vals.push(self.expr_as(it, t)?);
                }
                self.build_struct(&ty, vals)
            }
            Expr::StructLit { name, fields, spread } => {
                if spread.is_some() {
                    return unsupported("struct update syntax (`..base`, needs the `Default` trait)", 16);
                }
                let self_name = self.cur_self.as_ref().map(|t| t.to_string());
                let name: &String = &if name == "Self" { self_name.unwrap_or_else(|| name.clone()) } else { name.clone() };
                let Some((decl, _)) = self.structs.get(name).cloned() else {
                    return Err(CgErr(format!("unknown struct `{}`", name)));
                };
                let mut vals = Vec::new();
                for (fname, fty) in &decl {
                    let Some((_, fe)) = fields.iter().find(|(n, _)| n == fname) else {
                        return Err(CgErr(format!("missing field `{}` in `{}`", fname, name)));
                    };
                    vals.push(self.expr_as(fe, fty)?);
                }
                self.build_struct(&Ty::Named(name.clone()), vals)
            }
            Expr::Field { expr: base, name } => {
                let bty = self.ty_of(base)?;
                if self.is_place_expr(base) || matches!(bty, Ty::Ref(..)) {
                    // Read through the place (auto-deref of references, no whole-struct copy).
                    let (p, t) = self.place(e)?;
                    return self.load(p, &t);
                }
                let (idx, _) = self.field_index(&bty, name)?;
                let bv = self.expr(base)?;
                Ok(self.builder.build_extract_value(bv.into_struct_value(), idx, "field")?)
            }
            Expr::Index { .. } => {
                let (p, t) = self.place(e)?;
                self.load(p, &t)
            }
            Expr::Array(items) => {
                let ty = self.ty_of(e)?;
                let Ty::Fixed(et, _) = &ty else {
                    return unsupported("growable `[T]` array literals (annotate the type as `[T; N]`)", 11);
                };
                let et = (**et).clone();
                let lt = self.llty(&ty)?.into_array_type();
                let mut agg = lt.get_undef();
                for (i, it) in items.iter().enumerate() {
                    let v = self.expr_as(it, &et)?;
                    agg = self.builder.build_insert_value(agg, v, i as u32, "elem")?.into_array_value();
                }
                Ok(agg.into())
            }
            Expr::Borrow { expr: inner, .. } => {
                let ty = self.ty_of(e)?;
                self.gen_borrow(&ty, inner)
            }
            Expr::MethodCall { receiver, name, args } => self.method_call(receiver, name, args),
            Expr::If(i) => {
                let ty = self.ty_of(e)?;
                self.gen_if(i, &ty)
            }
            Expr::Match(m) => {
                let ty = self.ty_of(e)?;
                self.gen_match(m, &ty)
            }
            Expr::Loop(l) => {
                let ty = self.ty_of(e)?;
                self.gen_loop(l, &ty)
            }
            Expr::Block(b) | Expr::Unsafe(b) => Ok(self.gen_block(b)?.0),
            Expr::Unary { op, expr: inner } => self.unary(*op, inner, e),
            Expr::Binary { op, lhs, rhs } => {
                let ty = self.ty_of(e)?;
                self.binary(*op, lhs, rhs, &ty)
            }
            Expr::Assign { op, lhs, rhs } => {
                self.assign(*op, lhs, rhs)?;
                Ok(self.unit())
            }
            Expr::Cast { expr: inner, .. } => {
                let to = self.ty_of(e)?;
                let from = self.ty_of(inner)?;
                let v = self.expr_as(inner, &from)?;
                self.cast(v, &from, &to)
            }
            Expr::Propagate(inner) => self.propagate(inner),
            Expr::Call { callee, args } => self.call(e, callee, args),
            Expr::Return(v) => {
                self.gen_return(v.as_deref())?;
                Ok(self.unit())
            }
            Expr::Range { .. } => unsupported("range values outside a `for` loop", 16),
            Expr::Closure(_) => unsupported("closures", 12),
            Expr::Await(_) | Expr::Spawn { .. } => unsupported("`await`/`spawn` (async & concurrency)", 12),
            Expr::Select(_) => unsupported("`select` (channels)", 13),
            Expr::Query(_) => unsupported("`query` (embedded database)", 18),
            Expr::Yield(_) => unsupported("generators (`yield`)", 12),
            Expr::TryCatch { .. } => Err(CgErr("internal: a `try`/`catch` reached codegen without being desugared".into())),
            Expr::Throw(_) => Err(CgErr("`throw` outside a `try` block".into())),
            Expr::Styled { .. } | Expr::Layout { .. } | Expr::ComponentChildren { .. } | Expr::EventHandler { .. } => unsupported("UI expressions", 19),
        }
    }

    fn ident(&mut self, e: &Expr, name: &str) -> R<V<'ctx>> {
        if let Some((p, t)) = self.lookup_local(name) {
            return self.load(p, &t);
        }
        if let Some((ce, cty)) = self.consts.get(name).cloned() {
            // Document 3: a `const` is inlined at every use site.
            self.const_depth += 1;
            if self.const_depth > 64 {
                self.const_depth -= 1;
                return Err(CgErr(format!("constant `{}` refers to itself", name)));
            }
            let v = self.expr_as(ce, &cty);
            self.const_depth -= 1;
            return v;
        }
        if let Some((p, t)) = self.statics.get(name).cloned() {
            return self.load(p, &t);
        }
        if name == "None" {
            let ty = self.ty_of(e)?;
            if let Some(idx) = self.variant_index("None", &ty) {
                return self.make_enum(&ty, idx, Vec::new());
            }
            return Err(CgErr("cannot determine the type of `None` here".into()));
        }
        if self.fns.contains_key(name) {
            return unsupported("function values", 12);
        }
        Err(CgErr(format!("undefined variable `{}`", name)))
    }

    // ---- literals ------------------------------------------------------

    fn int_const(&mut self, ty: &Ty, v: u128, neg: bool) -> R<V<'ctx>> {
        let lt = self.llty(ty)?.into_int_type();
        let bits = lt.get_bit_width();
        let ok = if is_signed(ty) {
            let limit: u128 = 1u128 << (bits - 1);
            if neg { v <= limit } else { v < limit }
        } else {
            !neg && (bits == 128 || v < (1u128 << bits))
        };
        if !ok {
            return Err(CgErr(format!("integer literal `{}{}` is out of range for `{}`", if neg { "-" } else { "" }, v, ty)));
        }
        let raw: u128 = if neg { (!v).wrapping_add(1) } else { v };
        Ok(if bits > 64 {
            lt.const_int_arbitrary_precision(&[raw as u64, (raw >> 64) as u64]).into()
        } else {
            lt.const_int(raw as u64, false).into()
        })
    }

    pub(super) fn literal(&mut self, l: &Literal, ty: &Ty, neg: bool) -> R<V<'ctx>> {
        let int_val = match l {
            Literal::Int(t) => Some(parse_int_text(t, None)),
            Literal::IntHex(t) => Some(parse_int_text(t, Some(("0x", 16)))),
            Literal::IntOct(t) => Some(parse_int_text(t, Some(("0o", 8)))),
            Literal::IntBin(t) => Some(parse_int_text(t, Some(("0b", 2)))),
            _ => None,
        };
        if let Some(parsed) = int_val {
            let v = parsed.ok_or_else(|| CgErr("malformed or oversized integer literal".into()))?;
            if is_float(ty) {
                let f = if neg { -(v as f64) } else { v as f64 };
                return Ok(self.llty(ty)?.into_float_type().const_float(f).into());
            }
            if !is_int(ty) {
                return Err(CgErr(format!("an integer literal cannot have type `{}`", ty)));
            }
            return self.int_const(ty, v, neg);
        }
        match l {
            Literal::Float(t) => {
                let body: String = t.chars().filter(|c| *c != '_').collect();
                let body = body.trim_end_matches("f32").trim_end_matches("f64").to_string();
                let f: f64 = body.parse().map_err(|_| CgErr(format!("malformed float literal `{}`", t)))?;
                let f = if neg { -f } else { f };
                if !is_float(ty) {
                    return Err(CgErr(format!("a float literal cannot have type `{}`", ty)));
                }
                Ok(self.llty(ty)?.into_float_type().const_float(f).into())
            }
            Literal::Bool(b) => Ok(self.int_ty(1).const_int(*b as u64, false).into()),
            Literal::Char(t) => {
                let inner = t.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')).ok_or_else(|| CgErr(format!("malformed char literal {}", t)))?;
                let mut it = inner.chars().peekable();
                let cp = unescape_char(&mut it).ok_or_else(|| CgErr(format!("invalid escape in char literal {}", t)))?;
                Ok(self.int_ty(32).const_int(cp as u64, false).into())
            }
            Literal::Str(t) => {
                let inner = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')).ok_or_else(|| CgErr("malformed string literal".into()))?;
                let bytes = unescape_str(inner).ok_or_else(|| CgErr("invalid escape in string literal".into()))?;
                Ok(self.str_const(&bytes))
            }
            Literal::RawStr(t) => {
                let inner = t.strip_prefix("r\"").and_then(|s| s.strip_suffix('"')).ok_or_else(|| CgErr("malformed raw string literal".into()))?;
                Ok(self.str_const(inner.as_bytes()))
            }
            Literal::Null => unsupported("`null`", 11),
            _ => unreachable!(),
        }
    }

    fn str_const(&mut self, bytes: &[u8]) -> V<'ctx> {
        let p = self.global_str(bytes);
        let len = self.int_ty(self.ptr_bits()).const_int(bytes.len() as u64, false);
        self.context.const_struct(&[p.into(), len.into()], false).into()
    }

    // ---- aggregates ----------------------------------------------------

    fn build_struct(&mut self, ty: &Ty, vals: Vec<V<'ctx>>) -> R<V<'ctx>> {
        let st = self.llty(ty)?.into_struct_type();
        let mut agg = st.get_undef();
        for (i, v) in vals.into_iter().enumerate() {
            agg = self.builder.build_insert_value(agg, v, i as u32, "agg")?.into_struct_value();
        }
        Ok(agg.into())
    }

    /// Index and type of a named/positional field of a struct, tuple or tuple struct.
    fn field_index(&mut self, base: &Ty, field: &str) -> R<(u32, Ty)> {
        match base {
            Ty::Named(n) => {
                let Some((fields, _)) = self.structs.get(n) else {
                    return Err(CgErr(format!("`{}` is not a struct", n)));
                };
                fields
                    .iter()
                    .position(|(f, _)| f == field)
                    .map(|i| (i as u32, fields[i].1.clone()))
                    .ok_or_else(|| CgErr(format!("struct `{}` has no field `{}`", n, field)))
            }
            Ty::Tuple(ts) => {
                let i: usize = field.parse().map_err(|_| CgErr(format!("tuple has no field `{}`", field)))?;
                ts.get(i).map(|t| (i as u32, t.clone())).ok_or_else(|| CgErr(format!("tuple has no field `{}`", field)))
            }
            other => Err(CgErr(format!("type `{}` has no fields", other))),
        }
    }

    fn variant_index(&self, name: &str, ty: &Ty) -> Option<usize> {
        let last = name.rsplit("::").next().unwrap_or(name);
        self.variants_of(ty)?.iter().position(|(n, _)| n == last)
    }

    fn make_enum(&mut self, ty: &Ty, idx: usize, vals: Vec<V<'ctx>>) -> R<V<'ctx>> {
        let layout = self.enum_layout(ty)?;
        let tmp = self.alloca(layout.llty.into(), "enum")?;
        let tag = self.builder.build_struct_gep(layout.llty, tmp, 0, "tag")?;
        self.builder.build_store(tag, self.int_ty(32).const_int(idx as u64, false))?;
        if !vals.is_empty() {
            let payload = self.builder.build_struct_gep(layout.llty, tmp, 1, "payload")?;
            for (i, v) in vals.into_iter().enumerate() {
                let fp = self.builder.build_struct_gep(layout.variants[idx], payload, i as u32, "f")?;
                self.builder.build_store(fp, v)?;
            }
        }
        Ok(self.builder.build_load(layout.llty, tmp, "")?)
    }

    fn enum_tag(&mut self, ty: &Ty, place: PointerValue<'ctx>) -> R<IntValue<'ctx>> {
        let layout = self.enum_layout(ty)?;
        let tp = self.builder.build_struct_gep(layout.llty, place, 0, "tag")?;
        Ok(self.builder.build_load(self.int_ty(32), tp, "tag")?.into_int_value())
    }

    fn payload_ptr(&mut self, ty: &Ty, place: PointerValue<'ctx>, variant: usize, field: usize) -> R<PointerValue<'ctx>> {
        let layout = self.enum_layout(ty)?;
        let payload = self.builder.build_struct_gep(layout.llty, place, 1, "payload")?;
        Ok(self.builder.build_struct_gep(layout.variants[variant], payload, field as u32, "pf")?)
    }

    // ---- places / assignment ---------------------------------------------

    fn is_place_expr(&self, e: &Expr) -> bool {
        match e {
            Expr::Paren(i) => self.is_place_expr(i),
            Expr::Ident(n) => self.lookup_local(n).is_some() || self.statics.contains_key(n.as_str()),
            Expr::Field { .. } | Expr::Index { .. } => true,
            _ => false,
        }
    }

    /// The memory location of `e`; an rvalue is first spilled to a temporary.
    pub(super) fn place_or_spill(&mut self, e: &Expr) -> R<(PointerValue<'ctx>, Ty)> {
        if self.is_place_expr(e) {
            return self.place(e);
        }
        let ty = self.ty_of(e)?;
        let v = self.expr_as(e, &ty)?;
        let p = self.spill(v, &ty)?;
        Ok((p, ty))
    }

    fn place(&mut self, e: &Expr) -> R<(PointerValue<'ctx>, Ty)> {
        match e {
            Expr::Ident(n) => {
                if let Some(x) = self.lookup_local(n) {
                    return Ok(x);
                }
                if let Some(x) = self.statics.get(n.as_str()) {
                    return Ok(x.clone());
                }
                if let Some((ce, cty)) = self.consts.get(n.as_str()).cloned() {
                    let v = self.expr_as(ce, &cty)?;
                    let p = self.spill(v, &cty)?;
                    return Ok((p, cty));
                }
                Err(CgErr(format!("undefined variable `{}`", n)))
            }
            Expr::Paren(i) => self.place(i),
            Expr::Field { expr: base, name } => {
                let bty = self.ty_of(base)?;
                let (bp, st_ty) = match &bty {
                    Ty::Ref(_, inner) => {
                        // auto-deref: the reference value is the pointer to the struct
                        let mut v = self.expr(base)?;
                        let mut cur: Ty = (**inner).clone();
                        while let Ty::Ref(_, i2) = &cur {
                            let lt = self.llty(i2)?;
                            v = self.builder.build_load(lt, v.into_pointer_value(), "deref")?;
                            cur = (**i2).clone();
                        }
                        (v.into_pointer_value(), cur)
                    }
                    other => {
                        let (p, t) = self.place_or_spill(base)?;
                        let _ = other;
                        (p, t)
                    }
                };
                let (idx, fty) = self.field_index(&st_ty, name)?;
                let st = self.llty(&st_ty)?;
                Ok((self.builder.build_struct_gep(st, bp, idx, "fp")?, fty))
            }
            Expr::Index { expr: base, index } => self.index_place(base, index),
            _ => unsupported("this assignment target", 11),
        }
    }

    // ---- arrays, slices, references (Phase 11a) -------------------------------

    fn usize_ty(&self) -> IntType<'ctx> {
        self.int_ty(self.ptr_bits())
    }

    /// Converts an integer index/bound to pointer width. A negative signed
    /// value becomes a huge unsigned one and so fails the bounds check.
    fn to_usize(&mut self, v: V<'ctx>, ty: &Ty) -> R<IntValue<'ctx>> {
        if !is_int(ty) || matches!(ty, Ty::I128 | Ty::U128) {
            return Err(CgErr(format!("an index must be a (non-128-bit) integer, found `{}`", ty)));
        }
        let iv = v.into_int_value();
        let ut = self.usize_ty();
        let (fb, tb) = (iv.get_type().get_bit_width(), ut.get_bit_width());
        Ok(if fb == tb {
            iv
        } else if fb > tb {
            self.builder.build_int_truncate(iv, ut, "")?
        } else if is_signed(ty) {
            self.builder.build_int_s_extend(iv, ut, "")?
        } else {
            self.builder.build_int_z_extend(iv, ut, "")?
        })
    }

    /// `__mtn_oob(len, idx)` / `__mtn_slice_oob(lo, hi, len)`: print the
    /// Document 22 §8 style message and exit with code 101.
    fn oob_fn(&mut self, name: &str, nargs: usize, fmt: &str) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function(name) {
            return f;
        }
        let i32t = self.context.i32_type();
        let i64t = self.int_ty(64);
        let ptr = self.context.ptr_type(AddressSpace::default());
        let dprintf = self.get_extern("dprintf", Some(i32t.into()), &[i32t.into(), ptr.into()], true);
        let exit = self.get_extern("exit", None, &[i32t.into()], false);
        self.noreturn(exit);
        let ut = self.usize_ty();
        let params: Vec<BasicMetadataTypeEnum> = (0..nargs).map(|_| ut.into()).collect();
        let ft = self.context.void_type().fn_type(&params, false);
        let f = self.module.add_function(name, ft, None);
        self.noreturn(f);
        let bb = self.context.append_basic_block(f, "entry");
        let fmtp = self.c_str(fmt);
        let b = self.context.create_builder();
        b.position_at_end(bb);
        let mut args: Vec<BasicMetadataValueEnum> = vec![i32t.const_int(2, false).into(), fmtp.into()];
        for i in 0..nargs {
            let p = f.get_nth_param(i as u32).unwrap().into_int_value();
            let w = if p.get_type().get_bit_width() < 64 { b.build_int_z_extend(p, i64t, "").unwrap() } else { p };
            args.push(w.into());
        }
        b.build_call(dprintf, &args, "").unwrap();
        b.build_call(exit, &[i32t.const_int(101, false).into()], "").unwrap();
        b.build_unreachable().unwrap();
        f
    }

    fn bounds_check(&mut self, idx: IntValue<'ctx>, len: IntValue<'ctx>) -> R<()> {
        let oob = self.builder.build_int_compare(IntPredicate::UGE, idx, len, "oob")?;
        let f = self.oob_fn("__mtn_oob", 2, "thread 'main' panicked: index out of bounds: the length is %llu but the index is %llu\n");
        let fail = self.new_block("oob");
        let ok = self.new_block("inbounds");
        self.builder.build_conditional_branch(oob, fail, ok)?;
        self.builder.position_at_end(fail);
        self.builder.build_call(f, &[len.into(), idx.into()], "")?;
        self.builder.build_unreachable()?;
        self.position(ok);
        Ok(())
    }

    /// For an array-like expression: (data pointer, element type, length, whether the data is `[N x T]` or `T*`).
    fn array_parts(&mut self, base: &Expr) -> R<(PointerValue<'ctx>, Ty, IntValue<'ctx>, Option<Ty>)> {
        let bty = self.ty_of(base)?;
        let ut = self.usize_ty();
        match &bty {
            Ty::Fixed(t, n) => {
                let (p, _) = self.place_or_spill(base)?;
                Ok((p, (**t).clone(), ut.const_int(*n, false), Some(bty.clone())))
            }
            Ty::Ref(_, inner) => match inner.as_ref() {
                Ty::Fixed(t, n) => {
                    let p = self.expr(base)?.into_pointer_value();
                    Ok((p, (**t).clone(), ut.const_int(*n, false), Some((**inner).clone())))
                }
                Ty::Array(t) => {
                    let sv = self.expr(base)?.into_struct_value();
                    let data = self.builder.build_extract_value(sv, 0, "data")?.into_pointer_value();
                    let len = self.builder.build_extract_value(sv, 1, "len")?.into_int_value();
                    Ok((data, (**t).clone(), len, None))
                }
                other => Err(CgErr(format!("cannot index into `&{}`", other))),
            },
            Ty::Array(_) => unsupported("indexing a growable `[T]`", 11),
            other => Err(CgErr(format!("cannot index into type `{}`", other))),
        }
    }

    fn elem_ptr(&mut self, data: PointerValue<'ctx>, elem: &Ty, arr_ty: &Option<Ty>, idx: IntValue<'ctx>) -> R<PointerValue<'ctx>> {
        let zero = self.usize_ty().const_zero();
        // SAFETY: the index was bounds-checked against the array length just above.
        unsafe {
            Ok(match arr_ty {
                Some(at) => {
                    let lt = self.llty(at)?;
                    self.builder.build_in_bounds_gep(lt, data, &[zero, idx], "elem")?
                }
                None => {
                    let lt = self.llty(elem)?;
                    self.builder.build_in_bounds_gep(lt, data, &[idx], "elem")?
                }
            })
        }
    }

    fn index_place(&mut self, base: &Expr, index: &Expr) -> R<(PointerValue<'ctx>, Ty)> {
        let mut ie = index;
        while let Expr::Paren(i) = ie {
            ie = i;
        }
        if matches!(ie, Expr::Range { .. }) {
            return Err(CgErr("a range index `a[lo..hi]` makes a slice: write `borrow a[lo..hi]`".into()));
        }
        let (data, elem, len, arr_ty) = self.array_parts(base)?;
        let ity = self.ty_of(index)?;
        let iv = self.expr_as(index, &ity)?;
        let idx = self.to_usize(iv, &ity)?;
        self.bounds_check(idx, len)?;
        let p = self.elem_ptr(data, &elem, &arr_ty, idx)?;
        Ok((p, elem))
    }

    /// `borrow x` / `borrow mut x` with result type `ty`.
    fn gen_borrow(&mut self, ty: &Ty, inner: &Expr) -> R<V<'ctx>> {
        let mut ie = inner;
        while let Expr::Paren(i) = ie {
            ie = i;
        }
        // `borrow a[lo..hi]` -> a slice view
        if let Expr::Index { expr: base, index } = ie {
            let mut ix: &Expr = index;
            while let Expr::Paren(i) = ix {
                ix = i;
            }
            if let Expr::Range { lo, hi, inclusive } = ix {
                let (data, elem, len, arr_ty) = self.array_parts(base)?;
                let (lt, ht) = (self.ty_of(lo)?, self.ty_of(hi)?);
                let lv = self.expr_as(lo, &lt)?;
                let hv = self.expr_as(hi, &ht)?;
                let lo_i = self.to_usize(lv, &lt)?;
                let mut hi_i = self.to_usize(hv, &ht)?;
                let ut = self.usize_ty();
                if *inclusive {
                    // `lo..=hi` is `lo..hi+1`; `hi == usize::MAX` can never be in range.
                    let full = self.builder.build_int_compare(IntPredicate::EQ, hi_i, ut.const_all_ones(), "")?;
                    let f = self.oob_fn("__mtn_slice_oob", 3, "thread 'main' panicked: slice range out of bounds: the range is %llu..%llu but the length is %llu\n");
                    let fail = self.new_block("slice_oob");
                    let ok = self.new_block("slice_ok");
                    self.builder.build_conditional_branch(full, fail, ok)?;
                    self.builder.position_at_end(fail);
                    self.builder.build_call(f, &[lo_i.into(), hi_i.into(), len.into()], "")?;
                    self.builder.build_unreachable()?;
                    self.position(ok);
                    hi_i = self.builder.build_int_add(hi_i, ut.const_int(1, false), "")?;
                }
                let bad_order = self.builder.build_int_compare(IntPredicate::UGT, lo_i, hi_i, "")?;
                let bad_end = self.builder.build_int_compare(IntPredicate::UGT, hi_i, len, "")?;
                let bad = self.builder.build_or(bad_order, bad_end, "bad")?;
                let f = self.oob_fn("__mtn_slice_oob", 3, "thread 'main' panicked: slice range out of bounds: the range is %llu..%llu but the length is %llu\n");
                let fail = self.new_block("slice_oob");
                let ok = self.new_block("slice_ok");
                self.builder.build_conditional_branch(bad, fail, ok)?;
                self.builder.position_at_end(fail);
                self.builder.build_call(f, &[lo_i.into(), hi_i.into(), len.into()], "")?;
                self.builder.build_unreachable()?;
                self.position(ok);
                let start = self.elem_ptr_raw(data, &elem, &arr_ty, lo_i)?;
                let n = self.builder.build_int_sub(hi_i, lo_i, "n")?;
                return self.build_slice(start, n);
            }
        }
        match ty {
            Ty::Ref(_, target) if matches!(**target, Ty::Array(_)) => {
                let ity = self.ty_of(inner)?;
                let ut = self.usize_ty();
                match &ity {
                    Ty::Fixed(_, n) => {
                        let (p, _) = self.place_or_spill(inner)?;
                        self.build_slice(p, ut.const_int(*n, false))
                    }
                    Ty::Ref(_, i2) => match i2.as_ref() {
                        Ty::Array(_) => self.expr(inner),
                        Ty::Fixed(_, n) => {
                            let p = self.expr(inner)?.into_pointer_value();
                            self.build_slice(p, ut.const_int(*n, false))
                        }
                        other => Err(CgErr(format!("cannot borrow `{}` as a slice", other))),
                    },
                    other => Err(CgErr(format!("cannot borrow `{}` as a slice", other))),
                }
            }
            _ => {
                let (p, _) = self.place_or_spill(inner)?;
                Ok(p.into())
            }
        }
    }

    /// Pointer to element `idx` WITHOUT a bounds check (the caller checked the range).
    fn elem_ptr_raw(&mut self, data: PointerValue<'ctx>, elem: &Ty, arr_ty: &Option<Ty>, idx: IntValue<'ctx>) -> R<PointerValue<'ctx>> {
        self.elem_ptr(data, elem, arr_ty, idx)
    }

    fn build_slice(&mut self, data: PointerValue<'ctx>, len: IntValue<'ctx>) -> R<V<'ctx>> {
        let st = self.str_ty();
        let mut agg = st.get_undef();
        agg = self.builder.build_insert_value(agg, data, 0, "")?.into_struct_value();
        agg = self.builder.build_insert_value(agg, len, 1, "")?.into_struct_value();
        Ok(agg.into())
    }

    // ---- calls: arguments, methods, associated functions ---------------------

    /// One argument for parameter `p` (Document 10 §2, Document 6 §3).
    fn gen_arg(&mut self, p: &ParamInfo<'a>, arg: &Expr) -> R<V<'ctx>> {
        match p.mode {
            PMode::Value | PMode::Variadic => self.expr_as(arg, &p.ty),
            PMode::Ref(want_mut) => {
                let mut a = arg;
                while let Expr::Paren(i) = a {
                    a = i;
                }
                if let Expr::Borrow { mutable, expr: inner } = a {
                    if want_mut && !*mutable {
                        return Err(CgErr(format!("parameter `{}` is `borrow mut`: pass `borrow mut <place>`", p.name)));
                    }
                    let (ptr, _) = self.place_or_spill(inner)?;
                    return Ok(ptr.into());
                }
                let t = self.ty_of(a)?;
                if matches!(t, Ty::Ref(..)) {
                    return self.expr(a);
                }
                Err(CgErr(format!("the argument for the `borrow` parameter `{}` must be written `borrow <place>`", p.name)))
            }
        }
    }

    /// Matches call arguments to parameters: positional first, then named
    /// (any order), defaults for the rest, extras into the variadic slice.
    fn resolve_args(&mut self, fname: &str, params: &[ParamInfo<'a>], args: &[Arg]) -> R<Vec<V<'ctx>>> {
        let n = params.len();
        let var_idx = params.iter().position(|p| p.mode == PMode::Variadic);
        let fixed_n = var_idx.unwrap_or(n);
        let mut slots: Vec<Option<V<'ctx>>> = vec![None; n];
        let mut extra: Vec<V<'ctx>> = Vec::new();
        let mut seen_named = false;
        let mut pos = 0usize;
        for a in args {
            match &a.name {
                None => {
                    if seen_named {
                        return Err(CgErr(format!("call to `{}`: a positional argument cannot follow a named argument", fname)));
                    }
                    if pos < fixed_n {
                        let p = params[pos].clone();
                        slots[pos] = Some(self.gen_arg(&p, &a.value)?);
                    } else if let Some(vi) = var_idx {
                        let et = params[vi].ty.clone();
                        extra.push(self.expr_as(&a.value, &et)?);
                    } else {
                        return Err(CgErr(format!("`{}` takes {} argument(s), but more were given", fname, n)));
                    }
                    pos += 1;
                }
                Some(nm) => {
                    seen_named = true;
                    let Some(i) = params.iter().position(|p| &p.name == nm && p.mode != PMode::Variadic) else {
                        return Err(CgErr(format!("`{}` has no parameter named `{}` (Document 10 §2.2: named arguments must match the declared names)", fname, nm)));
                    };
                    if slots[i].is_some() {
                        return Err(CgErr(format!("call to `{}`: parameter `{}` is given twice", fname, nm)));
                    }
                    let p = params[i].clone();
                    slots[i] = Some(self.gen_arg(&p, &a.value)?);
                }
            }
        }
        let mut out = Vec::new();
        for (i, p) in params.iter().enumerate() {
            if p.mode == PMode::Variadic {
                out.push(self.variadic_slice(&p.ty, &extra)?);
                continue;
            }
            match slots[i].take() {
                Some(v) => out.push(v),
                None => match p.default {
                    Some(d) => out.push(self.expr_as(d, &p.ty)?),
                    None => return Err(CgErr(format!("call to `{}` is missing the argument for parameter `{}`", fname, p.name))),
                },
            }
        }
        Ok(out)
    }

    /// The extra arguments of a variadic call, as a stack array viewed as a slice.
    fn variadic_slice(&mut self, elem: &Ty, vals: &[V<'ctx>]) -> R<V<'ctx>> {
        let et = self.llty(elem)?;
        let arr_ty = et.array_type(vals.len() as u32);
        let slot = self.alloca(arr_ty.into(), "varargs")?;
        let zero = self.usize_ty().const_zero();
        for (i, v) in vals.iter().enumerate() {
            let idx = self.usize_ty().const_int(i as u64, false);
            // SAFETY: `i < vals.len()`, the array's length.
            let ep = unsafe { self.builder.build_in_bounds_gep(arr_ty, slot, &[zero, idx], "va")? };
            self.builder.build_store(ep, *v)?;
        }
        let len = self.usize_ty().const_int(vals.len() as u64, false);
        self.build_slice(slot, len)
    }

    fn call_fn(&mut self, key: &str, display: &str, recv: Option<V<'ctx>>, args: &[Arg]) -> R<V<'ctx>> {
        let (fv, params, ret) = {
            let i = &self.fns[key];
            (i.val, i.params.clone(), i.ret.clone())
        };
        let mut vals: Vec<BasicMetadataValueEnum> = Vec::new();
        if let Some(r) = recv {
            vals.push(r.into());
        }
        for v in self.resolve_args(display, &params, args)? {
            vals.push(v.into());
        }
        let call = self.builder.build_call(fv, &vals, "")?;
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

    fn find_method(&self, tystr: &str, name: &str) -> Option<String> {
        self.methods.get(&(tystr.to_string(), name.to_string())).and_then(|v| v.first().cloned())
    }

    fn method_call(&mut self, receiver: &Expr, name: &str, args: &[Arg]) -> R<V<'ctx>> {
        let rty = self.ty_of(receiver)?;
        let mut base = rty.clone();
        while let Ty::Ref(_, i) = base {
            base = *i;
        }
        if let Ty::DynTrait(_) = base {
            return unsupported("method calls on `dyn Trait` (dynamic dispatch)", 11);
        }
        let tystr = base.to_string();
        let Some(key) = self.find_method(&tystr, name) else {
            return Err(CgErr(format!("no method named `{}` found for type `{}` (standard-library methods arrive with the prelude in Phase 16)", name, tystr)));
        };
        let Some(kind) = self.fns[&key].self_kind else {
            return Err(CgErr(format!("`{}::{}` is an associated function without `self`: call it as `{}::{}(..)`", tystr, name, tystr, name)));
        };
        let recv: V<'ctx> = match kind {
            PMode::Ref(_) => {
                if matches!(rty, Ty::Ref(..)) {
                    let mut v = self.expr(receiver)?;
                    let mut cur = rty.clone();
                    while let Ty::Ref(_, i) = &cur {
                        if matches!(**i, Ty::Ref(..)) {
                            let lt = self.llty(i)?;
                            v = self.builder.build_load(lt, v.into_pointer_value(), "deref")?;
                            cur = (**i).clone();
                        } else {
                            break;
                        }
                    }
                    v
                } else {
                    self.place_or_spill(receiver)?.0.into()
                }
            }
            _ => {
                if let Ty::Ref(_, inner) = &rty {
                    let p = self.expr(receiver)?.into_pointer_value();
                    self.load(p, inner)?
                } else {
                    self.expr_as(receiver, &base)?
                }
            }
        };
        let display = format!("{}::{}", tystr, name);
        self.call_fn(&key, &display, Some(recv), args)
    }

    fn assign(&mut self, op: AssignOp, lhs: &Expr, rhs: &Expr) -> R<()> {
        let mut root = lhs;
        loop {
            match root {
                Expr::Paren(i) => root = i,
                Expr::Field { expr, .. } | Expr::Index { expr, .. } => root = expr,
                _ => break,
            }
        }
        if let Expr::Ident(n) = root {
            if self.lookup_local(n).is_none() && (self.statics.contains_key(n.as_str()) || self.consts.contains_key(n.as_str())) {
                return Err(CgErr(format!("cannot assign to the constant/static `{}` (statics are immutable; interior mutability needs `Atomic<T>`, Phase 13)", n)));
            }
        }
        let (p, lty) = self.place(lhs)?;
        if op == AssignOp::Eq {
            let rty = self.ty_of(rhs)?;
            let v = self.expr_as(rhs, &lty)?;
            if rty != Ty::Never {
                self.builder.build_store(p, v)?;
            }
            return Ok(());
        }
        let bop = match op {
            AssignOp::PlusEq => BinaryOp::Add,
            AssignOp::MinusEq => BinaryOp::Sub,
            AssignOp::StarEq => BinaryOp::Mul,
            AssignOp::SlashEq => BinaryOp::Div,
            AssignOp::PercentEq => BinaryOp::Mod,
            AssignOp::AmpEq => BinaryOp::BitAnd,
            AssignOp::PipeEq => BinaryOp::BitOr,
            AssignOp::CaretEq => BinaryOp::BitXor,
            AssignOp::ShlEq => BinaryOp::Shl,
            AssignOp::ShrEq => BinaryOp::Shr,
            AssignOp::Eq => unreachable!(),
        };
        self.reject_literal_zero_divisor(bop, rhs, &lty)?;
        let r = self.expr_as(rhs, &lty)?;
        let cur = self.load(p, &lty)?;
        let nv = self.arith(bop, cur, r, &lty)?;
        self.builder.build_store(p, nv)?;
        Ok(())
    }

    // ---- operators -------------------------------------------------------

    fn reject_literal_zero_divisor(&self, op: BinaryOp, rhs: &Expr, ty: &Ty) -> R<()> {
        if matches!(op, BinaryOp::Div | BinaryOp::Mod) && is_int(ty) {
            let mut r = rhs;
            while let Expr::Paren(i) = r {
                r = i;
            }
            if let Expr::Literal(Literal::Int(t)) = r {
                if parse_int_text(t, None) == Some(0) {
                    return Err(CgErr("division or remainder by the constant zero (Document 4 §1: a statically-detectable zero divisor is a compile error)".into()));
                }
            }
        }
        Ok(())
    }

    fn binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr, res: &Ty) -> R<V<'ctx>> {
        match op {
            BinaryOp::AndAnd | BinaryOp::OrOr => {
                let slot = self.alloca(self.int_ty(1).into(), "sc")?;
                let l = self.expr_as(lhs, &Ty::Bool)?.into_int_value();
                self.builder.build_store(slot, l)?;
                let rhs_bb = self.new_block("sc_rhs");
                let end_bb = self.new_block("sc_end");
                if op == BinaryOp::AndAnd {
                    self.builder.build_conditional_branch(l, rhs_bb, end_bb)?;
                } else {
                    self.builder.build_conditional_branch(l, end_bb, rhs_bb)?;
                }
                self.position(rhs_bb);
                let r = self.expr_as(rhs, &Ty::Bool)?;
                self.builder.build_store(slot, r)?;
                self.builder.build_unconditional_branch(end_bb)?;
                self.position(end_bb);
                self.load(slot, &Ty::Bool)
            }
            BinaryOp::Coalesce => self.coalesce(lhs, rhs, res),
            _ => {
                let lt = self.ty_of(lhs)?;
                self.reject_literal_zero_divisor(op, rhs, &lt)?;
                let l = self.expr_as(lhs, &lt)?;
                let r = self.expr_as(rhs, &lt)?;
                match op {
                    BinaryOp::EqEq | BinaryOp::NotEq | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::LtEq | BinaryOp::GtEq => self.compare(op, l, r, &lt),
                    _ => self.arith(op, l, r, &lt),
                }
            }
        }
    }

    fn coalesce(&mut self, lhs: &Expr, rhs: &Expr, res: &Ty) -> R<V<'ctx>> {
        let lty = self.ty_of(lhs)?;
        let lv = self.expr(lhs)?;
        let place = self.spill(lv, &lty)?;
        let (ok_idx, payload_ty) = match &lty {
            Ty::OptionTy(inner) => (0usize, (**inner).clone()),
            Ty::ResultTy(ok, _) => (0usize, (**ok).clone()),
            other => return Err(CgErr(format!("`??` needs an `Option` or `Result` on the left, found `{}`", other))),
        };
        let slot = if is_unit_like(res) { None } else { Some(self.alloca_ty(res, "coalesce")?) };
        let tag = self.enum_tag(&lty, place)?;
        let is_ok = self.builder.build_int_compare(IntPredicate::EQ, tag, self.int_ty(32).const_int(ok_idx as u64, false), "is_some")?;
        let some_bb = self.new_block("co_some");
        let none_bb = self.new_block("co_none");
        let end_bb = self.new_block("co_end");
        self.builder.build_conditional_branch(is_ok, some_bb, none_bb)?;
        self.position(some_bb);
        if let Some(s) = slot {
            let fp = self.payload_ptr(&lty, place, ok_idx, 0)?;
            let v = self.load(fp, &payload_ty)?;
            self.builder.build_store(s, v)?;
        }
        self.builder.build_unconditional_branch(end_bb)?;
        self.position(none_bb);
        let rty = self.ty_of(rhs)?;
        let rv = self.expr(rhs)?;
        if let (Some(s), true) = (slot, rty != Ty::Never) {
            self.builder.build_store(s, rv)?;
        }
        self.builder.build_unconditional_branch(end_bb)?;
        self.position(end_bb);
        match slot {
            Some(s) => self.load(s, res),
            None => Ok(self.unit()),
        }
    }

    fn checked_op(&mut self, base: &str, l: IntValue<'ctx>, r: IntValue<'ctx>, signed: bool, msg: &str) -> R<IntValue<'ctx>> {
        let name = format!("llvm.{}{}.with.overflow", if signed { "s" } else { "u" }, base);
        let intr = Intrinsic::find(&name).ok_or_else(|| CgErr(format!("internal: missing LLVM intrinsic {}", name)))?;
        let decl = intr.get_declaration(&self.module, &[l.get_type().into()]).ok_or_else(|| CgErr(format!("internal: cannot declare {}", name)))?;
        let call = self.builder.build_call(decl, &[l.into(), r.into()], "ovf")?;
        let ValueKind::Basic(agg) = call.try_as_basic_value() else { return Err(CgErr("internal: overflow intrinsic result".into())) };
        let agg = agg.into_struct_value();
        let res = self.builder.build_extract_value(agg, 0, "res")?.into_int_value();
        let flag = self.builder.build_extract_value(agg, 1, "flag")?.into_int_value();
        self.panic_if(flag, msg)?;
        Ok(res)
    }

    pub(super) fn arith(&mut self, op: BinaryOp, l: V<'ctx>, r: V<'ctx>, ty: &Ty) -> R<V<'ctx>> {
        if is_float(ty) {
            let (l, r) = (l.into_float_value(), r.into_float_value());
            return Ok(match op {
                BinaryOp::Add => self.builder.build_float_add(l, r, "")?.into(),
                BinaryOp::Sub => self.builder.build_float_sub(l, r, "")?.into(),
                BinaryOp::Mul => self.builder.build_float_mul(l, r, "")?.into(),
                BinaryOp::Div => self.builder.build_float_div(l, r, "")?.into(),
                BinaryOp::Mod => self.builder.build_float_rem(l, r, "")?.into(),
                BinaryOp::Pow => {
                    let name = if *ty == Ty::F32 { "llvm.pow" } else { "llvm.pow" };
                    let intr = Intrinsic::find(name).ok_or_else(|| CgErr("internal: missing llvm.pow".into()))?;
                    let decl = intr.get_declaration(&self.module, &[l.get_type().into()]).ok_or_else(|| CgErr("internal: cannot declare llvm.pow".into()))?;
                    let call = self.builder.build_call(decl, &[l.into(), r.into()], "pow")?;
                    let ValueKind::Basic(v) = call.try_as_basic_value() else { return Err(CgErr("internal: pow result".into())) };
                    v
                }
                _ => return Err(CgErr(format!("operator `{:?}` is not defined for `{}`", op, ty))),
            });
        }
        if !(is_int(ty) || *ty == Ty::Bool) {
            return Err(CgErr(format!("operator `{:?}` is not supported for type `{}` by codegen yet", op, ty)));
        }
        let (l, r) = (l.into_int_value(), r.into_int_value());
        let signed = is_signed(ty);
        let release = self.opts.release;
        if *ty == Ty::Bool && !matches!(op, BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor) {
            return Err(CgErr(format!("operator `{:?}` is not defined for `bool`", op)));
        }
        Ok(match op {
            BinaryOp::Add => {
                if release { self.builder.build_int_add(l, r, "")?.into() } else { self.checked_op("add", l, r, signed, "attempt to add with overflow")?.into() }
            }
            BinaryOp::Sub => {
                if release { self.builder.build_int_sub(l, r, "")?.into() } else { self.checked_op("sub", l, r, signed, "attempt to subtract with overflow")?.into() }
            }
            BinaryOp::Mul => {
                if release { self.builder.build_int_mul(l, r, "")?.into() } else { self.checked_op("mul", l, r, signed, "attempt to multiply with overflow")?.into() }
            }
            BinaryOp::Div | BinaryOp::Mod => {
                let is_div = op == BinaryOp::Div;
                let zero = l.get_type().const_zero();
                let z = self.builder.build_int_compare(IntPredicate::EQ, r, zero, "dz")?;
                self.panic_if(z, if is_div { "attempt to divide by zero" } else { "attempt to calculate the remainder with a divisor of zero" })?;
                if signed {
                    let bits = l.get_type().get_bit_width();
                    let min: IntValue = if bits > 64 { l.get_type().const_int_arbitrary_precision(&[0, 1u64 << 63]) } else { l.get_type().const_int(1u64 << (bits - 1), false) };
                    let minus1 = l.get_type().const_all_ones();
                    let is_min = self.builder.build_int_compare(IntPredicate::EQ, l, min, "ismin")?;
                    let is_m1 = self.builder.build_int_compare(IntPredicate::EQ, r, minus1, "ism1")?;
                    let both = self.builder.build_and(is_min, is_m1, "ovf")?;
                    self.panic_if(both, if is_div { "attempt to divide with overflow" } else { "attempt to calculate the remainder with overflow" })?;
                    if is_div { self.builder.build_int_signed_div(l, r, "")?.into() } else { self.builder.build_int_signed_rem(l, r, "")?.into() }
                } else if is_div {
                    self.builder.build_int_unsigned_div(l, r, "")?.into()
                } else {
                    self.builder.build_int_unsigned_rem(l, r, "")?.into()
                }
            }
            BinaryOp::BitAnd => self.builder.build_and(l, r, "")?.into(),
            BinaryOp::BitOr => self.builder.build_or(l, r, "")?.into(),
            BinaryOp::BitXor => self.builder.build_xor(l, r, "")?.into(),
            BinaryOp::Shl | BinaryOp::Shr => {
                let bits = l.get_type().get_bit_width();
                let amt = if release {
                    let mask = l.get_type().const_int((bits - 1) as u64, false);
                    self.builder.build_and(r, mask, "amt")?
                } else {
                    let limit = l.get_type().const_int(bits as u64, false);
                    let too_big = self.builder.build_int_compare(IntPredicate::UGE, r, limit, "shov")?;
                    self.panic_if(too_big, if op == BinaryOp::Shl { "attempt to shift left with overflow" } else { "attempt to shift right with overflow" })?;
                    r
                };
                if op == BinaryOp::Shl {
                    self.builder.build_left_shift(l, amt, "")?.into()
                } else {
                    self.builder.build_right_shift(l, amt, signed, "")?.into()
                }
            }
            BinaryOp::Pow => self.int_pow(l, r, ty)?,
            _ => return Err(CgErr(format!("internal: operator `{:?}` reached `arith`", op))),
        })
    }

    /// Integer `**` by square-and-multiply; uses the same overflow policy as `*`.
    fn int_pow(&mut self, base: IntValue<'ctx>, exp: IntValue<'ctx>, ty: &Ty) -> R<V<'ctx>> {
        let it = base.get_type();
        if is_signed(ty) {
            let neg = self.builder.build_int_compare(IntPredicate::SLT, exp, it.const_zero(), "negexp")?;
            self.panic_if(neg, "attempt to raise an integer to a negative power")?;
        }
        let lt: BasicTypeEnum = it.into();
        let res = self.alloca(lt, "pow_res")?;
        let b = self.alloca(lt, "pow_base")?;
        let ex = self.alloca(lt, "pow_exp")?;
        self.builder.build_store(res, it.const_int(1, false))?;
        self.builder.build_store(b, base)?;
        self.builder.build_store(ex, exp)?;
        let head = self.new_block("pow_head");
        let body = self.new_block("pow_body");
        let mulres = self.new_block("pow_mulres");
        let step = self.new_block("pow_step");
        let sq = self.new_block("pow_sq");
        let latch = self.new_block("pow_latch");
        let end = self.new_block("pow_end");
        self.builder.build_unconditional_branch(head)?;
        self.position(head);
        let e0 = self.builder.build_load(it, ex, "")?.into_int_value();
        let nz = self.builder.build_int_compare(IntPredicate::NE, e0, it.const_zero(), "")?;
        self.builder.build_conditional_branch(nz, body, end)?;
        self.position(body);
        let e1 = self.builder.build_load(it, ex, "")?.into_int_value();
        let low = self.builder.build_and(e1, it.const_int(1, false), "")?;
        let odd = self.builder.build_int_compare(IntPredicate::NE, low, it.const_zero(), "")?;
        self.builder.build_conditional_branch(odd, mulres, step)?;
        self.position(mulres);
        let cr = self.builder.build_load(it, res, "")?;
        let cb = self.builder.build_load(it, b, "")?;
        let nr = self.arith(BinaryOp::Mul, cr, cb, ty)?;
        self.builder.build_store(res, nr)?;
        self.builder.build_unconditional_branch(step)?;
        self.position(step);
        let e2 = self.builder.build_load(it, ex, "")?.into_int_value();
        let sh = self.builder.build_right_shift(e2, it.const_int(1, false), false, "")?;
        self.builder.build_store(ex, sh)?;
        let more = self.builder.build_int_compare(IntPredicate::NE, sh, it.const_zero(), "")?;
        self.builder.build_conditional_branch(more, sq, latch)?;
        self.position(sq);
        let cb2 = self.builder.build_load(it, b, "")?;
        let nb = self.arith(BinaryOp::Mul, cb2, cb2, ty)?;
        self.builder.build_store(b, nb)?;
        self.builder.build_unconditional_branch(latch)?;
        self.position(latch);
        self.builder.build_unconditional_branch(head)?;
        self.position(end);
        Ok(self.builder.build_load(it, res, "pow")?)
    }

    fn compare(&mut self, op: BinaryOp, l: V<'ctx>, r: V<'ctx>, ty: &Ty) -> R<V<'ctx>> {
        if is_float(ty) {
            let pred = match op {
                BinaryOp::EqEq => FloatPredicate::OEQ,
                BinaryOp::NotEq => FloatPredicate::UNE,
                BinaryOp::Lt => FloatPredicate::OLT,
                BinaryOp::Gt => FloatPredicate::OGT,
                BinaryOp::LtEq => FloatPredicate::OLE,
                _ => FloatPredicate::OGE,
            };
            return Ok(self.builder.build_float_compare(pred, l.into_float_value(), r.into_float_value(), "")?.into());
        }
        if is_int(ty) || matches!(ty, Ty::Bool | Ty::Char) {
            let s = is_signed(ty);
            let pred = match (op, s) {
                (BinaryOp::EqEq, _) => IntPredicate::EQ,
                (BinaryOp::NotEq, _) => IntPredicate::NE,
                (BinaryOp::Lt, true) => IntPredicate::SLT,
                (BinaryOp::Lt, false) => IntPredicate::ULT,
                (BinaryOp::Gt, true) => IntPredicate::SGT,
                (BinaryOp::Gt, false) => IntPredicate::UGT,
                (BinaryOp::LtEq, true) => IntPredicate::SLE,
                (BinaryOp::LtEq, false) => IntPredicate::ULE,
                (_, true) => IntPredicate::SGE,
                (_, false) => IntPredicate::UGE,
            };
            return Ok(self.builder.build_int_compare(pred, l.into_int_value(), r.into_int_value(), "")?.into());
        }
        Err(CgErr(format!("comparing values of type `{}` is not yet supported by codegen — Phase 16", ty)))
    }

    fn unary(&mut self, op: UnaryOp, inner: &Expr, whole: &Expr) -> R<V<'ctx>> {
        let ty = self.ty_of(whole)?;
        match op {
            UnaryOp::Not => {
                let v = self.expr_as(inner, &Ty::Bool)?.into_int_value();
                Ok(self.builder.build_not(v, "")?.into())
            }
            UnaryOp::BitNot => {
                let v = self.expr_as(inner, &ty)?;
                if !(is_int(&ty) || ty == Ty::Bool) {
                    return Err(CgErr(format!("`~` is not defined for `{}`", ty)));
                }
                Ok(self.builder.build_not(v.into_int_value(), "")?.into())
            }
            UnaryOp::Neg => {
                // `-5`, `-128i8`: fold into the literal so the magnitude check is exact.
                let mut lit = inner;
                while let Expr::Paren(i) = lit {
                    lit = i;
                }
                if let Expr::Literal(l @ (Literal::Int(_) | Literal::IntHex(_) | Literal::IntOct(_) | Literal::IntBin(_) | Literal::Float(_))) = lit {
                    return self.literal(l, &ty, true);
                }
                let v = self.expr_as(inner, &ty)?;
                if is_float(&ty) {
                    return Ok(self.builder.build_float_neg(v.into_float_value(), "")?.into());
                }
                if !is_signed(&ty) {
                    return Err(CgErr(format!("unary `-` cannot be applied to the unsigned type `{}`", ty)));
                }
                let it = v.into_int_value();
                if self.opts.release {
                    Ok(self.builder.build_int_neg(it, "")?.into())
                } else {
                    Ok(self.checked_op("sub", it.get_type().const_zero(), it, true, "attempt to negate with overflow")?.into())
                }
            }
        }
    }

    fn cast(&mut self, v: V<'ctx>, from: &Ty, to: &Ty) -> R<V<'ctx>> {
        if from == to {
            return Ok(v);
        }
        let to_lt = self.llty(to)?;
        let from_intlike = is_int(from) || matches!(from, Ty::Bool | Ty::Char);
        if from_intlike && is_int(to) {
            let (iv, dt) = (v.into_int_value(), to_lt.into_int_type());
            let (fb, tb) = (iv.get_type().get_bit_width(), dt.get_bit_width());
            let signed_src = is_signed(from);
            return Ok(if fb == tb {
                v
            } else if fb > tb {
                self.builder.build_int_truncate(iv, dt, "")?.into()
            } else if signed_src {
                self.builder.build_int_s_extend(iv, dt, "")?.into()
            } else {
                self.builder.build_int_z_extend(iv, dt, "")?.into()
            });
        }
        if is_int(from) && is_float(to) {
            let (iv, ft) = (v.into_int_value(), to_lt.into_float_type());
            return Ok(if is_signed(from) { self.builder.build_signed_int_to_float(iv, ft, "")?.into() } else { self.builder.build_unsigned_int_to_float(iv, ft, "")?.into() });
        }
        if is_float(from) && is_int(to) {
            let name = if is_signed(to) { "llvm.fptosi.sat" } else { "llvm.fptoui.sat" };
            let intr = Intrinsic::find(name).ok_or_else(|| CgErr("internal: missing saturating float->int intrinsic".into()))?;
            let decl = intr.get_declaration(&self.module, &[to_lt, v.get_type()]).ok_or_else(|| CgErr("internal: cannot declare saturating cast".into()))?;
            let call = self.builder.build_call(decl, &[v.into()], "cast")?;
            let ValueKind::Basic(r) = call.try_as_basic_value() else { return Err(CgErr("internal: cast result".into())) };
            return Ok(r);
        }
        if is_float(from) && is_float(to) {
            return Ok(self.builder.build_float_cast(v.into_float_value(), to_lt.into_float_type(), "")?.into());
        }
        if matches!(from, Ty::Bool) && is_int(to) {
            return Ok(self.builder.build_int_z_extend(v.into_int_value(), to_lt.into_int_type(), "")?.into());
        }
        if *from == Ty::U8 && *to == Ty::Char {
            return Ok(self.builder.build_int_z_extend(v.into_int_value(), to_lt.into_int_type(), "")?.into());
        }
        if let (Ty::Named(n), true) = (from, is_int(to)) {
            if self.enums.get(n).map(|vs| vs.iter().all(|(_, f)| f.is_empty())).unwrap_or(false) {
                let place = self.spill(v, from)?;
                let tag = self.enum_tag(from, place)?;
                return self.cast(tag.into(), &Ty::U32, to);
            }
        }
        Err(CgErr(format!("casting `{}` to `{}` with `as` is not supported", from, to)))
    }

    // ---- control flow ------------------------------------------------------

    fn gen_if(&mut self, i: &IfExpr, ty: &Ty) -> R<V<'ctx>> {
        let slot = if is_unit_like(ty) { None } else { Some(self.alloca_ty(ty, "if")?) };
        let cond = self.expr_as(&i.cond, &Ty::Bool)?.into_int_value();
        let then_bb = self.new_block("then");
        let end_bb = self.new_block("endif");
        let else_bb = if i.else_branch.is_some() { self.new_block("else") } else { end_bb };
        self.builder.build_conditional_branch(cond, then_bb, else_bb)?;
        self.position(then_bb);
        let (tv, tty) = self.gen_block(&i.then_block)?;
        if let (Some(s), true) = (slot, tty != Ty::Never) {
            self.builder.build_store(s, tv)?;
        }
        self.builder.build_unconditional_branch(end_bb)?;
        match &i.else_branch {
            Some(ElseBranch::Block(b)) => {
                self.position(else_bb);
                let (ev, ety) = self.gen_block(b)?;
                if let (Some(s), true) = (slot, ety != Ty::Never) {
                    self.builder.build_store(s, ev)?;
                }
                self.builder.build_unconditional_branch(end_bb)?;
            }
            Some(ElseBranch::If(inner)) => {
                self.position(else_bb);
                let ev = self.gen_if(inner, ty)?;
                if let Some(s) = slot {
                    self.builder.build_store(s, ev)?;
                }
                self.builder.build_unconditional_branch(end_bb)?;
            }
            None => {}
        }
        self.position(end_bb);
        match slot {
            Some(s) => self.load(s, ty),
            None => Ok(self.unit()),
        }
    }

    fn gen_loop(&mut self, l: &LoopExpr, ty: &Ty) -> R<V<'ctx>> {
        match l {
            LoopExpr::Loop { label, body } => {
                let result = if is_unit_like(ty) { None } else { Some((self.alloca_ty(ty, "loop_res")?, ty.clone())) };
                let head = self.new_block("loop");
                let end = self.new_block("loop_end");
                self.builder.build_unconditional_branch(head)?;
                self.position(head);
                self.loops.push(LoopCtx { label: label.clone(), break_bb: end, continue_bb: head, result: result.clone() });
                let r = self.gen_block(body);
                self.loops.pop();
                r?;
                self.builder.build_unconditional_branch(head)?;
                self.position(end);
                match result {
                    Some((s, t)) => self.load(s, &t),
                    None => Ok(self.unit()),
                }
            }
            LoopExpr::While { label, cond, body } => {
                let head = self.new_block("while");
                let body_bb = self.new_block("while_body");
                let end = self.new_block("while_end");
                self.builder.build_unconditional_branch(head)?;
                self.position(head);
                let c = self.expr_as(cond, &Ty::Bool)?.into_int_value();
                self.builder.build_conditional_branch(c, body_bb, end)?;
                self.position(body_bb);
                self.loops.push(LoopCtx { label: label.clone(), break_bb: end, continue_bb: head, result: None });
                let r = self.gen_block(body);
                self.loops.pop();
                r?;
                self.builder.build_unconditional_branch(head)?;
                self.position(end);
                Ok(self.unit())
            }
            LoopExpr::DoWhile { body, cond } => {
                let body_bb = self.new_block("do");
                let cond_bb = self.new_block("do_cond");
                let end = self.new_block("do_end");
                self.builder.build_unconditional_branch(body_bb)?;
                self.position(body_bb);
                self.loops.push(LoopCtx { label: None, break_bb: end, continue_bb: cond_bb, result: None });
                let r = self.gen_block(body);
                self.loops.pop();
                r?;
                self.builder.build_unconditional_branch(cond_bb)?;
                self.position(cond_bb);
                let c = self.expr_as(cond, &Ty::Bool)?.into_int_value();
                self.builder.build_conditional_branch(c, body_bb, end)?;
                self.position(end);
                Ok(self.unit())
            }
            LoopExpr::For { label, pattern, iter, body } => self.gen_for(label, pattern, iter, body),
        }
    }

    /// `for i in lo..hi` / `lo..=hi` (Document 9 §3.3). Other iterables need
    /// the `Iterable` trait (Phase 16).
    fn gen_for(&mut self, label: &Option<String>, pattern: &Pattern, iter: &Expr, body: &Block) -> R<V<'ctx>> {
        let mut it = iter;
        while let Expr::Paren(i) = it {
            it = i;
        }
        let Expr::Range { lo, hi, inclusive } = it else {
            return unsupported("`for` over anything but an integer range (needs the `Iterable` trait)", 16);
        };
        let ity = self.ty_of(lo)?;
        if !is_int(&ity) {
            return Err(CgErr(format!("`for` over a range of `{}` is not supported (integer ranges only)", ity)));
        }
        let lt = self.llty(&ity)?.into_int_type();
        let signed = is_signed(&ity);
        let lo_v = self.expr_as(lo, &ity)?;
        let hi_v = self.expr_as(hi, &ity)?;
        let cur = self.alloca(lt.into(), "for_i")?;
        let hi_slot = self.alloca(lt.into(), "for_hi")?;
        self.builder.build_store(cur, lo_v)?;
        self.builder.build_store(hi_slot, hi_v)?;
        let head = self.new_block("for");
        let body_bb = self.new_block("for_body");
        let step = self.new_block("for_step");
        let end = self.new_block("for_end");
        self.builder.build_unconditional_branch(head)?;
        self.position(head);
        let i = self.builder.build_load(lt, cur, "i")?.into_int_value();
        let h = self.builder.build_load(lt, hi_slot, "hi")?.into_int_value();
        let pred = match (inclusive, signed) {
            (false, true) => IntPredicate::SLT,
            (false, false) => IntPredicate::ULT,
            (true, true) => IntPredicate::SLE,
            (true, false) => IntPredicate::ULE,
        };
        let go = self.builder.build_int_compare(pred, i, h, "go")?;
        self.builder.build_conditional_branch(go, body_bb, end)?;
        self.position(body_bb);
        self.locals.push(HashMap::new());
        match pattern {
            Pattern::Ident(n) | Pattern::Mut(n) => {
                let slot = self.declare_local(n, &ity)?;
                self.builder.build_store(slot, i)?;
            }
            Pattern::Wildcard => {}
            _ => return unsupported("destructuring patterns in `for`", 16),
        }
        self.loops.push(LoopCtx { label: label.clone(), break_bb: end, continue_bb: step, result: None });
        let r = self.gen_block(body);
        self.loops.pop();
        self.locals.pop();
        r?;
        self.builder.build_unconditional_branch(step)?;
        self.position(step);
        let i2 = self.builder.build_load(lt, cur, "i")?.into_int_value();
        if *inclusive {
            // Stop BEFORE incrementing past the bound: `0..=255u8` must not wrap.
            let h2 = self.builder.build_load(lt, hi_slot, "hi")?.into_int_value();
            let at_end = self.builder.build_int_compare(IntPredicate::EQ, i2, h2, "last")?;
            let inc_bb = self.new_block("for_inc");
            self.builder.build_conditional_branch(at_end, end, inc_bb)?;
            self.position(inc_bb);
        }
        let next = self.builder.build_int_add(i2, lt.const_int(1, false), "next")?;
        self.builder.build_store(cur, next)?;
        self.builder.build_unconditional_branch(head)?;
        self.position(end);
        Ok(self.unit())
    }

    // ---- patterns / match -----------------------------------------------------

    fn irrefutable(&self, p: &Pattern, ty: &Ty) -> bool {
        match p {
            Pattern::Wildcard | Pattern::Mut(_) => true,
            Pattern::Ident(n) => !n.contains("::") && self.variant_index(n, ty).map(|i| self.variants_of(ty).map(|v| !v[i].1.is_empty()).unwrap_or(false)).unwrap_or(true),
            Pattern::Tuple(subs) => match ty {
                Ty::Tuple(ts) if ts.len() == subs.len() => subs.iter().zip(ts.iter()).all(|(p, t)| self.irrefutable(p, t)),
                _ => false,
            },
            _ => false,
        }
    }

    fn sw_kind(&self, p: &Pattern, sty: &Ty) -> Option<SwKind> {
        let scalar = is_int(sty) || matches!(sty, Ty::Bool | Ty::Char);
        match p {
            Pattern::Wildcard | Pattern::Mut(_) => Some(SwKind::CatchAll),
            Pattern::Ident(n) => match self.variant_index(n, sty) {
                Some(i) if self.variants_of(sty).map(|v| v[i].1.is_empty()).unwrap_or(false) => Some(SwKind::Const(i as u64)),
                _ if !n.contains("::") => Some(SwKind::CatchAll),
                _ => None,
            },
            Pattern::Literal(l) if scalar => match l {
                Literal::Int(t) => parse_int_text(t, None).filter(|v| *v <= u64::MAX as u128).map(|v| SwKind::Const(v as u64)),
                Literal::IntHex(t) => parse_int_text(t, Some(("0x", 16))).filter(|v| *v <= u64::MAX as u128).map(|v| SwKind::Const(v as u64)),
                Literal::IntOct(t) => parse_int_text(t, Some(("0o", 8))).filter(|v| *v <= u64::MAX as u128).map(|v| SwKind::Const(v as u64)),
                Literal::IntBin(t) => parse_int_text(t, Some(("0b", 2))).filter(|v| *v <= u64::MAX as u128).map(|v| SwKind::Const(v as u64)),
                Literal::Bool(b) => Some(SwKind::Const(*b as u64)),
                Literal::Char(t) => {
                    let inner = t.strip_prefix('\'')?.strip_suffix('\'')?;
                    let mut it = inner.chars().peekable();
                    unescape_char(&mut it).map(|c| SwKind::Const(c as u64))
                }
                _ => None,
            },
            Pattern::TupleStruct(name, subs) => {
                let variants = self.variants_of(sty)?;
                let i = self.variant_index(name, sty)?;
                let ftys = &variants[i].1;
                if ftys.len() == subs.len() && subs.iter().zip(ftys.iter()).all(|(p, t)| self.irrefutable(p, t)) {
                    Some(SwKind::Const(i as u64))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn switchable(&self, arms: &[MatchArm], sty: &Ty) -> bool {
        let scalar = is_int(sty) || matches!(sty, Ty::Bool | Ty::Char);
        if !(scalar || self.variants_of(sty).is_some()) {
            return false;
        }
        if scalar && matches!(sty, Ty::I128 | Ty::U128) {
            return false;
        }
        arms.iter().all(|a| {
            let flat = flatten_patterns(&a.patterns);
            a.guard.is_none()
                && flat.iter().all(|p| self.sw_kind(p, sty).is_some())
                && (flat.len() == 1 || !flat.iter().any(|p| self.pattern_really_binds(p, sty)))
        })
    }

    /// Does this pattern introduce a variable (as opposed to naming a unit variant)?
    fn pattern_really_binds(&self, p: &Pattern, ty: &Ty) -> bool {
        match p {
            Pattern::Mut(_) => true,
            Pattern::Ident(n) => !n.contains("::") && self.variant_index(n, ty).is_none(),
            Pattern::Tuple(s) => match ty {
                Ty::Tuple(ts) => s.iter().zip(ts.iter()).any(|(p, t)| self.pattern_really_binds(p, t)),
                _ => true,
            },
            Pattern::TupleStruct(name, subs) => {
                let Some(variants) = self.variants_of(ty) else {
                    return subs.iter().any(|p| pattern_binds(p));
                };
                match self.variant_index(name, ty) {
                    Some(i) => subs.iter().zip(variants[i].1.iter()).any(|(p, t)| self.pattern_really_binds(p, t)),
                    None => true,
                }
            }
            Pattern::Or(alts) => alts.iter().any(|a| self.pattern_really_binds(a, ty)),
            _ => false,
        }
    }

    /// Tests `pat` against the value stored at `place`; collects variable
    /// bindings. With `test == false` it only collects bindings (the caller
    /// already knows the pattern matches).
    fn pat_test(&mut self, pat: &Pattern, place: PointerValue<'ctx>, ty: &Ty, binds: &mut Vec<Bind<'ctx>>, test: bool) -> R<IntValue<'ctx>> {
        let t = self.int_ty(1).const_int(1, false);
        match pat {
            Pattern::Wildcard => Ok(t),
            Pattern::Mut(n) => {
                binds.push(Bind { name: n.clone(), ptr: place, ty: ty.clone() });
                Ok(t)
            }
            Pattern::Ident(n) => {
                if let Some(i) = self.variant_index(n, ty) {
                    if self.variants_of(ty).map(|v| v[i].1.is_empty()).unwrap_or(false) {
                        return if test { self.tag_is(ty, place, i) } else { Ok(t) };
                    }
                }
                if n.contains("::") {
                    return Err(CgErr(format!("unknown pattern `{}` for type `{}`", n, ty)));
                }
                binds.push(Bind { name: n.clone(), ptr: place, ty: ty.clone() });
                Ok(t)
            }
            Pattern::Literal(l) => {
                if !test {
                    return Ok(t);
                }
                let v = self.load(place, ty)?;
                let lit = match l {
                    Literal::Str(_) | Literal::RawStr(_) => return Err(CgErr("matching on string literals is not yet supported by codegen — Phase 16".into())),
                    other => self.literal(other, ty, false)?,
                };
                Ok(self.compare(BinaryOp::EqEq, v, lit, ty)?.into_int_value())
            }
            Pattern::Tuple(subs) => {
                let Ty::Tuple(tys) = ty else { return Err(CgErr(format!("a tuple pattern cannot match `{}`", ty))) };
                if tys.len() != subs.len() {
                    return Err(CgErr("tuple pattern arity mismatch".into()));
                }
                let tys = tys.clone();
                let st = self.llty(ty)?;
                let mut acc = t;
                for (i, (p, ft)) in subs.iter().zip(tys.iter()).enumerate() {
                    let fp = self.builder.build_struct_gep(st, place, i as u32, "tp")?;
                    let c = self.pat_test(p, fp, ft, binds, test)?;
                    if test {
                        acc = self.builder.build_and(acc, c, "")?;
                    }
                }
                Ok(acc)
            }
            Pattern::TupleStruct(name, subs) => {
                if let Some(variants) = self.variants_of(ty) {
                    let Some(idx) = self.variant_index(name, ty) else {
                        return Err(CgErr(format!("`{}` is not a variant of `{}`", name, ty)));
                    };
                    let ftys = variants[idx].1.clone();
                    if ftys.len() != subs.len() {
                        return Err(CgErr(format!("`{}` has {} field(s), the pattern has {}", name, ftys.len(), subs.len())));
                    }
                    let mut acc = if test { self.tag_is(ty, place, idx)? } else { t };
                    for (i, (p, ft)) in subs.iter().zip(ftys.iter()).enumerate() {
                        let fp = self.payload_ptr(ty, place, idx, i)?;
                        let c = self.pat_test(p, fp, ft, binds, test)?;
                        if test {
                            acc = self.builder.build_and(acc, c, "")?;
                        }
                    }
                    return Ok(acc);
                }
                if let Ty::Named(n) = ty {
                    if let Some((fields, true)) = self.structs.get(n).cloned() {
                        if n == name && fields.len() == subs.len() {
                            let st = self.llty(ty)?;
                            let mut acc = t;
                            for (i, (p, (_, ft))) in subs.iter().zip(fields.iter()).enumerate() {
                                let fp = self.builder.build_struct_gep(st, place, i as u32, "sp")?;
                                let c = self.pat_test(p, fp, ft, binds, test)?;
                                if test {
                                    acc = self.builder.build_and(acc, c, "")?;
                                }
                            }
                            return Ok(acc);
                        }
                    }
                }
                Err(CgErr(format!("pattern `{}(..)` does not match type `{}`", name, ty)))
            }
            Pattern::Or(alts) => {
                let mut acc = self.int_ty(1).const_zero();
                for a in alts {
                    let before = binds.len();
                    let c = self.pat_test(a, place, ty, binds, test)?;
                    if binds.len() != before {
                        return unsupported("variable bindings inside an or-pattern", 16);
                    }
                    if test {
                        acc = self.builder.build_or(acc, c, "")?;
                    }
                }
                Ok(if test { acc } else { t })
            }
            Pattern::Array(..) => unsupported("array patterns", 11),
        }
    }

    fn tag_is(&mut self, ty: &Ty, place: PointerValue<'ctx>, idx: usize) -> R<IntValue<'ctx>> {
        let tag = self.enum_tag(ty, place)?;
        Ok(self.builder.build_int_compare(IntPredicate::EQ, tag, self.int_ty(32).const_int(idx as u64, false), "is")?)
    }

    fn arm_body(&mut self, body: &MatchArmBody, slot: Option<PointerValue<'ctx>>, merge: BasicBlock<'ctx>) -> R<()> {
        let (v, ty) = match body {
            MatchArmBody::Expr(e) => {
                let ty = self.ty_of(e)?;
                (self.expr(e)?, if self.dead { Ty::Never } else { ty })
            }
            MatchArmBody::Block(b) => self.gen_block(b)?,
        };
        if let (Some(s), true) = (slot, ty != Ty::Never) {
            self.builder.build_store(s, v)?;
        }
        self.builder.build_unconditional_branch(merge)?;
        Ok(())
    }

    fn gen_match(&mut self, m: &MatchExpr, ty: &Ty) -> R<V<'ctx>> {
        let sty = self.ty_of(&m.scrutinee)?;
        let sval = self.expr(&m.scrutinee)?;
        let place = self.spill(sval, &sty)?;
        let slot = if is_unit_like(ty) { None } else { Some(self.alloca_ty(ty, "match")?) };
        let merge = self.new_block("match_end");
        if self.switchable(&m.arms, &sty) {
            self.match_switch(m, &sty, place, slot, merge)?;
        } else {
            self.match_chain(m, &sty, place, slot, merge)?;
        }
        self.position(merge);
        match slot {
            Some(s) => self.load(s, ty),
            None => Ok(self.unit()),
        }
    }

    /// Dense / enum-tag matches: one LLVM `switch` (Document 17 §6).
    fn match_switch(&mut self, m: &MatchExpr, sty: &Ty, place: PointerValue<'ctx>, slot: Option<PointerValue<'ctx>>, merge: BasicBlock<'ctx>) -> R<()> {
        let discr: IntValue = if self.variants_of(sty).is_some() {
            self.enum_tag(sty, place)?
        } else {
            self.load(place, sty)?.into_int_value()
        };
        let origin = self.builder.get_insert_block().unwrap();
        let dty = discr.get_type();
        let arm_bbs: Vec<BasicBlock> = m.arms.iter().map(|_| self.new_block("arm")).collect();
        let mut cases: Vec<(IntValue<'ctx>, BasicBlock<'ctx>)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut default: Option<BasicBlock<'ctx>> = None;
        for (arm, bb) in m.arms.iter().zip(arm_bbs.iter()) {
            for p in flatten_patterns(&arm.patterns) {
                match self.sw_kind(p, sty) {
                    Some(SwKind::Const(c)) => {
                        if seen.insert(c) {
                            cases.push((dty.const_int(c, false), *bb));
                        }
                    }
                    Some(SwKind::CatchAll) => {
                        if default.is_none() {
                            default = Some(*bb);
                        }
                    }
                    None => return Err(CgErr("internal: unswitchable pattern in a switch".into())),
                }
            }
        }
        let default_bb = match default {
            Some(d) => d,
            None => {
                let u = self.new_block("match_unreachable");
                self.builder.position_at_end(u);
                self.builder.build_unreachable()?;
                u
            }
        };
        self.builder.position_at_end(origin);
        self.builder.build_switch(discr, default_bb, &cases)?;
        for (arm, bb) in m.arms.iter().zip(arm_bbs.iter()) {
            self.position(*bb);
            self.locals.push(HashMap::new());
            let flat = flatten_patterns(&arm.patterns);
            let r = (|| -> R<()> {
                if let Some(p) = flat.first() {
                    let mut binds = Vec::new();
                    self.pat_test(p, place, sty, &mut binds, false)?;
                    self.materialize(binds)?;
                }
                self.arm_body(&arm.body, slot, merge)
            })();
            self.locals.pop();
            r?;
        }
        Ok(())
    }

    /// General matches (guards, tuples, structs, floats, ...): a chain of tests.
    fn match_chain(&mut self, m: &MatchExpr, sty: &Ty, place: PointerValue<'ctx>, slot: Option<PointerValue<'ctx>>, merge: BasicBlock<'ctx>) -> R<()> {
        for arm in &m.arms {
            let flat = flatten_patterns(&arm.patterns);
            let mut binds = Vec::new();
            let mut cond = self.int_ty(1).const_zero();
            for p in &flat {
                let before = binds.len();
                let c = self.pat_test(p, place, sty, &mut binds, true)?;
                if flat.len() > 1 && binds.len() != before {
                    return unsupported("variable bindings in an or-pattern arm", 16);
                }
                cond = self.builder.build_or(cond, c, "")?;
            }
            let body_bb = self.new_block("arm");
            let next_bb = self.new_block("arm_next");
            self.builder.build_conditional_branch(cond, body_bb, next_bb)?;
            self.position(body_bb);
            self.locals.push(HashMap::new());
            let res = (|| -> R<()> {
                self.materialize(binds)?;
                if let Some(g) = &arm.guard {
                    let gv = self.expr_as(g, &Ty::Bool)?.into_int_value();
                    let go = self.new_block("guard_ok");
                    self.builder.build_conditional_branch(gv, go, next_bb)?;
                    self.position(go);
                }
                self.arm_body(&arm.body, slot, merge)
            })();
            self.locals.pop();
            res?;
            self.position(next_bb);
        }
        // Exhaustiveness (checked earlier) makes this unreachable.
        self.builder.build_unreachable()?;
        Ok(())
    }

    // ---- `?` --------------------------------------------------------------------

    fn propagate(&mut self, inner: &Expr) -> R<V<'ctx>> {
        let oty = self.ty_of(inner)?;
        let v = self.expr(inner)?;
        let place = self.spill(v, &oty)?;
        let ret = self.ret_ty.clone();
        let (ok_ty, is_result) = match &oty {
            Ty::ResultTy(ok, _) => ((**ok).clone(), true),
            Ty::OptionTy(some) => ((**some).clone(), false),
            other => return Err(CgErr(format!("`?` needs a `Result` or `Option`, found `{}`", other))),
        };
        let tag = self.enum_tag(&oty, place)?;
        let ok_idx = 0u64; // Ok / Some are both variant 0
        let is_ok = self.builder.build_int_compare(IntPredicate::EQ, tag, self.int_ty(32).const_int(ok_idx, false), "is_ok")?;
        let ok_bb = self.new_block("q_ok");
        let err_bb = self.new_block("q_err");
        self.builder.build_conditional_branch(is_ok, ok_bb, err_bb)?;
        self.position(err_bb);
        if is_result {
            let Ty::ResultTy(_, src_err) = &oty else { unreachable!() };
            let Ty::ResultTy(_, tgt_err) = &ret else {
                return Err(CgErr(format!("`?` on a `Result` needs the function to return `Result<_, E>`, it returns `{}`", ret)));
            };
            if src_err != tgt_err {
                return unsupported(&format!("converting the error type `{}` into `{}` with `?` (a `From` impl call)", src_err, tgt_err), 11);
            }
            let ep = self.payload_ptr(&oty, place, 1, 0)?;
            let ev = self.load(ep, src_err)?;
            let out = self.make_enum(&ret, 1, vec![ev])?;
            self.builder.build_return(Some(&out))?;
        } else {
            if !matches!(ret, Ty::OptionTy(_)) {
                return Err(CgErr(format!("`?` on an `Option` needs the function to return `Option<_>`, it returns `{}`", ret)));
            }
            let out = self.make_enum(&ret, 1, Vec::new())?;
            self.builder.build_return(Some(&out))?;
        }
        self.position(ok_bb);
        if is_unit_like(&ok_ty) {
            return Ok(self.unit());
        }
        let fp = self.payload_ptr(&oty, place, 0, 0)?;
        self.load(fp, &ok_ty)
    }

    // ---- calls --------------------------------------------------------------------

    fn call(&mut self, whole: &Expr, callee: &Expr, args: &[Arg]) -> R<V<'ctx>> {
        match callee {
            Expr::Ident(name) => {
                let user_fn = self.fns.contains_key(name.as_str());
                if !user_fn {
                    match name.as_str() {
                        "Ok" | "Err" | "Some" => return self.ctor_call(whole, name, args),
                        "panic" => return self.gen_panic(args),
                        "assert" => return self.gen_assert(args),
                        "ensure" => return self.gen_ensure(whole, args),
                        "print" | "println" | "eprint" | "eprintln" => return self.gen_print(name, args),
                        _ => {}
                    }
                }
                if user_fn {
                    let key = name.clone();
                    return self.call_fn(&key, name, None, args);
                }
                if args.iter().any(|a| a.name.is_some()) {
                    return unsupported("named arguments here", 11);
                }
                if let Some((fields, true)) = self.structs.get(name.as_str()).cloned() {
                    if fields.len() != args.len() {
                        return Err(CgErr(format!("`{}` has {} field(s), found {} argument(s)", name, fields.len(), args.len())));
                    }
                    let mut vals = Vec::new();
                    for (a, (_, t)) in args.iter().zip(fields.iter()) {
                        vals.push(self.expr_as(&a.value, t)?);
                    }
                    return self.build_struct(&Ty::Named(name.clone()), vals);
                }
                if self.lookup_local(name).is_some() {
                    return unsupported("calling a function value / closure", 12);
                }
                Err(CgErr(format!("unknown function `{}` (standard-library functions arrive with the prelude in Phase 16)", name)))
            }
            Expr::Path(path) => {
                if let [tn, fname] = path.as_slice() {
                    let ty = self.ty_of(whole)?;
                    // enum variant constructor `Enum::Variant(..)`
                    let enum_ty = if tn == "Self" { self.cur_self.clone().unwrap_or_else(|| ty.clone()) } else { ty.clone() };
                    if let Some(idx) = self.variant_index(fname, &enum_ty) {
                        if matches!(&enum_ty, Ty::Named(n) if n == tn || tn == "Self") {
                            let ftys = self.variants_of(&enum_ty).unwrap()[idx].1.clone();
                            if ftys.len() != args.len() {
                                return Err(CgErr(format!("`{}` takes {} field(s), found {}", path.join("::"), ftys.len(), args.len())));
                            }
                            let mut vals = Vec::new();
                            for (a, t) in args.iter().zip(ftys.iter()) {
                                vals.push(self.expr_as(&a.value, t)?);
                            }
                            return self.make_enum(&enum_ty, idx, vals);
                        }
                    }
                    // associated function `Type::name(..)` / `Self::name(..)`
                    let tystr = if tn == "Self" {
                        self.cur_self.as_ref().map(|t| t.to_string()).ok_or_else(|| CgErr("`Self` used outside an `impl` block".into()))?
                    } else {
                        resolve_type(&Type::Primitive(tn.clone())).to_string()
                    };
                    if let Some(key) = self.find_method(&tystr, fname) {
                        if self.fns[&key].self_kind.is_some() {
                            return Err(CgErr(format!("`{}::{}` takes `self`: call it as `value.{}(..)`", tystr, fname, fname)));
                        }
                        let display = format!("{}::{}", tystr, fname);
                        return self.call_fn(&key, &display, None, args);
                    }
                    if matches!(tn.as_str(), "Box" | "Rc" | "Arc" | "Weak") {
                        return unsupported(&format!("`{}::{}` (heap / shared-ownership types)", tn, fname), 11);
                    }
                    return Err(CgErr(format!("no associated function `{}::{}` found", tn, fname)));
                }
                unsupported("calling a path (modules)", 14)
            }
            _ => unsupported("calling a computed function value", 12),
        }
    }

    fn ctor_call(&mut self, whole: &Expr, name: &str, args: &[Arg]) -> R<V<'ctx>> {
        let ty = self.ty_of(whole)?;
        let Some(idx) = self.variant_index(name, &ty) else {
            return Err(CgErr(format!("cannot determine the full type of `{}(..)`: add a type annotation", name)));
        };
        if args.len() != 1 {
            return Err(CgErr(format!("`{}` takes exactly one argument", name)));
        }
        let pty = self.variants_of(&ty).unwrap()[idx].1[0].clone();
        let v = self.expr_as(&args[0].value, &pty)?;
        self.make_enum(&ty, idx, vec![v])
    }

    fn gen_panic(&mut self, args: &[Arg]) -> R<V<'ctx>> {
        if args.len() != 1 {
            return Err(CgErr("`panic` takes exactly one argument".into()));
        }
        let msg = self.expr_as(&args[0].value, &Ty::StringTy)?.into_struct_value();
        let p = self.builder.build_extract_value(msg, 0, "p")?;
        let n = self.builder.build_extract_value(msg, 1, "n")?;
        let f = self.panic_fn();
        self.builder.build_call(f, &[p.into(), n.into()], "")?;
        self.builder.build_unreachable()?;
        self.terminate();
        Ok(self.unit())
    }

    fn gen_assert(&mut self, args: &[Arg]) -> R<V<'ctx>> {
        if args.len() != 1 {
            return Err(CgErr("`assert` takes exactly one argument".into()));
        }
        // Document 11 §5: `assert` exists in debug builds only.
        if self.opts.release {
            return Ok(self.unit());
        }
        let c = self.expr_as(&args[0].value, &Ty::Bool)?.into_int_value();
        let failed = self.builder.build_not(c, "failed")?;
        self.panic_if(failed, "assertion failed")?;
        Ok(self.unit())
    }

    /// `ensure(cond, err)` (Document 11 §5): always present; `Err(err)` when
    /// `cond` is false, `Ok(())` otherwise.
    fn gen_ensure(&mut self, whole: &Expr, args: &[Arg]) -> R<V<'ctx>> {
        if args.len() != 2 {
            return Err(CgErr("`ensure` takes exactly two arguments".into()));
        }
        let ty = self.ty_of(whole)?;
        let Ty::ResultTy(_, ety) = &ty else { return Err(CgErr(format!("internal: `ensure` typed `{}`", ty))) };
        let ety = (**ety).clone();
        let slot = self.alloca_ty(&ty, "ensure")?;
        let c = self.expr_as(&args[0].value, &Ty::Bool)?.into_int_value();
        let ok_bb = self.new_block("ens_ok");
        let err_bb = self.new_block("ens_err");
        let end_bb = self.new_block("ens_end");
        self.builder.build_conditional_branch(c, ok_bb, err_bb)?;
        self.position(ok_bb);
        let ok_v = self.make_enum(&ty, 0, vec![self.unit()])?;
        self.builder.build_store(slot, ok_v)?;
        self.builder.build_unconditional_branch(end_bb)?;
        self.position(err_bb);
        let ev = self.expr_as(&args[1].value, &ety)?;
        let err_v = self.make_enum(&ty, 1, vec![ev])?;
        self.builder.build_store(slot, err_v)?;
        self.builder.build_unconditional_branch(end_bb)?;
        self.position(end_bb);
        self.load(slot, &ty)
    }
}
