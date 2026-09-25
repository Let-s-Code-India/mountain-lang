//! Match Exhaustiveness Checker — Phase 8 (Document 25 §2.3).
//!
//! Implements Document 9 §2.5's exhaustiveness rule ("the compiler
//! rejects a `match` block that does not cover every possible variant
//! of an enum unless a wildcard `_` arm is present") via a real
//! pattern-matrix usefulness algorithm — the approach Document 17
//! §4.5 names explicitly ("the standard approach from Maranget's
//! 'Warnings for Pattern Matching' paper... a well-established,
//! correctly-provable algorithm, not a heuristic"). This is not a
//! "collect the top-level variant names and check the set" shortcut:
//! it decomposes nested patterns (an enum variant containing a tuple
//! containing another enum, etc.) exactly as the algorithm specifies,
//! via constructor specialization and default-matrix reduction.
//!
//! Called from `types::TypeChecker::check_match` once the scrutinee's
//! type is known (exhaustiveness is inherently type-directed — knowing
//! *which* enum, or that a type is `bool`, is what "complete
//! constructor set" means), so this module is a one-way dependency on
//! `types::Ty` rather than a fully independent pass the way
//! `borrow.rs` deliberately is; unlike move/borrow analysis,
//! exhaustiveness genuinely cannot be checked without type information
//! at all, so duplicating a shadow type system here (the concern that
//! kept `borrow.rs` independent) doesn't apply the same way — there is
//! nothing to duplicate; it consumes `Ty` read-only.
//!
//! ## Scope
//!
//! Real, general algorithm for:
//! - **Enums** (both user-declared, via the `enums` table passed in,
//!   and the standard library's `Option`/`Result`, which are ordinary
//!   enums per Document 7 §3.2 but are dedicated `Ty::OptionTy`/
//!   `Ty::ResultTy` variants rather than entries in the user `enums`
//!   table — special-cased here as synthetic two-variant enums
//!   `Some`/`None` and `Ok`/`Err`, since that's what they structurally
//!   are).
//! - **`bool`** as a genuine two-constructor finite domain (`true`/
//!   `false`) — exhaustive without a wildcard if and only if both
//!   appear, e.g. `match b { true => .., false => .. }` needs no `_`.
//! - **Tuples**, recursively over their element types.
//! - **Or-patterns**, both a `MatchArm`'s top-level `|`-separated
//!   pattern list (`Vec<Pattern>` — Document 9 §2.1's
//!   `HttpMethod::Put | HttpMethod::Delete => ..`, which this parser
//!   stores as multiple patterns per arm, not a nested `Pattern::Or`)
//!   and a nested `Pattern::Or` wherever it appears, both expanded into
//!   separate matrix rows (the standard treatment).
//! - **Guards never contribute to coverage** (Document 9 §2.3): an
//!   arm's pattern(s) are excluded from the matrix entirely when the
//!   arm has a guard, since the compiler cannot know at compile time
//!   whether the guard will hold.
//!
//! Every other type (integers, floats, strings, chars, structs,
//! arrays/slices) is treated as an **infinite/unenumerable domain**:
//! literal patterns over them are real constructors for matrix
//! purposes (so `match n { 0 => .., 1 => .. }` correctly reports
//! non-exhaustive), but the "is this constructor set complete"
//! question always answers "no" for them, so only a wildcard/binding
//! pattern (or, for a struct/array, a destructuring pattern whose own
//! sub-parts are themselves all-covering) can make such a match
//! exhaustive — matching Document 9 §2.2's own `match statusCode {
//! 200=>.., 404=>.., 500=>.., _=>..}` example, which needs the `_`
//! precisely because `i32`'s domain is never "complete" no matter how
//! many literals are listed. Struct/array patterns themselves are
//! treated as an opaque single constructor (their own sub-patterns to
//! decompose isn't attempted) — Document 9's own `match` examples never
//! exhaustively destructure a struct or array without a final wildcard
//! arm, so this is a safe, flagged scope cut rather than a silent gap:
//! it can never falsely accept a non-exhaustive match, only potentially
//! ask for a wildcard where a fully general algorithm might not need
//! one (over-strict, not unsound — see `expand_pattern`'s `Array`
//! arm).

