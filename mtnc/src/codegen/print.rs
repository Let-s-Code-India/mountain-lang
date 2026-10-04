//! Compiler-known `print`/`println`/`eprint`/`eprintln` (FLAGGED, Phase 10).
//!
//! Document 16 §1.21.1 makes these ordinary prelude functions over a
//! `Display` trait, built in Phase 16. Until then they are intrinsics for
//! integers, floats, `bool`, `char` and string values, lowered to libc
//! `dprintf`/`write` on file descriptor 1 (stdout) or 2 (stderr). Floats print
//! with C's `%g`. To be replaced by the real prelude in Phase 16.

use super::*;

impl<'ctx, 'a> Codegen<'ctx, 'a> {
    /// `__mtn_write_char(fd, codepoint)`: UTF-8-encodes one `char` and `write`s it.
    fn char_fn(&mut self) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function("__mtn_write_char") {
            return f;
        }
        let i32t = self.context.i32_type();
        let i8t = self.context.i8_type();
        let ptr = self.context.ptr_type(AddressSpace::default());
        let iptr = self.int_ty(self.ptr_bits());
        let write = self.get_extern("write", Some(iptr.into()), &[i32t.into(), ptr.into(), iptr.into()], false);
        let ft = self.context.void_type().fn_type(&[i32t.into(), i32t.into()], false);
        let f = self.module.add_function("__mtn_write_char", ft, None);
        let entry = self.context.append_basic_block(f, "entry");
        let b = self.context.create_builder();
        b.position_at_end(entry);
        let buf_ty = self.context.struct_type(&[i8t.into(), i8t.into(), i8t.into(), i8t.into()], false);
        let buf = b.build_alloca(buf_ty, "buf").unwrap();
        let len = b.build_alloca(i32t, "len").unwrap();
        let fd = f.get_nth_param(0).unwrap().into_int_value();
        let cp = f.get_nth_param(1).unwrap().into_int_value();
        let k = |v: u64| i32t.const_int(v, false);
        let bb1 = self.context.append_basic_block(f, "one");
        let t2 = self.context.append_basic_block(f, "t2");
        let bb2 = self.context.append_basic_block(f, "two");
        let t3 = self.context.append_basic_block(f, "t3");
        let bb3 = self.context.append_basic_block(f, "three");
        let bb4 = self.context.append_basic_block(f, "four");
        let out = self.context.append_basic_block(f, "out");
        // byte i = trunc(((cp >> shift) & mask) | tag)
        let put = |i: u32, shift: u64, mask: u64, tag: u64| {
            let sh = b.build_right_shift(cp, k(shift), false, "").unwrap();
            let m = b.build_and(sh, k(mask), "").unwrap();
            let t = b.build_or(m, k(tag), "").unwrap();
            let tr = b.build_int_truncate(t, i8t, "").unwrap();
            let p = b.build_struct_gep(buf_ty, buf, i, "").unwrap();
            b.build_store(p, tr).unwrap();
        };
        let c1 = b.build_int_compare(IntPredicate::ULT, cp, k(0x80), "").unwrap();
        b.build_conditional_branch(c1, bb1, t2).unwrap();
        b.position_at_end(bb1);
        put(0, 0, 0x7f, 0);
        b.build_store(len, k(1)).unwrap();
        b.build_unconditional_branch(out).unwrap();
        b.position_at_end(t2);
        let c2 = b.build_int_compare(IntPredicate::ULT, cp, k(0x800), "").unwrap();
        b.build_conditional_branch(c2, bb2, t3).unwrap();
        b.position_at_end(bb2);
        put(0, 6, 0x1f, 0xc0);
        put(1, 0, 0x3f, 0x80);
        b.build_store(len, k(2)).unwrap();
        b.build_unconditional_branch(out).unwrap();
        b.position_at_end(t3);
        let c3 = b.build_int_compare(IntPredicate::ULT, cp, k(0x10000), "").unwrap();
        b.build_conditional_branch(c3, bb3, bb4).unwrap();
        b.position_at_end(bb3);
        put(0, 12, 0x0f, 0xe0);
        put(1, 6, 0x3f, 0x80);
        put(2, 0, 0x3f, 0x80);
        b.build_store(len, k(3)).unwrap();
        b.build_unconditional_branch(out).unwrap();
        b.position_at_end(bb4);
        put(0, 18, 0x07, 0xf0);
        put(1, 12, 0x3f, 0x80);
        put(2, 6, 0x3f, 0x80);
        put(3, 0, 0x3f, 0x80);
        b.build_store(len, k(4)).unwrap();
        b.build_unconditional_branch(out).unwrap();
        b.position_at_end(out);
        let l = b.build_load(i32t, len, "").unwrap().into_int_value();
        let l64 = b.build_int_z_extend(l, iptr, "").unwrap();
        b.build_call(write, &[fd.into(), buf.into(), l64.into()], "").unwrap();
        b.build_return(None).unwrap();
        f
    }

    pub(super) fn gen_print(&mut self, name: &str, args: &[Arg]) -> R<V<'ctx>> {
        if args.len() != 1 {
            return Err(CgErr(format!("`{}` takes exactly one argument (the std prelude with `Display` arrives in Phase 16)", name)));
        }
        let newline = name.ends_with("ln");
        let fd = if name.starts_with('e') { 2u64 } else { 1u64 };
        let ty = self.ty_of(&args[0].value)?;
        let v = self.expr_as(&args[0].value, &ty)?;
        let i32t = self.context.i32_type();
        let ptr = self.context.ptr_type(AddressSpace::default());
        let dprintf = self.get_extern("dprintf", Some(i32t.into()), &[i32t.into(), ptr.into()], true);
        let fdv = i32t.const_int(fd, false);
        let nl = if newline { "\n" } else { "" };
        let mut call_args: Vec<BasicMetadataValueEnum> = vec![fdv.into()];
        let fmt: String;
        match &ty {
            t if is_int(t) => {
                if matches!(t, Ty::I128 | Ty::U128) {
                    return unsupported("printing 128-bit integers", 16);
                }
                let i64t = self.int_ty(64);
                let iv = v.into_int_value();
                let wide = if iv.get_type().get_bit_width() == 64 {
                    iv
                } else if is_signed(t) {
                    self.builder.build_int_s_extend(iv, i64t, "")?
                } else {
                    self.builder.build_int_z_extend(iv, i64t, "")?
                };
                fmt = format!("{}{}", if is_signed(t) { "%lld" } else { "%llu" }, nl);
                call_args.push(self.c_str(&fmt).into());
                call_args.push(wide.into());
            }
            Ty::F32 | Ty::F64 => {
                let fv = v.into_float_value();
                let d = if *&ty == Ty::F32 { self.builder.build_float_ext(fv, self.context.f64_type(), "")? } else { fv };
                fmt = format!("%g{}", nl);
                call_args.push(self.c_str(&fmt).into());
                call_args.push(d.into());
            }
            Ty::Bool => {
                let t = self.c_str("true");
                let f = self.c_str("false");
                let s = self.builder.build_select(v.into_int_value(), t, f, "")?;
                fmt = format!("%s{}", nl);
                call_args.push(self.c_str(&fmt).into());
                call_args.push(s.into());
            }
            Ty::StringTy => {
                let sv = v.into_struct_value();
                let p = self.builder.build_extract_value(sv, 0, "")?;
                let n = self.builder.build_extract_value(sv, 1, "")?.into_int_value();
                let n32 = self.builder.build_int_truncate_or_bit_cast(n, i32t, "")?;
                fmt = format!("%.*s{}", nl);
                call_args.push(self.c_str(&fmt).into());
                call_args.push(n32.into());
                call_args.push(p.into());
            }
            Ty::Char => {
                let cf = self.char_fn();
                self.builder.build_call(cf, &[fdv.into(), v.into()], "")?;
                if newline {
                    let cf2 = self.char_fn();
                    self.builder.build_call(cf2, &[fdv.into(), i32t.const_int(10, false).into()], "")?;
                }
                return Ok(self.unit());
            }
            other => return Err(CgErr(format!("`{}` cannot print a value of type `{}` yet (needs `Display`, Phase 16)", name, other))),
        }
        self.builder.build_call(dprintf, &call_args, "")?;
        Ok(self.unit())
    }
}
