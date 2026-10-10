//! Core Type System (Phase 3), per Document 25 §2.3's scope: primitive
//! types (Document 5 §2), the full 6-rule type-inference engine
//! (Document 5 §4), and the data-shape half of `struct`/`enum`
//! (Document 7) — fields/variants only, not traits/impl/generics
//! (Phase 4/5).
//!
//! Implementation approach note (flagged, not silently assumed):
//! Document 17 §4.2 describes semantic analysis as "a constraint-based
//! (Hindley-Milner-style) type-inference algorithm". This module
//! implements Document 5 §4's 6 rules directly as a **bidirectional,
//! expected-type-propagating checker** (infer when no expectation is
//! given, check against an expectation when one is) rather than a full
//! generalized HM unifier with a separate constraint-solving pass. This
//! is a deliberate scope decision, not an oversight: Document 5's rules
//! are themselves described operationally (explicit-annotation-wins,
//! literal-defaulting, contextual-adoption, no-cross-branch-guessing,
//! ambiguity-is-an-error) rather than mandating a particular algorithm,
//! and full HM-style unification only becomes necessary once generic
//! type *variables* exist to unify over — which is Phase 5's territory
//! (Document 25 §2.3 explicitly scopes generics/monomorphization there,
//! not here). Revisit this decision in Phase 5 if bidirectional checking
//! turns out to be insufficient once real generic functions need
//! inferring. Flagged for sign-off, consistent with the other flagged
//! deviations from prior phases.
//!
//! Also flagged: `ast::Expr`/`ast::Stmt` don't carry per-node `Span`
//! information yet (only `ast::Item` does, from Phase 2). Type errors
//! below therefore report the *item* they occurred in as context, not a
//! precise line/column — full source-span-per-expression is not
//! required by this phase's exit criteria (Document 25 §2.3 asks for
//! correct accept/reject behavior, not diagnostic precision — that's
//! Document 22/Phase 23's job) but is a real precision gap worth noting
//! rather than silently pretending otherwise.

use crate::ast::*;
use std::collections::HashMap;

// ---------- Resolved types ----------

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    I8, I16, I32, I64, I128, Isize,
    U8, U16, U32, U64, U128, Usize,
    F32, F64,
    Bool,
    Char,
    StringTy,
    Str,
    Unit,
    Never,
    Array(Box<Ty>),
    /// Phase 11a (Document 5 §3.1): the fixed-size array `[T; N]`, size is part
    /// of the type. Produced only when `N` is an integer literal or a `const`
    /// item; any other size expression (e.g. a const-generic `ROWS * COLS`,
    /// Document 8 §8) still resolves to the unsized `Array` as before.
    Fixed(Box<Ty>, u64),
    Tuple(Vec<Ty>),
    Ref(bool, Box<Ty>),
    /// A user-declared struct or enum with no generic parameters, by
    /// name. Phase 3 had no generics at all; as of Phase 5, a
    /// zero-generic-parameter struct/enum still resolves to this
    /// variant (simplest, most common case), while one with generic
    /// parameters resolves to `Ty::Generic` once concrete/const
    /// arguments are supplied.
    Named(String),
    /// A generic struct/enum instantiated with concrete type and/or
    /// const arguments (Document 8), e.g. `Matrix<f64, 2, 3>` resolves
    /// to `Generic("Matrix", [Type(F64), Const(2), Const(3)])`.
    Generic(String, Vec<GenericArg>),
    /// An unresolved reference to one of the *enclosing* declaration's
    /// own generic type parameters (e.g. `T` inside `struct Pair<A,
    /// B> { first: A, second: B }`'s field types, before any concrete
    /// instantiation is known). Only appears transiently during
    /// generic-declaration registration/substitution; never the final
    /// type of a checked expression in ordinary (non-generic-body)
    /// code.
    TypeParam(String),
    /// `dyn Trait` (Document 7 §4.5) — statically known only to
    /// implement `Trait`, resolved via vtable at runtime. Method
    /// resolution against this type looks up the *trait's* declared
    /// signature, not any concrete `impl`'s.
    DynTrait(String),
    OptionTy(Box<Ty>),
    ResultTy(Box<Ty>, Box<Ty>),
    Fn(Vec<Ty>, Box<Ty>),
    /// The type of the `null` literal itself (Document 5 §2.6) — only
    /// a valid value inside `unsafe { }` blocks; everywhere else,
    /// wanting "value or absence" must use `OptionTy`.
    Null,
}

/// One generic argument at an instantiation site: either a concrete
/// type (`Matrix<f64, ...>`'s `f64`) or a resolved const value
/// (`Matrix<..., 2, 3>`'s `2`/`3`, Document 8 §8). Phase 5 only
/// supports integer-literal const-generic arguments — Document 8 §8's
/// own examples (`Matrix<f64,2,3>`, array sizes) never show anything
/// else in this position, so nothing broader is invented.
#[derive(Debug, Clone, PartialEq)]
pub enum GenericArg {
    Type(Ty),
    Const(i64),
}

impl std::fmt::Display for GenericArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenericArg::Type(t) => write!(f, "{}", t),
            GenericArg::Const(n) => write!(f, "{}", n),
        }
    }
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::I8 => write!(f, "i8"), Ty::I16 => write!(f, "i16"), Ty::I32 => write!(f, "i32"),
            Ty::I64 => write!(f, "i64"), Ty::I128 => write!(f, "i128"), Ty::Isize => write!(f, "isize"),
            Ty::U8 => write!(f, "u8"), Ty::U16 => write!(f, "u16"), Ty::U32 => write!(f, "u32"),
            Ty::U64 => write!(f, "u64"), Ty::U128 => write!(f, "u128"), Ty::Usize => write!(f, "usize"),
            Ty::F32 => write!(f, "f32"), Ty::F64 => write!(f, "f64"),
            Ty::Bool => write!(f, "bool"), Ty::Char => write!(f, "char"),
            Ty::StringTy => write!(f, "String"), Ty::Str => write!(f, "str"),
            Ty::Unit => write!(f, "()"), Ty::Never => write!(f, "!"),
            Ty::Array(t) => write!(f, "[{}]", t),
            Ty::Fixed(t, n) => write!(f, "[{}; {}]", t, n),
            Ty::Tuple(ts) => write!(f, "({})", ts.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")),
            Ty::Ref(m, t) => write!(f, "&{}{}", if *m { "mut " } else { "" }, t),
            Ty::Named(n) => write!(f, "{}", n),
            Ty::Generic(n, args) => write!(f, "{}<{}>", n, args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ")),
            Ty::TypeParam(n) => write!(f, "{}", n),
            Ty::DynTrait(n) => write!(f, "dyn {}", n),
            Ty::OptionTy(t) => write!(f, "Option<{}>", t),
            Ty::ResultTy(o, e) => write!(f, "Result<{}, {}>", o, e),
            Ty::Fn(ps, r) => write!(f, "fn({}) -> {}", ps.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", "), r),
            Ty::Null => write!(f, "null"),
        }
    }
}

/// The single source of truth for `Option`/`Result`'s variants
/// (Document 7 §3.2: both are ordinary two-variant enums). Phase 9:
/// constructor typing (`Ok(..)`/`Err(..)`/`Some(..)`/`None`), pattern
/// binding, and `exhaustive.rs`'s constructor sets ALL read this one
/// table, so the three can never disagree about what an `Option` or
/// `Result` is made of. (Before Phase 9, `exhaustive.rs` carried its own
/// private copy.) Returns `None` for any other type.
pub fn builtin_variants(ty: &Ty) -> std::option::Option<Vec<(&'static str, Vec<Ty>)>> {
    match ty {
        Ty::OptionTy(inner) => Some(vec![
            ("Some", vec![(**inner).clone()]),
            ("None", vec![]),
        ]),
        Ty::ResultTy(ok, err) => Some(vec![
            ("Ok", vec![(**ok).clone()]),
            ("Err", vec![(**err).clone()]),
        ]),
        _ => None,
    }
}

/// Auto-dereferences any number of reference layers (`&T`, `borrow mut
/// T`) so field access, method resolution and indexing work through a
/// `borrow`ed parameter (Document 11 §5's `balance: borrow mut Account`
/// followed by `balance.funds`). Before Phase 9 a `Ty::Ref` receiver
/// silently fell through to `Ty::Unit`.
fn strip_refs(t: Ty) -> Ty {
    match t {
        Ty::Ref(_, inner) => strip_refs(*inner),
        other => other,
    }
}

/// True if a block can never fall off its end normally: no tail
/// expression, and its final statement is `return`/`break`/`continue`,
/// a `throw`, or a call to `panic`. Such a block has type `!`
/// (Document 5 §2.5) rather than `()`, so e.g. a `catch` block ending in
/// `return ...;` (Document 24 §1) unifies with the `try` block's value type.
fn block_diverges(block: &Block) -> bool {
    if block.tail.is_some() {
        return false;
    }
    match block.stmts.last() {
        Some(Stmt::Return(_)) | Some(Stmt::Break { .. }) | Some(Stmt::Continue { .. }) => true,
        Some(Stmt::Expr(e)) => expr_diverges(e),
        _ => false,
    }
}

fn expr_diverges(e: &Expr) -> bool {
    match e {
        Expr::Return(_) | Expr::Throw(_) => true,
        Expr::Call { callee, .. } => matches!(callee.as_ref(), Expr::Ident(n) if n.as_str() == "panic"),
        _ => false,
    }
}

/// The type a CALLER's argument must have for `p` (Phase 11a): a `borrow x: T` /
/// `borrow mut x: T` parameter is passed as the reference `borrow x`, i.e. `&T` /
/// `&mut T` (inside the function the parameter name is used as a plain `T`,
/// Document 6 §3 / Document 10 §2).
fn arg_ty(p: &Param, base: Ty) -> Ty {
    match p.ownership {
        OwnershipMod::Borrow if !p.is_variadic => Ty::Ref(false, Box::new(base)),
        OwnershipMod::BorrowMut if !p.is_variadic => Ty::Ref(true, Box::new(base)),
        _ => base,
    }
}

fn ty_is_integer(t: &Ty) -> bool {
    matches!(t, Ty::I8|Ty::I16|Ty::I32|Ty::I64|Ty::I128|Ty::Isize|Ty::U8|Ty::U16|Ty::U32|Ty::U64|Ty::U128|Ty::Usize)
}
fn ty_is_float(t: &Ty) -> bool {
    matches!(t, Ty::F32 | Ty::F64)
}
fn ty_is_numeric(t: &Ty) -> bool {
    ty_is_integer(t) || ty_is_float(t)
}

/// Resolves an `ast::Type` (syntactic, from the parser) to a `Ty`
/// (semantic). Struct/enum names are trusted to exist here and
/// validated separately by `TypeChecker::check_program`'s name-
/// resolution pass, so an unknown name still produces `Ty::Named`
/// rather than failing here — this function is pure syntax-to-shape
/// translation, not validation.
pub fn resolve_type(t: &Type) -> Ty {
    resolve_type_depth(t, 0)
}

thread_local! {
    /// Phase 11a: `type Name = ...;` aliases of the program being compiled
    /// (Document 3 Category A `type`: "purely a compile-time naming
    /// convenience"). `resolve_type` is a free function without access to the
    /// checker, so the item context is kept per thread; it is (re)filled by
    /// `register_item_context` at the start of every check/codegen.
    static ALIASES: std::cell::RefCell<HashMap<String, Type>> = std::cell::RefCell::new(HashMap::new());
    /// Phase 11a: integer-valued `const` items, so `[T; N]` with a named
    /// constant `N` resolves to a fixed array.
    static CONSTS: std::cell::RefCell<HashMap<String, i128>> = std::cell::RefCell::new(HashMap::new());
}