use crate::ast::{Literal, MatchArm, Pattern};
use crate::types::Ty;
use std::collections::HashMap;

/// Enum variant registry this module needs: enum name -> list of
/// (variant name, field types), exactly `types::EnumShape.variants`'
/// shape but passed by reference so this module doesn't need to know
/// about `EnumShape` itself (kept to the one thing actually used).
pub type EnumTable<'a> = HashMap<String, &'a [(String, Vec<Ty>)]>;

/// Tuple-struct registry: struct name -> field types, in declared
/// order (`types::StructShape.fields`' types only, filtered to
/// `is_tuple` structs by the caller -- a named-field struct has no
/// corresponding `Pattern` variant to match against at all, so it
/// would never be looked up here anyway, but the caller filters it out
/// explicitly rather than relying on that). Owned (`Vec<Ty>`), not
/// borrowed like `EnumTable`: a struct's field types have to be
/// extracted from `StructShape.fields`' `(name, Ty)` pairs into a
/// types-only list, which is a real allocation regardless -- there's
/// no pre-existing `Vec<Ty>`-shaped field to borrow a slice from the
/// way `EnumShape.variants` already matches `EnumTable`'s shape
/// exactly. (A first draft tried `&'a [Ty]` here anyway, which would
/// have borrowed from a temporary `Vec` dropped at the end of the
/// `.map()` closure that builds the table -- a dangling reference,
/// caught before it was ever compiled, not after.)
pub type StructTable = HashMap<String, Vec<Ty>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Ctor {
    /// A named variant (user enum, or synthetic `Some`/`None`/`Ok`/`Err`).
    Variant(String),
    Bool(bool),
    Tuple,
    /// A literal or otherwise-opaque constructor, identified by its
    /// normalized source text. Two `Opaque` patterns are the "same
    /// constructor" only if their text matches exactly; the
    /// constructor SET they belong to is never treated as complete
    /// (see the module doc), so `Opaque` never drives the "is this
    /// signature complete" branch of the algorithm, only literal-vs-
    /// literal specialization.
    Opaque(String),
}

#[derive(Debug, Clone)]
enum CtorPat {
    Wildcard,
    Ctor(Ctor, Vec<CtorPat>),
}

struct Ctx<'a> {
    enums: &'a EnumTable<'a>,
    structs: &'a StructTable,
}

impl<'a> Ctx<'a> {
    /// Resolves `ty` to its enum-like variant table, if it has one:
    /// user-declared enums via `self.enums`, plus the synthetic
    /// `Option`/`Result` cases. Returns `None` for anything else
    /// (including `bool`, handled separately since it isn't
    /// "enum-shaped" — its constructors carry no field types at all).
    fn variants_of(&self, ty: &Ty) -> Option<Vec<(String, Vec<Ty>)>> {
        match ty {
            Ty::OptionTy(inner) => Some(vec![
                ("Some".to_string(), vec![(**inner).clone()]),
                ("None".to_string(), vec![]),
            ]),
            Ty::ResultTy(ok, err) => Some(vec![
                ("Ok".to_string(), vec![(**ok).clone()]),
                ("Err".to_string(), vec![(**err).clone()]),
            ]),
            Ty::Named(name) => self.enums.get(name).map(|v| v.to_vec()),
            _ => None,
        }
    }

    /// A tuple STRUCT's single constructor (Document 9 §2.4's own
    /// `struct Point(f64, f64);` example): its field types, keyed by
    /// the struct's own type name -- a `Pattern::TupleStruct` over it
    /// (`Point(x, y)`) writes exactly that name as its "constructor",
    /// not a variant of it, since a struct has exactly one shape.
    /// `None` for anything that isn't a registered tuple struct
    /// (including a named-field struct, which has no corresponding
    /// `Pattern` variant to ever reach this at all).
    fn struct_fields(&self, ty: &Ty) -> Option<(String, Vec<Ty>)> {
        match ty {
            Ty::Named(name) => self.structs.get(name).map(|tys| (name.clone(), tys.to_vec())),
            _ => None,
        }
    }

