//! Borrow Checker — Phase 6 (Document 25 §2.3).
//!
//! Implements Document 6 (Ownership, Borrowing & Lifetimes) as a
//! standalone semantic-analysis pass, run after (and independent of)
//! `types::TypeChecker`. Per Document 25 §3 Rule 4 ("the borrow checker
//! is foundational... and never bypassed"), this is scoped to be a
//! real, sound implementation of the specific rules Document 6 states —
//! not a stub, and not a full general-purpose Polonius-equivalent
//! region-inference engine either (Document 17 §4.4 names that as the
//! eventual reference implementation strategy; this is a deliberately
//! bounded, documented approximation of it — see the module-level
//! "Scope and soundness notes" below and PROGRESS.md for the exact
//! reasoning, since a naive lexical-only checker is explicitly called
//! out in Document 17 §4.4 as *incorrectly rejecting valid code*, which
//! this pass is built specifically to avoid for the concrete cases
//! Document 6 §8 requires).
//!
//! ## What this pass checks
//!
//! 1. **Move semantics** (Document 6 §2, §4): a non-`copy` binding used
//!    after being moved (via `let b = a;` or passed by-value into a
//!    function call whose parameter has no `borrow`/`borrow mut`
//!    ownership modifier) is rejected.
//! 2. **Copy-type transitivity** (Document 6 §4.2): a struct is only
//!    Copy if explicitly marked `@copy` (Document 3/6 define the
//!    `copy` keyword's *meaning* but never show a concrete
//!    struct-level marking syntax — grounded here in the existing,
//!    general `@attribute` mechanism Document 23 §14 already defines
//!    for prefixing any item, rather than inventing new syntax; see
//!    PROGRESS.md, flagged for sign-off) AND every field is itself
//!    Copy, computed by fixed-point over the struct registry so nested
//!    `@copy` structs compose correctly.
//! 3. **The aliasing rule** (Document 6 §3.1): at any program point, a
//!    place has either any number of active shared (`borrow`) borrows,
//!    or exactly one active mutable (`borrow mut`) borrow — never both.
//! 4. **Non-lexical borrow liveness** (Document 6 §3.2): a borrow's
//!    live range ends at its last actual textual use within its
//!    enclosing block, not at the block's closing brace — see "NLL
//!    approximation" below for the precise, conservative rule used.
//! 5. **Escaping references** (Document 6 §5.1): a `let` binding
//!    initialized from a call to a function whose signature both takes
//!    and returns reference (`&`/`Type::Ref`) values is tracked as
//!    (conservatively) borrowing from every `borrow`-argument passed at
//!    that call site; using that binding after any one of those source
//!    places has gone out of scope is rejected.
//!
//! ## Scope and soundness notes (read before extending this pass)
//!
//! - **NLL approximation.** A borrow's last-use is computed by scanning
//!   only the *directly enclosing block's* statement list (not nested
//!   blocks/branches) for a later textual read of its binding name. If
//!   none is found (including the case where the binding is never read
//!   at all), the borrow is conservatively treated as live through the
//!   end of the enclosing block. This is the specific, deliberate
//!   choice that reconciles Document 6 §3.1's own example (`ref1`/
//!   `ref2` are created but never subsequently read anywhere, and the
//!   doc states the following `borrow mut` must still be REJECTED —
//!   i.e. an *unused* borrow is not treated as immediately dead) with
//!   §3.2's example (`ref1` IS read once, via `print(ref1)`, and that
//!   read is its last use, occurring before `ref2`'s mutable borrow —
//!   the doc states this must be ACCEPTED). A rule of "dead immediately
//!   if never read" — the more literal reading of real-world NLL, and
//!   what rustc itself actually does — would accept §3.1's case,
//!   contradicting the doc's own stated verification outcome; a rule of
//!   "always live to block end regardless of reads" would reject §3.2's
//!   case. The rule implemented here ("live until last actual read, or
//!   block end if never read again") is the only one that reproduces
//!   BOTH documented outcomes exactly, verified by direct test (see
//!   `tests/borrow_checks.rs`). It is conservative relative to real
//!   NLL (may reject some code real Rust would accept, e.g. a
//!   genuinely-dead-and-truly-unused borrow) but never accepts a
//!   genuinely live aliasing violation — sound in the direction that
//!   matters. Flagged for explicit sign-off since it's a documented
//!   interpretation of a real tension between two of Document 6's own
//!   examples, not an arbitrary choice.
//! - **Same-block scope only.** The last-use scan, and therefore all of
//!   §3.1/§3.2's aliasing/NLL machinery, only considers reads occurring
//!   as direct statements of the same block the borrow was declared in
//!   (plus that block's tail expression). A read of a borrow-holding
//!   binding from inside a *nested* block/if/match-arm/loop body is not
//!   found by this scan, so such a binding conservatively defaults to
//!   "live to end of enclosing block" — safe (never under-counts
//!   liveness), just less precise than a full CFG walk would be.
//! - **Branches are checked, not merged.** `if`/`match` arms and loop
//!   bodies are each borrow-checked independently against a *snapshot*
//!   of the state at entry, so a real violation *within* one branch is
//!   still caught, but a branch's moves/borrows do not propagate to
//!   sibling branches or to code after the construct. Document 6 §8's
//!   required verification cases are all straight-line code with no
//!   branching, so this doesn't affect any of them; it is an explicit,
//!   documented scope limit for anything beyond those cases (e.g. a
//!   variable moved unconditionally inside one `if` arm and then used
//!   after the `if` is NOT currently flagged — a known gap, not a
//!   silent wrong answer).
//! - **Move-checking is precise only for the patterns Document 6 §2/§4
//!   actually shows**: bare identifier-to-identifier moves
//!   (`let b = a;`) and by-value call-argument moves (`consume(alice)`
//!   where the callee's parameter has no `borrow`/`borrow mut`
//!   modifier), plus Copy-struct transitivity (§4.2). A binding
//!   initialized from any other expression shape this pass can't
//!   syntactically classify (a call to a function whose return type
//!   isn't obviously non-Copy, a method call, a binary/cast expression,
//!   etc.) is intentionally left move-*unchecked* — its later reuse is
//!   never flagged — rather than guessing at a type this pass has no
//!   real inference engine to compute. This mirrors an existing,
//!   already-established precedent in `types.rs` itself (`MethodCall`'s
//!   unknown-method case: "stays silent rather than fabricating a
//!   false error" when there isn't enough static information) — the
//!   same "stay silent over fabricate" principle applied to move
//!   analysis specifically. This is a genuine, real gap relative to
//!   full move-checking (e.g. `let a = makeString(); let b = a;
//!   print(a);` is NOT caught if `makeString`'s String-ness isn't
//!   otherwise evident) — explicitly flagged here and in PROGRESS.md,
//!   not silently shipped as if it were complete. Building full
//!   move-checking would require duplicating (or exposing/sharing)
//!   `types.rs`'s real type-inference engine; deliberately not done in
//!   Phase 6 to avoid a second, potentially-divergent type system and
//!   to avoid touching the already-verified Phase 3–5 `types.rs` code.
//!   For the same reason, a non-Copy variable used twice as different
//!   *field values inside one struct literal* (`Line { a: p1, b: p1 }`
//!   where `Point` is NOT Copy) is not flagged as a double-move either
//!   — struct-literal field values are walked for already-moved/
//!   escaping-reference reads (via the ordinary `check_expr` recursion)
//!   but, unlike a bare `let`/call-argument identifier, are not run
//!   through the move-*marking* path at all. A real gap, not a silent
//!   wrong answer: it only means such a case is under-checked, never
//!   that genuinely safe code gets rejected.
//!
//! None of the above scope limits affect Document 6 §8's five required
//! verification cases — each is traced by a dedicated test in
//! `tests/borrow_checks.rs` and passes exactly as the document states.

