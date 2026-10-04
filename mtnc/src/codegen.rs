//! LLVM IR generation (Phase 10; Document 17 §6, Document 25 §2.3 Phase 10).
//!
//! Input: the (already desugared, see `desugar.rs`) program plus the type
//! checker's per-expression type table (`TypeChecker::expr_types`). Output: an
//! LLVM `Module`. The IR is **target-agnostic**: nothing here names a CPU. The
//! only target facts used are the data layout / pointer width, taken from the
//! `TargetMachine` the caller supplies, so Phase 15 can reuse this module for
//! `wasm32` by passing a different machine (Document 17 §7, Document 21 §2).
//!
//! Supported (Phases 1-9 features, Document 25 Phase 10): non-generic
//! functions, every primitive numeric type, `bool`, `char`, arithmetic /
//! comparison / bitwise / short-circuit operators, `as` casts, `if`, `match`,
//! `loop`/`while`/`do-while`/`for` over ranges with labels, structs, enums
//! (tagged unions), tuples, `Option`/`Result`, `?`, `panic`, `assert`,
//! `ensure`, overflow / divide-by-zero panics. Everything else yields an error
//! of the form "... not yet supported by codegen — Phase N" -- never a crash
//! and never a silent miscompile.
//!
//! Value representation: every Mountain value is a first-class LLVM value
//! (structs/tuples/enums by value); every local lives in an `alloca` in the
//! function's entry block (LLVM's `mem2reg` promotes them). `()` and `!` are
//! the empty struct `{}`. A function returning `()`/`!` is a `void` function.
//! Enums are `{ i32 tag, [N x i64] payload }`; variant fields are stored
//! through the payload pointer as a per-variant struct.
//!
//! FLAGGED (needs owner sign-off, see PROGRESS.md): until the std prelude
//! exists (Phase 16) `print`/`println`/`eprint`/`eprintln` are compiler-known
//! intrinsics for integers, floats, `bool`, `char` and string values, lowered
//! to libc `dprintf`; a `String` is a static `{ptr, len}` view (no heap, no
//! concatenation, no `Display` trait).

use crate::ast::*;
use crate::types::{builtin_variants, resolve_type, Ty};
use inkwell::attributes::{Attribute, AttributeLoc};
use inkwell::basic_block::BasicBlock;
use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::intrinsics::Intrinsic;
use inkwell::module::Module;
use inkwell::targets::TargetData;
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, IntType, StructType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue, PointerValue, ValueKind};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Release semantics (Document 5 §2.1): integer overflow wraps and
    /// `assert` is stripped. Debug (default): overflow panics.
    pub release: bool,
}

/// A codegen failure (always a clear, user-facing message).
#[derive(Debug)]
pub struct CgErr(pub String);

impl From<BuilderError> for CgErr {
    fn from(e: BuilderError) -> Self {
        CgErr(format!("internal codegen error: {}", e))
    }
}

type R<T> = Result<T, CgErr>;
type V<'ctx> = BasicValueEnum<'ctx>;

fn unsupported<T>(what: &str, phase: u32) -> R<T> {
    Err(CgErr(format!("{} is not yet supported by codegen — Phase {}", what, phase)))
}

struct FnInfo<'ctx> {
    val: FunctionValue<'ctx>,
    params: Vec<Ty>,
    ret: Ty,
}

struct EnumLayout<'ctx> {
    llty: StructType<'ctx>,
    variants: Vec<StructType<'ctx>>,
}

struct LoopCtx<'ctx> {
    label: Option<String>,
    break_bb: BasicBlock<'ctx>,
    continue_bb: BasicBlock<'ctx>,
    result: Option<(PointerValue<'ctx>, Ty)>,
}

struct Bind<'ctx> {
    name: String,
    ptr: PointerValue<'ctx>,
    ty: Ty,
}

pub struct Codegen<'ctx, 'a> {
    context: &'ctx Context,
    pub module: Module<'ctx>,
    builder: Builder<'ctx>,
    alloca_builder: Builder<'ctx>,
    rt_builder: Builder<'ctx>,
    td: TargetData,
    types: &'a HashMap<usize, Ty>,
    opts: Options,