    fn field_types(&self, ty: &Ty, ctor: &Ctor) -> Vec<Ty> {
        match ctor {
            Ctor::Variant(name) => {
                if let Some(tys) = self
                    .variants_of(ty)
                    .and_then(|vs| vs.into_iter().find(|(n, _)| n == name).map(|(_, tys)| tys))
                {
                    return tys;
                }
                if let Some((sname, tys)) = self.struct_fields(ty) {
                    if &sname == name {
                        return tys;
                    }
                }
                vec![]
            }
            Ctor::Tuple => match ty {
                Ty::Tuple(tys) => tys.clone(),
                _ => vec![],
            },
            Ctor::Bool(_) | Ctor::Opaque(_) => vec![],
        }
    }

    /// The full constructor set for `ty`, if -- and only if -- it's
    /// one this module can enumerate completely: `bool`'s two values,
    /// a tuple's or tuple-struct's single always-present shape, or an
    /// enum's (real or synthetic) full variant list. `None` means "not
    /// enumerable" (Document 9 §2.2's rule: only a wildcard/binding can
    /// make a match over such a type exhaustive).
    ///
    /// **Bug fixed here, found via real CI failure on Document 9
    /// §2.4's own worked example** (`match pair { (0,0)=>.., (x,y) if
    /// x==y=>.., (x,y)=>.. }`, reported non-exhaustive when it's
    /// genuinely exhaustive): this function originally had no case for
    /// `Ty::Tuple` at all, so a tuple scrutinee always fell through to
    /// "not enumerable" — meaning the algorithm never attempted
    /// constructor-based specialization for tuples, and instead
    /// treated the whole match as needing a literal `CtorPat::Wildcard`
    /// row to be recognized as covered. But a tuple pattern is NEVER
    /// represented that way even when every one of its sub-patterns is
    /// a wildcard (`(x, y)` lowers to `Ctor(Tuple, [Wildcard,
    /// Wildcard])`, not to a bare `Wildcard`) — so the all-wildcard
    /// catch-all tuple arm could never be recognized as covering
    /// anything via the default-matrix path, regardless of guards. The
    /// guard-exclusion logic itself (`check_exhaustiveness` skipping
    /// guarded arms entirely) was traced and confirmed correct in
    /// isolation; the bug was entirely in this function not
    /// recognizing tuples (and, checked for the same class of issue as
    /// requested, tuple STRUCTS — Document 9 §2.4's other worked
    /// example, `Point(x, y)` — had the identical gap) as
    /// single-constructor, always-complete types at all. Fixed by
    /// giving both their own `Some(..)` case here, so the real
    /// constructor-specialization path (which already handles nested
    /// decomposition correctly, as the other 13 tests already showed)
    /// is used for them instead of the infinite-domain fallback.
    fn full_signature(&self, ty: &Ty) -> Option<Vec<Ctor>> {
        match ty {
            Ty::Bool => Some(vec![Ctor::Bool(true), Ctor::Bool(false)]),
            Ty::Tuple(_) => Some(vec![Ctor::Tuple]),
            _ => {
                if let Some(vs) = self.variants_of(ty) {
                    return Some(vs.into_iter().map(|(n, _)| Ctor::Variant(n)).collect());
                }
                if let Some((sname, _)) = self.struct_fields(ty) {
                    return Some(vec![Ctor::Variant(sname)]);
                }
                None
            }
        }
    }
}

