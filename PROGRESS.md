# Mountain (`mtnc`) — Build Progress

Tracks execution of the 25-phase roadmap defined in Document 25 §2.3.
Updated at the end of every phase per Document 25 §2.2, point 5.

## Status Legend
- 🟢 Complete and verified (tests passing in a real CI run)
- 🟡 Complete, logic-verified, **pending real-toolchain confirmation**
- ⚪ Not started

---

## Phase 1 — Project Scaffold, `mountain.toml` Parsing, CLI Skeleton, Lexer

**Status: 🟡 Code complete, logic-verified — pending real CI confirmation**

### Scope (per Document 25 §2.3)
- `cargo` project scaffold
- `mountain.toml` manifest parsing
- CLI skeleton (`mtnc build` / `mtnc check`)
- Full Lexer (Document 2)

### What was built
- `Cargo.toml` — zero external crates (see "Tooling constraint" below)
- `src/token.rs` — `Token`, `TokenKind`, `Keyword`, `Op`, `Delim`, `Span`
- `src/lexer.rs` — hand-written FSM lexer + 25 inline unit tests
- `src/manifest.rs` — hand-rolled `mountain.toml` parser (subset: sections,
  string/bool/string-array values) + 5 unit tests
- `src/diagnostics.rs` — minimal `Diagnostic` type (full system is Phase 23)
- `src/main.rs` — CLI: `build`, `check`, `--version`; `run`/`test`/`bench`/
  `doc`/`fmt` present but explicitly report "not yet implemented" rather
  than silently no-op'ing
- `src/lib.rs` — module root, so `tests/` can do integration testing
- `tests/integration.rs` — 6 integration tests against realistic snippets
  pulled from Documents 1, 2, 6, 20, plus a deliberately-invalid-input
  rejection test (Document 25 §2.2 point 2 requirement)
- `examples/hello.mtn` — smoke-test source file, used by CI
- `.github/workflows/ci.yml` — minimal single-platform (Ubuntu) build+test
  workflow; **not** the full Phase 24 release matrix

### Process note: how this was verified without a Rust toolchain
This sandbox has no `rustc`/`cargo` and no live network egress (confirmed:
`sh.rustup.rs` rejected by host allowlist; `apt-get install rustc cargo`
resolves locally but every package fetch returns `403 Forbidden`). Agreed
process with the user:
1. Prototyped and **actually executed** the full lexer logic in Python
   (`proto/lexer_proto.py` + `proto/run_tests.py`, not part of the
   deliverable) — 33 test cases, run for real, in this sandbox.