    structs: HashMap<String, (Vec<(String, Ty)>, bool)>,
    enums: HashMap<String, Vec<(String, Vec<Ty>)>>,
    fns: HashMap<String, FnInfo<'ctx>>,
    enum_layouts: HashMap<String, EnumLayout<'ctx>>,
    strings: HashMap<Vec<u8>, PointerValue<'ctx>>,

    // per-function state
    cur_fn: Option<FunctionValue<'ctx>>,
    alloca_bb: Option<BasicBlock<'ctx>>,
    ret_ty: Ty,
    locals: Vec<HashMap<String, (PointerValue<'ctx>, Ty)>>,
    loops: Vec<LoopCtx<'ctx>>,
    dead: bool,
    block_counter: usize,
}

fn is_signed(t: &Ty) -> bool {
    matches!(t, Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::I128 | Ty::Isize)
}
fn is_int(t: &Ty) -> bool {
    matches!(t, Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::I128 | Ty::Isize | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64 | Ty::U128 | Ty::Usize)
}
fn is_float(t: &Ty) -> bool {
    matches!(t, Ty::F32 | Ty::F64)
}
fn is_unit_like(t: &Ty) -> bool {
    matches!(t, Ty::Unit | Ty::Never)
}

fn parse_int_text(text: &str, radix_prefix: Option<(&str, u32)>) -> Option<u128> {
    let t: String = text.chars().filter(|c| *c != '_').collect();
    match radix_prefix {
        Some((prefix, radix)) => {
            let body = t.strip_prefix(prefix).or_else(|| t.strip_prefix(&prefix.to_uppercase()))?;
            u128::from_str_radix(body, radix).ok()
        }
        None => t.parse::<u128>().ok(),
    }
}

fn unescape_char(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<u32> {
    let c = chars.next()?;
    if c != '\\' {
        return Some(c as u32);
    }
    match chars.next()? {
        'n' => Some('\n' as u32),
        't' => Some('\t' as u32),
        'r' => Some('\r' as u32),
        '0' => Some(0),
        '\\' => Some('\\' as u32),
        '\'' => Some('\'' as u32),
        '"' => Some('"' as u32),
        'x' => {
            let h: String = chars.by_ref().take(2).collect();
            u32::from_str_radix(&h, 16).ok()
        }
        'u' => {
            if chars.next()? != '{' {
                return None;
            }
            let mut h = String::new();
            loop {
                let c = chars.next()?;
                if c == '}' {
                    break;
                }
                h.push(c);
            }
            u32::from_str_radix(&h, 16).ok()
        }
        _ => None,
    }
}

fn unescape_str(body: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut it = body.chars().peekable();
    while it.peek().is_some() {
        let cp = unescape_char(&mut it)?;
        let ch = char::from_u32(cp)?;
        let mut buf = [0u8; 4];
        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
    }
    Some(out)
}

impl<'ctx, 'a> Codegen<'ctx, 'a> {
    pub fn new(context: &'ctx Context, name: &str, td: TargetData, types: &'a HashMap<usize, Ty>, opts: Options) -> Self {
        let module = context.create_module(name);
        module.set_data_layout(&td.get_data_layout());
        Codegen {
            context,
            module,
            builder: context.create_builder(),
            alloca_builder: context.create_builder(),
            rt_builder: context.create_builder(),
            td,
            types,
            opts,
            structs: HashMap::new(),
            enums: HashMap::new(),
            fns: HashMap::new(),
            enum_layouts: HashMap::new(),
            strings: HashMap::new(),
            cur_fn: None,
            alloca_bb: None,
            ret_ty: Ty::Unit,
            locals: Vec::new(),
            loops: Vec::new(),
            dead: false,
            block_counter: 0,
        }
    }

    // ------------------------------------------------------------------
    // Types
    // ------------------------------------------------------------------

    fn ptr_bits(&self) -> u32 {
        self.td.get_pointer_byte_size(None) * 8
    }

    fn int_ty(&self, bits: u32) -> IntType<'ctx> {
        self.context.custom_width_int_type(std::num::NonZeroU32::new(bits).unwrap()).unwrap()
    }

    fn unit_ty(&self) -> StructType<'ctx> {
        self.context.struct_type(&[], false)
    }