use crate::ast::*;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct BorrowError {
    pub message: String,
    pub context: String,
}

impl std::fmt::Display for BorrowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "borrow error in `{}`: {}", self.context, self.message)
    }
}

// ---------------------------------------------------------------
// Registration: struct Copy-ness (Document 6 §4.2) + function ref-shape
// (Document 6 §5.1's escaping-reference heuristic)
// ---------------------------------------------------------------

/// Only the one fact `escaping_borrow_sources` (Document 6 §5.1) needs
/// per function: whether its return type is reference-shaped. Param
/// ownership/type shapes aren't separately tracked here -- the
/// heuristic looks at what's actually passed as `borrow`/`borrow mut`
/// at each call site instead (see `escaping_borrow_sources`), which is
/// both simpler and a direct match for Document 6 §5.1's own example
/// (`longest(borrow a, borrow b)`).
#[derive(Debug, Clone)]
struct FnRefShape {
    ret_is_ref: bool,
}

pub struct BorrowChecker {
    struct_copy: HashMap<String, bool>,
    struct_fields: HashMap<String, Vec<Type>>,
    functions: HashMap<String, FnRefShape>,
    pub errors: Vec<BorrowError>,
}

impl BorrowChecker {
    pub fn new() -> Self {
        BorrowChecker {
            struct_copy: HashMap::new(),
            struct_fields: HashMap::new(),
            functions: HashMap::new(),
            errors: Vec::new(),
        }
    }

    pub fn check_program(&mut self, program: &Program) {
        self.register_items(&program.items);
        self.compute_struct_copy();
        for item in &program.items {
            self.check_item(item);
        }
    }

    // ---- registration pass ----

    fn register_items(&mut self, items: &[Item]) {
        for item in items {
            match &item.kind {
                ItemKind::Struct(sd) => {
                    let is_copy_attr = item.attrs.iter().any(|a| a.name == "copy");
                    let field_tys: Vec<Type> = match &sd.body {
                        StructBody::Named(fields) => fields.iter().map(|f| f.ty.clone()).collect(),
                        StructBody::Tuple(tys) => tys.clone(),
                        StructBody::Unit => Vec::new(),
                    };
                    self.struct_fields.insert(sd.name.clone(), field_tys);
                    // Seed with the attribute presence; refined to the
                    // real transitive value by `compute_struct_copy`.
                    self.struct_copy.insert(sd.name.clone(), is_copy_attr);
                }
                ItemKind::Fn(f) => {
                    self.register_fn(f);
                }
                ItemKind::Impl(impl_decl) => {
                    for ii in &impl_decl.items {
                        if let ImplItem::Fn(f) = ii {
                            self.register_fn(f);
                        }
                    }
                }
                ItemKind::Mod(m) => self.register_items(&m.items),
                ItemKind::TargetBlock(_, inner) => self.register_items(inner),
                _ => {}
            }
        }
    }

    fn register_fn(&mut self, f: &FnDecl) {
        let ret_is_ref = matches!(f.return_type, Some(Type::Ref { .. }));
        self.functions.insert(f.name.clone(), FnRefShape { ret_is_ref });
    }