/// Converts one `Pattern` into every `CtorPat` it expands to (more
/// than one only for an `Or` pattern) — the "row expansion" standard
/// usefulness-checking implementations perform for or-patterns.
fn expand_pattern(ctx: &Ctx, pat: &Pattern, ty: &Ty) -> Vec<CtorPat> {
    match pat {
        Pattern::Wildcard | Pattern::Mut(_) => vec![CtorPat::Wildcard],
        Pattern::Or(alts) => alts.iter().flat_map(|p| expand_pattern(ctx, p, ty)).collect(),
        Pattern::Ident(name) => {
            // Document 9's own parser stores a bare, no-parens
            // reference to a nullary enum variant (`HttpMethod::Get`,
            // `None`) as `Pattern::Ident`, structurally identical to a
            // genuine capturing binding (`n` in `n if n < 0 => ..`) --
            // this is the one place a pattern's MEANING depends on
            // whether its name resolves against the scrutinee type's
            // known variants, not on its own shape. Resolved by
            // suffix match (`HttpMethod::Get` or bare `Get` both match
            // a registered variant named `Get`) against `ty`'s
            // variants, but ONLY for a variant with zero fields --
            // matching a syntax the parser could actually have
            // produced for it (a data-carrying variant always needs
            // parens, i.e. `Pattern::TupleStruct`, so an `Ident` can
            // never legitimately refer to one). If nothing matches,
            // it's a genuine wildcard-equivalent binding.
            if let Some(variants) = ctx.variants_of(ty) {
                let short = name.rsplit("::").next().unwrap_or(name);
                if let Some((vname, field_tys)) =
                    variants.iter().find(|(n, _)| n == name || n == short)
                {
                    if field_tys.is_empty() {
                        return vec![CtorPat::Ctor(Ctor::Variant(vname.clone()), vec![])];
                    }
                }
            }
            vec![CtorPat::Wildcard]
        }
        Pattern::Literal(lit) => {
            if let (Ty::Bool, Literal::Bool(b)) = (ty, lit) {
                vec![CtorPat::Ctor(Ctor::Bool(*b), vec![])]
            } else {
                vec![CtorPat::Ctor(Ctor::Opaque(literal_text(lit)), vec![])]
            }
        }
        Pattern::Tuple(pats) => {
            let elem_tys = match ty {
                Ty::Tuple(tys) if tys.len() == pats.len() => tys.clone(),
                _ => vec![Ty::Unit; pats.len()],
            };
            cartesian_ctor(ctx, Ctor::Tuple, pats, &elem_tys)
        }
        Pattern::TupleStruct(name, pats) => {
            let short = name.rsplit("::").next().unwrap_or(name);
            let (vname, field_tys) = if let Some((vname, field_tys)) = ctx
                .variants_of(ty)
                .and_then(|vs| vs.into_iter().find(|(n, _)| n == name || n == short))
            {
                (vname, field_tys)
            } else if let Some((sname, field_tys)) = ctx.struct_fields(ty) {
                // Document 9 §2.4's own `Point(x, y)` shape: a tuple
                // struct, not an enum variant -- now uses its real,
                // registered field types (see `full_signature`'s doc
                // comment for the bug this fixes) instead of the
                // Unit-placeholder fallback below, so its constructor
                // is correctly recognized as complete/decomposable.
                (sname, field_tys)
            } else {
                // Genuinely unknown/unregistered (e.g. a struct this
                // module has no registration for at all -- shouldn't
                // normally happen given the caller always builds
                // `structs` from the same registry `types.rs` itself
                // uses, but kept as a safe fallback): treat as
                // opaque-but-real, using the full written name as its
                // identity so at least repeated identical patterns are
                // recognized as redundant, without claiming any
                // completeness.
                (name.clone(), pats.iter().map(|_| Ty::Unit).collect())
            };
            cartesian_ctor(ctx, Ctor::Variant(vname), pats, &field_tys)
        }
        // Arrays/slices: out of scope (module doc) -- treated as an
        // opaque, never-complete single constructor so a match relying
        // solely on array patterns still correctly demands a wildcard,
        // the safe direction.
        Pattern::Array(..) => vec![CtorPat::Ctor(Ctor::Opaque("<array>".to_string()), vec![])],
    }
}