    fn unit(&self) -> V<'ctx> {
        self.unit_ty().const_zero().into()
    }

    fn str_ty(&self) -> StructType<'ctx> {
        let p = self.context.ptr_type(AddressSpace::default());
        self.context.struct_type(&[p.into(), self.int_ty(self.ptr_bits()).into()], false)
    }

    fn variants_of(&self, ty: &Ty) -> Option<Vec<(String, Vec<Ty>)>> {
        if let Some(v) = builtin_variants(ty) {
            return Some(v.into_iter().map(|(n, t)| (n.to_string(), t)).collect());
        }
        if let Ty::Named(n) = ty {
            return self.enums.get(n).cloned();
        }
        None
    }

    fn llty(&mut self, ty: &Ty) -> R<BasicTypeEnum<'ctx>> {
        Ok(match ty {
            Ty::I8 | Ty::U8 => self.int_ty(8).into(),
            Ty::I16 | Ty::U16 => self.int_ty(16).into(),
            Ty::I32 | Ty::U32 => self.int_ty(32).into(),
            Ty::I64 | Ty::U64 => self.int_ty(64).into(),
            Ty::I128 | Ty::U128 => self.int_ty(128).into(),
            Ty::Isize | Ty::Usize => self.int_ty(self.ptr_bits()).into(),
            Ty::F32 => self.context.f32_type().into(),
            Ty::F64 => self.context.f64_type().into(),
            Ty::Bool => self.int_ty(1).into(),
            Ty::Char => self.int_ty(32).into(),
            Ty::Unit | Ty::Never => self.unit_ty().into(),
            Ty::StringTy => self.str_ty().into(),
            Ty::Tuple(ts) => {
                let mut fs = Vec::new();
                for t in ts {
                    fs.push(self.llty(t)?);
                }
                self.context.struct_type(&fs, false).into()
            }
            Ty::Named(n) => {
                if let Some((fields, _)) = self.structs.get(n).cloned() {
                    let mut fs = Vec::new();
                    for (_, t) in &fields {
                        fs.push(self.llty(t)?);
                    }
                    self.context.struct_type(&fs, false).into()
                } else if self.enums.contains_key(n) {
                    self.enum_layout(ty)?.llty.into()
                } else {
                    return Err(CgErr(format!("unknown type `{}`", n)));
                }
            }
            Ty::OptionTy(_) | Ty::ResultTy(..) => self.enum_layout(ty)?.llty.into(),
            Ty::Str => return unsupported("the unsized `str` type", 11),
            Ty::Ref(..) => return unsupported("references (`borrow`/`&`)", 11),
            Ty::Array(_) => return unsupported("arrays / growable `[T]`", 11),
            Ty::Fn(..) => return unsupported("function values / closures", 11),
            Ty::Generic(..) | Ty::TypeParam(_) => return unsupported("generics (monomorphization)", 11),
            Ty::DynTrait(_) => return unsupported("`dyn Trait` (traits)", 11),
            Ty::Null => return unsupported("`null`", 11),
        })
    }

    fn enum_layout(&mut self, ty: &Ty) -> R<EnumLayoutRef<'ctx>> {
        let key = ty.to_string();
        if !self.enum_layouts.contains_key(&key) {
            let variants = self.variants_of(ty).ok_or_else(|| CgErr(format!("`{}` is not an enum", ty)))?;
            let mut vsts = Vec::new();
            let mut max_size = 0u64;
            for (_, fields) in &variants {
                let mut fs = Vec::new();
                for f in fields {
                    fs.push(self.llty(f)?);
                }
                let st = self.context.struct_type(&fs, false);
                if self.td.get_abi_alignment(&st) > 8 {
                    return unsupported("a 128-bit integer inside an enum payload", 11);
                }
                max_size = max_size.max(self.td.get_abi_size(&st));
                vsts.push(st);
            }
            let words = max_size.div_ceil(8) as u32;
            let payload = self.int_ty(64).array_type(words);
            let llty = self.context.struct_type(&[self.int_ty(32).into(), payload.into()], false);
            self.enum_layouts.insert(key.clone(), EnumLayout { llty, variants: vsts });
        }
        let l = &self.enum_layouts[&key];
        Ok(EnumLayoutRef { llty: l.llty, variants: l.variants.clone() })
    }

    fn ty_of(&self, e: &Expr) -> R<Ty> {
        self.types
            .get(&(e as *const Expr as usize))
            .cloned()
            .ok_or_else(|| CgErr("internal: the type checker recorded no type for an expression".into()))
    }

    // ------------------------------------------------------------------
    // Block / builder helpers
    // ------------------------------------------------------------------

    fn new_block(&mut self, name: &str) -> BasicBlock<'ctx> {
        self.block_counter += 1;
        self.context.append_basic_block(self.cur_fn.unwrap(), name)
    }

    fn position(&mut self, bb: BasicBlock<'ctx>) {
        self.builder.position_at_end(bb);
        self.dead = false;
    }

    /// After a terminator: continue emitting into a fresh unreachable block.
    fn terminate(&mut self) {
        let bb = self.new_block("dead");
        self.builder.position_at_end(bb);
        self.dead = true;
    }

    fn alloca(&mut self, ty: BasicTypeEnum<'ctx>, name: &str) -> R<PointerValue<'ctx>> {
        let bb = self.alloca_bb.unwrap();
        self.alloca_builder.position_at_end(bb);
        Ok(self.alloca_builder.build_alloca(ty, name)?)
    }

    fn alloca_ty(&mut self, ty: &Ty, name: &str) -> R<PointerValue<'ctx>> {
        let lt = self.llty(ty)?;
        self.alloca(lt, name)
    }

    fn lookup_local(&self, name: &str) -> Option<(PointerValue<'ctx>, Ty)> {
        for s in self.locals.iter().rev() {
            if let Some(x) = s.get(name) {
                return Some(x.clone());
            }
        }
        None
    }

    fn declare_local(&mut self, name: &str, ty: &Ty) -> R<PointerValue<'ctx>> {
        let lt = self.llty(ty)?;
        let p = self.alloca(lt, name)?;
        self.locals.last_mut().unwrap().insert(name.to_string(), (p, ty.clone()));
        Ok(p)
    }

    fn zero_of(&mut self, ty: &Ty) -> R<V<'ctx>> {
        Ok(self.llty(ty)?.const_zero())
    }

    // ------------------------------------------------------------------
    // Runtime support (declared lazily)
    // ------------------------------------------------------------------

    fn global_str(&mut self, bytes: &[u8]) -> PointerValue<'ctx> {
        if let Some(p) = self.strings.get(bytes) {
            return *p;
        }
        let arr = self.context.const_string(bytes, false);
        let g = self.module.add_global(arr.get_type(), None, &format!(".str.{}", self.strings.len()));
        g.set_initializer(&arr);
        g.set_constant(true);
        g.set_unnamed_addr(true);
        g.set_linkage(inkwell::module::Linkage::Private);
        let p = g.as_pointer_value();
        self.strings.insert(bytes.to_vec(), p);
        p
    }

    fn c_str(&mut self, s: &str) -> PointerValue<'ctx> {
        let mut b = s.as_bytes().to_vec();
        b.push(0);
        self.global_str(&b)
    }

    fn get_extern(&mut self, name: &str, ret: Option<BasicTypeEnum<'ctx>>, params: &[BasicMetadataTypeEnum<'ctx>], varargs: bool) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function(name) {
            return f;
        }
        let ft = match ret {
            Some(r) => r.fn_type(params, varargs),
            None => self.context.void_type().fn_type(params, varargs),
        };
        self.module.add_function(name, ft, None)
    }

    fn noreturn(&self, f: FunctionValue<'ctx>) {
        let kind = Attribute::get_named_enum_kind_id("noreturn");
        f.add_attribute(AttributeLoc::Function, self.context.create_enum_attribute(kind, 0));
    }

    /// `__mtn_panic(ptr, len)`: Document 11 §4's panic. Writes
    /// `thread 'main' panicked: <msg>` to stderr and exits with code 101.
    fn panic_fn(&mut self) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function("__mtn_panic") {
            return f;
        }
        let ptr = self.context.ptr_type(AddressSpace::default());
        let iptr = self.int_ty(self.ptr_bits());
        let i32t = self.context.i32_type();
        let write = self.get_extern("write", Some(iptr.into()), &[i32t.into(), ptr.into(), iptr.into()], false);
        let exit = self.get_extern("exit", None, &[i32t.into()], false);
        self.noreturn(exit);
        let ft = self.context.void_type().fn_type(&[ptr.into(), iptr.into()], false);
        let f = self.module.add_function("__mtn_panic", ft, None);
        self.noreturn(f);
        let bb = self.context.append_basic_block(f, "entry");
        let b = &self.rt_builder;
        b.position_at_end(bb);
        let prefix = b"thread 'main' panicked: ";
        let pre_arr = self.context.const_string(prefix, false);
        let pg = self.module.add_global(pre_arr.get_type(), None, ".str.panic_prefix");
        pg.set_initializer(&pre_arr);
        pg.set_constant(true);
        pg.set_linkage(inkwell::module::Linkage::Private);
        let nl_arr = self.context.const_string(b"\n", false);
        let ng = self.module.add_global(nl_arr.get_type(), None, ".str.panic_nl");
        ng.set_initializer(&nl_arr);
        ng.set_constant(true);
        ng.set_linkage(inkwell::module::Linkage::Private);
        let two = i32t.const_int(2, false);
        let _ = b.build_call(write, &[two.into(), pg.as_pointer_value().into(), iptr.const_int(prefix.len() as u64, false).into()], "");
        let _ = b.build_call(write, &[two.into(), f.get_nth_param(0).unwrap().into(), f.get_nth_param(1).unwrap().into()], "");
        let _ = b.build_call(write, &[two.into(), ng.as_pointer_value().into(), iptr.const_int(1, false).into()], "");
        let _ = b.build_call(exit, &[i32t.const_int(101, false).into()], "");
        let _ = b.build_unreachable();
        f
    }

    /// Emits a call to the panic routine with a constant message, followed by
    /// `unreachable`. Leaves the builder in the (now terminated) block.
    fn emit_panic(&mut self, msg: &str) -> R<()> {
        let f = self.panic_fn();
        let p = self.global_str(msg.as_bytes());
        let len = self.int_ty(self.ptr_bits()).const_int(msg.len() as u64, false);
        self.builder.build_call(f, &[p.into(), len.into()], "")?;
        self.builder.build_unreachable()?;
        Ok(())
    }

    /// `if cond { panic(msg) }`, continuing in the non-panicking block.
    fn panic_if(&mut self, cond: IntValue<'ctx>, msg: &str) -> R<()> {
        let fail = self.new_block("panic");
        let ok = self.new_block("ok");
        self.builder.build_conditional_branch(cond, fail, ok)?;
        self.builder.position_at_end(fail);
        self.emit_panic(msg)?;
        self.position(ok);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Items
    // ------------------------------------------------------------------

    pub fn generate(&mut self, program: &Program) -> Result<(), Vec<String>> {
        let mut errors: Vec<String> = Vec::new();
        let mut fn_items: Vec<&FnDecl> = Vec::new();
        self.register_items(&program.items, &mut fn_items, &mut errors);
        if !errors.is_empty() {
            return Err(errors);
        }
        // Pass 1: declare every function so bodies can call in any order.
        for f in &fn_items {
            if let Err(e) = self.declare_fn(f) {
                errors.push(format!("in `{}`: {}", f.name, e.0));
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        // Pass 2: bodies.
        for f in &fn_items {
            if let Err(e) = self.gen_fn(f) {
                errors.push(format!("in `{}`: {}", f.name, e.0));
            }
        }
        if errors.is_empty() {
            if let Err(e) = self.gen_main_wrapper() {
                errors.push(e.0);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn register_items<'p>(&mut self, items: &'p [Item], fns: &mut Vec<&'p FnDecl>, errors: &mut Vec<String>) {
        for item in items {
            match &item.kind {
                ItemKind::Fn(f) => fns.push(f),
                ItemKind::Struct(s) => {
                    if !s.generics.0.is_empty() {
                        errors.push(format!("generic struct `{}`: generics (monomorphization) are not yet supported by codegen — Phase 11", s.name));
                        continue;
                    }
                    let (fields, is_tuple) = match &s.body {
                        StructBody::Named(fs) => (fs.iter().map(|f| (f.name.clone(), resolve_type(&f.ty))).collect(), false),
                        StructBody::Tuple(ts) => (ts.iter().enumerate().map(|(i, t)| (i.to_string(), resolve_type(t))).collect(), true),
                        StructBody::Unit => (Vec::new(), false),
                    };
                    self.structs.insert(s.name.clone(), (fields, is_tuple));
                }
                ItemKind::Enum(en) => {
                    if !en.generics.0.is_empty() {
                        errors.push(format!("generic enum `{}`: generics (monomorphization) are not yet supported by codegen — Phase 11", en.name));
                        continue;
                    }
                    let vs = en.variants.iter().map(|v| (v.name.clone(), v.data.iter().map(resolve_type).collect())).collect();
                    self.enums.insert(en.name.clone(), vs);
                }
                ItemKind::TargetBlock(kind, inner) => {
                    // Document 2 §8: a native build includes `native` and `all` blocks only.
                    if matches!(kind, TargetKind::Native | TargetKind::All) {
                        self.register_items(inner, fns, errors);
                    }
                }
                ItemKind::Impl(_) | ItemKind::Trait(_) => errors.push("`impl`/`trait` items are not yet supported by codegen — Phase 11".into()),
                ItemKind::Mod(_) | ItemKind::Use(_) | ItemKind::Import(_) => errors.push("modules / `use` / `import` are not yet supported by codegen — Phase 14".into()),
                ItemKind::Const(_) | ItemKind::Static(_) => errors.push("`const`/`static` items are not yet supported by codegen — Phase 11".into()),
                ItemKind::TypeAlias(_) => errors.push("`type` aliases are not yet supported by codegen — Phase 11".into()),
                ItemKind::Table(_) | ItemKind::Index(_) | ItemKind::Schema(_) => errors.push("`table`/`index`/`schema` are not yet supported by codegen — Phase 18".into()),
                ItemKind::Ui(_) | ItemKind::Component(_) => errors.push("`ui`/`component` are not yet supported by codegen — Phase 19".into()),
                ItemKind::Server(_) => errors.push("`server` is not yet supported by codegen — Phase 17".into()),
                ItemKind::Actor(_) => errors.push("`actor` is not yet supported by codegen — Phase 12".into()),
            }
        }
    }

    fn fn_symbol(name: &str) -> String {
        format!("mtn_{}", name)
    }

    fn declare_fn(&mut self, f: &FnDecl) -> R<()> {
        if f.is_async {
            return unsupported("`async fn`", 12);
        }
        if !f.generics.0.is_empty() {
            return unsupported("generic functions", 11);
        }
        let Some(_) = &f.body else { return Err(CgErr("function without a body".into())) };
        let mut ptys = Vec::new();
        let mut lls: Vec<BasicMetadataTypeEnum> = Vec::new();
        for p in &f.params {
            if p.is_variadic {
                return unsupported("variadic parameters", 11);
            }
            if p.default.is_some() {
                return unsupported("default parameter values", 11);
            }
            if p.ownership == OwnershipMod::Borrow || p.ownership == OwnershipMod::BorrowMut || p.name == "self" {
                return unsupported("`borrow` parameters / methods", 11);
            }
            let t = resolve_type(&p.ty);
            lls.push(self.llty(&t)?.into());
            ptys.push(t);
        }
        let ret = f.return_type.as_ref().map(resolve_type).unwrap_or(Ty::Unit);
        let ft = if is_unit_like(&ret) {
            self.context.void_type().fn_type(&lls, false)
        } else {
            self.llty(&ret)?.fn_type(&lls, false)
        };
        if self.fns.contains_key(&f.name) {
            return Err(CgErr(format!("function `{}` is defined more than once", f.name)));
        }
        let val = self.module.add_function(&Self::fn_symbol(&f.name), ft, None);
        self.fns.insert(f.name.clone(), FnInfo { val, params: ptys, ret });
        Ok(())
    }

    fn gen_fn(&mut self, f: &FnDecl) -> R<()> {
        let (val, ptys, ret) = {
            let i = &self.fns[&f.name];
            (i.val, i.params.clone(), i.ret.clone())
        };
        let body = f.body.as_ref().unwrap();
        self.cur_fn = Some(val);
        self.ret_ty = ret.clone();
        self.locals = vec![HashMap::new()];
        self.loops.clear();
        self.dead = false;
        let alloca_bb = self.context.append_basic_block(val, "entry");
        self.alloca_bb = Some(alloca_bb);
        let body_bb = self.context.append_basic_block(val, "body");
        self.position(body_bb);
        for (i, p) in f.params.iter().enumerate() {
            let slot = self.declare_local(&p.name, &ptys[i])?;
            self.builder.build_store(slot, val.get_nth_param(i as u32).unwrap())?;
        }
        let (v, bty) = self.gen_block(body)?;
        if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
            if self.dead || bty == Ty::Never {
                self.builder.build_unreachable()?;
            } else if is_unit_like(&ret) {
                self.builder.build_return(None)?;
            } else {
                self.builder.build_return(Some(&v))?;
            }
        }
        self.alloca_builder.position_at_end(alloca_bb);
        self.alloca_builder.build_unconditional_branch(body_bb)?;
        self.cur_fn = None;
        Ok(())
    }

    /// The C `main`: calls `mtn_main`; for `fn main() -> Result<(), E>` an
    /// `Err` becomes exit code 1 (Document 19/24 show Result-returning mains).
    fn gen_main_wrapper(&mut self) -> R<()> {
        let Some(info) = self.fns.get("main") else {
            return Err(CgErr("no `fn main()` found: a program needs an entry point".into()));
        };
        let (mainf, ret, nparams) = (info.val, info.ret.clone(), info.params.len());
        if nparams != 0 {
            return Err(CgErr("`fn main` must take no parameters".into()));
        }
        let ft = self.context.i32_type().fn_type(&[], false);
        let w = self.module.add_function("main", ft, None);
        let bb = self.context.append_basic_block(w, "entry");
        self.cur_fn = Some(w);
        self.builder.position_at_end(bb);
        let call = self.builder.build_call(mainf, &[], "")?;
        let i32t = self.context.i32_type();
        match &ret {
            Ty::Unit | Ty::Never => {
                self.builder.build_return(Some(&i32t.const_zero()))?;
            }
            Ty::ResultTy(ok, _) if **ok == Ty::Unit => {
                let ValueKind::Basic(v) = call.try_as_basic_value() else { return Err(CgErr("internal: main result".into())) };
                let tag = self.builder.build_extract_value(v.into_struct_value(), 0, "tag")?.into_int_value();
                let is_err = self.builder.build_int_compare(IntPredicate::NE, tag, i32t.const_zero(), "is_err")?;
                let code = self.builder.build_select(is_err, i32t.const_int(1, false), i32t.const_zero(), "code")?;
                self.builder.build_return(Some(&code))?;
            }
            other => return Err(CgErr(format!("`fn main` must return `()` or `Result<(), E>`, found `{}`", other))),
        }
        self.cur_fn = None;
        Ok(())
    }
}

/// Owned copy of an enum layout (so callers can use it while mutating `self`).
pub struct EnumLayoutRef<'ctx> {
    pub llty: StructType<'ctx>,
    pub variants: Vec<StructType<'ctx>>,
}

mod expr;
mod print;
