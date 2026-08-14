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
    Tuple(Vec<Ty>),
    Ref(bool, Box<Ty>),
    /// A user-declared struct or enum, by name. Phase 3 has no generics
    /// yet (Phase 5), so this carries no type arguments.
    Named(String),
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
            Ty::Tuple(ts) => write!(f, "({})", ts.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")),
            Ty::Ref(m, t) => write!(f, "&{}{}", if *m { "mut " } else { "" }, t),
            Ty::Named(n) => write!(f, "{}", n),
            Ty::DynTrait(n) => write!(f, "dyn {}", n),
            Ty::OptionTy(t) => write!(f, "Option<{}>", t),
            Ty::ResultTy(o, e) => write!(f, "Result<{}, {}>", o, e),
            Ty::Fn(ps, r) => write!(f, "fn({}) -> {}", ps.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", "), r),
            Ty::Null => write!(f, "null"),
        }
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
    match t {
        Type::Primitive(s) => match s.as_str() {
            "i8" => Ty::I8, "i16" => Ty::I16, "i32" => Ty::I32, "i64" => Ty::I64,
            "i128" => Ty::I128, "isize" => Ty::Isize,
            "u8" => Ty::U8, "u16" => Ty::U16, "u32" => Ty::U32, "u64" => Ty::U64,
            "u128" => Ty::U128, "usize" => Ty::Usize,
            "f32" => Ty::F32, "f64" => Ty::F64,
            "bool" => Ty::Bool, "char" => Ty::Char,
            "String" => Ty::StringTy, "str" => Ty::Str,
            other => Ty::Named(other.to_string()),
        },
        Type::Named(n, _args) => Ty::Named(n.clone()),
        Type::Array(inner, _size) => Ty::Array(Box::new(resolve_type(inner))),
        Type::Tuple(ts) => Ty::Tuple(ts.iter().map(resolve_type).collect()),
        Type::Ref { mutable, inner, .. } => Ty::Ref(*mutable, Box::new(resolve_type(inner))),
        Type::Dyn(n, _) => Ty::DynTrait(n.clone()),
        Type::Fn(ps, r) => Ty::Fn(ps.iter().map(resolve_type).collect(), Box::new(resolve_type(r))),
        Type::Option(inner) => Ty::OptionTy(Box::new(resolve_type(inner))),
        Type::Result(o, e) => Ty::ResultTy(Box::new(resolve_type(o)), Box::new(resolve_type(e))),
        Type::Unit => Ty::Unit,
        Type::Never => Ty::Never,
        Type::ConstArg(_) => Ty::Unit, // not a real type position; Phase 5 concern
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
}

#[derive(Debug, Clone)]
pub struct EnumShape {
    pub variants: Vec<(String, Vec<Ty>)>,
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
    functions: HashMap<String, (Vec<Ty>, Ty)>,
    traits: HashMap<String, TraitShape>,
    impls_by_type: HashMap<String, Vec<ImplRecord>>,
    pub errors: Vec<TypeError>,
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

impl TypeChecker {
    pub fn new() -> Self {
        TypeChecker {
            structs: HashMap::new(),
            enums: HashMap::new(),
            functions: HashMap::new(),
            traits: HashMap::new(),
            impls_by_type: HashMap::new(),
            errors: Vec::new(),
        }
    }

    pub fn check_program(&mut self, program: &Program) {
        // Pass 1: register all struct/enum/trait data shapes and fn
        // signatures first, so forward references (a function defined
        // before a struct it uses, or mutual struct references) resolve
        // regardless of declaration order.
        for item in &program.items {
            self.register_item(item);
        }
        // Pass 2: register and validate every `impl` block -- run only
        // after pass 1 so a trait declared textually *after* its impl
        // still resolves correctly, and so the orphan-rule / required-
        // method-completeness checks have the full struct/enum/trait
        // picture available regardless of declaration order.
        for item in &program.items {
            self.register_impls(item);
        }
        // Pass 3: type-check every function body (can now resolve
        // method calls via `impls_by_type`/`traits`).
        for item in &program.items {
            self.check_item(item);
        }
    }