/// Expands a constructor's sub-patterns as a full cartesian product
/// (each sub-pattern may itself expand to multiple alternatives via a
/// nested `Or`), pairing each with its field type.
fn cartesian_ctor(ctx: &Ctx, ctor: Ctor, pats: &[Pattern], field_tys: &[Ty]) -> Vec<CtorPat> {
    let placeholder = Ty::Unit;
    let mut combos: Vec<Vec<CtorPat>> = vec![vec![]];
    for (i, p) in pats.iter().enumerate() {
        let ty = field_tys.get(i).unwrap_or(&placeholder);
        let expansions = expand_pattern(ctx, p, ty);
        let mut next = Vec::with_capacity(combos.len() * expansions.len());
        for combo in &combos {
            for e in &expansions {
                let mut c = combo.clone();
                c.push(e.clone());
                next.push(c);
            }
        }
        combos = next;
    }
    combos.into_iter().map(|sub| CtorPat::Ctor(ctor.clone(), sub)).collect()
}

fn literal_text(lit: &Literal) -> String {
    match lit {
        Literal::Int(s) | Literal::IntHex(s) | Literal::IntOct(s) | Literal::IntBin(s)
        | Literal::Float(s) | Literal::Str(s) | Literal::RawStr(s) | Literal::Char(s) => s.clone(),
        Literal::Bool(b) => b.to_string(),
        Literal::Null => "null".to_string(),
    }
}

/// The Maranget usefulness check: is `query` NOT covered by any row of
/// `rows`? `col_tys[i]` is the type of column `i` (needed to resolve
/// `Pattern::Ident`-as-variant and to compute constructor-set
/// completeness at each recursive step, since specializing by a
/// constructor replaces one column with that constructor's own field
/// columns).
fn is_useful(ctx: &Ctx, rows: &[Vec<CtorPat>], query: &[CtorPat], col_tys: &[Ty]) -> bool {
    let Some((q0, qrest)) = query.split_first() else {
        // No columns left: useful iff nothing already covers "the
        // empty row" -- i.e. the matrix itself has no rows reaching
        // this depth (specialization only keeps rows that could still
        // match).
        return rows.is_empty();
    };
    // Defensive: `col_tys` should always have at least as many entries
    // as `query` by construction, but a malformed/arity-mismatched
    // pattern (e.g. a type-checking gap letting `Some(a, b)` through
    // for a single-field variant) could in principle desync them.
    // Falling back to `Ty::Unit` (never enumerable, so it only ever
    // pushes this match toward "needs a wildcard") rather than
    // indexing out of bounds means a malformed pattern can make this
    // check imprecise, never make the compiler itself panic --
    // consistent with Document 1 Pillar I: a crash is strictly worse
    // than an overly-cautious diagnostic.
    let col0 = col_tys.first().cloned().unwrap_or(Ty::Unit);
    let rest_tys = if col_tys.is_empty() { &[][..] } else { &col_tys[1..] };
    match q0 {
        CtorPat::Ctor(ctor, sub) => {
            let spec = specialize(ctx, rows, ctor, &col0);
            let mut nq = sub.clone();
            nq.extend_from_slice(qrest);
            let sub_tys = ctx.field_types(&col0, ctor);
            let mut ntys = sub_tys;
            ntys.extend_from_slice(rest_tys);
            is_useful(ctx, &spec, &nq, &ntys)
        }
        CtorPat::Wildcard => {
            match ctx.full_signature(&col0) {
                Some(all_ctors) if !all_ctors.is_empty() => all_ctors.iter().any(|c| {
                    let spec = specialize(ctx, rows, c, &col0);
                    let sub_tys = ctx.field_types(&col0, c);
                    let mut nq = vec![CtorPat::Wildcard; sub_tys.len()];
                    nq.extend_from_slice(qrest);
                    let mut ntys = sub_tys;
                    ntys.extend_from_slice(rest_tys);
                    is_useful(ctx, &spec, &nq, &ntys)
                }),
                _ => {
                    // Not enumerable (or enumerable-but-empty, which
                    // can't actually happen for a real type): fall
                    // back to the default matrix, dropping column 0.
                    let def = default_matrix(rows);
                    is_useful(ctx, &def, qrest, rest_tys)
                }
            }
        }
    }
}

