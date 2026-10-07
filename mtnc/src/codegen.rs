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

/// How a parameter is passed (Document 10 §2, Document 6 §3).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum PMode {
    /// By value (also `move`): the callee owns the value.
    Value,
    /// `borrow` (false) / `borrow mut` (true): an opaque pointer to the caller's value.
    Ref(bool),
    /// `...name: T` (Document 10 §2.3): a stack-backed slice view `&[T]` of the extra arguments.
    Variadic,
}

#[derive(Clone)]
pub(crate) struct ParamInfo<'a> {
    pub name: String,
    /// The declared type; for `Ref` the pointee, for `Variadic` the element type.
    pub ty: Ty,
    pub mode: PMode,
    pub default: Option<&'a Expr>,
}

pub(crate) struct FnInfo<'ctx, 'a> {
    val: FunctionValue<'ctx>,
    /// Parameters excluding `self`.
    params: Vec<ParamInfo<'a>>,
    ret: Ty,
    /// `Some` for methods: how `self` is passed (`Value`, or `Ref(mutable)`).
    self_kind: Option<PMode>,
}

/// One function body to generate (free function or `impl` method).
struct FnWork<'a> {
    key: String,
    decl: &'a FnDecl,
    self_ty: Option<Ty>,
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
    fns: HashMap<String, FnInfo<'ctx, 'a>>,
    /// (type name, method name) -> function keys, inherent methods first.
    methods: HashMap<(String, String), Vec<String>>,
    consts: HashMap<String, (&'a Expr, Ty)>,
    statics: HashMap<String, (PointerValue<'ctx>, Ty)>,
    const_depth: u32,
    /// The concrete type `Self` stands for while generating an `impl` method.
    cur_self: Option<Ty>,
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
            methods: HashMap::new(),
            consts: HashMap::new(),
            statics: HashMap::new(),
            const_depth: 0,
            cur_self: None,
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
            Ty::Fixed(t, n) => {
                let et = self.llty(t)?;
                let n = u32::try_from(*n).map_err(|_| CgErr(format!("array length {} is too large", n)))?;
                et.array_type(n).into()
            }
            Ty::Ref(_, inner) => match inner.as_ref() {
                // `&[T]`: a slice view, `{ data pointer, length }` (Document 13 §1 point 3 style fat pointer).
                Ty::Array(_) => self.str_ty().into(),
                // `&str` has the same `{ptr, len}` shape as a string value.
                Ty::Str => self.str_ty().into(),
                _ => self.context.ptr_type(AddressSpace::default()).into(),
            },
            Ty::Str => return unsupported("the unsized `str` type (use `&str`)", 16),
            Ty::Array(_) => return unsupported("growable `[T]` arrays", 11),
            Ty::Fn(..) => return unsupported("function values / closures", 12),
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

    /// `Self` -> the current impl type, inside a resolved type.
    fn subst_self(&self, t: Ty) -> Ty {
        let Some(selfty) = &self.cur_self else { return t };
        fn go(t: Ty, s: &Ty) -> Ty {
            match t {
                Ty::Named(n) if n == "Self" => s.clone(),
                Ty::Array(i) => Ty::Array(Box::new(go(*i, s))),
                Ty::Fixed(i, n) => Ty::Fixed(Box::new(go(*i, s)), n),
                Ty::Tuple(ts) => Ty::Tuple(ts.into_iter().map(|x| go(x, s)).collect()),
                Ty::Ref(m, i) => Ty::Ref(m, Box::new(go(*i, s))),
                Ty::OptionTy(i) => Ty::OptionTy(Box::new(go(*i, s))),
                Ty::ResultTy(o, e) => Ty::ResultTy(Box::new(go(*o, s)), Box::new(go(*e, s))),
                other => other,
            }
        }
        go(t, selfty)
    }

    /// A written type, with aliases expanded and `Self` resolved.
    pub(crate) fn rty(&self, t: &Type) -> Ty {
        self.subst_self(resolve_type(t))
    }

    pub fn generate(&mut self, program: &'a Program) -> Result<(), Vec<String>> {
        let mut errors: Vec<String> = Vec::new();
        let items = crate::desugar::all_items(program);
        crate::types::register_item_context(&items);
        let mut work: Vec<FnWork<'a>> = Vec::new();
        let mut statics: Vec<&'a StaticDecl> = Vec::new();
        self.register_items(&items, &mut work, &mut statics, &mut errors);
        if !errors.is_empty() {
            return Err(errors);
        }
        for st in &statics {
            if let Err(e) = self.declare_static(st) {
                errors.push(format!("in static `{}`: {}", st.name, e.0));
            }
        }
        // Pass 1: declare every function so bodies can call in any order.
        for w in &work {
            if let Err(e) = self.declare_fn(w) {
                errors.push(format!("in `{}`: {}", w.key, e.0));
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        // Pass 2: bodies.
        for w in &work {
            if let Err(e) = self.gen_fn(w) {
                errors.push(format!("in `{}`: {}", w.key, e.0));
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

    fn register_items(&mut self, items: &[&'a Item], work: &mut Vec<FnWork<'a>>, statics: &mut Vec<&'a StaticDecl>, errors: &mut Vec<String>) {
        for item in items {
            let item: &'a Item = item;
            match &item.kind {
                ItemKind::Fn(f) => {
                    if work.iter().any(|w| w.key == f.name) {
                        errors.push(format!("function `{}` is defined more than once (items nested in functions share one namespace)", f.name));
                        continue;
                    }
                    work.push(FnWork { key: f.name.clone(), decl: f, self_ty: None });
                }
                ItemKind::Struct(s) => {
                    if !s.generics.0.is_empty() {
                        errors.push(format!("generic struct `{}`: generics (monomorphization) are not yet supported by codegen — Phase 11", s.name));
                        continue;
                    }
                    if self.structs.contains_key(&s.name) || self.enums.contains_key(&s.name) {
                        errors.push(format!("type `{}` is defined more than once", s.name));
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
                    if self.structs.contains_key(&en.name) || self.enums.contains_key(&en.name) {
                        errors.push(format!("type `{}` is defined more than once", en.name));
                        continue;
                    }
                    let vs = en.variants.iter().map(|v| (v.name.clone(), v.data.iter().map(resolve_type).collect())).collect();
                    self.enums.insert(en.name.clone(), vs);
                }
                ItemKind::TargetBlock(kind, inner) => {
                    // Document 2 §8: a native build includes `native` and `all` blocks only.
                    if matches!(kind, TargetKind::Native | TargetKind::All) {
                        let refs: Vec<&'a Item> = inner.iter().collect();
                        self.register_items(&refs, work, statics, errors);
                    }
                }
                ItemKind::Impl(im) => {
                    if !im.generics.0.is_empty() {
                        errors.push("generic `impl` blocks are not yet supported by codegen — Phase 11".into());
                        continue;
                    }
                    if !im.target.args.is_empty() || im.trait_ref.as_ref().map(|t| !t.args.is_empty()).unwrap_or(false) {
                        errors.push("`impl` of a generic type or trait (e.g. `impl From<E> for T`) is not yet supported by codegen — Phase 11".into());
                        continue;
                    }
                    let target_ty = resolve_type(&Type::Primitive(im.target.name.clone()));
                    let tystr = target_ty.to_string();
                    let trait_name = im.trait_ref.as_ref().map(|t| t.name.clone());
                    for ii in &im.items {
                        let ImplItem::Fn(f) = ii else { continue };
                        let key = match &trait_name {
                            Some(t) => format!("{}::{}::{}", tystr, t, f.name),
                            None => format!("{}::{}", tystr, f.name),
                        };
                        if work.iter().any(|w| w.key == key) {
                            errors.push(format!("method `{}` is defined more than once", key));
                            continue;
                        }
                        let entry = self.methods.entry((tystr.clone(), f.name.clone())).or_default();
                        if trait_name.is_none() {
                            entry.insert(0, key.clone());
                        } else {
                            entry.push(key.clone());
                        }
                        work.push(FnWork { key, decl: f, self_ty: Some(target_ty.clone()) });
                    }
                }
                ItemKind::Trait(t) => {
                    // Default method bodies are copied into every implementing `impl`
                    // before type checking (`desugar::inline_default_methods`), so a
                    // trait item itself generates no code (static dispatch only).
                    if !t.generics.0.is_empty() {
                        errors.push(format!("generic trait `{}` is not yet supported by codegen — Phase 11", t.name));
                    }
                }
                ItemKind::Const(c) => {
                    let ty = resolve_type(&c.ty);
                    if self.consts.insert(c.name.clone(), (&c.value, ty)).is_some() {
                        errors.push(format!("constant `{}` is defined more than once", c.name));
                    }
                }
                ItemKind::Static(st) => statics.push(st),
                ItemKind::TypeAlias(a) => {
                    if !a.generics.0.is_empty() {
                        errors.push(format!("generic type alias `{}` is not yet supported by codegen — Phase 11", a.name));
                    }
                }
                ItemKind::Mod(_) | ItemKind::Use(_) | ItemKind::Import(_) => errors.push("modules / `use` / `import` are not yet supported by codegen — Phase 14".into()),
                ItemKind::Table(_) | ItemKind::Index(_) | ItemKind::Schema(_) => errors.push("`table`/`index`/`schema` are not yet supported by codegen — Phase 18".into()),
                ItemKind::Ui(_) | ItemKind::Component(_) => errors.push("`ui`/`component` are not yet supported by codegen — Phase 19".into()),
                ItemKind::Server(_) => errors.push("`server` is not yet supported by codegen — Phase 17".into()),
                ItemKind::Actor(_) => errors.push("`actor` is not yet supported by codegen — Phase 12".into()),
            }
        }
    }

    fn fn_symbol(key: &str) -> String {
        format!("mtn_{}", key)
    }

    /// A `static` (Document 3 Category A): one fixed global with a constant
    /// initializer. Statics are read-only in this phase (the AST has no
    /// `static mut`; interior mutability needs `Atomic<T>`, Phase 13).
    fn declare_static(&mut self, st: &StaticDecl) -> R<()> {
        let ty = resolve_type(&st.ty);
        let init = self.const_val(&st.value, &ty)?;
        let lt = self.llty(&ty)?;
        let g = self.module.add_global(lt, None, &format!("mtn_static_{}", st.name));
        g.set_initializer(&init);
        g.set_constant(true);
        if self.statics.insert(st.name.clone(), (g.as_pointer_value(), ty)).is_some() {
            return Err(CgErr(format!("static `{}` is defined more than once", st.name)));
        }
        Ok(())
    }

    /// Constant folding for `static` initializers: literals, tuples, arrays,
    /// struct literals, other constants.
    fn const_val(&mut self, e: &Expr, ty: &Ty) -> R<V<'ctx>> {
        self.const_depth += 1;
        if self.const_depth > 64 {
            self.const_depth -= 1;
            return Err(CgErr("constants refer to each other in a cycle".into()));
        }
        let r = self.const_val_inner(e, ty);
        self.const_depth -= 1;
        r
    }

    fn const_val_inner(&mut self, e: &Expr, ty: &Ty) -> R<V<'ctx>> {
        let bad = || CgErr("a `static` initializer must be a constant literal expression (literals, tuples, arrays, struct literals, other constants)".into());
        match e {
            Expr::Paren(i) => self.const_val(i, ty),
            Expr::Literal(l) => self.literal(l, ty, false),
            Expr::Unary { op: UnaryOp::Neg, expr } => match expr.as_ref() {
                Expr::Literal(l @ (Literal::Int(_) | Literal::IntHex(_) | Literal::IntOct(_) | Literal::IntBin(_) | Literal::Float(_))) => self.literal(l, ty, true),
                _ => Err(bad()),
            },
            Expr::Ident(n) => match self.consts.get(n.as_str()).cloned() {
                Some((ce, cty)) => self.const_val(ce, &cty),
                None => Err(bad()),
            },
            Expr::Tuple(items) if !items.is_empty() => {
                let Ty::Tuple(tys) = ty else { return Err(bad()) };
                let tys = tys.clone();
                let mut vals = Vec::new();
                for (it, t) in items.iter().zip(tys.iter()) {
                    vals.push(self.const_val(it, t)?);
                }
                Ok(self.context.const_struct(&vals, false).into())
            }
            Expr::Array(items) => {
                let Ty::Fixed(et, n) = ty else { return Err(bad()) };
                if items.len() as u64 != *n {
                    return Err(CgErr("array initializer length does not match the array type".into()));
                }
                let et = (**et).clone();
                let mut vals = Vec::new();
                for it in items {
                    vals.push(self.const_val(it, &et)?);
                }
                self.const_array(&et, &vals)
            }
            Expr::StructLit { name, fields, spread: None } => {
                let Some((decl, _)) = self.structs.get(name).cloned() else { return Err(bad()) };
                let mut vals = Vec::new();
                for (fname, fty) in &decl {
                    let Some((_, fe)) = fields.iter().find(|(n, _)| n == fname) else { return Err(bad()) };
                    vals.push(self.const_val(fe, fty)?);
                }
                Ok(self.context.const_struct(&vals, false).into())
            }
            _ => Err(bad()),
        }
    }

    fn const_array(&mut self, et: &Ty, vals: &[V<'ctx>]) -> R<V<'ctx>> {
        let lt = self.llty(et)?;
        Ok(match lt {
            BasicTypeEnum::IntType(t) => t.const_array(&vals.iter().map(|v| v.into_int_value()).collect::<Vec<_>>()).into(),
            BasicTypeEnum::FloatType(t) => t.const_array(&vals.iter().map(|v| v.into_float_value()).collect::<Vec<_>>()).into(),
            BasicTypeEnum::StructType(t) => t.const_array(&vals.iter().map(|v| v.into_struct_value()).collect::<Vec<_>>()).into(),
            BasicTypeEnum::ArrayType(t) => t.const_array(&vals.iter().map(|v| v.into_array_value()).collect::<Vec<_>>()).into(),
            BasicTypeEnum::PointerType(t) => t.const_array(&vals.iter().map(|v| v.into_pointer_value()).collect::<Vec<_>>()).into(),
            _ => return Err(CgErr("unsupported element type in a constant array".into())),
        })
    }

    fn declare_fn(&mut self, w: &FnWork<'a>) -> R<()> {
        let f = w.decl;
        if f.is_async {
            return unsupported("`async fn`", 12);
        }
        if !f.generics.0.is_empty() {
            return unsupported("generic functions", 11);
        }
        let Some(_) = &f.body else { return Err(CgErr("function without a body".into())) };
        self.cur_self = w.self_ty.clone();
        let r = self.declare_fn_inner(w);
        self.cur_self = None;
        r
    }

    fn declare_fn_inner(&mut self, w: &FnWork<'a>) -> R<()> {
        let f = w.decl;
        let ptr = self.context.ptr_type(AddressSpace::default());
        let mut lls: Vec<BasicMetadataTypeEnum> = Vec::new();
        let mut params: Vec<ParamInfo<'a>> = Vec::new();
        let mut self_kind = None;
        for (i, p) in f.params.iter().enumerate() {
            if p.name == "self" {
                let Some(sty) = &w.self_ty else { return Err(CgErr("`self` parameter outside of an `impl` block".into())) };
                if i != 0 {
                    return Err(CgErr("`self` must be the first parameter".into()));
                }
                let kind = match p.ownership {
                    OwnershipMod::Borrow => PMode::Ref(false),
                    OwnershipMod::BorrowMut => PMode::Ref(true),
                    _ => PMode::Value,
                };
                lls.push(match kind {
                    PMode::Value => self.llty(sty)?.into(),
                    _ => ptr.into(),
                });
                self_kind = Some(kind);
                continue;
            }
            let t = self.rty(&p.ty);
            let mode = if p.is_variadic {
                PMode::Variadic
            } else {
                match p.ownership {
                    OwnershipMod::Borrow => PMode::Ref(false),
                    OwnershipMod::BorrowMut => PMode::Ref(true),
                    _ => PMode::Value,
                }
            };
            if p.default.is_some() && mode != PMode::Value {
                return Err(CgErr(format!("parameter `{}`: only by-value parameters can have a default value", p.name)));
            }
            match mode {
                PMode::Value => lls.push(self.llty(&t)?.into()),
                PMode::Ref(_) => {
                    // the pointee must be a type we can represent
                    self.llty(&t)?;
                    lls.push(ptr.into());
                }
                PMode::Variadic => {
                    self.llty(&t)?;
                    lls.push(self.str_ty().into());
                }
            }
            params.push(ParamInfo { name: p.name.clone(), ty: t, mode, default: p.default.as_ref() });
        }
        if params.iter().enumerate().any(|(i, p)| p.mode == PMode::Variadic && i + 1 != params.len()) {
            return Err(CgErr("a variadic parameter must be the last parameter".into()));
        }
        let ret = f.return_type.as_ref().map(|t| self.rty(t)).unwrap_or(Ty::Unit);
        let ft = if is_unit_like(&ret) {
            self.context.void_type().fn_type(&lls, false)
        } else {
            self.llty(&ret)?.fn_type(&lls, false)
        };
        let val = self.module.add_function(&Self::fn_symbol(&w.key), ft, None);
        self.fns.insert(w.key.clone(), FnInfo { val, params, ret, self_kind });
        Ok(())
    }

    fn gen_fn(&mut self, w: &FnWork<'a>) -> R<()> {
        let f = w.decl;
        let (val, params, ret, self_kind) = {
            let i = &self.fns[&w.key];
            (i.val, i.params.clone(), i.ret.clone(), i.self_kind)
        };
        let body = f.body.as_ref().unwrap();
        self.cur_fn = Some(val);
        self.cur_self = w.self_ty.clone();
        self.ret_ty = ret.clone();
        self.locals = vec![HashMap::new()];
        self.loops.clear();
        self.dead = false;
        let alloca_bb = self.context.append_basic_block(val, "entry");
        self.alloca_bb = Some(alloca_bb);
        let body_bb = self.context.append_basic_block(val, "body");
        self.position(body_bb);
        let mut llidx = 0u32;
        if let Some(kind) = self_kind {
            let sty = w.self_ty.clone().unwrap();
            let pv = val.get_nth_param(llidx).unwrap();
            llidx += 1;
            match kind {
                // `borrow self` / `borrow mut self`: the local IS the caller's value
                // (reads and writes go through the pointer, Document 6 §3).
                PMode::Ref(_) => {
                    self.locals.last_mut().unwrap().insert("self".to_string(), (pv.into_pointer_value(), sty));
                }
                _ => {
                    let slot = self.declare_local("self", &sty)?;
                    self.builder.build_store(slot, pv)?;
                }
            }
        }
        for p in &params {
            let pv = val.get_nth_param(llidx).unwrap();
            llidx += 1;
            match p.mode {
                PMode::Ref(_) => {
                    self.locals.last_mut().unwrap().insert(p.name.clone(), (pv.into_pointer_value(), p.ty.clone()));
                }
                PMode::Value => {
                    let slot = self.declare_local(&p.name, &p.ty)?;
                    self.builder.build_store(slot, pv)?;
                }
                PMode::Variadic => {
                    let sl = Ty::Ref(false, Box::new(Ty::Array(Box::new(p.ty.clone()))));
                    let slot = self.declare_local(&p.name, &sl)?;
                    self.builder.build_store(slot, pv)?;
                }
            }
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
        self.cur_self = None;
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