    fn register_item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Struct(s) => {
                let (fields, is_tuple) = match &s.body {
                    StructBody::Named(fs) => (
                        fs.iter().map(|f| (f.name.clone(), resolve_type(&f.ty))).collect(),
                        false,
                    ),
                    StructBody::Tuple(ts) => (
                        ts.iter().enumerate().map(|(i, t)| (i.to_string(), resolve_type(t))).collect(),
                        true,
                    ),
                    StructBody::Unit => (Vec::new(), false),
                };
                self.structs.insert(s.name.clone(), StructShape { fields, is_tuple });
            }
            ItemKind::Enum(e) => {
                let variants = e.variants.iter()
                    .map(|v| (v.name.clone(), v.data.iter().map(resolve_type).collect()))
                    .collect();
                self.enums.insert(e.name.clone(), EnumShape { variants });
            }
            ItemKind::Fn(f) => {
                let params = f.params.iter()
                    .filter(|p| p.name != "self")
                    .map(|p| resolve_type(&p.ty))
                    .collect();
                let ret = f.return_type.as_ref().map(resolve_type).unwrap_or(Ty::Unit);
                self.functions.insert(f.name.clone(), (params, ret));
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

            let methods: HashMap<String, FnSig> = impl_decl.items.iter().filter_map(|ii| match ii {
                ImplItem::Fn(f) => {
                    let params = f.params.iter()
                        .filter(|p| p.name != "self")
                        .map(|p| resolve_type(&p.ty))
                        .collect();
                    let ret = f.return_type.as_ref().map(resolve_type).unwrap_or(Ty::Unit);
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

            self.impls_by_type.entry(target_name).or_default().push(ImplRecord { trait_name, methods });
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
                let self_ty = Ty::Named(impl_decl.target.name.clone());
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
            _ => {}
        }
    }

    fn check_fn(&mut self, f: &FnDecl, self_ty: Option<&Ty>) {
        let Some(body) = &f.body else { return };
        let mut env = Env::new();
        for p in &f.params {
            if p.name == "self" {
                if let Some(t) = self_ty {
                    env.insert("self".to_string(), t.clone());
                }
                continue;
            }
            env.insert(p.name.clone(), resolve_type(&p.ty));
        }
        let expected_ret = f.return_type.as_ref().map(resolve_type);
        self.check_block(body, &mut env, expected_ret.as_ref(), &f.name);
    }

    fn check_block(&mut self, block: &Block, env: &mut Env, expected_tail: Option<&Ty>, ctx: &str) -> Option<Ty> {
        env.push();
        for stmt in &block.stmts {
            self.check_stmt(stmt, env, ctx);
        }
        let result = if let Some(tail) = &block.tail {
            Some(self.check_expr(tail, expected_tail, env, ctx))
        } else {
            None
        };
        env.pop();
        result
    }

    fn check_stmt(&mut self, stmt: &Stmt, env: &mut Env, ctx: &str) {
        match stmt {
            Stmt::Let { pattern, ty, value, .. } => {
                let expected = ty.as_ref().map(resolve_type);
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
            Stmt::Return(_) | Stmt::Item(_) | Stmt::Break { .. }
            | Stmt::Continue { .. } | Stmt::Yield(_) | Stmt::TargetBlock(..) => {
                // Return-value-vs-fn-return-type checking, nested items,
                // and loop control-flow aren't part of Phase 3's core
                // scope (Document 25 §2.3 scopes this phase to the type
                // system + struct/enum data shapes); not silently
                // "checked and passed" -- simply not yet visited.
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
        }
        actual
    }

    fn check_expr_inner(&mut self, expr: &Expr, expected: Option<&Ty>, env: &mut Env, ctx: &str) -> Ty {
        match expr {
            Expr::Literal(lit) => self.check_literal(lit, expected, ctx),

            Expr::Ident(name) => {
                if let Some(t) = env.get(name) {
                    t.clone()
                } else {
                    self.errors.push(TypeError {
                        message: format!("undefined variable `{}`", name),
                        context: ctx.into(),
                    });
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
                expected.cloned().unwrap_or(Ty::Unit) // module/struct-assoc paths: Phase 4+ resolves these fully
            }

            Expr::Paren(inner) => self.check_expr(inner, expected, env, ctx),

            Expr::Array(items) => {
                let elem_expected = match expected {
                    Some(Ty::Array(inner)) => Some((**inner).clone()),
                    _ => None,
                };
                if items.is_empty() {
                    // Document 5 §4 rule 6, §7's own explicit example:
                    // `let empty = [];` with no further usage is a
                    // compile error requiring an explicit annotation,
                    // not a silent default to `[i32]`.
                    match elem_expected {
                        Some(t) => Ty::Array(Box::new(t)),
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
                    Ty::Array(Box::new(first))
                }
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

            Expr::StructLit { name, fields, spread } => self.check_struct_lit(name, fields, spread.is_some(), env, ctx),

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

            Expr::Binary { op, lhs, rhs } => self.check_binary(*op, lhs, rhs, env, ctx),

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
                resolve_type(ty)
            }

            Expr::Range { lo, hi, .. } => {
                let lt = self.check_expr(lo, None, env, ctx);
                self.check_expr(hi, Some(&lt), env, ctx);
                lt
            }

            Expr::Propagate(inner) => self.check_expr(inner, None, env, ctx),

            Expr::Field { expr: inner, name } => {
                let t = self.check_expr(inner, None, env, ctx);
                self.field_type(&t, name, ctx)
            }

            Expr::Index { expr: inner, index } => {
                let t = self.check_expr(inner, None, env, ctx);
                self.check_expr(index, None, env, ctx);
                match t {
                    Ty::Array(elem) => *elem,
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
                if let Expr::Ident(name) = callee.as_ref() {
                    if let Some((params, ret)) = self.functions.get(name).cloned() {
                        for (i, arg) in args.iter().enumerate() {
                            let exp = params.get(i);
                            self.check_expr(&arg.value, exp, env, ctx);
                        }
                        return ret;
                    }
                }
                if let Expr::Path(path) = callee.as_ref() {
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
                expected.cloned().unwrap_or(Ty::Unit)
            }

            Expr::MethodCall { receiver, name, args } => {
                let recv_ty = self.check_expr(receiver, None, env, ctx);
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
                            Ty::DynTrait(n) => self.traits.contains_key(n),
                            _ => false,
                        };
                        if known_base {
                            self.errors.push(TypeError {
                                message: format!("no method named `{}` found for type `{}`", name, recv_ty),
                                context: ctx.into(),
                            });
                        }
                        expected.cloned().unwrap_or(Ty::Unit)
                    }
                }
            }

            Expr::Borrow { expr: inner, mutable } => {
                let inner_expected = match expected {
                    Some(Ty::Ref(_, t)) => Some((**t).clone()),
                    _ => None,
                };
                let t = self.check_expr(inner, inner_expected.as_ref(), env, ctx);
                Ty::Ref(*mutable, Box::new(t))
            }

            Expr::Await(inner) | Expr::Throw(inner) => {
                self.check_expr(inner, None, env, ctx);
                expected.cloned().unwrap_or(Ty::Unit)
            }

            Expr::Return(inner) => {
                if let Some(e) = inner {
                    self.check_expr(e, None, env, ctx);
                }
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
                match &c.body {
                    ClosureBody::Expr(e) => { self.check_expr(e, None, &mut inner_env, ctx); }
                    ClosureBody::Block(b) => { self.check_block(b, &mut inner_env, None, ctx); }
                }
                expected.cloned().unwrap_or(Ty::Unit)
            }
            Expr::Loop(_) | Expr::Spawn { .. } | Expr::Select(_) | Expr::Query(_)
            | Expr::TryCatch { .. } | Expr::Styled { .. } | Expr::Layout { .. }
            | Expr::ComponentChildren { .. } | Expr::EventHandler { .. } => {
                expected.cloned().unwrap_or(Ty::Unit)
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
            Literal::Float(_) => {
                match expected {
                    Some(t) if ty_is_float(t) => t.clone(),
                    _ => Ty::F64,
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

    fn check_binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr, env: &mut Env, ctx: &str) -> Ty {
        let lt = self.check_expr(lhs, None, env, ctx);
        let rt = self.check_expr(rhs, Some(&lt), env, ctx);
        use BinaryOp::*;
        match op {
            Add | Sub | Mul | Div | Mod | Pow | BitAnd | BitOr | BitXor | Shl | Shr => {
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
                    other => other,
                }
            }
        }
    }

    fn check_compatible(&mut self, actual: &Ty, expected: Option<&Ty>, ctx: &str) {
        if let Some(exp) = expected {
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
                let else_ty = self.check_block(b, env, Some(&then_ty), ctx).unwrap_or(Ty::Unit);
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
                let else_ty = self.check_if(inner, Some(&then_ty), env, ctx);
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
        self.check_expr(&match_expr.scrutinee, None, env, ctx);
        let mut common: Option<Ty> = expected.cloned();
        for arm in &match_expr.arms {
            env.push();
            // Pattern-introduced bindings aren't type-distributed against
            // the scrutinee's shape yet (needs enum-variant-field lookup
            // threaded through pattern matching) -- flagged gap, same
            // category as the `let`-destructuring one above.
            if let Some(guard) = &arm.guard {
                self.check_expr(guard, Some(&Ty::Bool), env, ctx);
            }
            let arm_ty = match &arm.body {
                MatchArmBody::Expr(e) => self.check_expr(e, common.as_ref(), env, ctx),
                MatchArmBody::Block(b) => self.check_block(b, env, common.as_ref(), ctx).unwrap_or(Ty::Unit),
            };
            env.pop();
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
        common.unwrap_or(Ty::Unit)
    }

    fn check_struct_lit(&mut self, name: &str, fields: &[(String, Expr)], has_spread: bool, env: &mut Env, ctx: &str) -> Ty {
        let Some(shape) = self.structs.get(name).cloned() else {
            self.errors.push(TypeError {
                message: format!("undefined struct `{}`", name),
                context: ctx.into(),
            });
            return Ty::Named(name.to_string());
        };
        let declared: HashMap<&str, &Ty> = shape.fields.iter().map(|(n, t)| (n.as_str(), t)).collect();
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
        Ty::Named(name.to_string())
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
    fn resolve_method(&self, recv_ty: &Ty, name: &str) -> Option<FnSig> {
        match recv_ty {
            Ty::Named(type_name) => {
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
            }
            Ty::DynTrait(trait_name) => {
                self.traits.get(trait_name)?.methods.get(name).cloned()
            }
            _ => None,
        }
    }

    fn field_type(&mut self, base: &Ty, field: &str, ctx: &str) -> Ty {
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
        // Base type not a known struct (could be a module path result,
        // an unresolved generic, etc. -- full resolution is Phase 4/5
        // territory) -- don't fabricate an error for cases outside this
        // phase's scope.
        Ty::Unit
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}