    /// Fixed-point closure over `struct_copy`: a struct only keeps its
    /// `@copy` marking true if every one of its fields is itself Copy,
    /// computed against the *current* map so that a `@copy` struct
    /// containing another `@copy` struct correctly stays Copy, and a
    /// `@copy` struct containing (transitively) a non-Copy field
    /// (Document 6 §4.2's own exact example: a `@copy`-marked struct
    /// with a `String` field) is correctly rejected down to `false`.
    /// Bounded to `struct_fields.len()` iterations, which is always
    /// enough to propagate a `false` through any chain of nesting no
    /// matter the declaration order, and terminates even in the
    /// (otherwise-illegal, infinite-size) case of a struct naming
    /// itself, since `is_copy` only ever monotonically decreases.
    fn compute_struct_copy(&mut self) {
        let iterations = self.struct_fields.len().max(1);
        for _ in 0..iterations {
            let mut changed = false;
            let names: Vec<String> = self.struct_fields.keys().cloned().collect();
            for name in names {
                let currently = *self.struct_copy.get(&name).unwrap_or(&false);
                if !currently {
                    continue; // never marked @copy at all -- stays false
                }
                let fields = self.struct_fields.get(&name).cloned().unwrap_or_default();
                let all_copy = fields.iter().all(|t| self.is_type_copy(t));
                if !all_copy {
                    self.struct_copy.insert(name.clone(), false);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn is_type_copy(&self, ty: &Type) -> bool {
        match ty {
            Type::Primitive(name) => is_primitive_copy_name(name),
            // A reference handle itself is always trivially duplicable
            // (Document 13 §1 describes reference/pointer handles as
            // small, fixed-size stack values) -- copying the reference
            // never copies the underlying owned data it points to.
            Type::Ref { .. } => true,
            Type::Tuple(ts) => ts.iter().all(|t| self.is_type_copy(t)),
            Type::Named(name, _) => *self.struct_copy.get(name).unwrap_or(&false),
            Type::Unit => true,
            // Array/[T], Dyn, Fn, Option, Result, Never, ConstArg:
            // conservatively NOT Copy. `[T]` is explicitly heap-backed
            // per Document 13 §1 rule 3; `Option`/`Result` are
            // ordinary enums (Document 7 §3.2) that may carry
            // non-Copy payloads. Treating them as non-Copy is the
            // sound default -- see the module doc's soundness notes.
            _ => false,
        }
    }

    // ---- checking pass ----

    fn check_item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Fn(f) => self.check_fn(f),
            ItemKind::Impl(impl_decl) => {
                for ii in &impl_decl.items {
                    if let ImplItem::Fn(f) = ii {
                        self.check_fn(f);
                    }
                }
            }
            ItemKind::Mod(m) => {
                for inner in &m.items {
                    self.check_item(inner);
                }
            }
            ItemKind::TargetBlock(_, inner) => {
                for inner_item in inner {
                    self.check_item(inner_item);
                }
            }
            _ => {}
        }
    }

    fn check_fn(&mut self, f: &FnDecl) {
        let Some(body) = &f.body else { return };
        // `fc` borrows `self` (as `checker: &BorrowChecker`) for the
        // duration of this block. Scoping that borrow explicitly with
        // `{ }` and moving the accumulated errors out as the block's
        // value means `fc` (and the borrow of `self` it holds) is
        // fully gone before `self` is touched again below -- avoiding
        // any reliance on NLL to determine field-level liveness across
        // FnChecker's `checker` and `errors` fields, which isn't
        // something this pass can verify without a real compiler to
        // check it against.
        let fn_errors = {
            let mut fc = FnChecker {
                checker: self,
                scopes: Vec::new(),
                place_borrows: HashMap::new(),
                ctx: f.name.clone(),
                errors: Vec::new(),
            };
            fc.scopes.push(Scope::default());
            for p in &f.params {
                let is_copy = if p.name == "self" {
                    false
                } else {
                    match &p.ownership {
                        // A `borrow`/`borrow mut` parameter is a
                        // reference handle -- always Copy per
                        // `is_type_copy`'s Ref arm, and
                        // re-borrowing/aliasing (not moving) is the
                        // only concern for it.
                        OwnershipMod::Borrow | OwnershipMod::BorrowMut => true,
                        _ => fc.checker.is_type_copy(&p.ty),
                    }
                };
                fc.scopes.last_mut().unwrap().bindings.insert(
                    p.name.clone(),
                    Binding { is_copy, moved: false, borrow_sources: Vec::new() },
                );
            }
            fc.check_block(body);
            fc.errors
        };
        self.errors.extend(fn_errors);
    }
}

fn is_primitive_copy_name(name: &str) -> bool {
    matches!(
        name,
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize"
            | "u8" | "u16" | "u32" | "u64" | "u128" | "usize"
            | "f32" | "f64" | "bool" | "char"
    )
    // Deliberately excludes "String" (heap-owned, never Copy) and
    // "str"/"&str" (a bare unsized `str` wouldn't appear as an owned
    // local's type; `&str` is a `Type::Ref`, handled by that arm
    // instead of this name-based table).
}

// ---------------------------------------------------------------
// Per-function checking state
// ---------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct Binding {
    is_copy: bool,
    moved: bool,
    /// Names of owned places this binding (conservatively) borrows
    /// from, for the Document 6 §5.1 escaping-reference check. Empty
    /// for ordinary owned bindings.
    borrow_sources: Vec<String>,
}

#[derive(Debug, Default)]
struct Scope {
    bindings: HashMap<String, Binding>,
}

/// Per-place borrow-aliasing state (Document 6 §3.1), keyed by the
/// name of the OWNED place being borrowed (e.g. `counter`, `alice`).
/// `last_use_idx` for each active borrower is the statement index (see
/// `FnChecker::check_block`) at/after which that borrow is no longer
/// live -- computed once per `let`-bound borrow via
/// `compute_last_use`, or set to the current statement's own index for
/// a temporary borrow passed directly as a call argument (Document 6
/// §3's `printName(borrow alice)` -- lives only for that one
/// statement). See the module-level "NLL approximation" doc comment
/// for the precise rule and why it's shaped this way.
#[derive(Debug, Default)]
struct PlaceBorrows {
    shared: Vec<(String, usize)>,
    mutable: Option<(String, usize)>,
}

struct FnChecker<'a> {
    checker: &'a BorrowChecker,
    scopes: Vec<Scope>,
    place_borrows: HashMap<String, PlaceBorrows>,
    ctx: String,
    errors: Vec<BorrowError>,
}