/// `S(c, P)` — keeps rows whose first pattern is `c` (dropping that
/// column and splicing in its sub-patterns) or a wildcard (splicing in
/// fresh wildcards matching `c`'s arity, computed via `ctx` against
/// `col_ty` — the column's type is required here specifically so a
/// wildcard row expands to the SAME column count as a real `c`-headed
/// row would, which is what keeps every row/query in the recursion at
/// matching width); drops every other row.
fn specialize(ctx: &Ctx, rows: &[Vec<CtorPat>], ctor: &Ctor, col_ty: &Ty) -> Vec<Vec<CtorPat>> {
    let arity = ctx.field_types(col_ty, ctor).len();
    let mut out = Vec::new();
    for row in rows {
        let Some((r0, rrest)) = row.split_first() else { continue };
        match r0 {
            CtorPat::Ctor(c, sub) if c == ctor => {
                let mut new_row = sub.clone();
                new_row.extend_from_slice(rrest);
                out.push(new_row);
            }
            CtorPat::Ctor(_, _) => {}
            CtorPat::Wildcard => {
                let mut new_row = vec![CtorPat::Wildcard; arity];
                new_row.extend_from_slice(rrest);
                out.push(new_row);
            }
        }
    }
    out
}

/// `D(P)` — the default matrix: rows whose first pattern is a
/// wildcard, with that column dropped (Document 17 §4.5 / Maranget's
/// `D` function). Constructor rows are dropped entirely.
fn default_matrix(rows: &[Vec<CtorPat>]) -> Vec<Vec<CtorPat>> {
    rows.iter()
        .filter_map(|row| match row.split_first() {
            Some((CtorPat::Wildcard, rest)) => Some(rest.to_vec()),
            _ => None,
        })
        .collect()
}

/// Reports the missing case, if any, for a `match` over `scrutinee_ty`
/// with the given arms. Returns `Ok(())` if exhaustive, `Err(message)`
/// otherwise. Guarded arms' patterns are excluded from the coverage
/// matrix entirely (Document 9 §2.3).
pub fn check_exhaustiveness(
    scrutinee_ty: &Ty,
    arms: &[MatchArm],
    enums: &EnumTable,
    structs: &StructTable,
) -> Result<(), String> {
    let ctx = Ctx { enums, structs };
    let mut rows: Vec<Vec<CtorPat>> = Vec::new();
    for arm in arms {
        if arm.guard.is_some() {
            continue; // Document 9 §2.3: guards never count toward exhaustiveness
        }
        for pat in &arm.patterns {
            for cp in expand_pattern(&ctx, pat, scrutinee_ty) {
                rows.push(vec![cp]);
            }
        }
    }
    let query = vec![CtorPat::Wildcard];
    if is_useful(&ctx, &rows, &query, std::slice::from_ref(scrutinee_ty)) {
        Err(missing_case_message(&ctx, &rows, scrutinee_ty))
    } else {
        Ok(())
    }
}

/// Best-effort description of a missing case for the diagnostic
/// message: if the scrutinee type has a known, complete constructor
/// signature, names the first variant/value not covered by any row;
/// otherwise gives the generic "needs a wildcard" message, since for
/// an unenumerable domain there's no finite list of "missing" values to
/// name (Document 22's fuller diagnostic formatting is Phase 23's job —
/// this is just enough context to act on, consistent with this pass
/// not attempting Phase 23's polish).
fn missing_case_message(ctx: &Ctx, rows: &[Vec<CtorPat>], ty: &Ty) -> String {
    if let Some(all) = ctx.full_signature(ty) {
        for c in &all {
            let spec = specialize(ctx, rows, c, ty);
            let sub_tys = ctx.field_types(ty, c);
            let q = vec![CtorPat::Wildcard; sub_tys.len()];
            if is_useful(ctx, &spec, &q, &sub_tys) {
                let name = match c {
                    Ctor::Variant(n) => n.clone(),
                    Ctor::Bool(b) => b.to_string(),
                    _ => "?".to_string(),
                };
                return format!(
                    "non-exhaustive match: missing case for `{}` (Document 9 §2.5 — every enum variant must be handled, or add a `_` wildcard arm)",
                    name
                );
            }
        }
    }
    "non-exhaustive match: not all possible values are covered — add a `_` wildcard arm or cover every remaining case explicitly (Document 9 §2.5)".to_string()
}