2. Found and fixed one real bug this way: raw-string dispatch (`r"..."`)
   was being shadowed by the generic identifier branch because `r` is
   alphabetic; fixed by checking for raw strings before the identifier
   branch. Also caught one real spec-completeness bug: `as` (Document 4
   §6's cast operator) was missing from the initial keyword table derived
   from Document 3's categories — added after cross-checking Document 3
   Category F/Document 4 §6 against the keyword list.
3. Ported the verified Python logic to Rust 1:1 (`src/lexer.rs`).
4. Cross-checked every Rust unit-test's expected output against the
   Python prototype as ground truth (not hand-derived) before finalizing.
5. Rust `#[test]` functions are written and mirror/extend the Python
   suite (33 unit tests in `lexer.rs` + 6 integration tests), but **have
   not yet been executed by an actual `rustc`/`cargo`** — that requires
   the GitHub Actions run described in the handoff instructions given to
   the user. This phase is not being marked 🟢 until that real run is
   confirmed green, per explicit user instruction.

### CI Run #1 — FAILED (real signal, as expected process)
`cargo build` failed with 3 compile errors: E0428, E0308, E0618. Root cause
(confirmed by tracing, cross-checked against the actual error codes):

1. **E0428 — duplicate `Select` variant.** Document 3 lists `select` once
   under Category D (concurrency `select { case ... }`) and again under
   Category H (database query-context CRUD keyword). These are the same
   lexical token reused in two grammatical contexts, not two reserved
   words — collapsed to a single `Keyword::Select` variant. Disambiguating
   *which* meaning applies is correctly the Parser's job (Phase 2+), not
   the Lexer's.
2. **E0308 / E0618 — prelude shadowing via glob import.** `Keyword::from_str`
   had `use Keyword::*;` in scope, and several Mountain keywords are
   spelled identically to Rust prelude items (`Result`, `Ok`, `Err`,
   `Option`, `Some`, `None`, `Copy`, `Clone`, `Drop`, `Send`, `Sync`,
   `Sized`, `Default`, `From`, `Into`, `Box`, `Fn` — **17 total**, audited
   programmatically, see below). The glob import shadowed
   `std::option::Option::{Some,None}` with the unit variants
   `Keyword::{Some,None}` inside that function, breaking `Some(x)`
   (E0618: unit variant isn't callable) and `return None` (E0308: wrong
   type returned).

   **Fix applied generally, not as a one-off patch:** removed the glob
   import; every `Keyword` variant reference in `from_str` is now fully
   qualified (`Keyword::Let`, `Keyword::Ok`, etc.), and the function
   signature uses `std::option::Option<Keyword>` explicitly. Applied the
   same "never glob-import a local enum, always qualify" policy to the
   `Op`/`Delim` `Display` impls too, even though those weren't actually
   broken (pattern-position matching only resolves through the value
   namespace, so `Eq => "="` in a `match` arm doesn't hit the same
   ambiguity a constructor call does) — kept consistent so there's no
   asymmetric exception for a future reader/editor to trip over.

   Full audit of every `Keyword` variant against the Rust 2021 prelude,
   run programmatically rather than by inspection:
   `Box, Clone, Copy, Default, Drop, Err, Fn, From, Into, None, Ok,
   Option, Result, Send, Sized, Some, Sync` — 17 collisions, all covered
   by the same general fix. (`Op::Eq` also collides with prelude `Eq`;
   already safe since `Op` is never glob-imported anywhere.)

### CI Run #2 — FAILED (different bug, same category)
`cargo build` failed with E0659 ("`Keyword`/`Op`/`Delim` is ambiguous").
Root cause: `lexer.rs`'s test module had **two simultaneous glob imports**
— `use super::*;` (bringing in the `Keyword`/`Op`/`Delim` *types*) and
`use crate::token::TokenKind::*;` (bringing in `TokenKind`'s variants,
which are *also* named `Keyword`/`Op`/`Delim`). Every constructor call
like `Op(Op::Dot)` became ambiguous between "the `Op` type" and "the
`TokenKind::Op` variant". This is the same bug *category* as CI Run #1
(unqualified access to a glob-imported name colliding with something else
in scope), but a different concrete instance the earlier fix didn't touch,
since that fix only audited `token.rs`'s own `Display` impls, not every
file's imports.

**Fix:** removed `use crate::token::TokenKind::*;` entirely; every
`TokenKind` variant reference across all 24 lexer unit tests is now
fully qualified (`TokenKind::Op(Op::Dot)`, etc.). Verified via a script
(not by eye) that after the fix, exactly two `use ...::*;` remain in the
whole `src/` tree — both are ordinary `use super::*;` in `#[cfg(test)]`
modules, and neither co-occurs with a second glob import of a colliding
enum's variants (the actual danger pattern). Grep output confirming this:

```
$ grep -rn "::\*" mtnc/src/
mtnc/src/lexer.rs:451:    use super::*;
mtnc/src/manifest.rs:189:    use super::*;
```
(all other grep hits were comment text referencing the removed imports,
not live code)

Also ran a systematic, whole-crate script check (not manual inspection)
for the general shape of this bug: for every `pub enum` in the crate,
does any variant name equal the name of a different declared type?
Result: exactly the three already-known cases —
`TokenKind::Keyword`/`TokenKind::Op`/`TokenKind::Delim` colliding with
the `Keyword`/`Op`/`Delim` types — and nothing else anywhere in the
crate (`TomlValue`'s `Bool`/`Str`/`Array` variants collide with nothing).
This pattern is structurally inherent to a wrapper enum like `TokenKind`
and isn't a problem by itself — it's only a problem when both the type
and the wrapping enum's variants are glob-imported into one scope at
once, which no longer happens anywhere in the crate.

### Design decisions made (flagged, not silently assumed)
1. **Primitive type names (`i32`, `f64`, `bool`, `String`, etc.) are
   lexed as plain identifiers, not keywords.** Documents 2/3 only list
   structural words (`let`, `fn`, `struct`, ...) under "Keywords" —
   primitive types are never included in Document 3's keyword categories
   A–L. This matches Rust's own lexer precedent (rustc lexes `i32` as an
   identifier, resolved to a builtin type during name resolution, not
   lexing). **Revisit in Phase 3 (Type System)** if this reading turns
   out to be wrong once the type checker is built.
2. **Full keyword table (Document 3, Categories A–L, ~95+ words) is
   loaded now**, not deferred to later phases, even though most of these
   domains (UI, DB, networking, AI, concurrency) aren't implemented until
   much later. Reasoning: keyword-vs-identifier disambiguation is
   correctly a lexer-level concern for a single-pass hand-written lexer,
   and Document 3 §12 already establishes this as the *current* full
   list (with future documents only *adding* to it, never replacing it).
   `Keyword::from_str` is structured as the deliberate extension point
   for those additions.
3. **`mountain.toml` parsing is a hand-rolled subset parser**, not
   `serde`/`toml`, purely due to the no-registry-access tooling
   constraint in this sandbox (see above) — not a language design
   decision. Supports exactly the value grammar Document 15 §3.1's
   example uses: strings, bools, string arrays. Swapping in the real
   `toml`/`serde` crates later is a drop-in replacement.
4. **Doc comments (`///`) are preserved as `DocComment` tokens**;
   ordinary `//` and `/* */` comments are discarded at the lexer level
   and never reach the token stream, per Document 2 §3's distinction.

### Exit criteria (Document 25 §2.3) — self-assessment
> "Lexer correctly tokenizes every literal/operator/comment form in Doc 2;
> invalid tokens produce a recoverable diagnostic, not a crash"

- All literal forms (int/hex/oct/bin, float w/ exponent + suffix, string,
  raw string, char, bool, null) — covered, tested. ✅ (logic-verified)
- All comment forms (line, block incl. nested, doc) — covered, tested. ✅
- Full Document 4 operator set incl. maximal-munch stress cases
  (`<<=`, `..=`, `??`, `?.`) — covered, tested. ✅
- Invalid characters produce a `Diagnostic` + `Error` token and lexing
  **continues** (does not abort) — covered, tested explicitly. ✅
- **Not yet independently confirmed by a real `cargo test` run.** ⏳

### Known gaps / explicitly deferred (not silently skipped)
- No Parser yet — `build`/`check` only run the Lexer stage. Expected;
  Parser is Phase 2.
- Unicode identifier rules beyond "alphabetic or `_`, then alphanumeric
  or `_`" are not specified anywhere in Documents 1–25, so none were
  invented. If this needs tightening later, it must come from a spec
  update, not a silent implementation choice.
- Escape-sequence *validation* inside string/char literals (e.g.
  rejecting `\q` as an invalid escape) is deferred — the lexer currently
  accepts any `\<char>` pair and defers validation to a later semantic
  pass, since Document 2 doesn't fully enumerate the legal escape set.

---

## Phase 2 — Parser
**Status: ⚪ Not started**

## Phases 3–25
**Status: ⚪ Not started**

(Full phase table: see Document 25 §2.3.)