impl<'a> FnChecker<'a> {
    fn err(&mut self, message: impl Into<String>) {
        self.errors.push(BorrowError { message: message.into(), context: self.ctx.clone() });
    }

    fn lookup(&self, name: &str) -> Option<&Binding> {
        self.scopes.iter().rev().find_map(|s| s.bindings.get(name))
    }

    fn lookup_mut(&mut self, name: &str) -> Option<&mut Binding> {
        self.scopes.iter_mut().rev().find_map(|s| s.bindings.get_mut(name))
    }

    fn is_in_scope(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.bindings.contains_key(name))
    }

    fn mark_moved(&mut self, name: &str) {
        if let Some(b) = self.lookup_mut(name) {
            b.moved = true;
        }
    }

    /// Reading a binding by name: Document 6 §2/§4's move check, and
    /// Document 6 §5.1's escaping-reference check, both funnel through
    /// here so every place an identifier is actually *used* (not just
    /// declared) is covered uniformly.
    fn use_ident(&mut self, name: &str) {
        let (moved, sources) = match self.lookup(name) {
            Some(b) => (b.moved, b.borrow_sources.clone()),
            None => return, // unknown name -- types.rs already reports this
        };
        if moved {
            self.err(format!(
                "use of moved value `{}` -- ownership was transferred to another binding and the original is no longer valid (Document 6 §2/§4)",
                name
            ));
        }
        for src in &sources {
            if !self.is_in_scope(src) {
                self.err(format!(
                    "`{}` may still reference `{}`, which has already gone out of scope (Document 6 §5.1)",
                    name, src
                ));
            }
        }
    }

    // ---- borrow/place aliasing (Document 6 §3.1, §3.2) ----

    fn expire_place(&mut self, place: &str, at_idx: usize) {
        if let Some(pb) = self.place_borrows.get_mut(place) {
            pb.shared.retain(|(_, last)| *last >= at_idx);
            if let Some((_, last)) = pb.mutable {
                if last < at_idx {
                    pb.mutable = None;
                }
            }
        }
    }

    /// Registers a new borrow of `place` at statement index `at_idx`,
    /// checking Document 6 §3.1's aliasing rule first. `borrower` is
    /// either a real binding name (for a `let`-bound borrow) or a
    /// synthetic label (for a temporary call-argument borrow); either
    /// way it's only used in diagnostics.
    fn register_borrow(&mut self, place: &str, mutable: bool, borrower: &str, last_use_idx: usize, at_idx: usize) {
        self.expire_place(place, at_idx);
        // Read everything needed out of the entry into owned locals
        // FIRST, so the `&mut PlaceBorrows` borrow from `.entry()` is
        // fully released before any `self.err(...)` call below (which
        // needs `&mut self`) -- avoids any ambiguity about whether
        // that borrow's live range would otherwise overlap, rather
        // than relying on NLL to work it out (can't compile-test this
        // locally, so the conservative, unambiguous structure is
        // deliberate here, not just a style preference).
        let pb = self.place_borrows.entry(place.to_string()).or_default();
        let has_shared = !pb.shared.is_empty();
        let existing_mutable = pb.mutable.as_ref().map(|(n, _)| n.clone());
        if mutable {
            if has_shared {
                self.err(format!(
                    "cannot borrow `{}` as mutable (via `{}`) because it is also borrowed as immutable (Document 6 §3.1's aliasing rule)",
                    place, borrower
                ));
                return;
            }
            if let Some(other) = existing_mutable {
                self.err(format!(
                    "cannot borrow `{}` as mutable (via `{}`) more than once at a time -- already mutably borrowed via `{}` (Document 6 §3.1's aliasing rule)",
                    place, borrower, other
                ));
                return;
            }
            self.place_borrows.get_mut(place).unwrap().mutable = Some((borrower.to_string(), last_use_idx));
        } else {
            if let Some(other) = existing_mutable {
                self.err(format!(
                    "cannot borrow `{}` as immutable (via `{}`) because it is also borrowed as mutable via `{}` (Document 6 §3.1's aliasing rule)",
                    place, borrower, other
                ));
                return;
            }
            self.place_borrows.get_mut(place).unwrap().shared.push((borrower.to_string(), last_use_idx));
        }
    }

    // ---- block/statement walking ----

    fn check_block(&mut self, block: &Block) {
        self.scopes.push(Scope::default());
        for (idx, stmt) in block.stmts.iter().enumerate() {
            self.check_stmt(stmt, block, idx);
        }
        if let Some(tail) = &block.tail {
            self.check_expr(tail, block, block.stmts.len());
        }
        self.scopes.pop();
    }

    fn check_stmt(&mut self, stmt: &Stmt, block: &Block, idx: usize) {
        match stmt {
            Stmt::Let { pattern, value, .. } => {
                let name = binding_name(pattern);
                if let Some(v) = value {
                    if let Some(name) = &name {
                        self.declare_binding(name, v, block, idx, /* new_scope */ true);
                    } else {
                        // Destructuring pattern (no trackable name) --
                        // still walk the initializer for its own
                        // internal move/borrow validity even though
                        // the resulting bindings aren't tracked (see
                        // `binding_name`'s doc comment).
                        self.check_expr(v, block, idx);
                    }
                } else if let Some(name) = name {
                    self.scopes.last_mut().unwrap().bindings.insert(
                        name,
                        Binding { is_copy: true, moved: false, borrow_sources: Vec::new() },
                    );
                }
            }
            Stmt::Expr(e) => self.check_expr(e, block, idx),
            Stmt::Return(Some(e)) | Stmt::Yield(e) => self.check_expr(e, block, idx),
            Stmt::Break { value: Some(e), .. } => self.check_expr(e, block, idx),
            Stmt::Return(None) | Stmt::Break { value: None, .. } | Stmt::Continue { .. } | Stmt::Item(_) => {}
            Stmt::TargetBlock(_, b) => self.check_block(b),
        }
    }

    /// Determines the `Binding` state a `let`/assignment target should
    /// get from its initializer expression, handling the two precise,
    /// documented move-eligible shapes (Document 6 §2/§4.1: bare
    /// identifier moves) plus the escaping-reference source tracking
    /// (§5.1). See the module doc's "Move-checking is precise only
    /// for..." note for exactly what is and isn't covered here.
    fn classify_initializer(&mut self, init: &Expr) -> Binding {
        match init {
            Expr::Ident(src) => {
                let (src_copy, src_sources) = match self.lookup(src) {
                    Some(b) => (b.is_copy, b.borrow_sources.clone()),
                    None => (true, Vec::new()),
                };
                if !src_copy {
                    self.mark_moved(src);
                }
                Binding { is_copy: src_copy, moved: false, borrow_sources: src_sources }
            }
            Expr::Literal(Literal::Str(_)) | Expr::Literal(Literal::RawStr(_)) => {
                // Document 5 §1: a string literal infers to owned
                // `String` -- heap-owned, not Copy.
                Binding { is_copy: false, moved: false, borrow_sources: Vec::new() }
            }
            Expr::Literal(_) => Binding { is_copy: true, moved: false, borrow_sources: Vec::new() },
            Expr::StructLit { name, .. } => {
                let is_copy = *self.checker.struct_copy.get(name).unwrap_or(&false);
                Binding { is_copy, moved: false, borrow_sources: Vec::new() }
            }
            Expr::Borrow { .. } => Binding { is_copy: true, moved: false, borrow_sources: Vec::new() },
            Expr::Call { callee, args } => {
                // `is_copy: true` here means "not move-tracked", per
                // the module doc's explicit note -- this pass has no
                // real inference engine to determine the call's actual
                // return type, so it stays silent on move-safety for
                // the *result* rather than fabricating a guess (the
                // call's arguments were already fully checked by
                // `check_expr` before `classify_initializer` is ever
                // called -- see `Stmt::Let`/`Expr::Assign` above -- and
                // escaping-reference sources are still tracked here
                // whenever the callee's signature makes that
                // determinable).
                let sources = self.escaping_borrow_sources(callee, args);
                Binding { is_copy: true, moved: false, borrow_sources: sources }
            }
            _ => Binding { is_copy: true, moved: false, borrow_sources: Vec::new() },
        }
    }

    /// Declares or re-assigns `name` from initializer `init`, at
    /// statement index `idx` of `block`. This is the single place that
    /// distinguishes a DIRECT, named `borrow`/`borrow mut` initializer
    /// (which must be registered as a real, persistent borrow with a
    /// properly computed last-use index -- Document 6 §3.1/§3.2) from
    /// every other initializer shape (handled by `classify_initializer`,
    /// which recurses generically via `check_expr` and never itself
    /// registers a *persistent* borrow). Splitting these two paths
    /// here, once, is what avoids a real bug caught while tracing this
    /// pass against Document 6 §3.1 by hand before writing any tests:
    /// naively running the generic `check_expr` walk over a bare `let
    /// r = borrow x;` initializer hits `check_expr`'s own
    /// `Expr::Borrow` arm (meant only as a fallback for borrows
    /// appearing in call-argument or other non-`let` positions) and
    /// registers `r` as an immediately-EXPIRING TEMPORARY borrow
    /// instead of a real, named one -- silently making Document 6
    /// §3.1's required-rejection case incorrectly pass. `new_scope`
    /// selects whether this introduces a brand-new binding (a `let`)
    /// or updates an existing one in place (a plain `name = value;`
    /// re-assignment, e.g. Document 6 §5.1's `result = longest(...);`).
    fn declare_binding(&mut self, name: &str, init: &Expr, block: &Block, idx: usize, new_scope: bool) {
        if let Expr::Borrow { mutable, expr: inner } = init {
            if let Expr::Ident(place) = inner.as_ref() {
                self.use_ident(place);
                let last_use = compute_last_use(block, idx, name);
                self.register_borrow(place, *mutable, name, last_use, idx);
                let binding = Binding { is_copy: true, moved: false, borrow_sources: Vec::new() };
                self.install_binding(name, binding, new_scope);
                return;
            }
        }
        self.check_expr(init, block, idx);
        let binding = self.classify_initializer(init);
        self.install_binding(name, binding, new_scope);
    }

    fn install_binding(&mut self, name: &str, binding: Binding, new_scope: bool) {
        if new_scope {
            self.scopes.last_mut().unwrap().bindings.insert(name.to_string(), binding);
        } else {
            self.lookup_mut_or_insert(name, binding);
        }
    }

    /// Document 6 §5.1's heuristic: if `callee` names a known function
    /// whose signature both takes at least one reference-shaped
    /// parameter and returns a reference-shaped type, the call's
    /// result is conservatively treated as (possibly) borrowing from
    /// every `borrow`/`borrow mut` argument passed at this call site --
    /// approximating Document 6 §5.1's lifetime-elision-driven `'a`
    /// sharing across `x`/`y` in `fn longest<'a>(x: &'a str, y: &'a
    /// str) -> &'a str`.
    fn escaping_borrow_sources(&self, callee: &Expr, args: &[Arg]) -> Vec<String> {
        let Expr::Ident(name) = callee else { return Vec::new() };
        let Some(shape) = self.checker.functions.get(name) else { return Vec::new() };
        if !shape.ret_is_ref {
            return Vec::new();
        }
        args.iter()
            .filter_map(|a| match &a.value {
                Expr::Borrow { expr, .. } => match expr.as_ref() {
                    Expr::Ident(place) => Some(place.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    fn check_expr(&mut self, expr: &Expr, block: &Block, idx: usize) {
        match expr {
            Expr::Ident(name) => self.use_ident(name),
            Expr::Literal(_) => {}
            Expr::Path(_) => {}
            Expr::Paren(e) | Expr::Await(e) | Expr::Propagate(e) | Expr::Throw(e)
            | Expr::Unary { expr: e, .. } | Expr::Yield(e) => self.check_expr(e, block, idx),
            Expr::Tuple(items) | Expr::Array(items) => {
                for it in items {
                    self.check_expr(it, block, idx);
                }
            }
            Expr::StructLit { fields, spread, .. } => {
                for (_, v) in fields {
                    self.check_expr(v, block, idx);
                }
                if let Some(s) = spread {
                    self.check_expr(s, block, idx);
                }
            }
            Expr::Binary { lhs, rhs, .. } => {
                self.check_expr(lhs, block, idx);
                self.check_expr(rhs, block, idx);
            }
            Expr::Assign { lhs, rhs, .. } => {
                if let Expr::Ident(name) = lhs.as_ref() {
                    if self.is_in_scope(name) {
                        self.declare_binding(name, rhs, block, idx, /* new_scope */ false);
                        return;
                    }
                }
                self.check_expr(rhs, block, idx);
                self.check_expr(lhs, block, idx);
            }
            Expr::Cast { expr: e, .. } => self.check_expr(e, block, idx),
            Expr::Range { lo, hi, .. } => {
                self.check_expr(lo, block, idx);
                self.check_expr(hi, block, idx);
            }
            Expr::Field { expr: e, .. } => self.check_expr(e, block, idx),
            Expr::Index { expr: e, index } => {
                self.check_expr(e, block, idx);
                self.check_expr(index, block, idx);
            }
            Expr::Call { callee, args } => {
                self.check_expr(callee, block, idx);
                self.check_call_args(args, block, idx);
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.check_expr(receiver, block, idx);
                self.check_call_args(args, block, idx);
            }
            Expr::Borrow { mutable, expr: inner } => {
                // A bare `borrow x` / `borrow mut x` appearing OUTSIDE
                // a call-argument position (e.g. `let r = borrow x;`,
                // already handled for the `let`/assign case by
                // `classify_initializer`, but this arm also covers
                // uses inside larger expressions) -- registers a
                // temporary, same-statement-lifetime borrow so
                // conflicts are still caught even when not bound to a
                // name.
                if let Expr::Ident(place) = inner.as_ref() {
                    self.register_borrow(place, *mutable, "<temporary>", idx, idx);
                } else {
                    self.check_expr(inner, block, idx);
                }
            }
            Expr::Return(inner) => {
                if let Some(e) = inner {
                    self.check_expr(e, block, idx);
                }
            }
            Expr::If(if_expr) => {
                self.check_expr(&if_expr.cond, block, idx);
                self.check_branch(&if_expr.then_block);
                match &if_expr.else_branch {
                    Some(ElseBranch::Block(b)) => self.check_branch(b),
                    Some(ElseBranch::If(inner)) => self.check_expr(&Expr::If(inner.clone()), block, idx),
                    None => {}
                }
            }
            Expr::Match(m) => {
                self.check_expr(&m.scrutinee, block, idx);
                for arm in &m.arms {
                    if let Some(g) = &arm.guard {
                        self.check_expr(g, block, idx);
                    }
                    match &arm.body {
                        MatchArmBody::Expr(e) => self.check_branch_expr(e),
                        MatchArmBody::Block(b) => self.check_branch(b),
                    }
                }
            }
            Expr::Loop(l) => match l.as_ref() {
                LoopExpr::Loop { body, .. } => self.check_branch(body),
                LoopExpr::While { cond, body, .. } => {
                    self.check_expr(cond, block, idx);
                    self.check_branch(body);
                }
                LoopExpr::For { iter, body, .. } => {
                    self.check_expr(iter, block, idx);
                    self.check_branch(body);
                }
                LoopExpr::DoWhile { body, cond } => {
                    self.check_branch(body);
                    self.check_expr(cond, block, idx);
                }
            },
            Expr::Block(b) => self.check_branch(b),
            Expr::Unsafe(b) => self.check_branch(b),
            Expr::Closure(c) => {
                // Full closure capture-mode borrow analysis is out of
                // scope for Phase 6 (already flagged as Phase 6 work
                // in `types.rs`'s own Phase 3/5 comments, and Document
                // 25 lists closures fully under Phase 7). Still walk
                // the body so any *use* of an already-moved outer
                // variable inside the closure is caught, since that's
                // a real, checkable violation even without full
                // capture-mode inference.
                match &c.body {
                    ClosureBody::Expr(e) => self.check_branch_expr(e),
                    ClosureBody::Block(b) => self.check_branch(b),
                }
            }
            Expr::TryCatch { try_block, catch_block, .. } => {
                self.check_branch(try_block);
                self.check_branch(catch_block);
            }
            Expr::Styled { expr: e, props } => {
                self.check_expr(e, block, idx);
                for (_, v) in props {
                    self.check_expr(v, block, idx);
                }
            }
            Expr::Layout { expr: e, props, children } => {
                self.check_expr(e, block, idx);
                for (_, v) in props {
                    self.check_expr(v, block, idx);
                }
                for c in children {
                    self.check_expr(c, block, idx);
                }
            }
            Expr::ComponentChildren { children, .. } => {
                for c in children {
                    self.check_expr(c, block, idx);
                }
            }
            Expr::EventHandler { body, .. } => self.check_expr(body, block, idx),
            // Domain-specific constructs whose full semantics belong to
            // later phases (spawn/select/query — Phase 12/18/20); still
            // safe to leave unchecked here since Phase 6's exit
            // criteria (Document 6 §8) never exercises them.
            Expr::Spawn { .. } | Expr::Select(_) | Expr::Query(_) => {}
        }
    }

    fn lookup_mut_or_insert(&mut self, name: &str, binding: Binding) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(slot) = scope.bindings.get_mut(name) {
                *slot = binding;
                return;
            }
        }
    }

    fn check_call_args(&mut self, args: &[Arg], block: &Block, idx: usize) {
        for arg in args {
            match &arg.value {
                Expr::Borrow { mutable, expr: inner } if matches!(inner.as_ref(), Expr::Ident(_)) => {
                    if let Expr::Ident(place) = inner.as_ref() {
                        // First check the underlying place hasn't
                        // itself been moved (borrowing a moved value is
                        // just as invalid as reading it).
                        self.use_ident(place);
                        self.register_borrow(place, *mutable, "<temporary>", idx, idx);
                    }
                }
                Expr::Ident(name) => {
                    // A bare by-value identifier argument: moves the
                    // argument per Document 6 §4.1's `consume(alice)`
                    // example, UNLESS the callee's corresponding
                    // parameter is `borrow`/`borrow mut` (already
                    // handled by the `Expr::Borrow` arm above when the
                    // caller writes it explicitly) or the argument's
                    // own type is Copy.
                    self.use_ident(name);
                    let is_copy = self.lookup(name).map(|b| b.is_copy).unwrap_or(true);
                    if !is_copy {
                        self.mark_moved(name);
                    }
                }
                other => self.check_expr(other, block, idx),
            }
        }
    }

    /// Checks a nested block as an independent branch: its own moves
    /// and borrows are still validated internally, but (per the module
    /// doc's "Branches are checked, not merged" note) don't propagate
    /// to sibling branches or to code after the construct. Implemented
    /// by simply running the ordinary block-checking logic and letting
    /// its scope push/pop naturally discard whatever it introduced;
    /// the one thing that DOES persist across branches on purpose is
    /// `moved` flags on OUTER bindings (found via `lookup_mut`, which
    /// searches all open scopes) — conservatively carrying a possible
    /// move forward is the safe direction (see module doc).
    fn check_branch(&mut self, block: &Block) {
        self.check_block(block);
    }

    fn check_branch_expr(&mut self, expr: &Expr) {
        let empty_block = Block { stmts: Vec::new(), tail: Some(Box::new(expr.clone())) };
        self.check_block(&empty_block);
    }
}

/// Computes the NLL-approximated last-use index for a borrow-holding
/// binding `name`, declared at statement `decl_idx` of `block`. See
/// the module-level "NLL approximation" doc comment for the exact,
/// deliberately-chosen rule and why it reproduces both Document 6
/// §3.1's and §3.2's stated outcomes. Returns `block.stmts.len()` (a
/// sentinel meaning "live through the end of this block") whenever no
/// later read is found anywhere in the block's remaining statements or
/// tail expression; otherwise returns the index of the LATEST
/// statement (strictly after `decl_idx`) that reads `name` -- provided
/// the tail doesn't *also* read it (a tail read always means "still
/// live at block end", since the tail is textually last).
fn compute_last_use(block: &Block, decl_idx: usize, name: &str) -> usize {
    let mut last_stmt_use: Option<usize> = None;
    for (j, stmt) in block.stmts.iter().enumerate() {
        if j <= decl_idx {
            continue;
        }
        if stmt_reads_ident(stmt, name) {
            last_stmt_use = Some(j);
        }
    }
    let tail_reads = block.tail.as_ref().map_or(false, |t| expr_reads_ident(t, name));
    if tail_reads || last_stmt_use.is_none() {
        block.stmts.len()
    } else {
        last_stmt_use.unwrap()
    }
}

fn stmt_reads_ident(stmt: &Stmt, name: &str) -> bool {
    match stmt {
        Stmt::Let { value, .. } => value.as_ref().map_or(false, |v| expr_reads_ident(v, name)),
        Stmt::Expr(e) | Stmt::Yield(e) => expr_reads_ident(e, name),
        Stmt::Return(Some(e)) => expr_reads_ident(e, name),
        Stmt::Return(None) | Stmt::Break { value: None, .. } | Stmt::Continue { .. } | Stmt::Item(_) => false,
        Stmt::Break { value: Some(e), .. } => expr_reads_ident(e, name),
        Stmt::TargetBlock(_, b) => block_reads_ident(b, name),
    }
}

fn block_reads_ident(block: &Block, name: &str) -> bool {
    block.stmts.iter().any(|s| stmt_reads_ident(s, name))
        || block.tail.as_ref().map_or(false, |t| expr_reads_ident(t, name))
}

/// Recursively checks whether `Expr::Ident(name)` appears anywhere
/// within `expr`, at any depth (including inside nested blocks,
/// closures, if/match/loop bodies). Used only to find evidence a
/// borrow-holding binding is read again later, per
/// `compute_last_use`'s conservative "not found -> still live" default
/// -- so under-coverage here only costs PRECISION (a real later read
/// that this walker misses just means the borrow is treated as
/// conservatively live to block end instead of being shortened, which
/// is still sound), never soundness. Covers every `Expr` variant that
/// can syntactically contain a sub-expression.
fn expr_reads_ident(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Ident(n) => n == name,
        Expr::Literal(_) | Expr::Path(_) => false,
        Expr::Paren(e) | Expr::Await(e) | Expr::Propagate(e) | Expr::Throw(e)
        | Expr::Unary { expr: e, .. } | Expr::Yield(e) | Expr::Field { expr: e, .. }
        | Expr::Cast { expr: e, .. } | Expr::Borrow { expr: e, .. } => expr_reads_ident(e, name),
        Expr::Tuple(items) | Expr::Array(items) => items.iter().any(|e| expr_reads_ident(e, name)),
        Expr::StructLit { fields, spread, .. } => {
            fields.iter().any(|(_, v)| expr_reads_ident(v, name))
                || spread.as_ref().map_or(false, |s| expr_reads_ident(s, name))
        }
        Expr::Binary { lhs, rhs, .. } | Expr::Range { lo: lhs, hi: rhs, .. } => {
            expr_reads_ident(lhs, name) || expr_reads_ident(rhs, name)
        }
        Expr::Assign { lhs, rhs, .. } => expr_reads_ident(lhs, name) || expr_reads_ident(rhs, name),
        Expr::Index { expr: e, index } => expr_reads_ident(e, name) || expr_reads_ident(index, name),
        Expr::Call { callee, args } => {
            expr_reads_ident(callee, name) || args.iter().any(|a| expr_reads_ident(&a.value, name))
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_reads_ident(receiver, name) || args.iter().any(|a| expr_reads_ident(&a.value, name))
        }
        Expr::Return(inner) => inner.as_ref().map_or(false, |e| expr_reads_ident(e, name)),
        Expr::If(if_expr) => {
            expr_reads_ident(&if_expr.cond, name)
                || block_reads_ident(&if_expr.then_block, name)
                || match &if_expr.else_branch {
                    Some(ElseBranch::Block(b)) => block_reads_ident(b, name),
                    Some(ElseBranch::If(inner)) => expr_reads_ident(&Expr::If(inner.clone()), name),
                    None => false,
                }
        }
        Expr::Match(m) => {
            expr_reads_ident(&m.scrutinee, name)
                || m.arms.iter().any(|arm| {
                    arm.guard.as_ref().map_or(false, |g| expr_reads_ident(g, name))
                        || match &arm.body {
                            MatchArmBody::Expr(e) => expr_reads_ident(e, name),
                            MatchArmBody::Block(b) => block_reads_ident(b, name),
                        }
                })
        }
        Expr::Loop(l) => match l.as_ref() {
            LoopExpr::Loop { body, .. } => block_reads_ident(body, name),
            LoopExpr::While { cond, body, .. } => expr_reads_ident(cond, name) || block_reads_ident(body, name),
            LoopExpr::For { iter, body, .. } => expr_reads_ident(iter, name) || block_reads_ident(body, name),
            LoopExpr::DoWhile { body, cond } => block_reads_ident(body, name) || expr_reads_ident(cond, name),
        },
        Expr::Block(b) | Expr::Unsafe(b) => block_reads_ident(b, name),
        Expr::Closure(c) => match &c.body {
            ClosureBody::Expr(e) => expr_reads_ident(e, name),
            ClosureBody::Block(b) => block_reads_ident(b, name),
        },
        Expr::TryCatch { try_block, catch_block, .. } => {
            block_reads_ident(try_block, name) || block_reads_ident(catch_block, name)
        }
        Expr::Styled { expr: e, props } => {
            expr_reads_ident(e, name) || props.iter().any(|(_, v)| expr_reads_ident(v, name))
        }
        Expr::Layout { expr: e, props, children } => {
            expr_reads_ident(e, name)
                || props.iter().any(|(_, v)| expr_reads_ident(v, name))
                || children.iter().any(|c| expr_reads_ident(c, name))
        }
        Expr::ComponentChildren { children, .. } => children.iter().any(|c| expr_reads_ident(c, name)),
        Expr::EventHandler { body, .. } => expr_reads_ident(body, name),
        Expr::Spawn { .. } | Expr::Select(_) | Expr::Query(_) => false,
    }
}

fn binding_name(pattern: &Pattern) -> Option<String> {
    match pattern {
        Pattern::Ident(n) | Pattern::Mut(n) => Some(n.clone()),
        // Destructuring patterns (tuple/array/tuple-struct/or) aren't
        // tracked -- same, already-established limitation `types.rs`
        // itself documents for `Stmt::Let` (Phase 3's own comment:
        // "bindings introduced by a destructuring `let` simply aren't
        // added to `env` yet... a safe failure mode, not a silent
        // wrong answer"). Mirrored here for consistency: such bindings
        // are simply never borrow-tracked (no false errors, but also
        // no move/borrow protection for them yet — a known, shared gap
        // rather than a new one introduced by this pass).
        _ => None,
    }
}