/// Integer constant folding for array sizes / `const` items (literals, other
/// consts, parentheses, unary minus, `+ - * / % <<`).
pub fn eval_const_int(e: &Expr, consts: &HashMap<String, i128>) -> Option<i128> {
    match e {
        Expr::Literal(Literal::Int(t)) => t.replace('_', "").parse::<i128>().ok(),
        Expr::Literal(Literal::IntHex(t)) => i128::from_str_radix(t.replace('_', "").trim_start_matches("0x"), 16).ok(),
        Expr::Literal(Literal::IntOct(t)) => i128::from_str_radix(t.replace('_', "").trim_start_matches("0o"), 8).ok(),
        Expr::Literal(Literal::IntBin(t)) => i128::from_str_radix(t.replace('_', "").trim_start_matches("0b"), 2).ok(),
        Expr::Paren(i) => eval_const_int(i, consts),
        Expr::Ident(n) => consts.get(n).copied(),
        Expr::Unary { op: UnaryOp::Neg, expr } => eval_const_int(expr, consts)?.checked_neg(),
        Expr::Binary { op, lhs, rhs } => {
            let (a, b) = (eval_const_int(lhs, consts)?, eval_const_int(rhs, consts)?);
            match op {
                BinaryOp::Add => a.checked_add(b),
                BinaryOp::Sub => a.checked_sub(b),
                BinaryOp::Mul => a.checked_mul(b),
                BinaryOp::Div => a.checked_div(b),
                BinaryOp::Mod => a.checked_rem(b),
                BinaryOp::Shl => u32::try_from(b).ok().and_then(|s| a.checked_shl(s)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Fills the per-thread alias / constant tables from `items` (top-level and
/// hoisted nested items). Idempotent.
pub fn register_item_context(items: &[&Item]) {
    let mut aliases = HashMap::new();
    for it in items {
        if let ItemKind::TypeAlias(a) = &it.kind {
            if a.generics.0.is_empty() {
                aliases.insert(a.name.clone(), a.ty.clone());
            }
        }
    }
    ALIASES.with(|c| *c.borrow_mut() = aliases);
    let mut consts: HashMap<String, i128> = HashMap::new();
    for _ in 0..8 {
        for it in items {
            if let ItemKind::Const(c) = &it.kind {
                if !consts.contains_key(&c.name) {
                    if let Some(v) = eval_const_int(&c.value, &consts) {
                        consts.insert(c.name.clone(), v);
                    }
                }
            }
        }
    }
    CONSTS.with(|c| *c.borrow_mut() = consts);
}

fn resolve_type_depth(t: &Type, depth: u32) -> Ty {
    let alias = |name: &str| -> Option<Type> { ALIASES.with(|a| a.borrow().get(name).cloned()) };
    match t {
        Type::Primitive(s) => match s.as_str() {
            "i8" => Ty::I8, "i16" => Ty::I16, "i32" => Ty::I32, "i64" => Ty::I64,
            "i128" => Ty::I128, "isize" => Ty::Isize,
            "u8" => Ty::U8, "u16" => Ty::U16, "u32" => Ty::U32, "u64" => Ty::U64,
            "u128" => Ty::U128, "usize" => Ty::Usize,
            "f32" => Ty::F32, "f64" => Ty::F64,
            "bool" => Ty::Bool, "char" => Ty::Char,
            "String" => Ty::StringTy, "str" => Ty::Str,
            other => match alias(other) {
                Some(a) if depth < 16 => resolve_type_depth(&a, depth + 1),
                _ => Ty::Named(other.to_string()),
            },
        },
        Type::Named(n, _args) => match alias(n) {
            Some(a) if depth < 16 => resolve_type_depth(&a, depth + 1),
            _ => Ty::Named(n.clone()),
        },
        Type::Array(inner, size) => {
            let elem = resolve_type_depth(inner, depth);
            if let Some(sz) = size {
                let n = CONSTS.with(|c| eval_const_int(sz, &c.borrow()));
                if let Some(n) = n {
                    if (0..=u32::MAX as i128).contains(&n) {
                        return Ty::Fixed(Box::new(elem), n as u64);
                    }
                }
            }
            Ty::Array(Box::new(elem))
        }
        Type::Tuple(ts) => Ty::Tuple(ts.iter().map(|x| resolve_type_depth(x, depth)).collect()),
        Type::Ref { mutable, inner, .. } => Ty::Ref(*mutable, Box::new(resolve_type_depth(inner, depth))),
        Type::Dyn(n, _) => Ty::DynTrait(n.clone()),
        Type::Fn(ps, r) => Ty::Fn(ps.iter().map(|x| resolve_type_depth(x, depth)).collect(), Box::new(resolve_type_depth(r, depth))),
        Type::Option(inner) => Ty::OptionTy(Box::new(resolve_type_depth(inner, depth))),
        Type::Result(o, e) => Ty::ResultTy(Box::new(resolve_type_depth(o, depth)), Box::new(resolve_type_depth(e, depth))),
        Type::Unit => Ty::Unit,
        Type::Never => Ty::Never,
        Type::ConstArg(_) => Ty::Unit, // not a real type position; see const_int_value below for the Phase 5 handling
    }
}

/// Extracts an integer value from a const-generic argument expression
/// (Document 8 §8's `Matrix<f64, 2, 3>` — the `2`/`3`). Phase 5 only
/// supports a bare integer literal here (optionally negative, though no
/// spec example shows a negative dimension) — Document 8's own examples
/// never show anything more complex (no const-generic arithmetic
/// expressions like `N + 1`), so nothing broader is invented.
fn const_int_value(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Literal(Literal::Int(s)) => s.replace('_', "").parse::<i64>().ok(),
        Expr::Unary { op: UnaryOp::Neg, expr: inner } => {
            const_int_value(inner).map(|n| -n)
        }
        _ => None,
    }
}

/// Replaces `Ty::TypeParam(n)` with the concrete type supplied for `n`
/// in `subst` (used when checking a generic struct literal's fields
/// against a known concrete instantiation, e.g. `Pair<i32, String>`'s
/// `first` field should be checked as `i32`, not the abstract
/// `TypeParam("A")` stored in the struct's registered shape).
fn substitute_type_params(ty: &Ty, subst: &HashMap<String, Ty>) -> Ty {
    match ty {
        Ty::TypeParam(n) => subst.get(n).cloned().unwrap_or_else(|| ty.clone()),
        Ty::Array(inner) => Ty::Array(Box::new(substitute_type_params(inner, subst))),
        Ty::Fixed(inner, n) => Ty::Fixed(Box::new(substitute_type_params(inner, subst)), *n),
        Ty::Tuple(ts) => Ty::Tuple(ts.iter().map(|t| substitute_type_params(t, subst)).collect()),
        Ty::Ref(m, inner) => Ty::Ref(*m, Box::new(substitute_type_params(inner, subst))),
        Ty::OptionTy(inner) => Ty::OptionTy(Box::new(substitute_type_params(inner, subst))),
        Ty::ResultTy(o, e) => Ty::ResultTy(
            Box::new(substitute_type_params(o, subst)),
            Box::new(substitute_type_params(e, subst)),
        ),
        Ty::Fn(ps, r) => Ty::Fn(
            ps.iter().map(|t| substitute_type_params(t, subst)).collect(),
            Box::new(substitute_type_params(r, subst)),
        ),
        other => other.clone(),
    }
}

/// Structural unification for generic-parameter inference (Document 8
/// §2's own example: `fn largest<T>(list: [T]) -> T`, called as
/// `largest(nums)` where `nums: [i32]`, must infer `T = i32`). Walks
/// `param_ty` (which may contain `Ty::TypeParam` placeholders, from a
/// generic function's registered signature) against `arg_ty` (the
/// actual, concrete argument type), binding each `TypeParam` name to
/// whatever concrete type structurally occupies that position. Only
/// handles the structural shapes Document 8's own examples actually
/// use (a bare type parameter, one level of `[T]`/`&T`/tuple nesting) —
/// deeper or more exotic unification isn't attempted; an unresolved
/// type parameter is simply left unbound rather than reported as an
/// error (a known, flagged scope simplification).
fn unify_infer(param_ty: &Ty, arg_ty: &Ty, out: &mut HashMap<String, Ty>) {
    match (param_ty, arg_ty) {
        (Ty::TypeParam(n), t) => {
            out.entry(n.clone()).or_insert_with(|| t.clone());
        }
        (Ty::Array(p), Ty::Array(a)) => unify_infer(p, a, out),
        (Ty::Array(p), Ty::Fixed(a, _)) | (Ty::Fixed(p, _), Ty::Fixed(a, _)) => unify_infer(p, a, out),
        (Ty::Ref(_, p), Ty::Ref(_, a)) => unify_infer(p, a, out),
        (Ty::Tuple(ps), Ty::Tuple(as_)) if ps.len() == as_.len() => {
            for (p, a) in ps.iter().zip(as_.iter()) {
                unify_infer(p, a, out);
            }
        }
        _ => {}
    }
}

/// Canonical registry-lookup key for a `Ty` — the same key `impls_by_type`
/// is keyed by (an `impl`'s `target.name`, which for a primitive like
/// `impl Comparable for i32` is literally the text `"i32"`, since
/// primitive type names lex as plain identifiers, not keywords — Phase
/// 1's design decision). Used by `satisfies_bound` so a trait bound can
/// be checked against a monomorphized primitive type just as well as a
/// user-declared struct/enum.
fn ty_lookup_name(ty: &Ty) -> Option<String> {
    match ty {
        Ty::Named(n) | Ty::Generic(n, _) => Some(n.clone()),
        Ty::I8 => Some("i8".into()), Ty::I16 => Some("i16".into()), Ty::I32 => Some("i32".into()),
        Ty::I64 => Some("i64".into()), Ty::I128 => Some("i128".into()), Ty::Isize => Some("isize".into()),
        Ty::U8 => Some("u8".into()), Ty::U16 => Some("u16".into()), Ty::U32 => Some("u32".into()),
        Ty::U64 => Some("u64".into()), Ty::U128 => Some("u128".into()), Ty::Usize => Some("usize".into()),
        Ty::F32 => Some("f32".into()), Ty::F64 => Some("f64".into()),
        Ty::Bool => Some("bool".into()), Ty::Char => Some("char".into()),
        Ty::StringTy => Some("String".into()), Ty::Str => Some("str".into()),
        _ => None,
    }
}

// ---------- Errors ----------

#[derive(Debug, Clone, PartialEq)]
pub struct TypeError {
    pub message: String,
    /// Best-available context: which item this occurred in. See the
    /// module doc comment's note on why this isn't a precise span yet.
    pub context: String,
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "type error in `{}`: {}", self.context, self.message)
    }
}

// ---------- Struct/enum data-shape registry ----------

#[derive(Debug, Clone)]
pub struct StructShape {
    pub fields: Vec<(String, Ty)>,
    /// `true` for `struct Foo(A, B);` tuple structs — fields are
    /// positionally named "0", "1", ... internally.
    pub is_tuple: bool,
    /// This struct's own generic parameters, in declaration order
    /// (Document 8 §1/§8) — empty for a non-generic struct. Field types
    /// referencing one of these by name are stored as `Ty::TypeParam`,
    /// not `Ty::Named` (see `mark_type_params`).
    pub generics: Vec<GenericParam>,
}

/// Recursively replaces `Ty::Named(n)` with `Ty::TypeParam(n)` wherever
/// `n` matches one of the enclosing declaration's own generic
/// parameter names — so a generic struct's field types (e.g. `struct
/// Pair<A, B> { first: A, second: B }`) correctly distinguish "this
/// field's type is the generic parameter A" from "this field's type is
/// a concrete struct that happens to be named A". Applied once, right
/// after a generic struct/enum's fields are resolved via the ordinary
/// (context-free) `resolve_type`.
fn mark_type_params(ty: Ty, param_names: &[String]) -> Ty {
    match ty {
        Ty::Named(n) if param_names.contains(&n) => Ty::TypeParam(n),
        Ty::Array(inner) => Ty::Array(Box::new(mark_type_params(*inner, param_names))),
        Ty::Fixed(inner, n) => Ty::Fixed(Box::new(mark_type_params(*inner, param_names)), n),
        Ty::Tuple(ts) => Ty::Tuple(ts.into_iter().map(|t| mark_type_params(t, param_names)).collect()),
        Ty::Ref(m, inner) => Ty::Ref(m, Box::new(mark_type_params(*inner, param_names))),
        Ty::OptionTy(inner) => Ty::OptionTy(Box::new(mark_type_params(*inner, param_names))),
        Ty::ResultTy(o, e) => Ty::ResultTy(
            Box::new(mark_type_params(*o, param_names)),
            Box::new(mark_type_params(*e, param_names)),
        ),
        Ty::Fn(ps, r) => Ty::Fn(
            ps.into_iter().map(|t| mark_type_params(t, param_names)).collect(),
            Box::new(mark_type_params(*r, param_names)),
        ),
        other => other,
    }
}

#[derive(Debug, Clone)]
pub struct EnumShape {
    pub variants: Vec<(String, Vec<Ty>)>,
    pub generics: Vec<GenericParam>,
}

/// A method signature (params excluding `self`, return type). Used for
/// both trait-declared methods and concrete `impl`-provided methods.
#[derive(Debug, Clone)]
pub struct FnSig {
    pub params: Vec<Ty>,
    pub ret: Ty,
    /// `true` if this is a trait method with a default body (Document 7
    /// §4.3) — such methods are *not* required to be re-implemented by
    /// an `impl`. Always `true` (irrelevant) for `ImplRecord` methods,
    /// which always have a body by construction.
    pub has_default_body: bool,
}

#[derive(Debug, Clone)]
pub struct TraitShape {
    pub methods: HashMap<String, FnSig>,
}

/// One `impl` block's provided methods for a given target type. `Vec`
/// (not a single record) because a type can have both an inherent impl
/// and multiple trait impls, all contributing callable methods.
#[derive(Debug, Clone)]
pub struct ImplRecord {
    pub trait_name: Option<String>,
    /// The trait reference's own generic arguments, resolved (Phase 9):
    /// `impl From<IoError> for AppError` records `[IoError]` here. Needed
    /// so the `?` operator's `.into()` step (Document 11 §2) can ask "does
    /// `From<E2> for E` exist" -- `trait_name` alone can't distinguish
    /// `From<IoError>` from `From<ParseError>` on the same target type.
    pub trait_args: Vec<Ty>,
    pub methods: HashMap<String, FnSig>,
}

pub struct TypeChecker {
    structs: HashMap<String, StructShape>,
    enums: HashMap<String, EnumShape>,
    /// Registered `fn` signatures (Document 5 §5: params/return type
    /// are always explicit, so these are always fully known) — enables
    /// Document 5 rule 3 (contextual inference) to work through real
    /// function calls, e.g. `fn setAge(age: u8) {...} setAge(25);`
    /// (Document 5 §4's own example) correctly infers `25: u8`.
    /// Extended in Phase 5 to also carry generic parameters and
    /// where-clause bounds (Document 8 §2/§3), so a call to a generic
    /// function can infer its type parameters from argument types and
    /// verify the declared bounds are actually satisfied.
    functions: HashMap<String, FunctionShape>,
    traits: HashMap<String, TraitShape>,
    impls_by_type: HashMap<String, Vec<ImplRecord>>,
    /// Phase 11a: `const`/`static` items (name -> declared type); an identifier
    /// that is not a local resolves here.
    globals: HashMap<String, Ty>,
    /// Phase 11a: the concrete type of the `impl` whose method is being checked
    /// (what `Self` means in expressions: `Self { .. }`, `Self::new()`).
    self_type: Option<Ty>,
    /// Stack of currently-enclosing loops (Phase 8, Document 9 §3.5),
    /// innermost last. Replaces an earlier draft that walked each
    /// `loop`'s body twice (once to collect `break <value>;` types,
    /// once normally) -- which would have reported any real type error
    /// inside a break-value expression twice over. This single-pass
    /// design has `check_stmt`'s `Break` handling record a break's
    /// checked value type directly into its target frame as it's
    /// encountered during the ONE normal walk, via `find_loop_frame`.
    loop_frames: Vec<LoopFrame>,
    /// Phase 9: stack of propagation targets (see `PropTarget`).
    prop_targets: Vec<PropTarget>,
    /// Phase 9: set whenever a call/method/path/field/await fell back to
    /// the "unresolved, stay silent" result. `check_propagate` consults
    /// it to tell "`?` applied to an unresolvable stdlib call" (silent)
    /// from "`?` applied to something genuinely not a Result/Option"
    /// (a real error).
    saw_unresolved: bool,
    /// Phase 9: every resolved propagation edge, in source order.
    pub propagations: Vec<PropagationRecord>,
    /// Phase 10 (Document 17 §1 "TYPED AST"): the resolved type of every
    /// expression the checker visited, keyed by the expression node's
    /// address (`&Expr as *const Expr as usize`). The AST is never
    /// mutated or moved after parsing, so node addresses are stable
    /// identities; this is the smallest mechanism that gives codegen a
    /// typed view without adding a type slot/id to every `Expr`. Look up
    /// with `TypeChecker::type_of`.
    pub expr_types: HashMap<usize, Ty>,
    /// Phase 10: for every `try`/`catch` expression (keyed like
    /// `expr_types`), the resolved `(value type, error type)` of Document
    /// 11 §3's implicit wrapper `Result<value, error>`; consumed by the
    /// `desugar` pass.
    pub try_info: HashMap<usize, (Ty, Ty)>,
    pub errors: Vec<TypeError>,
}

#[derive(Debug, Clone)]
pub struct FunctionShape {
    pub params: Vec<Ty>,
    pub ret: Ty,
    pub generics: Vec<GenericParam>,
    /// Flattened from `ast::WhereClause`: type-param name -> the list of
    /// trait names it must satisfy (Document 8 §3's `T: A + B` becomes
    /// `["A", "B"]`). Trait-bound generic *arguments* (e.g. a bound
    /// written `T: Container<i32>`) aren't tracked — Document 8's own
    /// examples (`T: Comparable`, `T: Serializable + Comparable + Clone`)
    /// never show a parameterized bound, so nothing broader is invented.
    pub bounds: Vec<(String, Vec<String>)>,
}

/// Local variable environment: a stack of scopes (blocks introduce a
/// new scope; `let` bindings add to the innermost one).
struct Env {
    scopes: Vec<HashMap<String, Ty>>,
}
impl Env {
    fn new() -> Self { Env { scopes: vec![HashMap::new()] } }
    fn push(&mut self) { self.scopes.push(HashMap::new()); }
    fn pop(&mut self) { self.scopes.pop(); }
    fn insert(&mut self, name: String, ty: Ty) {
        self.scopes.last_mut().unwrap().insert(name, ty);
    }
    fn get(&self, name: &str) -> Option<&Ty> {
        for scope in self.scopes.iter().rev() {
            if let Some(t) = scope.get(name) {
                return Some(t);
            }
        }
        None
    }
}

/// Where a `?`, `throw` or `return` at the current lexical position
/// sends control (Phase 9, Document 11 §2-§4). A stack, innermost last.
///
/// Pillar I design point: a `try` block is NOT a separate error
/// mechanism -- it is just another `PropTarget` on the SAME stack a
/// function body uses, and `?` inside it is routed by the SAME
/// `route_result_error` code that routes `?` in a `fn` body. That shared
/// code path is what makes the Document 11 §3 desugaring equivalence
/// structural rather than coincidental.
enum PropTarget {
    /// An enclosing `fn` body; holds its declared return type (`()` if
    /// none was declared).
    FnRet(Ty),
    /// A `try { }` block currently being checked (Document 11 §3).
    Try(TryFrame),
    /// A closure body: the closure's own return type isn't tracked yet
    /// (Phase 7 gave closures capture analysis only), so `?`/`return`
    /// inside one is not validated -- silent, like every other
    /// unresolved construct in this checker, never a fabricated error.
    Opaque,
}

/// Error-propagation facts collected while checking one `try` block.
struct TryFrame {
    /// The error type of every `?`-on-`Result` and every `throw` inside
    /// the block, in source order.
    err_sources: Vec<Ty>,
    /// True if a `?` was applied to an expression whose type this
    /// checker couldn't resolve (stdlib functions before Phase 16, etc.).
    /// Then "no error sources found" is not evidence the block has none.
    saw_opaque: bool,
}

/// Owned snapshot of the top `PropTarget`, so callers can inspect it
/// and then call `&mut self` methods without holding a borrow.
enum TargetView {
    Fn(Ty),
    Try,
    Opaque,
}

/// Which propagation path a `PropagationRecord` describes.
#[derive(Debug, Clone, PartialEq)]
pub enum PropKind {
    /// `?` on a `Result`: the `Err` value is propagated.
    ResultErr,
    /// `?` on an `Option`: `None` is propagated.
    OptionNone,
}

/// One resolved error-propagation edge (Phase 9). Recorded identically
/// for `?` in a `fn` body and for `?`/`throw` inside a `try` block, so
/// two programs with the same propagation behavior yield equal record
/// lists -- this is the pre-codegen check for Document 11 §7's
/// "`try`/`catch` is byte-identical to the hand-written `?`-chain".
#[derive(Debug, Clone, PartialEq)]
pub struct PropagationRecord {
    pub kind: PropKind,
    /// The error type being propagated (`None` for `OptionNone`).
    pub source: Option<Ty>,
    /// The error type it lands in (`None` for `OptionNone`).
    pub target: Option<Ty>,
    /// True iff `source != target`, i.e. a `From` conversion runs.
    pub converted: bool,
}

/// One entry in `TypeChecker::loop_frames` — Phase 8.
struct LoopFrame {
    label: Option<String>,
    /// Only `LoopExpr::Loop` collects break values (Document 9 §3.1:
    /// only `loop` can be an expression) -- `while`/`for`/`do-while`
    /// still push a frame (so `break`/`continue` inside them resolves
    /// correctly), just never accumulate into `break_types`.
    collects_breaks: bool,
    break_types: Vec<Ty>,
}

impl TypeChecker {
    pub fn new() -> Self {
        TypeChecker {
            structs: HashMap::new(),
            enums: HashMap::new(),
            functions: HashMap::new(),
            traits: HashMap::new(),
            impls_by_type: HashMap::new(),
            loop_frames: Vec::new(),
            prop_targets: Vec::new(),
            saw_unresolved: false,
            propagations: Vec::new(),
            expr_types: HashMap::new(),
            try_info: HashMap::new(),
            globals: HashMap::new(),
            self_type: None,
            errors: Vec::new(),
        }
    }

    /// Phase 10: the type the checker resolved for `expr` (see
    /// `expr_types`), if it was visited.
    pub fn type_of(&self, expr: &Expr) -> Option<&Ty> {
        self.expr_types.get(&(expr as *const Expr as usize))
    }

    /// Context-aware type resolution for positions where a user writes
    /// an explicit type annotation on an *expression* (a `let` binding,
    /// a cast target) — as opposed to the plain, context-free
    /// `resolve_type` free function used for struct/fn *declaration*
    /// registration. This is where Document 8 §8's `Matrix<f64, 2, 3>`
    /// actually needs full validation: arg count against the struct's
    /// declared generic parameters, and arg *kind* (a type argument
    /// where a type parameter is declared, a const value where a const
    /// parameter is declared).
    ///
    /// Scoped deliberately narrow rather than replacing `resolve_type`
    /// everywhere: struct/enum FIELD types and function PARAMETER type
    /// *registration* still use the plain function (so e.g. `fn
    /// foo(m: Matrix<f64,2,3>)`'s parameter type is registered as
    /// `Ty::Named("Matrix")`, args discarded, not validated) — flagged
    /// as a known scope boundary in PROGRESS.md rather than silently
    /// pretending full coverage, given the concrete exit-criteria
    /// example (Document 8 §9) is phrased as a direct expression, which
    /// this covers.
    fn resolve_type_full(&mut self, t: &Type, ctx: &str) -> Ty {
        if let Type::Named(name, args) = t {
            if !args.is_empty() {
                let generics = self.structs.get(name).map(|s| s.generics.clone())
                    .or_else(|| self.enums.get(name).map(|e| e.generics.clone()));
                if let Some(generics) = generics {
                    if args.len() != generics.len() {
                        self.errors.push(TypeError {
                            message: format!(
                                "`{}` expects {} generic argument(s), found {}",
                                name, generics.len(), args.len()
                            ),
                            context: ctx.into(),
                        });
                        return Ty::Named(name.clone());
                    }
                    let mut resolved = Vec::new();
                    let mut kind_error = false;
                    for (param, arg) in generics.iter().zip(args.iter()) {
                        match (param, arg) {
                            (GenericParam::Type { .. }, Type::ConstArg(_)) => {
                                self.errors.push(TypeError {
                                    message: format!("`{}`: expected a type argument, found a const value", name),
                                    context: ctx.into(),
                                });
                                kind_error = true;
                            }
                            (GenericParam::Type { .. }, ty) => {
                                resolved.push(GenericArg::Type(self.resolve_type_full(ty, ctx)));
                            }
                            (GenericParam::Const { .. }, Type::ConstArg(expr)) => {
                                match const_int_value(expr) {
                                    Some(n) => resolved.push(GenericArg::Const(n)),
                                    None => {
                                        self.errors.push(TypeError {
                                            message: format!("`{}`: const-generic argument must be an integer literal", name),
                                            context: ctx.into(),
                                        });
                                        kind_error = true;
                                    }
                                }
                            }
                            (GenericParam::Const { .. }, _) => {
                                self.errors.push(TypeError {
                                    message: format!("`{}`: expected a const value argument, found a type", name),
                                    context: ctx.into(),
                                });
                                kind_error = true;
                            }
                        }
                    }
                    if kind_error {
                        return Ty::Named(name.clone());
                    }
                    return Ty::Generic(name.clone(), resolved);
                }
            }
        }
        resolve_type(t)
    }

    pub fn check_program(&mut self, program: &Program) {
        // Phase 11a: items declared inside function bodies are hoisted (they
        // are checked and registered like top-level items; same-named nested
        // items of different functions are rejected by codegen).
        let items = crate::desugar::all_items(program);
        register_item_context(&items);
        // Pass 1: register all struct/enum/trait data shapes and fn
        // signatures first, so forward references (a function defined
        // before a struct it uses, or mutual struct references) resolve
        // regardless of declaration order.
        for item in &items {
            self.register_item(item);
        }
        // Pass 2: register and validate every `impl` block -- run only
        // after pass 1 so a trait declared textually *after* its impl
        // still resolves correctly, and so the orphan-rule / required-
        // method-completeness checks have the full struct/enum/trait
        // picture available regardless of declaration order.
        for item in &items {
            self.register_impls(item);
        }
        // Pass 3: type-check every function body (can now resolve
        // method calls via `impls_by_type`/`traits`).
        for item in &items {
            self.check_item(item);
        }
    }

    fn register_item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Const(c) => {
                self.globals.insert(c.name.clone(), resolve_type(&c.ty));
            }
            ItemKind::Static(c) => {
                self.globals.insert(c.name.clone(), resolve_type(&c.ty));
            }
            ItemKind::Struct(s) => {
                let generics = s.generics.0.clone();
                let type_param_names: Vec<String> = generics.iter().filter_map(|g| match g {
                    GenericParam::Type { name, .. } => Some(name.clone()),
                    GenericParam::Const { .. } => None,
                }).collect();
                let (fields, is_tuple) = match &s.body {
                    StructBody::Named(fs) => (
                        fs.iter().map(|f| (f.name.clone(), mark_type_params(resolve_type(&f.ty), &type_param_names))).collect(),
                        false,
                    ),
                    StructBody::Tuple(ts) => (
                        ts.iter().enumerate().map(|(i, t)| (i.to_string(), mark_type_params(resolve_type(t), &type_param_names))).collect(),
                        true,
                    ),
                    StructBody::Unit => (Vec::new(), false),
                };
                self.structs.insert(s.name.clone(), StructShape { fields, is_tuple, generics });
            }
            ItemKind::Enum(e) => {
                let generics = e.generics.0.clone();
                let type_param_names: Vec<String> = generics.iter().filter_map(|g| match g {
                    GenericParam::Type { name, .. } => Some(name.clone()),
                    GenericParam::Const { .. } => None,
                }).collect();
                let variants = e.variants.iter()
                    .map(|v| (v.name.clone(), v.data.iter().map(|t| mark_type_params(resolve_type(t), &type_param_names)).collect()))
                    .collect();
                self.enums.insert(e.name.clone(), EnumShape { variants, generics });
            }
            ItemKind::Fn(f) => {
                let generics = f.generics.0.clone();
                let type_param_names: Vec<String> = generics.iter().filter_map(|g| match g {
                    GenericParam::Type { name, .. } => Some(name.clone()),
                    GenericParam::Const { .. } => None,
                }).collect();
                let params = f.params.iter()
                    .filter(|p| p.name != "self")
                    .map(|p| arg_ty(p, mark_type_params(resolve_type(&p.ty), &type_param_names)))
                    .collect();
                let ret = f.return_type.as_ref()
                    .map(|t| mark_type_params(resolve_type(t), &type_param_names))
                    .unwrap_or(Ty::Unit);
                let bounds = f.where_clause.0.iter()
                    .map(|wb| (wb.name.clone(), wb.bounds.iter().map(|tb| tb.name.clone()).collect()))
                    .collect();
                self.functions.insert(f.name.clone(), FunctionShape { params, ret, generics, bounds });
            }
            ItemKind::Trait(t) => {
                let methods = t.items.iter().filter_map(|ti| match ti {
                    TraitItem::Fn(f) => {
                        let params = f.params.iter()
                            .filter(|p| p.name != "self")
                            .map(|p| resolve_type(&p.ty))
                            .collect();
                        let ret = f.return_type.as_ref().map(resolve_type).unwrap_or(Ty::Unit);
                        Some((f.name.clone(), FnSig { params, ret, has_default_body: f.body.is_some() }))
                    }
                    TraitItem::AssocType(_) => None,
                }).collect();
                self.traits.insert(t.name.clone(), TraitShape { methods });
            }
            ItemKind::Mod(m) => {
                for inner in &m.items {
                    self.register_item(inner);
                }
            }
            _ => {}
        }
    }

    /// Pass 2: register every `impl` block's methods into
    /// `impls_by_type`, and validate the two things Document 25 §2.3's
    /// exit criteria requires for this phase: the orphan-rule check
    /// (Document 7 §5) and required-trait-method completeness
    /// (Document 7 §4.2/§4.3 — every non-default trait method must be
    /// provided).
    fn register_impls(&mut self, item: &Item) {
        if let ItemKind::Impl(impl_decl) = &item.kind {
            let target_name = impl_decl.target.name.clone();
            let trait_name = impl_decl.trait_ref.as_ref().map(|tr| tr.name.clone());
            // Same `Self`-substitution as `check_fn` (see its doc
            // comment), applied here too so a *registered* method
            // signature (what `resolve_method` hands back to a real
            // call site) has `Self` resolved to the concrete target
            // type, not just the body-checking pass. Consistent with
            // `check_item`'s `ItemKind::Impl` branch, which builds the
            // same `Ty::Named(target.name)` self-type — generic impl
            // targets aren't handled at all yet (Phase 4/5 doesn't
            // consult `impl_decl.generics` anywhere), so this is exactly
            // as simplified as the rest of the impl-checking pipeline
            // already is, not a new gap introduced here.
            let self_ty = resolve_type(&Type::Primitive(target_name.clone()));
            let self_subst: HashMap<String, Ty> = std::iter::once(("Self".to_string(), self_ty)).collect();
            let resolve_with_self = |ty: &Type| -> Ty {
                substitute_type_params(&mark_type_params(resolve_type(ty), &["Self".to_string()]), &self_subst)
            };

            let methods: HashMap<String, FnSig> = impl_decl.items.iter().filter_map(|ii| match ii {
                ImplItem::Fn(f) => {
                    let params = f.params.iter()
                        .filter(|p| p.name != "self")
                        .map(|p| arg_ty(p, resolve_with_self(&p.ty)))
                        .collect();
                    let ret = f.return_type.as_ref().map(|t| resolve_with_self(t)).unwrap_or(Ty::Unit);
                    Some((f.name.clone(), FnSig { params, ret, has_default_body: true }))
                }
                ImplItem::AssocType(..) => None,
            }).collect();

            if let Some(tn) = &trait_name {
                // Document 7 §5's orphan rule, grounded via Document 15
                // §3.2's `use`/`import` distinction: Phase 4 doesn't have
                // the real module/package system yet (Document 15 is
                // Phase 14), so "defined in the current package" is
                // approximated here as "declared locally in this
                // Program" (a `trait`/`struct`/`enum` item actually
                // present) -- anything not locally declared is treated
                // as foreign, consistent with how `import` (cross-
                // package) vs `use` (same-package) already distinguish
                // these in the parsed AST. Flagged for sign-off, same as
                // every other spec-grounded extension in prior phases.
                let trait_is_local = self.traits.contains_key(tn);
                let type_is_local = self.structs.contains_key(&target_name) || self.enums.contains_key(&target_name);
                if !trait_is_local && !type_is_local {
                    self.errors.push(TypeError {
                        message: format!(
                            "orphan rule violation: `impl {} for {}` is not allowed -- neither the trait nor the type is defined in the current package (Document 7 §5)",
                            tn, target_name
                        ),
                        context: format!("impl {} for {}", tn, target_name),
                    });
                }

                // Required-method completeness -- only checkable when
                // the trait itself is locally declared (a foreign
                // trait's full method list isn't known to this checker).
                if let Some(shape) = self.traits.get(tn).cloned() {
                    for (mname, sig) in &shape.methods {
                        if !sig.has_default_body && !methods.contains_key(mname) {
                            self.errors.push(TypeError {
                                message: format!(
                                    "`impl {} for {}` is missing required method `{}` (Document 7 §4.2/§4.3)",
                                    tn, target_name, mname
                                ),
                                context: format!("impl {} for {}", tn, target_name),
                            });
                        }
                    }
                }
            }

            let trait_args: Vec<Ty> = impl_decl.trait_ref.as_ref()
                .map(|tr| tr.args.iter().map(resolve_type).collect::<Vec<Ty>>())
                .unwrap_or_default();
            self.impls_by_type.entry(target_name).or_default().push(ImplRecord { trait_name, trait_args, methods });
        } else if let ItemKind::Mod(m) = &item.kind {
            for inner in &m.items {
                self.register_impls(inner);
            }
        }
    }

    fn check_item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Fn(f) => self.check_fn(f, None),
            ItemKind::Impl(impl_decl) => {
                let self_ty = resolve_type(&Type::Primitive(impl_decl.target.name.clone()));
                for ii in &impl_decl.items {
                    if let ImplItem::Fn(f) = ii {
                        self.check_fn(f, Some(&self_ty));
                    }
                }
            }
            ItemKind::Mod(m) => {
                for inner in &m.items {
                    self.check_item(inner);
                }
            }
            ItemKind::Const(c) => self.check_global_init(&c.name, &c.ty, &c.value),
            ItemKind::Static(c) => self.check_global_init(&c.name, &c.ty, &c.value),
            _ => {}
        }
    }

    /// Phase 11a: a `const`/`static` initializer must have the declared type.
    fn check_global_init(&mut self, name: &str, ty: &Type, value: &Expr) {
        let declared = resolve_type(ty);
        let mut env = Env::new();
        // `check_expr` reports a mismatch against `declared` itself.
        self.check_expr(value, Some(&declared), &mut env, name);
    }

    fn check_fn(&mut self, f: &FnDecl, self_ty: Option<&Ty>) {
        let Some(body) = &f.body else { return };
        let mut env = Env::new();
        // When checking a method body (`self_ty` is `Some`), any
        // parameter/return type written as `Self` (Document 7 §4.1's
        // own canonical trait-method example, `other: borrow Self`)
        // must resolve to the enclosing `impl`'s concrete target type,
        // not stay as an unresolved `Ty::Named("Self")` placeholder —
        // the three Phase 5 test failures this fixes specifically
        // needed `compareTo`'s `Self` parameter to resolve to `i32`
        // inside `impl Comparable for i32`. Reuses the existing
        // mark-then-substitute pipeline already built for ordinary
        // generic type parameters (`mark_type_params` +
        // `substitute_type_params`) rather than writing a separate
        // substitution mechanism — "Self" is treated as if it were a
        // one-element generic parameter list scoped to this single
        // function-checking call, which is structurally exactly what
        // it is.
        let self_subst: HashMap<String, Ty> = match self_ty {
            Some(t) => std::iter::once(("Self".to_string(), t.clone())).collect(),
            None => HashMap::new(),
        };
        let resolve_with_self = |ty: &Type| -> Ty {
            let raw = resolve_type(ty);
            if self_ty.is_some() {
                substitute_type_params(&mark_type_params(raw, &["Self".to_string()]), &self_subst)
            } else {
                raw
            }
        };
        let saved_self_type = self.self_type.take();
        self.self_type = self_ty.cloned();
        for p in &f.params {
            if p.name == "self" {
                if let Some(t) = self_ty {
                    env.insert("self".to_string(), t.clone());
                }
                continue;
            }
            let pty = resolve_with_self(&p.ty);
            if let Some(d) = &p.default {
                // Document 10 §2.1: the default value must have the parameter's type.
                let mut denv = Env::new();
                let dt = self.check_expr(d, Some(&pty), &mut denv, &f.name);
                if dt != pty && dt != Ty::Never {
                    self.errors.push(TypeError {
                        message: format!("default value of parameter `{}` has type `{}`, expected `{}`", p.name, dt, pty),
                        context: f.name.clone(),
                    });
                }
            }
            if p.is_variadic {
                // Document 10 §2.3: inside the function a variadic parameter
                // is the sequence of the extra arguments; Phase 11a passes it
                // as a (stack-backed) slice view `&[T]`.
                env.insert(p.name.clone(), Ty::Ref(false, Box::new(Ty::Array(Box::new(pty)))));
            } else {
                env.insert(p.name.clone(), pty);
            }
        }
        let expected_ret = f.return_type.as_ref().map(|t| resolve_with_self(t));
        // Phase 9: `?`/`return` inside this body propagate to this
        // function's declared return type (Document 11 §2/§4).
        self.prop_targets.push(PropTarget::FnRet(expected_ret.clone().unwrap_or(Ty::Unit)));
        self.check_block(body, &mut env, expected_ret.as_ref(), &f.name);
        self.prop_targets.pop();
        self.self_type = saved_self_type;
    }

    fn check_block(&mut self, block: &Block, env: &mut Env, expected_tail: Option<&Ty>, ctx: &str) -> Option<Ty> {
        env.push();
        for stmt in &block.stmts {
            self.check_stmt(stmt, env, ctx);
        }
        let result = if let Some(tail) = &block.tail {
            Some(self.check_expr(tail, expected_tail, env, ctx))
        } else if block_diverges(block) {
            // Phase 9: a block that can't fall through has type `!`
            // (Document 5 §2.5), not `()`.
            Some(Ty::Never)
        } else {
            None
        };
        env.pop();
        result
    }

    fn check_stmt(&mut self, stmt: &Stmt, env: &mut Env, ctx: &str) {
        match stmt {
            Stmt::Let { pattern, ty, value, .. } => {
                let expected = ty.as_ref().map(|t| self.resolve_type_full(t, ctx));
                let inferred = match value {
                    Some(v) => Some(self.check_expr(v, expected.as_ref(), env, ctx)),
                    None => None,
                };
                // Rule 1: explicit annotation wins -- if both an
                // annotation and a value are present, the binding's
                // type is the annotation (already checked-against
                // above); if only a value is present, use its inferred
                // type; if neither, this is an error (nothing to infer
                // from) -- Document 5 doesn't show a bare `let x;` with
                // no type and no value anywhere, so this is treated as
                // an ambiguity error per rule 6's spirit rather than
                // inventing a default.
                let final_ty = match (expected, inferred) {
                    (Some(t), _) => t,
                    (None, Some(t)) => t,
                    (None, None) => {
                        self.errors.push(TypeError {
                            message: "cannot infer type: no annotation and no initializer".into(),
                            context: ctx.into(),
                        });
                        Ty::Unit
                    }
                };
                if let Pattern::Ident(name) = pattern {
                    env.insert(name.clone(), final_ty);
                } else if let Pattern::Mut(name) = pattern {
                    env.insert(name.clone(), final_ty);
                } else {
                    // Phase 10: destructuring `let (a, b) = pair;`
                    // (Document 5 §3.2, Document 10 §3.3) -- reuse the
                    // match-arm binder so tuple/tuple-struct patterns
                    // distribute their field types.
                    self.bind_pattern(pattern, &final_ty, env);
                }
                // Other pattern shapes (tuple/array/tuple-struct
                // destructuring) would need per-field type distribution;
                // Phase 3 scope is the core type system, not full
                // pattern-type distribution -- flagged as a known gap
                // rather than silently mishandled (bindings introduced
                // by a destructuring `let` simply aren't added to `env`
                // yet, so using them later would report "undefined
                // variable" rather than a wrong type -- a safe failure
                // mode, not a silent wrong answer).
            }
            Stmt::Expr(e) => {
                self.check_expr(e, None, env, ctx);
            }
            Stmt::Return(inner) => {
                // Phase 9: checked against the enclosing function's
                // declared return type (Phase 8 only walked the value).
                self.check_return(inner.as_ref(), env, ctx);
            }
            Stmt::Yield(e) => {
                self.check_expr(e, None, env, ctx);
            }
            Stmt::Break { label, value } => {
                let target = self.find_loop_frame(label, ctx);
                let bty = value.as_ref().map(|e| self.check_expr(e, None, env, ctx));
                if let (Some(idx), Some(ty)) = (target, bty) {
                    if self.loop_frames[idx].collects_breaks {
                        self.loop_frames[idx].break_types.push(ty);
                    }
                }
            }
            Stmt::Continue { label } => {
                self.find_loop_frame(label, ctx);
            }
            Stmt::TargetBlock(_, block) => {
                // Document 2 §8's `#target(native|wasm|all) { .. }`
                // directive blocks weren't walked into at all before
                // (an orthogonal, incidental gap noticed and fixed
                // here alongside Phase 8's control-flow work, since
                // it's the same match arm and a one-line fix -- not
                // itself Phase 8 scope, called out separately in
                // PROGRESS.md rather than silently folded in).
                self.check_block(block, env, None, ctx);
            }
            Stmt::Item(_) => {
                // Nested item declarations (a `fn`/`struct`/etc.
                // declared inside a block) still aren't registered or
                // checked -- would need a real nested-registration
                // pass, out of scope here, unchanged from before.
            }
        }
    }

    /// Document 9 §3.5: resolves a `break`/`continue`'s optional label
    /// to the index of its target frame in `loop_frames` (searching the
    /// whole stack, not just the innermost entry, so a label naming an
    /// OUTER loop from inside a nested one still resolves correctly);
    /// `None` label targets the innermost frame unconditionally.
    /// Reports an error (label doesn't name any enclosing loop; or bare
    /// `break`/`continue` with no enclosing loop at all) and returns
    /// `None` when nothing valid is found.
    fn find_loop_frame(&mut self, label: &Option<String>, ctx: &str) -> Option<usize> {
        match label {
            Some(name) => {
                let found = self.loop_frames.iter().rposition(|f| f.label.as_deref() == Some(name.as_str()));
                if found.is_none() {
                    self.errors.push(TypeError {
                        message: format!(
                            "label `'{}'` does not name an enclosing loop (Document 9 §3.5)",
                            name
                        ),
                        context: ctx.into(),
                    });
                }
                found
            }
            None => {
                if self.loop_frames.is_empty() {
                    self.errors.push(TypeError {
                        message: "`break`/`continue` outside of any loop".into(),
                        context: ctx.into(),
                    });
                    None
                } else {
                    Some(self.loop_frames.len() - 1)
                }
            }
        }
    }

    /// Bidirectional check: if `expected` is `Some`, verify the
    /// expression's type is compatible with it (Document 5 rule 1/3);
    /// if `None`, infer a type from the expression alone (defaulting
    /// per rule 2 where relevant). Always returns *some* `Ty` so
    /// callers can keep walking, even after recording an error — errors
    /// are accumulated in `self.errors`, not used to abort the whole
    /// pass, consistent with the error-recovery philosophy carried
    /// through every phase so far.
    /// Bidirectional check, entry point: computes the expression's
    /// actual type via `check_expr_inner`, then performs ONE final,
    /// uniform compatibility check against `expected` here — applying
    /// to every expression kind, not just the ones whose inner logic
    /// happened to check it themselves. This was added after tracing a
    /// real gap: `check_expr_inner`'s `Array`/`Tuple` branches only used
    /// `expected` as a soft hint to guide *element* inference, never
    /// verifying the *whole* result actually matched afterward -- so
    /// `let x: [i32] = [1.5];` would have silently produced
    /// `Array(F64)` (the float literal defaulting per rule 2 since
    /// `i32` isn't a float type) with no error at all, even though `x`
    /// is declared `[i32]`. Rather than add a `check_compatible` call
    /// to every individual branch that needed one, this wraps all of
    /// them once, at the single point every branch already funnels
    /// through.
    fn check_expr(&mut self, expr: &Expr, expected: Option<&Ty>, env: &mut Env, ctx: &str) -> Ty {
        let actual = self.check_expr_inner(expr, expected, env, ctx);
        self.expr_types.insert(expr as *const Expr as usize, actual.clone());
        // Only numeric literals and `null` are excluded from this
        // blanket check -- `check_literal`'s `Int`/`IntHex`/`IntOct`/
        // `IntBin`/`Float` arms already adopt-or-default against
        // `expected` per rules 2/3 (so `actual` already equals
        // `expected` whenever that's even possible), and `Null` reports
        // its own specific, clearer error when incompatible. Running the
        // generic check again for those would double-report the same
        // mismatch a second time with a less specific message.
        //
        // `Str`/`RawStr`/`Char`/`Bool` were WRONGLY included in this
        // exclusion in an earlier version of this fix (blanket-excluding
        // all of `Expr::Literal(_)`) -- `check_literal`'s arms for those
        // never compare against `expected` at all, they just return a
        // fixed type unconditionally. That meant `let x: bool = "hi";`
        // or a `dyn`-dispatched call passing `true` where `f64` was
        // expected would have silently type-checked. Caught by hand-
        // tracing this phase's own dyn-dispatch arg-type test before
        // trusting it, not by a later CI failure.
        let self_checking_literal = matches!(expr, Expr::Literal(
            Literal::Int(_) | Literal::IntHex(_) | Literal::IntOct(_)
            | Literal::IntBin(_) | Literal::Float(_) | Literal::Null
        ));
        if !self_checking_literal {
            self.check_compatible(&actual, expected, ctx);
        } else {
            // Phase 9 fix of a latent Phase 3 hole: the exemption above
            // assumes `check_literal` already adopted `expected`, which
            // is only true when `expected` is numeric (int literals
            // adopt any numeric type, float literals any float type).
            // For a NON-numeric expected type (`let s: String = 5;`,
            // `Err(5)` against `Result<_, IoError>`), `check_literal`
            // just returns `i32`/`f64` and nothing ever reported the
            // mismatch. `Ty::TypeParam` stays exempt: an unannotated
            // generic struct literal legitimately passes its field's
            // placeholder type as `expected` (see `check_struct_lit`).
            let literal_mismatch = match (expr, expected) {
                (Expr::Literal(Literal::Int(_) | Literal::IntHex(_) | Literal::IntOct(_) | Literal::IntBin(_)), Some(t)) => {
                    !matches!(t, Ty::TypeParam(_)) && !ty_is_numeric(t)
                }
                (Expr::Literal(Literal::Float(_)), Some(t)) => {
                    !matches!(t, Ty::TypeParam(_)) && !ty_is_float(t)
                }
                _ => false,
            };
            if literal_mismatch {
                self.check_compatible(&actual, expected, ctx);
            }
        }
        actual
    }

    fn check_expr_inner(&mut self, expr: &Expr, expected: Option<&Ty>, env: &mut Env, ctx: &str) -> Ty {
        match expr {
            Expr::Literal(lit) => self.check_literal(lit, expected, ctx),

            Expr::Ident(name) => {
                // Phase 9: `None` (Document 11 §1.2) is a keyword, so it
                // can never be a local variable -- it is always the
                // built-in `Option` variant.
                if name.as_str() == "None" {
                    return self.check_variant_ctor("None", &[], expected, env, ctx);
                }
                if let Some(t) = env.get(name) {
                    t.clone()
                } else if let Some(t) = self.globals.get(name) {
                    t.clone()
                } else {
                    self.errors.push(TypeError {
                        message: format!("undefined variable `{}`", name),
                        context: ctx.into(),
                    });
                    // Already reported; don't let a downstream `?` pile a
                    // second, derived error on top of this one.
                    self.saw_unresolved = true;
                    expected.cloned().unwrap_or(Ty::Unit)
                }
            }

            Expr::Path(path) => {
                // Two-segment path (`EnumName::Variant`) against a known
                // enum, with no call parens -- must be a unit variant
                // (Document 7 §3.1). This is what actually makes the
                // enum registry (`self.enums`) a real consumer of
                // Document 7's data-shape information, not just a
                // write-only registry -- see the module-level note on
                // why this was added rather than left unused.
                if let [enum_name, variant_name] = path.as_slice() {
                    if let Some(shape) = self.enums.get(enum_name) {
                        match shape.variants.iter().find(|(n, _)| n == variant_name) {
                            Some((_, data)) if data.is_empty() => return Ty::Named(enum_name.clone()),
                            Some((_, _)) => {
                                self.errors.push(TypeError {
                                    message: format!(
                                        "enum variant `{}::{}` carries data and must be called with arguments",
                                        enum_name, variant_name
                                    ),
                                    context: ctx.into(),
                                });
                                return Ty::Named(enum_name.clone());
                            }
                            None => {
                                self.errors.push(TypeError {
                                    message: format!("enum `{}` has no variant `{}`", enum_name, variant_name),
                                    context: ctx.into(),
                                });
                                return Ty::Named(enum_name.clone());
                            }
                        }
                    }
                }
                self.saw_unresolved = true;
                expected.cloned().unwrap_or(Ty::Unit) // module/struct-assoc paths: Phase 4+ resolves these fully
            }

            Expr::Paren(inner) => self.check_expr(inner, expected, env, ctx),

            Expr::Array(items) => {
                let elem_expected = match expected {
                    Some(Ty::Array(inner)) | Some(Ty::Fixed(inner, _)) => Some((**inner).clone()),
                    _ => None,
                };
                let fixed_len = match expected {
                    Some(Ty::Fixed(_, n)) => Some(*n),
                    _ => None,
                };
                if let Some(n) = fixed_len {
                    if items.len() as u64 != n {
                        self.errors.push(TypeError {
                            message: format!("array literal has {} element(s) but the type requires {}", items.len(), n),
                            context: ctx.into(),
                        });
                    }
                }
                if items.is_empty() {
                    // Document 5 §4 rule 6, §7's own explicit example:
                    // `let empty = [];` with no further usage is a
                    // compile error requiring an explicit annotation,
                    // not a silent default to `[i32]`.
                    match elem_expected {
                        Some(t) => match fixed_len {
                            Some(n) => Ty::Fixed(Box::new(t), n),
                            None => Ty::Array(Box::new(t)),
                        },
                        None => {
                            self.errors.push(TypeError {
                                message: "cannot infer type of empty array literal `[]` -- add an explicit type annotation".into(),
                                context: ctx.into(),
                            });
                            Ty::Array(Box::new(Ty::Unit))
                        }
                    }
                } else {
                    let first = self.check_expr(&items[0], elem_expected.as_ref(), env, ctx);
                    for item in &items[1..] {
                        let t = self.check_expr(item, Some(&first), env, ctx);
                        if t != first {
                            self.errors.push(TypeError {
                                message: format!("array element type mismatch: expected `{}`, found `{}`", first, t),
                                context: ctx.into(),
                            });
                        }
                    }
                    match fixed_len {
                        Some(n) => Ty::Fixed(Box::new(first), n),
                        None => Ty::Array(Box::new(first)),
                    }
                }
            }

            Expr::Tuple(items) if items.is_empty() => {
                // `()` is the unit VALUE (Document 5 §2.5). It parses as
                // an empty tuple expression, but its type is `Ty::Unit`,
                // not `Ty::Tuple(vec![])` -- otherwise `Ok(())` could never
                // satisfy `Result<(), E>` (Document 11 §5's own example).
                Ty::Unit
            }

            Expr::Tuple(items) => {
                let expected_elems: Vec<Option<Ty>> = match expected {
                    Some(Ty::Tuple(ts)) if ts.len() == items.len() => ts.iter().cloned().map(Some).collect(),
                    _ => vec![None; items.len()],
                };
                let tys = items.iter().zip(expected_elems)
                    .map(|(it, exp)| self.check_expr(it, exp.as_ref(), env, ctx))
                    .collect();
                Ty::Tuple(tys)
            }

            Expr::StructLit { name, fields, spread } => self.check_struct_lit(name, fields, spread.is_some(), expected, env, ctx),

            Expr::If(if_expr) => self.check_if(if_expr, expected, env, ctx),

            Expr::Match(match_expr) => self.check_match(match_expr, expected, env, ctx),

            Expr::Block(b) => self.check_block(b, env, expected, ctx).unwrap_or(Ty::Unit),

            Expr::Unsafe(b) => self.check_block(b, env, expected, ctx).unwrap_or(Ty::Unit),

            Expr::Unary { op, expr: inner } => {
                let t = self.check_expr(inner, expected, env, ctx);
                match op {
                    UnaryOp::Not => {
                        if t != Ty::Bool {
                            self.errors.push(TypeError {
                                message: format!("`!` requires `bool`, found `{}`", t),
                                context: ctx.into(),
                            });
                        }
                        Ty::Bool
                    }
                    UnaryOp::Neg => {
                        if !ty_is_numeric(&t) {
                            self.errors.push(TypeError {
                                message: format!("unary `-` requires a numeric type, found `{}`", t),
                                context: ctx.into(),
                            });
                        }
                        t
                    }
                    UnaryOp::BitNot => t,
                }
            }

            Expr::Binary { op, lhs, rhs } => self.check_binary_expected(*op, lhs, rhs, expected, env, ctx),

            Expr::Assign { lhs, rhs, .. } => {
                let lt = self.check_expr(lhs, None, env, ctx);
                self.check_expr(rhs, Some(&lt), env, ctx);
                Ty::Unit
            }

            Expr::Cast { expr: inner, ty } => {
                // Document 4 §6 / Document 5 §6: `as` is the explicit
                // escape hatch from the no-implicit-coercion rule --
                // Phase 3 doesn't validate which primitive-to-primitive
                // casts are semantically legal (that's a Document 4 §6
                // detail beyond this phase's core-type-system scope),
                // only that `as` itself always type-checks to its
                // target type regardless of the source expression's type.
                self.check_expr(inner, None, env, ctx);
                self.resolve_type_full(ty, ctx)
            }

            Expr::Range { lo, hi, .. } => {
                // `1..n` with a bare literal start takes its type from the other
                // bound (`n: usize` makes the whole range `usize`), Document 5 rule 3.
                let lo_is_literal = matches!(lo.as_ref(), Expr::Literal(Literal::Int(_)));
                if lo_is_literal && expected.is_none() {
                    let ht = self.check_expr(hi, None, env, ctx);
                    self.check_expr(lo, Some(&ht), env, ctx);
                    return ht;
                }
                let lt = self.check_expr(lo, expected, env, ctx);
                self.check_expr(hi, Some(&lt), env, ctx);
                lt
            }

            Expr::Propagate(inner) => self.check_propagate(inner, expected, env, ctx),

            Expr::Field { expr: inner, name } => {
                let t = self.check_expr(inner, None, env, ctx);
                self.field_type(&t, name, ctx)
            }

            Expr::Index { expr: inner, index } => {
                let t = strip_refs(self.check_expr(inner, None, env, ctx));
                let mut idx_inner: &Expr = index;
                while let Expr::Paren(i) = idx_inner {
                    idx_inner = i;
                }
                let is_range = matches!(idx_inner, Expr::Range { .. });
                if is_range {
                    self.check_expr(index, None, env, ctx);
                } else {
                    // Any integer type may index (Document 5 gives no index type; codegen converts).
                    self.check_expr(index, None, env, ctx);
                }
                match t {
                    // `a[lo..hi]` is the unsized slice place `[T]` (Document 5
                    // §3.1's `&list[1..3]`); `a[i]` is one element.
                    Ty::Array(elem) | Ty::Fixed(elem, _) if is_range => Ty::Array(elem),
                    Ty::Array(elem) | Ty::Fixed(elem, _) => *elem,
                    other => {
                        self.errors.push(TypeError {
                            message: format!("cannot index into type `{}`", other),
                            context: ctx.into(),
                        });
                        Ty::Unit
                    }
                }
            }

            Expr::Call { callee, args } => {
                // Phase 9 (Document 11): the built-in variant constructors
                // and the error-handling intrinsics. `Ok`/`Err`/`Some` are
                // keywords and always mean the built-in variants;
                // `panic`/`assert`/`ensure` yield to a user-declared
                // function of the same name (they are ordinary prelude
                // names, not reserved by the grammar).
                if let Expr::Ident(name) = callee.as_ref() {
                    match name.as_str() {
                        "Ok" | "Err" | "Some" => {
                            return self.check_variant_ctor(name, args, expected, env, ctx);
                        }
                        "panic" | "assert" | "ensure" if !self.functions.contains_key(name.as_str()) => {
                            return self.check_error_intrinsic(name, args, expected, env, ctx);
                        }
                        _ => {}
                    }
                }
                if let Expr::Ident(name) = callee.as_ref() {
                    if let Some(shape) = self.functions.get(name).cloned() {
                        if shape.generics.is_empty() {
                            for (i, arg) in args.iter().enumerate() {
                                let exp = shape.params.get(i);
                                self.check_expr(&arg.value, exp, env, ctx);
                            }
                            return shape.ret;
                        }
                        // Generic function call (Document 8 §2): infer
                        // each type parameter from the actual argument
                        // types via structural unification against the
                        // (TypeParam-marked) declared parameter types,
                        // then verify every `where`-clause bound
                        // (§3's `T: A + B`) against the *concrete*
                        // inferred type using Phase 4's impl registry.
                        // Substituting a concrete `Ty` here (never a
                        // `Ty::DynTrait`) is what makes this resolve
                        // through static/monomorphized dispatch, not
                        // the `dyn` vtable machinery Phase 4 built for
                        // the unrelated `dyn Trait` case — this phase's
                        // "zero dyn dispatch for generic-only code"
                        // exit criterion holds by construction, not by
                        // a separate runtime check (there is no runtime
                        // yet); see PROGRESS.md's verification section
                        // for the concrete test that confirms it.
                        let arg_tys: Vec<Ty> = args.iter()
                            .map(|a| self.check_expr(&a.value, None, env, ctx))
                            .collect();
                        let mut subst: HashMap<String, Ty> = HashMap::new();
                        for (p, a) in shape.params.iter().zip(arg_tys.iter()) {
                            unify_infer(p, a, &mut subst);
                        }
                        for (param_name, trait_names) in &shape.bounds {
                            if let Some(concrete) = subst.get(param_name) {
                                for tn in trait_names {
                                    if !self.satisfies_bound(concrete, tn) {
                                        self.errors.push(TypeError {
                                            message: format!(
                                                "type `{}` does not satisfy bound `{}` required for generic parameter `{}` of `{}` (Document 8 §2/§3)",
                                                concrete, tn, param_name, name
                                            ),
                                            context: ctx.into(),
                                        });
                                    }
                                }
                            }
                        }
                        return substitute_type_params(&shape.ret, &subst);
                    }
                }
                if let Expr::Path(path) = callee.as_ref() {
                    // Phase 11a: associated function `Type::name(args)` /
                    // `Self::name(args)` (Document 7 §2.3's `User::newUser`).
                    if let [tn, fname] = path.as_slice() {
                        let type_name = if tn == "Self" { self.self_type.as_ref().map(|t| t.to_string()) } else { Some(tn.clone()) };
                        let is_variant = self.enums.get(tn).map(|e| e.variants.iter().any(|(n, _)| n == fname)).unwrap_or(false);
                        if let (Some(type_name), false) = (type_name, is_variant) {
                            // Several impls of one generic trait (`From<A>`, `From<B>` for the same
                            // type, Document 11 §2): choose the one whose first parameter fits.
                            let cands: Vec<FnSig> = self
                                .impls_by_type
                                .get(&type_name)
                                .map(|v| v.iter().filter_map(|r| r.methods.get(fname.as_str()).cloned()).collect())
                                .unwrap_or_default();
                            if cands.len() > 1 && !args.is_empty() {
                                let at = self.check_expr(&args[0].value, None, env, ctx);
                                if let Some(sig) = cands.iter().find(|c| c.params.first() == Some(&at)).cloned() {
                                    for (i, arg) in args.iter().enumerate().skip(1) {
                                        self.check_expr(&arg.value, sig.params.get(i), env, ctx);
                                    }
                                    return sig.ret;
                                }
                            }
                            let sig = self.resolve_method(&Ty::Named(type_name), fname);
                            if let Some(sig) = sig {
                                for (i, arg) in args.iter().enumerate() {
                                    self.check_expr(&arg.value, sig.params.get(i), env, ctx);
                                }
                                return sig.ret;
                            }
                        }
                    }
                    if let [enum_name, variant_name] = path.as_slice() {
                        if let Some(shape) = self.enums.get(enum_name).cloned() {
                            match shape.variants.iter().find(|(n, _)| n == variant_name) {
                                Some((_, data_tys)) => {
                                    for (i, arg) in args.iter().enumerate() {
                                        let exp = data_tys.get(i);
                                        self.check_expr(&arg.value, exp, env, ctx);
                                    }
                                    if args.len() != data_tys.len() {
                                        self.errors.push(TypeError {
                                            message: format!(
                                                "`{}::{}` expects {} argument(s), found {}",
                                                enum_name, variant_name, data_tys.len(), args.len()
                                            ),
                                            context: ctx.into(),
                                        });
                                    }
                                    return Ty::Named(enum_name.clone());
                                }
                                None => {
                                    self.errors.push(TypeError {
                                        message: format!("enum `{}` has no variant `{}`", enum_name, variant_name),
                                        context: ctx.into(),
                                    });
                                    return Ty::Named(enum_name.clone());
                                }
                            }
                        }
                    }
                }
                for arg in args {
                    self.check_expr(&arg.value, None, env, ctx);
                }
                self.saw_unresolved = true;
                expected.cloned().unwrap_or(Ty::Unit)
            }

            Expr::MethodCall { receiver, name, args } => {
                let recv_ty = strip_refs(self.check_expr(receiver, None, env, ctx));
                let sig = self.resolve_method(&recv_ty, name);
                match sig {
                    Some(sig) => {
                        for (i, arg) in args.iter().enumerate() {
                            self.check_expr(&arg.value, sig.params.get(i), env, ctx);
                        }
                        if args.len() != sig.params.len() {
                            self.errors.push(TypeError {
                                message: format!(
                                    "method `{}` on `{}` expects {} argument(s), found {}",
                                    name, recv_ty, sig.params.len(), args.len()
                                ),
                                context: ctx.into(),
                            });
                        }
                        sig.ret
                    }
                    None => {
                        for arg in args {
                            self.check_expr(&arg.value, None, env, ctx);
                        }
                        // Only report "no such method" when the receiver
                        // is a type this checker actually has full
                        // knowledge of (a locally-declared struct/enum,
                        // or a dyn-Trait whose trait is locally
                        // declared) -- for anything else (foreign types,
                        // primitives, stdlib collections with no
                        // registered methods yet) Phase 4 doesn't have
                        // enough information to say the method doesn't
                        // exist, so it stays silent rather than
                        // fabricating a false error, consistent with
                        // Phase 3's existing field-access behavior for
                        // unknown base types.
                        let known_base = match &recv_ty {
                            Ty::Named(n) => self.structs.contains_key(n) || self.enums.contains_key(n),
                            Ty::Generic(n, _) => self.structs.contains_key(n) || self.enums.contains_key(n),
                            Ty::DynTrait(n) => self.traits.contains_key(n),
                            _ => false,
                        };
                        if known_base {
                            self.errors.push(TypeError {
                                message: format!("no method named `{}` found for type `{}`", name, recv_ty),
                                context: ctx.into(),
                            });
                        }
                        self.saw_unresolved = true;
                        expected.cloned().unwrap_or(Ty::Unit)
                    }
                }
            }

            Expr::Borrow { expr: inner, mutable } => {
                let inner_expected = match expected {
                    // a slice target `&[T]` is reached by unsizing a fixed array, so the
                    // operand itself is NOT expected to be an unsized `[T]`
                    Some(Ty::Ref(_, t)) if !matches!(**t, Ty::Array(_)) => Some((**t).clone()),
                    _ => None,
                };
                let t = self.check_expr(inner, inner_expected.as_ref(), env, ctx);
                // Phase 11a: borrowing a fixed array where a slice `&[T]` is
                // expected is the (only) unsizing coercion, the array-to-slice view.
                if let (Some(Ty::Ref(_, want)), Ty::Fixed(have, _)) = (expected, &t) {
                    if let Ty::Array(w) = want.as_ref() {
                        if w == have {
                            return Ty::Ref(*mutable, want.clone());
                        }
                    }
                }
                Ty::Ref(*mutable, Box::new(t))
            }

            Expr::Await(inner) => {
                self.check_expr(inner, None, env, ctx);
                // `await`'s result type isn't modeled until Phase 12.
                self.saw_unresolved = true;
                expected.cloned().unwrap_or(Ty::Unit)
            }

            Expr::Throw(inner) => {
                // Document 11 §3.1: `throw <value>` inside a `try` block
                // is `return Err(<value>)` of the implicit wrapper -- so
                // the value's type is an error source for the block,
                // exactly like the error type of a `?`.
                let t = self.check_expr(inner, None, env, ctx);
                if !self.record_try_source(t) && !self.top_is_opaque() {
                    self.errors.push(TypeError {
                        message: "`throw` is only valid inside a `try` block (Document 11 §3.1); outside one, return `Err(..)` instead".into(),
                        context: ctx.into(),
                    });
                }
                Ty::Never
            }

            Expr::Return(inner) => {
                self.check_return(inner.as_deref(), env, ctx);
                Ty::Never
            }

            Expr::Yield(inner) => {
                self.check_expr(inner, None, env, ctx);
                Ty::Unit
            }

            // Constructs whose full semantic checking depends on later
            // phases (closures need capture/borrow analysis, Phase 6;
            // spawn/select/query/loops need their own domain checkers,
            // Phase 12/18/20/8) -- visited for sub-expression coverage
            // where cheap, but not fully type-checked here. Not silently
            // claimed as "checked".
            Expr::Closure(c) => {
                let mut inner_env = Env::new();
                inner_env.scopes = env.scopes.clone();
                inner_env.push();
                for (pat, ty) in &c.params {
                    if let Pattern::Ident(n) = pat {
                        inner_env.insert(n.clone(), ty.as_ref().map(resolve_type).unwrap_or(Ty::Unit));
                    }
                }
                // Phase 9: `?`/`return`/`throw` inside a closure body do
                // not target the enclosing function.
                self.prop_targets.push(PropTarget::Opaque);
                match &c.body {
                    ClosureBody::Expr(e) => { self.check_expr(e, None, &mut inner_env, ctx); }
                    ClosureBody::Block(b) => { self.check_block(b, &mut inner_env, None, ctx); }
                }
                self.prop_targets.pop();
                expected.cloned().unwrap_or(Ty::Unit)
            }
            Expr::Loop(loop_expr) => self.check_loop(loop_expr, expected, env, ctx),
            Expr::TryCatch { try_block, catch_var, catch_block } => {
                self.check_try_catch(expr as *const Expr as usize, try_block, catch_var, catch_block, expected, env, ctx)
            }
            Expr::Spawn { .. } | Expr::Select(_) | Expr::Query(_)
            | Expr::Styled { .. } | Expr::Layout { .. }
            | Expr::ComponentChildren { .. } | Expr::EventHandler { .. } => {
                expected.cloned().unwrap_or(Ty::Unit)
            }
        }
    }

    /// Phase 8 (Document 9 §3): full loop-form type-checking. Prior to
    /// this phase `Expr::Loop` was entirely unchecked -- a loop body's
    /// contents (and everything nested inside it) were silently never
    /// visited by the type checker at all, regardless of loop form.
    fn check_loop(&mut self, loop_expr: &LoopExpr, expected: Option<&Ty>, env: &mut Env, ctx: &str) -> Ty {
        match loop_expr {
            LoopExpr::Loop { label, body } => {
                // Document 9 §3.1: `loop` is the one form that can
                // produce a value, via `break <value>;` — its overall
                // type is the common type of every `break <value>;`
                // that targets THIS loop specifically (Rule 5's "no
                // cross-branch guessing" applies here exactly as it
                // does to `match` arms: incompatible break-value types
                // are a compile error, not unioned). Collected via
                // `loop_frames` as `check_block` runs its one, normal
                // pass over `body` below — not a separate pre-pass, so
                // a type error inside a `break <value>;` expression is
                // reported exactly once, not twice.
                self.loop_frames.push(LoopFrame { label: label.clone(), collects_breaks: true, break_types: Vec::new() });
                env.push();
                self.check_block(body, env, None, ctx);
                env.pop();
                let frame = self.loop_frames.pop().unwrap();
                let mut common = expected.cloned();
                for bty in frame.break_types {
                    match &common {
                        Some(c) if *c != bty => {
                            self.errors.push(TypeError {
                                message: format!(
                                    "`loop`'s `break` values have incompatible types: `{}` and `{}` (Document 5 rule 5, applied to Document 9 §3.1's loop-as-expression form)",
                                    c, bty
                                ),
                                context: ctx.into(),
                            });
                        }
                        None => common = Some(bty),
                        _ => {}
                    }
                }
                common.unwrap_or(Ty::Unit)
            }
            LoopExpr::While { label, cond, body } => {
                self.check_expr(cond, Some(&Ty::Bool), env, ctx);
                self.loop_frames.push(LoopFrame { label: label.clone(), collects_breaks: false, break_types: Vec::new() });
                env.push();
                self.check_block(body, env, None, ctx);
                env.pop();
                self.loop_frames.pop();
                Ty::Unit
            }
            LoopExpr::For { label, pattern, iter, body } => {
                let iter_ty = self.check_expr(iter, None, env, ctx);
                self.loop_frames.push(LoopFrame { label: label.clone(), collects_breaks: false, break_types: Vec::new() });
                env.push();
                // Element-type inference is limited to what's directly
                // determinable without a real `Iterable` trait (that's
                // Document 16/Phase 16's `collections::iterator`, not
                // built yet): `[T]`/`[T; N]` unwrap to `T`; anything
                // else (including a `Range`, whose own `check_expr`
                // already returns the bare element type directly, e.g.
                // `0..10` checks as `i32` already, not a wrapped
                // "Range<i32>") is used as-is. This covers Document 9
                // §3.3's own two examples (`for i in 0..10`, and
                // element access via `collection.enumerate()`-style
                // iteration) without inventing a full trait-resolution
                // pass; anything genuinely needing `Iterable` dispatch
                // to know its element type is a documented gap, not a
                // silent wrong answer -- see PROGRESS.md.
                let elem_ty = match &iter_ty {
                    Ty::Array(elem) => (**elem).clone(),
                    other => other.clone(),
                };
                if let Pattern::Ident(name) | Pattern::Mut(name) = pattern {
                    env.insert(name.clone(), elem_ty);
                }
                self.check_block(body, env, None, ctx);
                env.pop();
                self.loop_frames.pop();
                Ty::Unit
            }
            LoopExpr::DoWhile { body, cond } => {
                // No label slot on `DoWhile` at all (see the parser's
                // own note where labels are wired up) -- still pushes
                // a frame (label `None`) so an unlabeled `break`/
                // `continue` inside it is recognized as being inside
                // *some* loop.
                self.loop_frames.push(LoopFrame { label: None, collects_breaks: false, break_types: Vec::new() });
                env.push();
                self.check_block(body, env, None, ctx);
                env.pop();
                self.check_expr(cond, Some(&Ty::Bool), env, ctx);
                self.loop_frames.pop();
                Ty::Unit
            }
        }
    }

    fn check_literal(&mut self, lit: &Literal, expected: Option<&Ty>, ctx: &str) -> Ty {
        match lit {
            Literal::Int(_) | Literal::IntHex(_) | Literal::IntOct(_) | Literal::IntBin(_) => {
                match expected {
                    // Rule 3: contextual/bidirectional inference -- an
                    // integer literal adopts a required numeric type.
                    Some(t) if ty_is_numeric(t) => t.clone(),
                    // Rule 2: literal defaulting -- untyped integer
                    // literal defaults to i32.
                    _ => Ty::I32,
                }
            }
            Literal::Float(text) => {
                // Document 2 §6.2: an explicit `f32`/`f64` suffix fixes the
                // literal's precision (Phase 10: it was previously ignored).
                if text.ends_with("f32") {
                    Ty::F32
                } else if text.ends_with("f64") {
                    Ty::F64
                } else {
                    match expected {
                        Some(t) if ty_is_float(t) => t.clone(),
                        _ => Ty::F64,
                    }
                }
            }
            Literal::Str(_) | Literal::RawStr(_) => Ty::StringTy,
            Literal::Char(_) => Ty::Char,
            Literal::Bool(_) => Ty::Bool,
            Literal::Null => {
                // Document 5 §2.6: `null` is only valid in unsafe/FFI
                // contexts -- Phase 3 doesn't yet track "is this
                // specific literal lexically inside an unsafe block"
                // as a separate flag threaded through `check_expr`
                // (that plumbing is straightforward but not added this
                // phase); instead, the concrete rule actually tested by
                // Document 5 §7 -- "`null` is not usable where an
                // `Option<T>` or plain `T` is expected in safe code" --
                // is enforced directly here: using `null` against any
                // expected type other than `Ty::Null` itself is an
                // error unconditionally, which correctly rejects every
                // safe-code use shown anywhere in Documents 1-24. This
                // is stricter than the full "safe code" carve-out (it
                // would also flag a hypothetical `unsafe { let x: i32 =
                // null; }`, which the language spec does intend to
                // permit) -- flagged as a known simplification, not
                // silently treated as complete unsafe-context support.
                if let Some(t) = expected {
                    if *t != Ty::Null {
                        self.errors.push(TypeError {
                            message: format!("`null` is not usable where `{}` is expected -- use `Option<T>` in safe code (Document 5 §2.6)", t),
                            context: ctx.into(),
                        });
                    }
                }
                Ty::Null
            }
        }
    }

    /// `expected` (Phase 11b-1, Document 5 rule 3): for arithmetic / bitwise operators
    /// the surrounding type flows into the left operand, so `let x: u8 = 200 + 55;`
    /// types the literals as `u8` instead of the default `i32`.
    fn check_binary_expected(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr, expected: Option<&Ty>, env: &mut Env, ctx: &str) -> Ty {
        let arith = matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr | BinaryOp::Pow);
        let lhs_hint = match expected {
            Some(t) if arith && (ty_is_integer(t) || ty_is_float(t)) => Some(t),
            _ => None,
        };
        let lt = self.check_expr(lhs, lhs_hint, env, ctx);
        // For most arithmetic, passing `Some(&lt)` as rhs's expected
        // type lets a literal rhs (`x + 5`) adopt lt's type (rule 3).
        // But for generic-struct operands, a differently-shaped-but-
        // still-compatible rhs (`Matrix<f64,3,5>` against a
        // `Matrix<f64,2,3>` lhs, Document 8 §9's own valid case) is
        // exactly what needs to be *accepted* here -- passing `Some(&lt)`
        // would make the outer `check_expr` wrapper's blanket
        // compatibility check reject it before this function's own
        // dimension-aware logic below ever runs. So: only use `lt` as
        // an rhs hint when it isn't a generic-struct type.
        // Phase 10: for `a ?? b` the fallback `b` has the payload type of
        // `a` (`Option<T>`/`Result<T, _>` -> `T`), not `a`'s own type.
        let coalesce_hint: Option<Ty> = if op == BinaryOp::Coalesce {
            match &lt {
                Ty::OptionTy(inner) => Some((**inner).clone()),
                Ty::ResultTy(ok, _) => Some((**ok).clone()),
                _ => None,
            }
        } else {
            None
        };
        let rhs_hint = if op == BinaryOp::Coalesce {
            coalesce_hint.as_ref()
        } else if matches!(lt, Ty::Generic(..)) {
            None
        } else {
            Some(&lt)
        };
        let rt = self.check_expr(rhs, rhs_hint, env, ctx);
        use BinaryOp::*;
        match op {
            Mul => {
                // Document 8 §8/§9: const-generic matrix multiplication
                // dimension check, when both operands are the same
                // generic struct. See `check_matrix_multiply`'s doc
                // comment for the scope/grounding of this special case.
                if let (Ty::Generic(ln, largs), Ty::Generic(rn, rargs)) = (&lt, &rt) {
                    if ln == rn {
                        let (ln, largs, rargs) = (ln.clone(), largs.clone(), rargs.clone());
                        return self.check_matrix_multiply(&ln, &largs, &rargs, ctx);
                    }
                }
                if lt != rt {
                    self.errors.push(TypeError {
                        message: format!("`Mul`: mismatched types `{}` and `{}` (Document 5 §6: no implicit coercion, use `as`)", lt, rt),
                        context: ctx.into(),
                    });
                }
                lt
            }
            Add | Sub | Div | Mod | Pow | BitAnd | BitOr | BitXor | Shl | Shr => {
                if lt != rt {
                    self.errors.push(TypeError {
                        message: format!("`{:?}`: mismatched types `{}` and `{}` (Document 5 §6: no implicit coercion, use `as`)", op, lt, rt),
                        context: ctx.into(),
                    });
                }
                lt
            }
            EqEq | NotEq | Lt | Gt | LtEq | GtEq => {
                if lt != rt {
                    self.errors.push(TypeError {
                        message: format!("comparison between mismatched types `{}` and `{}` (Document 5 §6)", lt, rt),
                        context: ctx.into(),
                    });
                }
                Ty::Bool
            }
            AndAnd | OrOr => {
                if lt != Ty::Bool {
                    self.errors.push(TypeError { message: format!("`&&`/`||` requires `bool`, found `{}`", lt), context: ctx.into() });
                }
                if rt != Ty::Bool {
                    self.errors.push(TypeError { message: format!("`&&`/`||` requires `bool`, found `{}`", rt), context: ctx.into() });
                }
                Ty::Bool
            }
            Coalesce => {
                match lt {
                    Ty::OptionTy(inner) => *inner,
                    Ty::ResultTy(ok, _) => *ok,
                    other => other,
                }
            }
        }
    }

    /// Document 8 §8/§9's const-generic matrix-multiplication dimension
    /// check: `Matrix<f64,2,3> * Matrix<f64,4,5>` must be rejected
    /// (3≠4), while `Matrix<f64,2,3> * Matrix<f64,3,5>` must be
    /// accepted and produce `Matrix<f64,2,5>`.
    ///
    /// FLAGGED, SCOPED INTERPRETATION: Mountain's spec doesn't define
    /// any general operator-overloading mechanism anywhere in Documents
    /// 1–24 (no trait-based `Mul` dispatch is described), so this is
    /// implemented as a direct, special-cased rule for `*` between two
    /// instances of the *same* generic struct carrying exactly two
    /// const-generic arguments — matching Document 8 §8's own
    /// `Matrix<T, const ROWS: usize, const COLS: usize>` declaration
    /// shape exactly, with the two const positions read positionally
    /// (first = rows-analog, second = cols-analog) per that
    /// declaration's literal parameter order. This is grounded directly
    /// in Document 8's own concrete struct declaration and §9's
    /// verification trace, not invented from nothing — but it is a
    /// narrow, name-and-shape-triggered rule, not a general arithmetic-
    /// on-generics mechanism. Needs explicit sign-off, same as every
    /// other flagged deviation.
    fn check_matrix_multiply(&mut self, name: &str, largs: &[GenericArg], rargs: &[GenericArg], ctx: &str) -> Ty {
        let const_values = |args: &[GenericArg]| -> Vec<i64> {
            args.iter().filter_map(|a| match a { GenericArg::Const(n) => Some(*n), _ => None }).collect()
        };
        let lconsts = const_values(largs);
        let rconsts = const_values(rargs);
        if lconsts.len() == 2 && rconsts.len() == 2 {
            let (l_rows, l_cols) = (lconsts[0], lconsts[1]);
            let (r_rows, r_cols) = (rconsts[0], rconsts[1]);
            if l_cols != r_rows {
                self.errors.push(TypeError {
                    message: format!(
                        "cannot multiply `{}` with column count {} by `{}` with row count {}: dimension mismatch (Document 8 §9)",
                        name, l_cols, name, r_rows
                    ),
                    context: ctx.into(),
                });
                return Ty::Generic(name.to_string(), largs.to_vec());
            }
            // Result shape is (l_rows, r_cols); rewrite the two const
            // positions in a copy of `largs` (which also carries the
            // element type argument in whichever position it declared),
            // preserving every non-const argument's position untouched.
            let mut result_args = largs.to_vec();
            let mut const_idx = 0;
            for a in result_args.iter_mut() {
                if let GenericArg::Const(n) = a {
                    *n = if const_idx == 0 { l_rows } else { r_cols };
                    const_idx += 1;
                }
            }
            return Ty::Generic(name.to_string(), result_args);
        }
        // Same struct name, but not this 2-const-param shape -- fall
        // back to ordinary strict equality of the full argument list.
        if largs != rargs {
            self.errors.push(TypeError {
                message: format!("`*`: mismatched generic arguments for `{}`", name),
                context: ctx.into(),
            });
        }
        Ty::Generic(name.to_string(), largs.to_vec())
    }

    fn check_compatible(&mut self, actual: &Ty, expected: Option<&Ty>, ctx: &str) {
        // `!` (Document 5 §2.5) is the bottom type: an expression that
        // never produces a value (`panic(..)`, `return ..`, `throw ..`)
        // is compatible with any expected type.
        if *actual == Ty::Never {
            return;
        }
        if let Some(exp) = expected {
            // A string literal is a `&str` (Document 5 §2.4: `str`/`&str` is the borrowed view).
            if let (Ty::StringTy, Ty::Ref(false, e)) = (actual, exp) {
                if **e == Ty::Str {
                    return;
                }
            }
            // `borrow mut x` may be passed where a shared `borrow x` is expected.
            if let (Ty::Ref(true, a), Ty::Ref(false, e)) = (actual, exp) {
                if a == e {
                    return;
                }
            }
            if actual != exp {
                self.errors.push(TypeError {
                    message: format!("expected `{}`, found `{}` (Document 5 §6: no implicit coercion, use `as`)", exp, actual),
                    context: ctx.into(),
                });
            }
        }
    }

    fn check_if(&mut self, if_expr: &IfExpr, expected: Option<&Ty>, env: &mut Env, ctx: &str) -> Ty {
        self.check_expr(&if_expr.cond, Some(&Ty::Bool), env, ctx);
        let then_ty = self.check_block(&if_expr.then_block, env, expected, ctx).unwrap_or(Ty::Unit);
        match &if_expr.else_branch {
            Some(ElseBranch::Block(b)) => {
                // Phase 9: a diverging (`!`) branch contributes no value,
                // so it neither constrains nor conflicts with the other.
                let else_expected = if then_ty == Ty::Never { expected } else { Some(&then_ty) };
                let else_ty = self.check_block(b, env, else_expected, ctx).unwrap_or(Ty::Unit);
                if then_ty == Ty::Never {
                    return else_ty;
                }
                if else_ty == Ty::Never {
                    return then_ty;
                }
                // Rule 5: no cross-branch guessing -- both arms of an
                // if-expression must agree; the compiler never
                // synthesizes a union/common-supertype.
                if else_ty != then_ty {
                    self.errors.push(TypeError {
                        message: format!(
                            "`if`/`else` branches have incompatible types: `{}` and `{}` (Document 5 rule 5: no cross-branch guessing)",
                            then_ty, else_ty
                        ),
                        context: ctx.into(),
                    });
                }
            }
            Some(ElseBranch::If(inner)) => {
                let else_expected = if then_ty == Ty::Never { expected } else { Some(&then_ty) };
                let else_ty = self.check_if(inner, else_expected, env, ctx);
                if then_ty == Ty::Never {
                    return else_ty;
                }
                if else_ty == Ty::Never {
                    return then_ty;
                }
                if else_ty != then_ty {
                    self.errors.push(TypeError {
                        message: format!(
                            "`if`/`else if` branches have incompatible types: `{}` and `{}` (Document 5 rule 5)",
                            then_ty, else_ty
                        ),
                        context: ctx.into(),
                    });
                }
            }
            None => {
                // Document 9 §1.2: an `if` without a matching `else`
                // cannot be used as a value-producing expression --
                // Phase 3 doesn't yet distinguish statement-position
                // from expression-position `if` (that requires threading
                // "is this result actually used" through the caller),
                // so this isn't separately enforced yet; flagged rather
                // than silently claimed as checked.
            }
        }
        then_ty
    }

    fn check_match(&mut self, match_expr: &MatchExpr, expected: Option<&Ty>, env: &mut Env, ctx: &str) -> Ty {
        let saved_unresolved = std::mem::replace(&mut self.saw_unresolved, false);
        let scrutinee_ty = self.check_expr(&match_expr.scrutinee, None, env, ctx);
        // Phase 9: a scrutinee whose type could not be resolved (an
        // unmodeled stdlib call, e.g. Document 24 §5's `match rx.recv()
        // { Ok(..) => .., Err(_) => .. }`) is typed `()`. To the
        // exhaustiveness checker `()` is an unenumerable domain, so any
        // constructor-only match over it would be reported non-exhaustive
        // -- a false positive on valid code. Skip the check in exactly
        // that case, the same "unresolved => stay silent" convention used
        // everywhere else in this checker.
        let scrutinee_unresolved = scrutinee_ty == Ty::Unit && self.saw_unresolved;
        self.saw_unresolved = saved_unresolved || self.saw_unresolved;
        // Phase 8 (Document 9 §2.5 / Document 17 §4.5): real
        // pattern-matrix exhaustiveness checking, not a heuristic --
        // see `exhaustive.rs`'s module doc for the algorithm and its
        // scope. `enum_table`/`struct_table` are rebuilt from
        // `self.enums`/`self.structs` each call rather than cached:
        // counts are small, this keeps `exhaustive.rs` fully decoupled
        // from `EnumShape`/`StructShape`'s own layout, and `check_match`
        // isn't a hot path relative to the rest of type-checking.
        // `struct_table` only includes tuple structs (`is_tuple`) --
        // Document 9 §2.4's own `Point(x, y)` shape -- since a
        // named-field struct has no corresponding `Pattern` variant to
        // ever be looked up against anyway.
        let enum_table: crate::exhaustive::EnumTable = self
            .enums
            .iter()
            .map(|(name, shape)| (name.clone(), shape.variants.as_slice()))
            .collect();
        let struct_table: crate::exhaustive::StructTable = self
            .structs
            .iter()
            .filter(|(_, shape)| shape.is_tuple)
            .map(|(name, shape)| (name.clone(), shape.fields.iter().map(|(_, ty)| ty.clone()).collect()))
            .collect();
        if !scrutinee_unresolved {
            if let Err(message) = crate::exhaustive::check_exhaustiveness(&scrutinee_ty, &match_expr.arms, &enum_table, &struct_table) {
                self.errors.push(TypeError { message, context: ctx.into() });
            }
        }
        let mut common: Option<Ty> = expected.cloned();
        let mut saw_arm = false;
        let mut all_arms_diverge = true;
        for arm in &match_expr.arms {
            env.push();
            // Phase 9: pattern-introduced bindings are now distributed
            // against the scrutinee's type for the shapes Document 11
            // needs (`Ok(v)`/`Err(e)`/`Some(x)`/`None`) and, by the same
            // table lookup, user enum variants, tuples and tuple structs
            // (see `bind_pattern`). `let`-destructuring is still the
            // Phase 3 gap (only plain identifiers bind there).
            for pat in &arm.patterns {
                self.bind_pattern(pat, &scrutinee_ty, env);
            }
            if let Some(guard) = &arm.guard {
                self.check_expr(guard, Some(&Ty::Bool), env, ctx);
            }
            let arm_ty = match &arm.body {
                MatchArmBody::Expr(e) => self.check_expr(e, common.as_ref(), env, ctx),
                MatchArmBody::Block(b) => self.check_block(b, env, common.as_ref(), ctx).unwrap_or(Ty::Unit),
            };
            env.pop();
            saw_arm = true;
            // Phase 9: an arm of type `!` (`panic(..)`, `return ..`)
            // contributes no value and can't conflict (Document 9 §2.3's
            // own `n if n < 0 => panic("invalid age")` example).
            if arm_ty == Ty::Never {
                continue;
            }
            all_arms_diverge = false;
            match &common {
                Some(c) if *c != arm_ty => {
                    // Rule 5 again, for `match`: Document 5 §7's own
                    // explicit verification case (one arm `i32`, another
                    // `String`) -- rejected, not unioned.
                    self.errors.push(TypeError {
                        message: format!(
                            "`match` arms have incompatible types: `{}` and `{}` (Document 5 rule 5: no cross-branch guessing)",
                            c, arm_ty
                        ),
                        context: ctx.into(),
                    });
                }
                None => common = Some(arm_ty),
                _ => {}
            }
        }
        if saw_arm && all_arms_diverge && common.is_none() {
            return Ty::Never;
        }
        common.unwrap_or(Ty::Unit)
    }

    fn check_struct_lit(&mut self, name: &str, fields: &[(String, Expr)], has_spread: bool, expected: Option<&Ty>, env: &mut Env, ctx: &str) -> Ty {
        let self_name = self.self_type.as_ref().map(|t| t.to_string());
        let name: &str = if name == "Self" { self_name.as_deref().unwrap_or(name) } else { name };
        let Some(shape) = self.structs.get(name).cloned() else {
            self.errors.push(TypeError {
                message: format!("undefined struct `{}`", name),
                context: ctx.into(),
            });
            return Ty::Named(name.to_string());
        };

        // Document 5 rule 1 (explicit annotation wins), applied to
        // generic struct literals: if `expected` names this same
        // generic struct with concrete args (e.g. `let p: Pair<i32,
        // String> = Pair { first: 1, second: "x" };`), substitute those
        // concrete types for the struct's own `TypeParam` placeholders
        // before checking each field, and use `expected`'s args as the
        // literal's own resolved type. Without a matching annotation,
        // Phase 5 doesn't infer generic args from field values alone
        // (that would need real unification across every field) —
        // falls back to `Ty::Named(name)`, a known, flagged
        // simplification rather than a silent wrong answer.
        let (subst, result_ty) = if !shape.generics.is_empty() {
            match expected {
                Some(Ty::Generic(en, eargs)) if en == name && eargs.len() == shape.generics.len() => {
                    let subst: HashMap<String, Ty> = shape.generics.iter().zip(eargs.iter())
                        .filter_map(|(param, arg)| match (param, arg) {
                            (GenericParam::Type { name, .. }, GenericArg::Type(t)) => Some((name.clone(), t.clone())),
                            _ => None,
                        }).collect();
                    (subst, Ty::Generic(name.to_string(), eargs.clone()))
                }
                _ => (HashMap::new(), Ty::Named(name.to_string())),
            }
        } else {
            (HashMap::new(), Ty::Named(name.to_string()))
        };

        let declared: HashMap<String, Ty> = shape.fields.iter()
            .map(|(n, t)| (n.clone(), substitute_type_params(t, &subst)))
            .collect();
        let mut provided = std::collections::HashSet::new();
        for (fname, fval) in fields {
            provided.insert(fname.as_str());
            match declared.get(fname.as_str()) {
                Some(ft) => { self.check_expr(fval, Some(ft), env, ctx); }
                None => {
                    self.errors.push(TypeError {
                        message: format!("struct `{}` has no field `{}`", name, fname),
                        context: ctx.into(),
                    });
                }
            }
        }
        if !has_spread {
            // Document 7 §2.2: every field must be explicitly
            // initialized unless a `..` spread is present.
            for (fname, _) in &shape.fields {
                if !provided.contains(fname.as_str()) {
                    self.errors.push(TypeError {
                        message: format!("missing field `{}` in struct literal for `{}`", fname, name),
                        context: ctx.into(),
                    });
                }
            }
        }
        result_ty
    }

    /// Resolves a method call against a receiver's static type. Two
    /// dispatch modes, per Document 7 §4.4/§4.5 and this phase's exit
    /// criteria:
    /// - **Static dispatch** (`Ty::Named`): search every `impl` block
    ///   registered for that concrete type (inherent *and* trait impls
    ///   both contribute) for a matching method name. This is what
    ///   "static/monomorphized, zero-cost" dispatch means at the
    ///   type-checking level — the exact concrete implementation is
    ///   known here, at compile time, not deferred to a vtable.
    /// - **Dynamic dispatch** (`Ty::DynTrait`): resolve against the
    ///   *trait's own* declared signature instead of any concrete
    ///   `impl` — correct, because with `dyn Trait` the concrete
    ///   implementing type isn't known until runtime (Document 7 §4.5:
    ///   "resolved via vtable lookup at runtime"); statically, all that
    ///   can be verified is that the trait itself declares a method with
    ///   this name and signature.
    /// Checks whether a concrete type satisfies a named trait bound
    /// (Document 8 §2/§3), by looking up whether any registered `impl`
    /// of that trait exists for the type's canonical name — the same
    /// registry (`impls_by_type`) Phase 4 built, reused here rather
    /// than duplicated. Works for both user-declared structs/enums and
    /// primitives (`impl Comparable for i32` registers under the key
    /// `"i32"`, matching how `parse_type_ref` already lexes primitive
    /// type names as plain identifiers — see Phase 1's design note).
    // ==================================================================
    // Phase 9 — Error handling (Document 11)
    // ==================================================================

    /// Owned snapshot of the innermost propagation target.
    fn top_target(&self) -> Option<TargetView> {
        match self.prop_targets.last() {
            Some(PropTarget::FnRet(t)) => Some(TargetView::Fn(t.clone())),
            Some(PropTarget::Try(_)) => Some(TargetView::Try),
            Some(PropTarget::Opaque) => Some(TargetView::Opaque),
            None => None,
        }
    }

    fn top_is_opaque(&self) -> bool {
        matches!(self.prop_targets.last(), Some(PropTarget::Opaque))
    }

    /// If the innermost target is a `try` block, records `ty` as one of
    /// its error sources and returns true; otherwise returns false.
    fn record_try_source(&mut self, ty: Ty) -> bool {
        if let Some(PropTarget::Try(frame)) = self.prop_targets.last_mut() {
            frame.err_sources.push(ty);
            true
        } else {
            false
        }
    }

    fn mark_try_opaque(&mut self) {
        if let Some(PropTarget::Try(frame)) = self.prop_targets.last_mut() {
            frame.saw_opaque = true;
        }
    }

    /// Does `impl From<src> for target` exist (Document 3 Category F;
    /// Document 11 §2)? Identical types always convert (the blanket
    /// `From<T> for T`), handled by the caller.
    fn from_impl_exists(&self, src: &Ty, target: &Ty) -> bool {
        let Some(key) = ty_lookup_name(target) else { return false };
        self.impls_by_type
            .get(&key)
            .map(|impls| {
                impls.iter().any(|r| {
                    r.trait_name.as_deref() == Some("From") && r.trait_args.first() == Some(src)
                })
            })
            .unwrap_or(false)
    }

    /// Document 11 §2: `expr?`. On `Result<T, E2>` it evaluates to `T`
    /// and routes `E2` to the innermost propagation target; on
    /// `Option<T>` it evaluates to `T` and routes `None`.
    fn check_propagate(&mut self, inner: &Expr, expected: Option<&Ty>, env: &mut Env, ctx: &str) -> Ty {
        let saved = std::mem::replace(&mut self.saw_unresolved, false);
        let operand_ty = self.check_expr(inner, None, env, ctx);
        let operand_unresolved = self.saw_unresolved;
        self.saw_unresolved = saved || operand_unresolved;
        match operand_ty {
            Ty::ResultTy(ok, err) => {
                self.route_result_error(&err, ctx);
                *ok
            }
            Ty::OptionTy(some) => {
                self.route_option_none(ctx);
                *some
            }
            other => {
                if operand_unresolved {
                    // Can't tell what the operand is (stdlib call not yet
                    // modeled) -- stay silent, but remember it so a
                    // `try` block doesn't wrongly conclude "no errors".
                    self.mark_try_opaque();
                } else {
                    self.errors.push(TypeError {
                        message: format!(
                            "the `?` operator can only be applied to `Result<_, _>` or `Option<_>`, found `{}` (Document 11 §2)",
                            other
                        ),
                        context: ctx.into(),
                    });
                }
                expected.cloned().unwrap_or(Ty::Unit)
            }
        }
    }

    /// Returns `Some(converted)` if `src` can flow into `target` (equal
    /// types, or a real `impl From<src> for target`); `None` after
    /// reporting an error otherwise. Document 11 §2: the conversion is
    /// "never automatic/implicit type coercion ... always backed by a
    /// real, developer-written trait implementation".
    fn check_err_conversion(&mut self, src: &Ty, target: &Ty, ctx: &str) -> Option<bool> {
        if src == target {
            return Some(false);
        }
        if self.from_impl_exists(src, target) {
            return Some(true);
        }
        self.errors.push(TypeError {
            message: format!(
                "`?` cannot convert error type `{}` into `{}`: no `impl From<{}> for {}` exists (Document 11 §2)",
                src, target, src, target
            ),
            context: ctx.into(),
        });
        None
    }

    /// Routes the `Err(E2)` half of `?` to the innermost target. The
    /// SAME function serves `?` in a `fn` body and `?` in a `try` block
    /// -- see the `PropTarget` doc comment for why that matters.
    fn route_result_error(&mut self, err: &Ty, ctx: &str) {
        match self.top_target() {
            Some(TargetView::Fn(ret)) => match ret {
                Ty::ResultTy(_, target_err) => {
                    if let Some(converted) = self.check_err_conversion(err, &target_err, ctx) {
                        self.propagations.push(PropagationRecord {
                            kind: PropKind::ResultErr,
                            source: Some(err.clone()),
                            target: Some(*target_err),
                            converted,
                        });
                    }
                }
                other => {
                    self.errors.push(TypeError {
                        message: format!(
                            "`?` on a `Result` requires the enclosing function to return `Result<_, E>`, but it returns `{}` (Document 11 §2)",
                            other
                        ),
                        context: ctx.into(),
                    });
                }
            },
            Some(TargetView::Try) => {
                self.record_try_source(err.clone());
            }
            Some(TargetView::Opaque) => {}
            None => {
                self.errors.push(TypeError {
                    message: "`?` used outside any function body".into(),
                    context: ctx.into(),
                });
            }
        }
    }

    /// Routes the `None` half of `?` on an `Option`.
    fn route_option_none(&mut self, ctx: &str) {
        match self.top_target() {
            Some(TargetView::Fn(ret)) => {
                if matches!(ret, Ty::OptionTy(_)) {
                    self.propagations.push(PropagationRecord {
                        kind: PropKind::OptionNone,
                        source: None,
                        target: None,
                        converted: false,
                    });
                } else {
                    self.errors.push(TypeError {
                        message: format!(
                            "`?` on an `Option` requires the enclosing function to return `Option<_>`, but it returns `{}` (Document 11 §2)",
                            ret
                        ),
                        context: ctx.into(),
                    });
                }
            }
            Some(TargetView::Try) => {
                self.errors.push(TypeError {
                    message: "`?` on an `Option` inside a `try` block is not supported: the block's implicit wrapper returns `Result`, which cannot carry `None` (Document 11 §3; see PROGRESS.md Phase 9 flagged interpretation 3)".into(),
                    context: ctx.into(),
                });
            }
            Some(TargetView::Opaque) => {}
            None => {
                self.errors.push(TypeError {
                    message: "`?` used outside any function body".into(),
                    context: ctx.into(),
                });
            }
        }
    }

    /// Picks the single error type a `try` block's implicit wrapper
    /// returns (Document 11 §3's `Result<(), ConfigError>`): the first
    /// source type that EVERY source type equals or converts into via a
    /// real `From` impl. Also emits one `PropagationRecord` per source.
    /// FLAGGED INTERPRETATION: Document 11 §3's example wrapper names a
    /// single declared error type without saying how it is derived when
    /// the block's `?`s carry different types; this is the rule that
    /// reproduces its example (`IoError` + `ConfigError` ->
    /// `ConfigError`, given `From<IoError> for ConfigError`).
    fn resolve_try_error_type(&mut self, sources: &[Ty], ctx: &str) -> Option<Ty> {
        if sources.is_empty() {
            return None;
        }
        let mut chosen: Option<Ty> = None;
        for cand in sources {
            if sources.iter().all(|s| s == cand || self.from_impl_exists(s, cand)) {
                chosen = Some(cand.clone());
                break;
            }
        }
        match chosen {
            Some(target) => {
                for s in sources {
                    self.propagations.push(PropagationRecord {
                        kind: PropKind::ResultErr,
                        source: Some(s.clone()),
                        target: Some(target.clone()),
                        converted: *s != target,
                    });
                }
                Some(target)
            }
            None => {
                let listed: Vec<String> = sources.iter().map(|t| format!("`{}`", t)).collect();
                self.errors.push(TypeError {
                    message: format!(
                        "`try` block propagates error types {} but none of them is a type all the others convert into via `impl From<..>` (Document 11 §3, §2)",
                        listed.join(", ")
                    ),
                    context: ctx.into(),
                });
                Some(sources[0].clone())
            }
        }
    }

    /// Document 11 §3: `try { .. } catch (e) { .. }`.
    fn check_try_catch(
        &mut self,
        key: usize,
        try_block: &Block,
        catch_var: &str,
        catch_block: &Block,
        expected: Option<&Ty>,
        env: &mut Env,
        ctx: &str,
    ) -> Ty {
        self.prop_targets.push(PropTarget::Try(TryFrame { err_sources: Vec::new(), saw_opaque: false }));
        let try_raw = self.check_block(try_block, env, expected, ctx);
        let frame = match self.prop_targets.pop() {
            Some(PropTarget::Try(f)) => f,
            _ => TryFrame { err_sources: Vec::new(), saw_opaque: false },
        };
        let try_ty = try_raw.unwrap_or(Ty::Unit);

        let bound = match self.resolve_try_error_type(&frame.err_sources, ctx) {
            Some(t) => t,
            None => {
                if !frame.saw_opaque {
                    self.errors.push(TypeError {
                        message: "cannot infer the error type of this `try` block: it contains no `?` on a `Result` and no `throw` (Document 5 rule 6: ambiguity is an error, not a default)".into(),
                        context: ctx.into(),
                    });
                }
                Ty::Unit
            }
        };

        // Phase 10: record the wrapper's `Result<value, error>` shape for
        // the desugar pass (a block that cannot fall through carries `()`).
        self.try_info.insert(key, (if try_ty == Ty::Never { Ty::Unit } else { try_ty.clone() }, bound.clone()));

        env.push();
        env.insert(catch_var.to_string(), bound);
        let catch_expected: Option<Ty> = if try_ty == Ty::Never { expected.cloned() } else { Some(try_ty.clone()) };
        let catch_ty = self.check_block(catch_block, env, catch_expected.as_ref(), ctx).unwrap_or(Ty::Unit);
        env.pop();

        if try_ty == Ty::Never {
            catch_ty
        } else if catch_ty == Ty::Never {
            try_ty
        } else {
            if catch_ty != try_ty {
                self.errors.push(TypeError {
                    message: format!(
                        "`try` block and `catch` block have incompatible types: `{}` and `{}` (Document 5 rule 5: no cross-branch guessing)",
                        try_ty, catch_ty
                    ),
                    context: ctx.into(),
                });
            }
            try_ty
        }
    }

    /// `return` / `return <expr>` (Document 11 §4). The value is checked
    /// against the enclosing function's declared return type.
    /// Bare `return;` is NOT validated against a non-unit return type
    /// (generators/async make that rule non-trivial) -- known gap.
    fn check_return(&mut self, value: Option<&Expr>, env: &mut Env, ctx: &str) {
        let expected: Option<Ty> = match self.top_target() {
            Some(TargetView::Fn(t)) => Some(t),
            Some(TargetView::Try) => {
                self.errors.push(TypeError {
                    message: "`return` directly inside a `try` block is ambiguous: under Document 11 §3's wrapper-function desugaring it would return from the wrapper, not the enclosing function; move the `return` into the `catch` block or after the `try` (see PROGRESS.md Phase 9 flagged interpretation 2)".into(),
                    context: ctx.into(),
                });
                None
            }
            Some(TargetView::Opaque) | None => None,
        };
        if let Some(e) = value {
            self.check_expr(e, expected.as_ref(), env, ctx);
        }
    }

    /// `Ok(x)` / `Err(e)` / `Some(x)` / `None` (Document 11 §1, built on
    /// `builtin_variants`'s single table).
    ///
    /// `Some(x)` alone determines its full type (`Option<typeof x>`).
    /// `Ok(x)`, `Err(e)` and `None` do NOT -- each leaves one type
    /// parameter free -- so without an expected type they are ambiguous
    /// and, per Document 5 inference rule 6, an error asking for an
    /// annotation (never a silent default). Inside a closure body,
    /// where return types aren't tracked yet, they stay silent instead.
    fn check_variant_ctor(&mut self, name: &str, args: &[Arg], expected: Option<&Ty>, env: &mut Env, ctx: &str) -> Ty {
        let arity = if name == "None" { 0 } else { 1 };
        if args.len() != arity {
            self.errors.push(TypeError {
                message: format!("`{}` expects {} argument(s), found {} (Document 11 §1)", name, arity, args.len()),
                context: ctx.into(),
            });
            for a in args {
                self.check_expr(&a.value, None, env, ctx);
            }
            return expected.cloned().unwrap_or(Ty::Unit);
        }
        let (payload_expected, whole): (Option<Ty>, Option<Ty>) = match (name, expected) {
            ("Ok", Some(Ty::ResultTy(ok, _))) => (Some((**ok).clone()), expected.cloned()),
            ("Err", Some(Ty::ResultTy(_, err))) => (Some((**err).clone()), expected.cloned()),
            ("Some", Some(Ty::OptionTy(inner))) => (Some((**inner).clone()), expected.cloned()),
            ("None", Some(Ty::OptionTy(_))) => (None, expected.cloned()),
            _ => (None, None),
        };
        let payload_ty: Option<Ty> = if arity == 1 {
            Some(self.check_expr(&args[0].value, payload_expected.as_ref(), env, ctx))
        } else {
            None
        };
        if let Some(w) = whole {
            return w;
        }
        let family = if name == "Ok" || name == "Err" { "Result" } else { "Option" };
        if let Some(exp) = expected {
            self.errors.push(TypeError {
                message: format!("`{}` builds a `{}` value, but `{}` is expected", name, family, exp),
                context: ctx.into(),
            });
            return exp.clone();
        }
        if name == "Some" {
            return Ty::OptionTy(Box::new(payload_ty.unwrap_or(Ty::Unit)));
        }
        if !self.top_is_opaque() {
            self.errors.push(TypeError {
                message: format!(
                    "cannot infer the full type of `{}`: it leaves a type parameter of `{}` undetermined -- add a type annotation (Document 5 rule 6)",
                    name, family
                ),
                context: ctx.into(),
            });
        }
        Ty::Unit
    }

    /// `panic(msg)`, `assert(cond)`, `ensure(cond, err)` (Document 11
    /// §4, §5). Type-level behavior only; debug-vs-release stripping of
    /// `assert` is a codegen concern (Phase 10) with nothing to strip
    /// yet -- see PROGRESS.md.
    fn check_error_intrinsic(&mut self, name: &str, args: &[Arg], expected: Option<&Ty>, env: &mut Env, ctx: &str) -> Ty {
        let (want, result): (usize, Ty) = match name {
            "panic" => (1, Ty::Never),
            "assert" => (1, Ty::Unit),
            _ => (2, Ty::Unit), // `ensure`, result rebuilt below
        };
        if args.len() != want {
            self.errors.push(TypeError {
                message: format!("`{}` expects {} argument(s), found {} (Document 11 §4/§5)", name, want, args.len()),
                context: ctx.into(),
            });
            for a in args {
                self.check_expr(&a.value, None, env, ctx);
            }
            return if name == "panic" { Ty::Never } else { expected.cloned().unwrap_or(Ty::Unit) };
        }
        match name {
            "panic" => {
                self.check_expr(&args[0].value, Some(&Ty::StringTy), env, ctx);
                result
            }
            "assert" => {
                self.check_expr(&args[0].value, Some(&Ty::Bool), env, ctx);
                result
            }
            _ => {
                self.check_expr(&args[0].value, Some(&Ty::Bool), env, ctx);
                let err_expected = match expected {
                    Some(Ty::ResultTy(_, e)) => Some((**e).clone()),
                    _ => None,
                };
                let err_ty = self.check_expr(&args[1].value, err_expected.as_ref(), env, ctx);
                Ty::ResultTy(Box::new(Ty::Unit), Box::new(err_ty))
            }
        }
    }

    /// The field types a constructor named `name` (last path segment)
    /// carries when matched against a value of type `ty`, if known:
    /// built-in `Option`/`Result` variants, user enum variants, or a
    /// tuple struct's single constructor.
    fn ctor_field_types(&self, name: &str, ty: &Ty) -> Option<Vec<Ty>> {
        if let Some(variants) = builtin_variants(ty) {
            return variants.into_iter().find(|(n, _)| *n == name).map(|(_, tys)| tys);
        }
        if let Ty::Named(tn) = ty {
            if let Some(shape) = self.enums.get(tn) {
                return shape.variants.iter().find(|(n, _)| n.as_str() == name).map(|(_, tys)| tys.clone());
            }
            if let Some(shape) = self.structs.get(tn) {
                if shape.is_tuple && tn.as_str() == name {
                    return Some(shape.fields.iter().map(|(_, t)| t.clone()).collect());
                }
            }
        }
        None
    }

    /// Binds the variables a match-arm pattern introduces, with types
    /// taken from the scrutinee's shape. When a constructor's field
    /// types can't be determined (scrutinee is an unresolved stdlib
    /// value), sub-pattern variables are bound as `()` -- this
    /// checker's existing "unknown => `()`" convention -- instead of
    /// being left undefined, which would report a false "undefined
    /// variable" for perfectly valid code.
    fn bind_pattern(&self, pat: &Pattern, ty: &Ty, env: &mut Env) {
        match pat {
            Pattern::Wildcard | Pattern::Literal(_) | Pattern::Array(..) => {}
            Pattern::Mut(n) => env.insert(n.clone(), ty.clone()),
            Pattern::Ident(n) => {
                // A path (`HttpMethod::Get`) or a nullary variant of the
                // scrutinee's own type (`None`) is a constructor test,
                // not a fresh binding -- same rule `exhaustive.rs` uses.
                let is_ctor = n.contains("::") || {
                    let last = n.rsplit("::").next().unwrap_or(n.as_str());
                    self.ctor_field_types(last, ty).map(|f| f.is_empty()).unwrap_or(false)
                };
                if !is_ctor {
                    env.insert(n.clone(), ty.clone());
                }
            }
            Pattern::Tuple(subs) => match ty {
                Ty::Tuple(tys) if tys.len() == subs.len() => {
                    for (p, t) in subs.iter().zip(tys.iter()) {
                        self.bind_pattern(p, t, env);
                    }
                }
                _ => {
                    for p in subs {
                        self.bind_pattern(p, &Ty::Unit, env);
                    }
                }
            },
            Pattern::TupleStruct(name, subs) => {
                let last = name.rsplit("::").next().unwrap_or(name.as_str());
                match self.ctor_field_types(last, ty) {
                    Some(tys) if tys.len() == subs.len() => {
                        for (p, t) in subs.iter().zip(tys.iter()) {
                            self.bind_pattern(p, t, env);
                        }
                    }
                    _ => {
                        for p in subs {
                            self.bind_pattern(p, &Ty::Unit, env);
                        }
                    }
                }
            }
            Pattern::Or(alts) => {
                for a in alts {
                    self.bind_pattern(a, ty, env);
                }
            }
        }
    }

    fn satisfies_bound(&self, ty: &Ty, trait_name: &str) -> bool {
        let Some(key) = ty_lookup_name(ty) else { return false };
        self.impls_by_type.get(&key)
            .map(|impls| impls.iter().any(|r| r.trait_name.as_deref() == Some(trait_name)))
            .unwrap_or(false)
    }

    fn resolve_method(&self, recv_ty: &Ty, name: &str) -> Option<FnSig> {
        match recv_ty {
            Ty::DynTrait(trait_name) => {
                self.traits.get(trait_name)?.methods.get(name).cloned()
            }
            // Phase 11a: impls on primitive types (`impl Trait for i32`) are keyed by
            // the primitive's name, so `i32`, `bool`, ... resolve like named types.
            other => {
                let type_name: &String = &match other {
                    Ty::Named(n) | Ty::Generic(n, _) => n.clone(),
                    o => o.to_string(),
                };
                let impls = self.impls_by_type.get(type_name)?;
                // Inherent methods (`impl Type { ... }`, no trait) take
                // precedence over trait-provided methods of the same
                // name -- the usual method-resolution order. This is
                // also what makes `ImplRecord::trait_name` an actually-
                // consumed piece of information rather than write-only
                // (caught proactively via `grep -n "\.trait_name\b"`
                // returning nothing before this fix, same discipline as
                // Phase 3's `self.enums` catch).
                impls.iter().find(|r| r.trait_name.is_none())
                    .and_then(|r| r.methods.get(name).cloned())
                    .or_else(|| impls.iter().find_map(|r| r.methods.get(name).cloned()))
                    // Phase 11a: a trait's default method (Document 7 §4.3) that
                    // the impl did not override, with `Self` = the impl's type.
                    .or_else(|| {
                        let self_subst: HashMap<String, Ty> = std::iter::once(("Self".to_string(), Ty::Named(type_name.clone()))).collect();
                        impls.iter().find_map(|r| {
                            let tn = r.trait_name.as_ref()?;
                            let sig = self.traits.get(tn)?.methods.get(name)?;
                            if !sig.has_default_body {
                                return None;
                            }
                            let fix = |t: &Ty| substitute_type_params(&mark_type_params(t.clone(), &["Self".to_string()]), &self_subst);
                            Some(FnSig { params: sig.params.iter().map(fix).collect(), ret: fix(&sig.ret), has_default_body: true })
                        })
                    })
            }
        }
    }

    fn field_type(&mut self, base: &Ty, field: &str, ctx: &str) -> Ty {
        // Auto-deref through `&T` / `borrow T` (Phase 9; see `strip_refs`).
        if let Ty::Ref(_, inner) = base {
            let inner = (**inner).clone();
            return self.field_type(&inner, field, ctx);
        }
        if let Ty::Named(n) = base {
            if let Some(shape) = self.structs.get(n) {
                for (fname, fty) in &shape.fields {
                    if fname == field {
                        return fty.clone();
                    }
                }
                self.errors.push(TypeError {
                    message: format!("struct `{}` has no field `{}`", n, field),
                    context: ctx.into(),
                });
                return Ty::Unit;
            }
        }
        if let Ty::Generic(n, args) = base {
            if let Some(shape) = self.structs.get(n).cloned() {
                let subst: HashMap<String, Ty> = shape.generics.iter().zip(args.iter())
                    .filter_map(|(param, arg)| match (param, arg) {
                        (GenericParam::Type { name, .. }, GenericArg::Type(t)) => Some((name.clone(), t.clone())),
                        _ => None,
                    }).collect();
                for (fname, fty) in &shape.fields {
                    if fname == field {
                        return substitute_type_params(fty, &subst);
                    }
                }
                self.errors.push(TypeError {
                    message: format!("struct `{}` has no field `{}`", n, field),
                    context: ctx.into(),
                });
                return Ty::Unit;
            }
        }
        // Phase 10: positional access on a tuple (`pair.0`, Document 5 §3.2)
        // used to fall through to the silent `()` below.
        if let Ty::Tuple(ts) = base {
            if let Ok(i) = field.parse::<usize>() {
                if let Some(t) = ts.get(i) {
                    return t.clone();
                }
            }
            self.errors.push(TypeError {
                message: format!("tuple `{}` has no field `{}`", base, field),
                context: ctx.into(),
            });
            return Ty::Unit;
        }
        // Base type not a known struct (could be a module path result,
        // a foreign/unregistered generic, etc.) -- don't fabricate an
        // error for cases outside this phase's scope.
        self.saw_unresolved = true;
        Ty::Unit
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}
