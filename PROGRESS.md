# Mountain (`mtnc`) — Build Progress

Tracks execution of the 25-phase roadmap defined in Document 25 §2.3.
Updated at the end of every phase per Document 25 §2.2, point 5.

## Status Legend
- 🟢 Complete and verified (tests passing in a real CI run)
- 🟡 Complete, logic-verified, **pending real-toolchain confirmation**
- ⚪ Not started

---

## Phase 1 — Project Scaffold, `mountain.toml` Parsing, CLI Skeleton, Lexer

**Status: 🟢 Complete — confirmed via real GitHub Actions CI run**

CI run confirmed externally: 29 unit tests + 6 integration tests, 0
failures, including the specific exit-criteria tests
(`invalid_character_recoverable_lexing_continues`,
`unterminated_string_is_recoverable_not_a_crash`,
`deliberately_invalid_source_is_rejected_not_silently_accepted`). This
is a real `cargo test` result from GitHub Actions, not sandbox tracing —
per the standing rule agreed with the user, Phase 1 was not marked 🟢
until this external confirmation came back.

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

### CI Run — 56/57 core tests pass, 1 failure in `parser::tests::call_then_field`
`to_sexpr` (the test-only s-expression printer) had no match arm for
`Expr::MethodCall`, falling through to a `{:?}` Debug-dump catch-all —
confirmed by reading the function directly rather than assuming. But the
test's *expected* value was also wrong, not just the printer: it was
carried over from the Python precedence-prototype, which never modeled
method calls as a distinct node (it only needed to validate precedence/
associativity) and always decomposed `.b(1)` into generic field-access-
then-call. The real parser is more faithful to Document 23's actual
grammar (`.IDENT` combined with an optional `(args)` is *one* postfix
production) and correctly builds a single `MethodCall{receiver, name,
args}` node. Fixed both: added the missing printer arm, and corrected
the test's expected string to `"(. (methodcall a b [1]) c)"` — what the
AST is actually supposed to produce, not what the simplified prototype
happened to produce. Verified no other unit test secretly exercised a
method-call pattern through `to_sexpr` (grepped all 24
`to_sexpr(&parse_expr_str(...))` call sites) before considering this closed.

### CI Run — 56/57 core tests pass; 5/6 `parser_doc24` tests failed (new coverage finding real gaps, not a regression)
Core lexer/manifest/parser suite (57 unit + 6 original integration = 63)
fully green — confirms the grammar/precedence engine itself is solid.
The 6 new `parser_doc24` tests (deliberately more demanding, real-world
Document 24 examples) surfaced 5 concrete gaps, one of which turned out
to be two failures sharing one root cause, and one of which turned out
to be a **different** root cause than either of us initially guessed
from the error text alone — each traced against the actual embedded
test source and the actual current parser code before fixing, not
assumed:

1. **`layout { gap: 8, align: Align::Center }` (Doc24 §2, confirmed at
   line 13 of the embedded source)** — `align` is a Document 3 keyword
   (Category K, `#[align(N)]`), and `parse_brace_prop_list` still called
   `expect_ident()`. Rather than patch this one site, did the systematic
   sweep the report asked for: grepped every remaining
   `self.expect_ident()` call site in `parser.rs`, mapped each to its
   enclosing function, and — since `expect_word()` is a strict superset
   of `expect_ident()` (accepts everything the old call did, plus
   keyword-as-word) — replaced essentially all of them in one pass
   rather than triaging case-by-case, closing this class of bug for good
   instead of leaving another one for Phase 3 to find by accident.
2. **`use std::ai::{tensor, layers, ...};` — initially suspected, but
   traced and ruled out.** Re-checked the actual current
   `parse_use_decl` code directly: the grouped-import `{ ... }` handling
   was already implemented correctly. The CI line number (5) actually
   pointed at `layer1: layers::Dense,` — a struct field with a
   **multi-segment type path**, which `parse_type()` had never
   supported at all (it only ever consumed one identifier/word, then
   immediately returned). This is the *same* root cause as the Document
   24 §4 `orderbook::OrderBook` failure — fixed once, via a shared
   `parse_further_path_segments()` helper used by both `parse_type` and
   the expression-path builder in `parse_primary` (and applied to
   pattern-path parsing too, for consistency, even though no current
   example strictly requires it there).
3. **`channel::<i32>()` (Doc24 §5)** — the same shared-helper fix
   resolved this as a side effect: the old path-building loop
   unconditionally tried to consume another word after every `::`,
   including when the `::` was actually introducing turbofish generic
   args, so it choked on `<` instead of leaving it for the
   already-existing turbofish-handling code right after the loop.
4. **`spawn { ... }` as a statement / `on submitOrder(...)` inside
   `actor` — both turned out to be cascading symptoms, not separate
   bugs.** `parse_primary` already had a dedicated `spawn` branch, and
   `parse_actor_decl` already had a dedicated `on`-handler branch — both
   confirmed by reading the actual code before assuming a fix was
   needed. The real failures were the `orderbook::OrderBook` type-path
   gap (item 2 above) causing a parse error mid-actor-body, which
   triggered item-level error recovery (`synchronize_item`) to skip
   forward and misinterpret whatever token it landed on next as a fresh
   top-level item. Fixing item 2 resolves both of these without any
   separate change.
5. **`Text(product.name)` inside `Column { ... }` (Doc24 §6, confirmed
   at line 28) and the same pattern throughout Doc24 §2** — a real,
   previously-undiscovered gap: `Name { child, child, ... }` (Document
   18's UI children-list call pattern) is syntactically ambiguous with a
   struct literal (`Name { field: expr, ... }`) at the token level, and
   the parser always assumed struct-literal. Added `Expr::ComponentChildren`
   and a lookahead helper (`looks_like_struct_lit_fields`, checking
   whether the first token after `{` looks like `word :`) to disambiguate,
   consistent with Document 23's own `struct_literal` grammar requiring
   at least one `IDENT ":" expr` before anything else — so a children
   list (which never starts that way) is correctly distinguished.

All fixes re-verified with the same discipline as every prior round:
delimiter-balance check, the AST field/variant cross-reference script,
and a fresh glob-import audit — all clean, and each fix was hand-traced
step-by-step against the actual reported failing construct (not just
"should work now") before packaging. Cannot run `cargo test` directly in
this sandbox (same standing constraint), so this hand-tracing plus the
scripted checks remain the verification method until the next real CI run.

### CI Run — down to 2/6 `parser_doc24` failures (4 now passing: examples 1, 4, 5, 6)
Both remaining failures traced against the actual embedded test source
and actual current parser code before fixing:

1. **`on: click => addTask()` (Document 24 §2, line 17)** — confirmed:
   Document 18 §5 defines `on: <eventName> => <expression or closure>`
   as syntax scoped to the `on:` argument-value position specifically,
   not a general expression production (`click` is a literal event-name
   marker, not a variable/pattern; Document 23's core grammar only uses
   `=>` in `match_arm` and `closure_expr`, both different shapes). Checked
   `ast.rs` for an existing fit before adding anything new — `OnHandler`
   (used by `server`/`actor`: `on IDENT(params) { block }`) is a
   declaration-shaped construct with a name and parameter list, not an
   argument value, so it doesn't fit. Added `Expr::EventHandler{event,
   body}`, special-cased narrowly in `parse_arg` (only when the argument
   label is literally `"on"`, detected the value via a 2-token lookahead
   for `word =>` so it can't interfere with any other argument shape).
2. **`|(input, _)| model.forward(input)` (Document 24 §3, line 25, col
   42 — confirmed exact)** — confirmed: `closure_expr`'s grammar (Doc23
   §6) only ever allowed a bare `IDENT` per parameter, unlike `fn`
   params/`let`/`match`, which all already use the general `pattern`
   production (§7). This is a real EBNF gap, not just a parser
   oversight — flagged in `PROGRESS.md`'s deviations list per the
   instruction to note it formally like the tensor-shape one. Widened
   `ClosureExpr.params` from `Vec<(String, Option<Type>)>` to
   `Vec<(Pattern, Option<Type>)>` and switched the parser to call the
   **same** pattern-parsing function used everywhere else, not a second
   one — with one catch found and fixed before it became a silent bug:
   `parse_pattern()` (not `parse_pattern_single()`) also checks for a
   trailing `|` to build an or-pattern, which would misfire on the
   closure's own closing `|` delimiter (`|(input, _)| body` — right
   after the tuple pattern, the very next token *is* a bare `|`, but
   it's the parameter list's closer). Used `parse_pattern_single()` at
   this call site specifically to avoid that collision, while still
   sharing the same underlying single-pattern parser as every other use
   site.

Re-verified with the same standing checks (delimiter balance, AST
field/variant cross-reference script, glob-import audit — all clean)
plus hand-tracing both fixes token-by-token against the exact failing
constructs. Still no real `cargo test` available in this sandbox.

### Housekeeping fixed this round
- **`.github/workflows/ci.yml`**: bumped `actions/checkout@v4` →
  `actions/checkout@v7`. Verified the actual current latest via web
  search rather than trusting the suggested `v5` — `v7.0.1` (July 2026)
  is genuinely the latest as of this writing; `v5` would have been a
  regression, not even an update.
- **`warning: field 'src' is never read` (present in every CI run since
  Phase 1, never addressed)**: checked which of the two explanations was
  actually true rather than suppressing it. Grepped every reference to
  `self.src` in `lexer.rs` — it was set once in `new()` and never read
  anywhere; all real scanning goes through the separate `chars: Vec<char>`
  field. This confirms it was genuinely dead code (a leftover from an
  early design intention to do byte-level fast-path scanning over ASCII
  structural characters that was never actually implemented), **not** a
  sign that some other part of the lexer was silently relying on the
  wrong field. Removed the field outright, which also removed the
  struct's now-unnecessary lifetime parameter (`Lexer<'a>` → `Lexer`) —
  confirmed via grep that the only other reference (`Lexer::new(src)` in
  the `tokenize()` free function) is lifetime-elided and needed no change.

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

**Status: 🟢 Complete — confirmed via actual downloaded CI log**

Final CI run confirmed from the raw log file (not just the Actions UI
summary badge): 57 lib tests + 6 integration tests + 6 `parser_doc24`
tests, all passing, 0 failures, 0 ignored, across the full run.

### Scope (per Document 25 §2.3)
Hand-written recursive-descent parser with Pratt parsing for expressions,
producing an untyped AST, covering Document 4 (operator precedence),
Document 9 (control flow grammar), and Document 23 (authoritative EBNF).

### What was built
- `src/ast.rs` — full AST node types for the whole Document 23 grammar
  surface: items (fn/struct/enum/trait/impl/mod/use/import/const/static/
  type-alias/table/index/schema/ui/component/server/actor/target-block),
  generics/where-clauses, types, patterns, statements/blocks, and the
  full expression grammar (33 `Expr` variants).
- `src/parser.rs` — the parser itself: token-stream helpers, item-level
  parsing with error recovery (Document 17 §2's synchronize-to-next-item
  strategy, extended from Document 25 Phase 1's lexer-level precedent),
  and a Pratt expression parser whose binding-power table was verified
  against Document 4 §10/§11 via an **executed Python prototype (26/26
  cases passing)** before being ported to Rust — see the process note
  below.
- `tests/parser_doc24.rs` — round-trips Document 24's 6 example programs
  through Lexer→Parser.
- Inline `#[cfg(test)]` module in `parser.rs` — 20 unit tests, including
  all 26 of the Python-verified precedence cases ported 1:1 (Document 4
  §11's exact verification-pass cases plus the full §10 table spot-check),
  a non-chaining-comparison rejection test, and a parser-level error-
  recovery test (one malformed top-level item doesn't block the rest of
  the file from parsing).

### Process note: verification approach (same discipline as Phase 1)
Cannot run `cargo build`/`cargo test` in this sandbox (same constraint as
Phase 1 — no toolchain, no network). Verification performed instead via:

1. **Pratt-parser precedence engine prototyped and executed in Python**
   (`proto2/pratt_proto.py` + `proto2/run_pratt_tests.py`, not part of
   the deliverable) before writing any Rust. Found and fixed two real
   logic bugs this way:
   - Unary operators were binding looser than `**` (produced `-(a**b)`
     instead of the correct `(-a)**b`) — fixed by parsing the unary
     operand via `parse_prefix()` directly rather than `parse_expr(bp)`.
   - Non-chaining comparison rejection (`a < b < c`) wasn't actually
     triggering, because giving comparison operators equal left/right
     binding power let the *second* occurrence get silently absorbed by
     a fresh recursive call (with its own fresh "have I seen a
     comparison yet" state) instead of staying in the same stack frame
     where the check could see it. Fixed by using `(l_bp, l_bp+1)` like
     ordinary left-associative operators, so a repeated same-row
     operator stays in the frame that's tracking it.
   - Final result: 26/26 cases passing, including every row of Document
     4 §10's table spot-checked pairwise and both exact cases from §11.
2. **Ported the verified logic 1:1 to Rust**, then applied the Phase-1
   lesson proactively from the start: every enum reference is fully
   qualified, no glob-importing a local enum's own variants anywhere
   `use crate::ast::*` or similar could create the CI-run-#2 class of
   ambiguity. Verified this **systematically with scripts**, not by
   eye, before claiming it:
   - Whole-crate check for the actual danger pattern (wrapper-enum
     variant name == another declared type name, PLUS both glob-imported
     into the same scope): only the already-known `TokenKind::{Keyword,
     Op,Delim}` case exists anywhere in the crate; nothing new introduced.
   - Every top-level `ast.rs` type name checked against the Rust 2021
     prelude (the `use crate::ast::*` in `parser.rs` is a full-file glob
     import) — zero collisions.
   - The two new local-enum glob imports in the test module
     (`use BinaryOp::*`, `use AssignOp::*`) checked the same way:
     `AssignOp::Eq` does collide with the prelude's `Eq` trait, but every
     occurrence within that scope is in **pattern position** (match arms
     only), which — per the mechanism already proven safe in Phase 1's
     `Op`/`Delim` `Display` impls — cannot hit the ambiguity, since
     pattern resolution only searches the value namespace, not the type
     namespace where the trait lives.
3. **Cross-referenced every AST struct-literal and enum-variant usage in
   `parser.rs` against the actual declarations in `ast.rs`** with a
   script (not by eye), after this exact review caught two real
   regressions during editing (see below).
4. **Systematically scanned all 6 Document 24 examples with the Python
   lexer harness** for every keyword token appearing in "word" position
   (path segment after `::`, method/field name after `.`, or immediately
   before `:` as a would-be named-argument label) — this caught a whole
   class of real bugs (below) that manual reading of the examples had
   missed on the first two passes.

### Real bugs found and fixed during this phase (not just claimed — each traced)
- **Two AST enum variants were accidentally deleted by earlier
  `str_replace` edits** (`Expr::Return` and `Expr::Throw` both briefly
  vanished while inserting doc-comments for adjacent variants). Caught
  immediately by the field/variant cross-reference script — not by
  compiling, since compiling isn't available here — which is exactly why
  that script exists as a standing check now, not a one-off.
- **`use std::db::query;` / `use std::net::server;` would fail to
  parse** — `query`/`server` are Document 3 keywords, but `use`-path
  segments were parsed with `expect_ident()`, which rejects keyword
  tokens outright.
- **Named-argument labels `on:`/`bind:` (Document 24 §2) would fail** —
  same root cause, in `parse_arg`'s named-argument lookahead.
- **`.match(...)`, `.insert(...)`, `.send(...)`, `.recv(...)`,
  `.message(...)`, `.listen(...)` as method names (Document 24 §1/§4/§5)
  would all fail** — same root cause, in the postfix `.name` parser.
- **The single biggest one: `Some(x)` / `None` / `Ok(x)` / `Err(x)` as
  match patterns — used constantly across Documents 1–24 — would have
  failed to parse at all**, because `parse_pattern`'s catch-all branch
  also called `expect_ident()` first, before ever reaching the
  tuple-struct-pattern logic that would otherwise have handled them.
- **`server::Http::bind(...)` / `TextDecoration::None` /
  `MatchingEngine::spawn()` as path *expressions*** (not just patterns)
  had the same problem one level up: the primary-expression entry gate
  itself (`check_ident() || check_kw(SelfValue)`) didn't admit a leading
  keyword at all, so execution never even reached the path-building loop.

**General fix** (not five one-off patches): added `Keyword::as_source_text()`
(token.rs) — the reverse of `Keyword::from_str` — and a parser-level
`expect_word()` helper that accepts a plain identifier *or* a keyword
used as an ordinary word, applied at every name-like position where this
collision is real: `use`/`import` path segments, named-argument labels,
postfix `.field`/`.method` names, struct-literal field names, pattern
heads, and expression path segments. This is a real, structural
consequence of Mountain's own keyword surface being large (Document 3's
~95+ keywords) while also reusing many of those same words as ordinary
stdlib/method/module names (Document 15/16/19's own examples do this
constantly) — not a parser implementation mistake so much as something
the grammar itself needs to tolerate, which `expect_word()` now does
uniformly rather than via scattered special cases.

### Flagged deviations from Document 23 (need explicit sign-off)
Document 23 was treated as ground truth per this phase's instructions,
but 6 concrete cases were found where Document 24's *required-to-parse*
example code doesn't fit Document 23's literal grammar. Each is a small,
localized, documented extension (or, in one case, an acknowledged open
gap) rather than a silent reinterpretation:

1. **`fn` inside `ui`/`component` blocks** (`ast.rs`'s `UiItem::Fn`) —
   Document 23 §10's `ui_item` production only lists
   `state_decl | prop_decl | render_block | mount_block | unmount_block`.
   Document 24 §2 declares methods (`fn addTask(borrow mut self) {...}`)
   directly inside `ui TodoApp { }`. Implemented permissively so §2 parses.
2. **`try { } catch (e) { }` as an expression**, not just a statement
   (`Expr::TryCatch`) — Document 23 §8's `try_stmt` is statement-only.
   Document 24 §1 uses it as `let body = try { ... } catch (e) { ... };`.
3. **`return expr` usable as a match-arm body expression**
   (`Expr::Return`), not just a statement — Document 24 §1 has
   `_ => return HttpResponse::notFound(),` with no enclosing block.
4. **`style { ... }` / `layout { ... } { ... }` postfix modifiers**
   (`Expr::Styled`, `Expr::Layout`) — absent from Document 23's EBNF
   entirely (not a differing snippet — a missing production), but
   grounded in Document 18 §7/§7.1's own concrete examples rather than
   invented from nothing. Needed for Document 24 §2 to parse.
5. **Const-generic argument values** (`Type::ConstArg`) — Document 23's
   `generic_args` grammar only allows `type`, but Document 8 §8's
   `Matrix<f64, 2, 3>` needs numeric literals in that position.
6. **Turbofish (`::<T>`) parsed and discarded** at call sites (Document
   8 §2, Document 16 §1.21.1's `.parse::<u64>()`) — not tracked in the
   AST yet since Phase 2's AST is untyped by design (Document 17 §3);
   this is a pure syntax-acceptance fix, revisit when generics need real
   representation (Phase 5).
7. **`Expr::ComponentChildren`** — `Name { child, child, ... }` (Document
   18's `Column { Text("Left"), Text("Right") }` pattern) has no
   production anywhere in Document 23 (same situation as `Styled`/
   `Layout` — a missing production, not a differing snippet), and is
   syntactically ambiguous with a struct literal at the token level.
   Disambiguated by lookahead: Document 23's own `struct_literal`
   grammar requires the content to start with `IDENT ":" expr`, so
   checking whether the first token after `{` looks like `word :`
   reliably distinguishes the two. Found while re-verifying Document 24
   §2/§6 after the `align`/`layout` fix let parsing reach further into
   those examples. Needs explicit sign-off, like the others above.
8. **`Expr::EventHandler`** — Document 18 §5's `on: <eventName> =>
   <expression or closure>`, scoped to the `on:` argument label. Not a
   general expression production anywhere in Document 23. See CI-run
   notes above for why `OnHandler` doesn't fit this shape.
9. **`ClosureExpr.params: Vec<(Pattern, Option<Type>)>`** (was
   `Vec<(String, Option<Type>)>`) — Document 23 §6's `closure_expr`
   grammar only allows a bare `IDENT` per parameter, unlike `fn`
   params/`let`/`match`, which already use the general `pattern`
   production (§7). Document 24 §3's `|(input, _)| ...` needs
   tuple-destructuring closure parameters. A genuine EBNF
   clarification worth formal note, not just a parser patch.

### Known open gap — NOT resolved, explicit ask
**Document 24 §3's `tensor<f32>[784]` type syntax does not parse**, and
unlike the cases above, there's no other spec document with a concrete
example to ground a reasonable extension against — Document 8 §8 and
Document 16 §1.9 both use tensor/matrix types but never show this exact
"generic type immediately followed by a `[N]` shape suffix" spelling.
Document 23's only `[...]` type production is `[T; N]` (array-of-T),
which is structurally different. `tests/parser_doc24.rs`'s
`doc24_example3_ai_training_loop_up_to_known_gap` test isolates this:
everything else in Example 3 (struct/impl, named args, closures over
tuples, `gradient(..., respectTo: ...)`, `for epoch in 0..10`) is
asserted to parse cleanly, and the tensor-shape fragment is asserted to
still fail, with a comment pointing back here. This isolation is now
confirmed accurate (see below — it was previously masked by an earlier
bug that failed before ever reaching this construct). **This needs your
guidance** — options as I see them: (a) treat `tensor<f32>[784]` as
sugar for a `Named("tensor", [f32; expr])`-shaped const-generic array
type, (b) treat the trailing `[N]` as a distinct postfix "shape"
annotation attached to the type, or (c) something else you have in mind
for Document 8's own tensor-shape story. I didn't want to guess at
which one is intended and bake it into the parser unilaterally.

### Exit criteria (Document 25 §2.3) — self-assessment
> "Parser round-trips every code example in Doc 24 into a correct AST;
> precedence table (Doc 4 §10) verified via the exact test cases in
> Doc 4 §11"

- Document 4 §11's exact cases + full §10 table (26 cases): ✅ passing,
  confirmed by real CI (all 20 precedence unit tests green, including the
  one regression found and fixed — see the CI-run notes above).
- Document 24 examples 1, 2, 4, 5, 6: all 5 gaps found by the second CI
  run traced and fixed (see CI-run section above) — expected to now
  parse cleanly, **pending the next real `cargo test` confirmation**;
  not claiming this as independently verified until that comes back.
- Document 24 example 3: expected to now correctly isolate *only* the
  flagged tensor-shape gap (the earlier `layers::Dense` multi-segment
  path failure that was masking this is fixed) — still blocked on your
  guidance for that one specific construct.
- **Not yet independently confirmed by a real `cargo test` run.** ⏳

### `.github/workflows/ci.yml`
`actions/checkout@v4` → `actions/checkout@v7` (see Housekeeping above).
Otherwise no changes needed — `cargo test --verbose` already picks up
the new `tests/parser_doc24.rs` file and the new unit tests in `parser.rs`
automatically; the workflow itself doesn't need to know about individual
test files.

---

## Phase 3 — Core Type System

**Status: 🟡 Code complete, extensively self-verified — pending real CI confirmation**

### Scope (per Document 25 §2.3)
Primitive types (Document 5 §2), the full 6-rule type-inference engine
(Document 5 §4), and the data-shape half of `struct`/`enum` (Document 7)
— fields/variants only, not traits/impl/generics (Phase 4/5).

### What was built
- `src/types.rs` — `Ty` (resolved-type representation), `resolve_type`
  (syntactic `ast::Type` → semantic `Ty`), the struct/enum data-shape
  registry, and `TypeChecker` (a bidirectional, expected-type-propagating
  checker implementing Document 5 §4's 6 rules).
- `tests/types_doc5.rs` — 26 tests: all 4 of Document 5 §7's official
  cases (each with a positive counterpart), rule-by-rule coverage for
  rules 1/2/3/5/6 individually, and struct/enum data-shape tests
  (missing/unknown/wrong-type fields, spread, field access, unit/data
  enum variants, wrong arg counts).
- **Closed a real Phase 2 gap found while implementing this phase**:
  `unsafe { }` block parsing didn't exist at all (needed for Document 5
  §7's null/unsafe interaction) — confirmed via `grep -n "Unsafe"
  src/parser.rs src/ast.rs` returning nothing before adding it. Added
  `Expr::Unsafe(Box<Block>)` and its parser support.

### Design decision (flagged, not silently assumed)
**Bidirectional checker, not a full HM unifier.** Document 17 §4.2
describes semantic analysis as "constraint-based (Hindley-Milner-style)".
This implementation instead does expected-type propagation directly
(infer when nothing is expected, check against an expectation when one
exists) — Document 5's 6 rules are themselves described operationally,
not as a mandate for a specific algorithm, and full HM-style unification
over type *variables* only becomes necessary once real generics exist to
unify over (Phase 5, not this phase). Revisit if this proves
insufficient once Phase 5 needs to infer through actual generic
functions. Flagged for sign-off like every other deviation this phase.

### A real bug found and fixed during this phase's own verification (not just claimed)
Traced `let x: [i32] = [1.5];`-shaped cases by hand and found `check_expr`
only used `expected` as a *soft hint* to guide inference for some
expression kinds (`Array`, `Tuple`) but never verified the *final*
result actually matched it — so a float literal in a slot annotated
`[i32]` would have silently produced `Array(F64)` with zero errors.
Fixed generally, not per-branch: split `check_expr` into a thin public
wrapper (does one uniform final `check_compatible` against `expected`)
and `check_expr_inner` (the actual per-kind logic), so every expression
kind gets the check automatically, including future kinds. This
surfaced a second issue immediately: `Expr::Ident` had its own internal
`check_compatible` call, which would now double-fire alongside the new
wrapper's — removed the now-redundant internal call. Same for `Literal`
generally: `check_literal` already fully handles expected-compatibility
itself (numeric literals adopt/default per rules 2/3; `null` reports its
own specific, clearer message) — the blanket wrapper check is
deliberately skipped for `Expr::Literal` so the same mismatch isn't
reported twice with a less specific message the second time.

### Systematic verification performed (same discipline as Phases 1–2)
- Delimiter-balance check on every new/touched file — clean.
- AST enum-variant cross-reference script (every `ast::Enum::Variant`
  referenced in `types.rs` checked against actual `ast.rs` declarations,
  not by eye) — clean.
- Broad field/method-access sanity sweep (every `.identifier` access in
  `types.rs` checked against declared `ast.rs` struct fields plus known
  stdlib methods) — two unmatched hits, both traced and confirmed false
  positives (one inside a doc comment, one being `Vec<String>::join`
  which was just missing from the checker's known-methods list).
- Glob-import audit: `use crate::ast::*` (top-level) and `use
  BinaryOp::*` (inside `check_binary`) — the same two patterns already
  proven safe in Phase 2 (zero prelude collisions for `BinaryOp`,
  pattern-position-only usage); no new glob-import risk introduced.
- Checked `Ty`'s own variant names against the Rust prelude before
  calling this done: `Ty::Fn` does collide with the prelude `Fn` trait
  by name, same as Phase 1's `Keyword::Fn` did — but `Ty` is never
  glob-imported anywhere in the crate (`grep -n "use Ty::"` returns
  nothing), so this is inert, not a live risk. `Option`/`Result`/`String`
  were deliberately named `OptionTy`/`ResultTy`/`StringTy` from the
  start specifically to avoid this class of collision proactively,
  rather than fixing it reactively after a CI failure this time.
- Proactively checked for a *self-introduced* dead-code situation before
  it could become a repeat of the Phase 1 `Lexer.src` warning: the
  `enums: HashMap<...>` registry was written during registration but
  never read anywhere in the checker — confirmed via `grep -n
  "self\.enums"`. Unlike `Lexer.src`, this data genuinely belongs in
  Phase 3's stated scope (enum data-shapes), so the correct fix was to
  actually use it, not delete it — implemented real enum-variant-
  construction checking (`EnumName::Variant` and `EnumName::Variant(args)`)
  against the registry, which is also directly useful Document 7
  coverage, not just a warning-silencer.

### Known simplifications (flagged, not silently claimed as complete)
- **`null`-in-`unsafe` is not fully implemented** — the checker rejects
  `null` against any non-`Ty::Null` expected type *unconditionally*,
  regardless of whether it's lexically inside an `unsafe { }` block.
  This correctly enforces exactly what Document 5 §7 tests ("`null` is
  not usable where `Option<T>`/`T` is expected in safe code" — every
  example anywhere in Documents 1–24 is safe code), but is stricter than
  the full spec intent, which does mean to permit `null` inside
  `unsafe`. Threading an "am I inside unsafe" flag through `check_expr`
  is straightforward but wasn't added this phase.
- **Destructuring patterns in `let`/`match` don't distribute field types
  yet** — `let (a, b) = pair;` or `Some(x) => ...` don't bind `a`/`b`/`x`
  into the environment with their real per-field types (only
  `Pattern::Ident`/`Pattern::Mut` at the top level of a `let` are
  handled). This is a safe failure mode (using such a binding later
  reports "undefined variable", not a wrong type silently accepted), not
  a silent gap — but it does mean pattern-heavy code isn't fully
  type-checked yet.
- **Struct-literal `..spread` expressions aren't themselves type-checked**
  — `check_struct_lit` only checks whether a spread is *present* (to
  relax the missing-field requirement), never actually visits the spread
  expression to verify it's the right struct type.
- **Type errors carry item-level context, not precise spans** — `ast::Expr`
  doesn't carry `Span` yet (only `ast::Item` does, from Phase 2), so
  diagnostics report which function/item an error occurred in, not an
  exact line/column. Precise per-expression diagnostics are Document
  22/Phase 23's job, not this phase's exit criteria.
- **One minor diagnostic-noise case, not a correctness bug**: an
  ambiguous empty-array `let` binding that's later passed to a function
  expecting a concrete array type produces two errors (the original
  ambiguity error, plus a cascading mismatch against the function's
  parameter type) rather than one. Traced by hand and confirmed this
  never causes a false accept/reject — just an extra, related message.

### Exit criteria (Document 25 §2.3) — self-assessment
> "All 4 inference-rule test cases from Doc 5 §7 pass; ambiguous-type
> cases correctly produce 'annotation required' diagnostics, not guesses"

- All 4 of Document 5 §7's official cases: implemented and tested, each
  with a positive counterpart proving the checker isn't just rejecting
  everything. **Pending real `cargo test` confirmation.**
- Ambiguous empty-array literal: produces a real, specific diagnostic
  (`"cannot infer type of empty array literal []..."`), not a silent
  default — tested both for the exact Document 5 §7 phrasing and a
  second, differently-shaped call site to confirm it's a general rule,
  not a pattern-matched special case.
- **Not yet independently confirmed by a real `cargo test` run.** ⏳

### `.github/workflows/ci.yml`
No changes needed — picks up `tests/types_doc5.rs` and the new `types.rs`
unit coverage automatically.

## Phase 4 — Traits & `impl`

**Status: 🟢 Complete — confirmed via actual downloaded CI log**

Final CI run confirmed from the raw log: 110 tests total across all
targets (57 lib + 6 integration + 6 `parser_doc24` + 15 `traits_impls`
+ 26 `types_doc5`), 0 failures, 0 warnings in the build step.

**Correction on the record**: my Phase 4 handoff report claimed "17 new
tests in `tests/traits_impls.rs`" — the actual, counted figure is 15
(`grep -c "#\[test\]" tests/traits_impls.rs` → 15), matching the real CI
log's "running 15 tests" for that target. All 15 genuinely passed, so
this was a reporting-accuracy issue, not a coverage/correctness one —
but the number itself was wrong, from recollection rather than an actual
count at the time of reporting. Going forward, every test-count (or
other numeric) claim in this file and in chat reports is taken from a
fresh `grep -c` (or equivalent) at the moment of reporting, not memory.

### Scope (per Document 25 §2.3)
Full trait declarations (Document 7 §4) including default method bodies,
`impl Trait for Type` resolution, the orphan-rule coherence check
(Document 7 §5), and both dispatch modes: static/monomorphized (default)
and `dyn`-based dynamic dispatch (Document 7 §4.5).

### What was built
- `src/types.rs` additions: `Ty::DynTrait` (was previously
  indistinguishable from `Ty::Named` — `resolve_type` mapped `Type::Dyn`
  to `Ty::Named` in Phase 3, which would have made static and dynamic
  dispatch unable to be told apart; fixed as part of this phase), `FnSig`
  (method signatures), `TraitShape` (trait method registry, tracking
  which methods have default bodies), `ImplRecord` (one impl block's
  provided methods, with an optional trait name), and a new
  `resolve_method` doing real two-mode dispatch resolution.
- `tests/traits_impls.rs` — 17 tests: orphan rule (violation + two
  "allowed" cases, one per which side is local, + confirmation the rule
  doesn't apply to inherent impls), required-vs-default method
  completeness (both directions), static dispatch (inherent + trait-
  provided, correct args, wrong arg type, undefined method, inherent-
  over-trait precedence), and `dyn` dispatch (resolves via trait
  signature, undefined method, wrong arg type, and a side-by-side test
  confirming static and dyn dispatch are genuinely distinguished, not
  accidentally sharing one resolution path).
- Extended `check_fn`/`check_item` to bind `self` to the enclosing
  impl's target type when checking a method body, so `self.field` and
  `self.method()` resolve correctly inside `impl` blocks (Phase 3 only
  ever checked free functions, where `self` never appeared).

### Design decision (flagged, not silently assumed)
**Orphan-rule "package" approximation.** Document 7 §5's orphan rule is
about cross-*package* coherence, but the real module/package system
(Document 15) isn't built until Phase 14 — this phase's `Program` is
effectively one file with no package boundary of its own. Grounded the
approximation in Document 15 §3.2's own documented `use` (same-package)
vs `import` (cross-package) distinction: a trait or type is treated as
"local" if it's actually declared (`trait`/`struct`/`enum` item) in the
checked `Program`, and "foreign" otherwise — regardless of whether an
explicit `import` statement is present, since Phase 4 doesn't yet
validate `import` targets either. This correctly rejects the two-
foreign-things case Document 7 §8 describes and correctly allows both
"trait local" and "type local" cases, but is necessarily a
simplification until Phase 14's real package system exists to check
against directly. Flagged for sign-off, same as every deviation before it.

### A real bug found and fixed during this phase's own verification (not just claimed)
Hand-tracing the `dyn_dispatch_wrong_arg_type_rejected` test (`shape.scale(true)`
where `scale` expects `f64`) before trusting it surfaced a real gap left
over from Phase 3's own fix: the blanket exclusion of `Expr::Literal`
from `check_expr`'s final compatibility check was too broad. It was
written reasoning that "`check_literal` already handles
expected-compatibility internally" — true for `Int`/`Float` (which
adopt/default per rules 2/3) and `Null` (which reports its own specific
error), but **false** for `Str`/`RawStr`/`Char`/`Bool` literals, whose
arms in `check_literal` never compare against `expected` at all — they
just return a fixed type unconditionally. That meant `let x: bool =
"hi";`, or (as this phase's own test would have shown) passing `true`
where a trait method declares an `f64` parameter, would have silently
type-checked with zero errors. Narrowed the exclusion to exactly the
literal kinds that actually self-check (`Int`/`IntHex`/`IntOct`/`IntBin`/
`Float`/`Null`), restoring the blanket check for `Str`/`RawStr`/`Char`/
`Bool`. Confirmed no existing Phase 3 test relied on the old (incorrect)
permissive behavior before finalizing this fix.

### Systematic verification performed (same discipline as Phases 1–3)
- Delimiter-balance check on every touched file — clean, re-run twice
  more after subsequent edits.
- AST enum-variant cross-reference script, re-run three times across
  this phase's edits — clean each time, including after the literal-
  exclusion fix.
- Direct field-name verification for every struct-*variant* of `Expr`
  destructured in `types.rs` (`MethodCall`, `StructLit`, `Call`, `Field`,
  `Index`, `Assign`, `Cast`, `Range`, `Borrow`, `Unary`, `Binary`) against
  the actual `ast.rs` declarations — done explicitly this phase because
  the automated struct-field checker only covers standalone `pub
  struct`s, not enum struct-variants like `Expr::MethodCall { .. }`,
  which it silently can't see. All 11 confirmed correct by direct grep,
  not by trusting the automated script's (incomplete, in this respect)
  "clean" result alone.
- Glob-import audit: no new glob imports this phase; the two present
  (`use crate::ast::*`, `use BinaryOp::*`) are the same ones already
  proven safe in Phase 2/3.
- Proactively checked for a second self-introduced dead-field situation
  before it could become a warning (following the Phase 3 precedent):
  `ImplRecord::trait_name` was written during registration but never
  read anywhere — confirmed via `grep -n "\.trait_name\b"` returning
  nothing. Unlike a case where the data has no real use, this one had an
  obvious, correct, in-scope semantic role (inherent methods should take
  precedence over trait methods of the same name during resolution) —
  implemented that rule, which both fixes the dead-field issue *and* adds
  real, tested method-resolution-order coverage
  (`inherent_method_takes_precedence_over_trait_method_of_same_name`).

### Known simplifications (flagged, not silently claimed as complete)
- **Method-signature matching for completeness checking is name-only,
  not full type-checking** — `register_impls`'s required-method check
  verifies every non-default trait method *name* is provided, but
  doesn't verify the *implementing* method's parameter/return types
  actually match the trait's declared signature (e.g., an `impl` could
  provide `fn name(borrow self) -> i32` where the trait declares `-> String`
  and this wouldn't be caught as a signature mismatch, only as "the
  method exists"). Document 7's examples don't show a signature-mismatch
  test case to ground stricter checking against, and full structural
  signature comparison interacts with `Self`-typed parameters in a way
  that's cleaner to handle once Phase 5's generics work exists (`Self`
  in a trait signature can't resolve to a concrete `Ty` without knowing
  the implementing type, which the checker does know by the time it
  reaches an `impl` block, but treating it fully generally is more
  Phase-5-shaped work).
- **Associated types (`TraitItem::AssocType`, `ImplItem::AssocType`,
  Document 8 §5) are parsed but not semantically checked** — Document
  25 §2.3 scopes associated types to Phase 5 alongside the rest of
  generics, not this phase.
- **No detection of duplicate/conflicting impls** (e.g. two separate
  `impl Describable for Widget` blocks both providing `describe` via the
  *same* trait, which should probably be a coherence error) — only the
  orphan rule itself is checked this phase, not general impl-coherence
  beyond it.

### Exit criteria (Document 25 §2.3) — self-assessment
> "Orphan-rule violation test (Doc 7 §8) correctly rejected; trait
> method resolution correct for both static and `dyn` dispatch cases"

- Orphan-rule violation: rejected, with a dedicated diagnostic message
  (`"orphan rule violation: ..."`), plus two positive counterparts
  (trait-local, type-local) and one confirming inherent impls aren't
  subject to the rule at all. **Pending real `cargo test` confirmation.**
- Static dispatch: resolves through concrete `impl` blocks (inherent and
  trait-provided), checks argument types, rejects undefined methods,
  and correctly orders inherent-over-trait precedence.
- `dyn` dispatch: resolves through the trait's own declared signature
  (not any concrete impl), checks argument types, rejects undefined
  methods, and is confirmed (via a side-by-side test) to genuinely use a
  different resolution path than static dispatch, not the same one.
- **Not yet independently confirmed by a real `cargo test` run.** ⏳

### `.github/workflows/ci.yml`
No changes needed — picks up `tests/traits_impls.rs` automatically.

## Phase 5 — Generics

**Status: 🟡 Code complete, extensively self-verified — pending real CI confirmation**

### Scope (per Document 25 §2.3)
Monomorphization engine, `where`-clause trait bounds (single and
multiple via `+`), associated types (Document 8 §5), and const generics
including the tensor-shape-suffix grammar formalized in Document 23
during Phase 2.

### Genuine tooling event this phase — flagged per standing instruction 6
Mid-phase, this sandbox's home directory was wiped (not something I
did — the container reset, `/home/claude` came back empty). Lost a small
amount of just-started, unpackaged work (the initial `Ty::Generic`/
`Ty::TypeParam`/`GenericArg` additions and `StructShape`/`EnumShape`
generics fields). Recovered by unzipping the last *presented*,
confirmed-good deliverable (`mountain-phase4.zip`, still present in
`/mnt/user-data/outputs/`) and redoing the lost edits from there — this
is exactly why every phase's deliverable gets fully packaged and handed
off rather than left only in-sandbox. No completed/confirmed work was
lost, only a few minutes of the current phase's own in-progress edits.

### What was built
- `Ty::Generic(String, Vec<GenericArg>)` — instantiated generic types
  (`Matrix<f64,2,3>`), `Ty::TypeParam(String)` — unresolved references
  to an enclosing declaration's own generic parameter (e.g. `A`/`B`
  inside `struct Pair<A,B>`'s field types, before instantiation),
  `GenericArg::{Type,Const}` — a single generic argument.
- `StructShape`/`EnumShape` extended with their own `generics: Vec<GenericParam>`;
  field/variant types now pass through `mark_type_params` at
  registration so a generic struct's own type parameters are correctly
  distinguished from references to other concrete structs of the same name.
- `FunctionShape` (replacing Phase 3/4's flat `(Vec<Ty>, Ty)` tuple):
  adds `generics` and flattened `where`-clause `bounds`.
- `TypeChecker::resolve_type_full` — context-aware type resolution for
  *expression*-position type annotations (`let`/cast targets), where
  Document 8 §8's `Matrix<f64,2,3>` needs real arg-count and arg-*kind*
  validation against the struct's declared generic parameters. Plain
  `resolve_type` (unchanged, context-free) still handles declaration-time
  struct-field/fn-param registration — see the scope note below.
- `unify_infer` — structural type-parameter inference for generic
  function calls (Document 8 §2's `largest(nums)` inferring `T = i32`
  from `nums: [i32]` against the declared `list: [T]`).
- `satisfies_bound`/`ty_lookup_name` — where-clause bound checking
  against Phase 4's existing `impls_by_type` registry (no duplicate
  registry built).
- `check_matrix_multiply` — Document 8 §8/§9's dimension check (see the
  flagged design decision below).
- `substitute_type_params` — resolves a generic struct's field types (or
  a generic function's return type) against a concrete instantiation's
  arguments; used by struct-literal checking, field access, and generic
  call return-type resolution.
- `tests/generics.rs` — 12 tests: the core exit-criteria Matrix
  dimension-mismatch case plus its accepted counterpart (with the
  *result shape* verified correct, not just "no error"), a wrong-
  result-shape-annotation negative case, generic-argument count/kind
  mismatches, generic struct field type-checking (including field
  access through substitution), single and multiple (`+`) where-clause
  bounds (both satisfied and unsatisfied), and the static-vs-`dyn`
  resolution proof.

### Design decisions (flagged, not silently assumed)

**Matrix multiplication is a narrow, name-and-shape-triggered special
case, not a general operator-overloading mechanism.** Mountain's spec
doesn't define trait-based operator overloading anywhere in Documents
1–24 — Document 8 §9 is a *traced verification example*, not a runnable
syntax specification for how `*` dispatches. `check_matrix_multiply`
fires specifically for `*` between two instances of the *same* generic
struct carrying exactly two const-generic arguments (matching Document 8
§8's own `Matrix<T, const ROWS, const COLS>` shape exactly), reading the
two const positions positionally (first = rows-analog, second =
cols-analog) per that declaration's literal parameter order. Grounded
directly in Document 8's own concrete example, not invented from
nothing — but a real scope decision, flagged for sign-off.

**Bidirectional/unification hybrid, not full generalized inference.**
Consistent with Phase 3's earlier flagged choice (bidirectional checker,
not full HM), generic-parameter inference here (`unify_infer`) is a
targeted structural walk handling exactly the shapes Document 8's own
examples use (bare type param, one level of array/ref/tuple nesting) —
not a general constraint-solving unifier. An unresolved type parameter
is left unbound rather than reported as an ambiguity error; none of
Document 8's examples hit this case, so it isn't exercised, but it's a
real, flagged gap versus a fully general implementation.

**`resolve_type_full` is scoped to expression-annotation sites, not
everywhere.** Function *parameter* type registration (`fn foo(m:
Matrix<f64,2,3>)`) and struct *field* type registration still use the
plain, context-free `resolve_type` (unchanged from Phase 3), meaning a
generic-struct-typed function parameter's arguments aren't validated for
count/kind at registration time — only `let`-binding and cast-target
annotations get full validation. Chosen because Document 8 §9's concrete
exit-criteria example is phrased as a direct expression
(`Matrix<f64,2,3> * Matrix<f64,4,5>`), which the `let`-binding path
covers completely; extending full validation to every type-annotation
position everywhere was judged to be more scope than the time budget for
this phase could verify carefully. Flagged as a real, known boundary,
not silently claimed as complete coverage.

### A real bug found and fixed during this phase's own verification (not just claimed)
Before writing the Matrix tests, traced the existing `check_binary`
structure and found it would have passed `Some(&lt)` as the rhs's
expected type unconditionally — which works fine for ordinary numeric
literal-adoption (`x + 5`), but for two *different* `Ty::Generic`
operands (`Matrix<f64,2,3> * Matrix<f64,3,5>`, a **valid** multiplication
per Document 8 §9), this would have made the *outer* `check_expr`
wrapper's blanket compatibility check reject the rhs as a plain type
mismatch — before `check_binary`'s own dimension-aware logic ever ran.
That would have made the checker reject every *valid* differently-shaped
matrix multiplication, not just the genuinely invalid ones. Fixed by
only using `lt` as an rhs hint when `lt` isn't a generic-struct type,
letting `check_matrix_multiply`'s own logic — not blind equality — decide
compatibility for that case. Caught by hand-tracing before writing any
test that could have hidden it by coincidence.

### Second real gap found via the standing dead-field sweep
Extended the write-then-grep-`.field`-occurrences sweep (the Phase 3/4
precedent) to every `pub struct` declared in `types.rs` itself this
time, not just the newly-added ones — and found `StructShape.is_tuple`
has **zero** read occurrences anywhere, and has had none since Phase 3
(this is a pre-existing gap that slipped past this exact check in both
prior phases). Traced *why*: tuple-struct positional field access
(`origin.0`, Document 7 §2.4) would require the parser to accept a
numeric-literal token immediately after `.` in postfix position — but
the lexer tokenizes `0` after a dot as an `Int` token, not an
identifier, and `parser.rs`'s postfix-dot handling calls `expect_word()`
(identifier-or-keyword only), which would reject it. So tuple-struct
field access doesn't parse at all yet, which is why `is_tuple` has never
had a consumer. This is a Phase-2-shaped parser gap, not something to
patch mid-Phase-5 without risking an under-verified rushed change — left
unfixed, `is_tuple` left in place (removing it would lose the data
future work needs once the parser gap is closed), and flagged explicitly
here rather than silently carried forward unmentioned a third time.

### Systematic verification performed (same discipline as Phases 1–4)
- Delimiter-balance check after every edit batch — clean throughout.
- AST enum-variant cross-reference script, re-run after all edits — clean.
- **Manual struct-variant field verification** (the Phase 4 lesson,
  applied without relying on the script this time): every
  `GenericParam::Type{..}`/`GenericParam::Const{..}` destructuring in
  `types.rs` checked directly against `ast.rs`'s confirmed real
  declaration (`Type{name,bounds}`, `Const{name,ty}`) via grep, not the
  automated script (which still can't see enum struct-variants).
- Glob-import audit: no new glob imports this phase; the two present
  are the same ones already proven safe in Phases 2–4.
- **Comprehensive dead-field sweep across every `pub struct` in
  `types.rs`** (not just the phase's new additions) — this is what
  caught `StructShape.is_tuple` above, and confirmed every other field
  (all of `FunctionShape`, `StructShape.generics`, `EnumShape.generics`,
  etc.) has real, verified read-sites.
- Hand-traced the full Matrix dimension-check pipeline token-by-token
  for both the rejected case (2×3 · 4×5) and the accepted case (2×3 ·
  3×5, confirming the *result type* is genuinely `Matrix<f64,2,5>`, not
  just "no error raised") against the actual current code before
  trusting either test.

### Known simplifications (flagged, not silently claimed as complete)
- **Associated types are registered structurally but not semantically
  checked.** `TraitItem::AssocType`/`ImplItem::AssocType` parse and
  don't cause errors, but there's no completeness check (a trait
  declaring `type Item;` doesn't require an implementing `impl` to
  provide `type Item = ...;`) and no `Self::Item`-style resolution.
  Document 8 §5's own example (`Container`/`Stack<i32>`) is the concrete
  grounding for what a fuller implementation would need to cover; not
  attempted this phase given the scope already covered.
- **Generic function *bodies* aren't meaningfully re-checked per call
  site** — `check_fn` binds a generic function's own parameters via
  plain `resolve_type` (giving `Ty::Named("T")` for a type parameter
  named `T`, not `Ty::TypeParam("T")`), so expressions inside a generic
  function's body that reference its own type parameters don't resolve
  meaningfully during body-checking. This is a safe failure mode (no
  method/field lookup on a type parameter produces a false accept — it
  just silently doesn't validate, since `known_base` correctly reports
  `false` for an unrecognized bare name like `"T"`), not a source of
  incorrect acceptance — real monomorphized-body verification per
  concrete instantiation is closer to Phase 10 (actual codegen) territory.
- **`is_tuple` gap** — see the dedicated section above.
- **Const-generic values are integer literals only** (`const_int_value`)
  — no const-generic arithmetic expressions (`N + 1`), matching every
  example in Documents 1–24, none of which show anything more complex.

### Exit criteria (Document 25 §2.3) — self-assessment
> "`Matrix<f64,2,3> * Matrix<f64,4,5>` dimension mismatch correctly
> caught as a compile-time error, not a runtime error; monomorphized
> output verified to contain zero `dyn`-style dispatch for generic-only code"

- Matrix dimension mismatch: rejected with a dimension-specific
  diagnostic; the valid, differently-shaped counterpart is correctly
  *accepted* with the *correct* result shape verified by a dedicated
  negative test (wrong-shape annotation on an otherwise-valid
  multiplication is rejected). **Pending real `cargo test` confirmation.**
- Zero-`dyn`-dispatch: demonstrated via `satisfies_bound`/`ty_lookup_name`
  having no code path that could succeed for `Ty::DynTrait` — a generic
  call's bound-check only passes when the substitution produced a
  concrete, registry-lookupable type, which is direct, checkable
  evidence the substitution never used the `dyn` machinery Phase 4 built.
- **Not yet independently confirmed by a real `cargo test` run.** ⏳

### `.github/workflows/ci.yml`
No changes needed — picks up `tests/generics.rs` automatically.

### CI Run — 62/65 integration tests pass; 3 failures, all in `tests/generics.rs`, all parse errors (never reached the generics logic)
All three failures traced to the actual reported error before fixing,
and the root cause turned out to be **narrower and different** than the
initial hypothesis — worth recording precisely:

- **Initial hypothesis** (reasonable from the error text alone): `Self`
  used as a parameter *type* (Document 7 §4.1's `other: borrow Self`)
  wasn't handled by `parse_type()`.
- **Actual root cause, found by tracing the exact reported error
  (`"expected \`)\`, found Keyword(SelfType)"` at the precise column) against
  the real code**: `Self` *alone* already parsed correctly (an existing
  fallback branch in `parse_type()` already handled `Keyword::SelfType`).
  The real gap was one step earlier: `parse_type()` had **no branch for
  `Keyword::Borrow` at all**. When it hit `borrow` (in `other: borrow
  Self`), it fell through to the generic keyword-as-word fallback and
  silently *mis-parsed the word "borrow" itself as a bogus type name*
  (`Type::Named("borrow")`) — succeeding without an error at that point,
  then leaving `Self` as an unconsumed leftover token, which is what
  actually produced the reported error one token later, at the closing
  paren. Confirmed via `grep -n "borrow\|Self" tests/traits_impls.rs`:
  every `borrow` in Phase 4's passing tests is the *parameter*-position
  form (`borrow self`, `borrow mut self`); none is in *type* position
  (after a colon), and bare `Self`-as-a-type never appears at all —
  confirming this was a real, previously-uncovered gap, not a
  regression.
- **Fix, applied once in `parse_type()` itself** (not per-call-site),
  so it's closed uniformly for every type position that flows through
  that function — return types, struct fields, generic arguments,
  where-clause bound types, not just function parameters: added a
  `Keyword::Borrow` branch mirroring the existing `&Type` branch exactly
  (`borrow Type`/`borrow mut Type` reuses the existing `Type::Ref`
  variant — checked `ast.rs` first, per standing practice, before
  considering a new variant; none was needed).
- **Second half of the fix**: even with parsing fixed, `types.rs` needed
  to actually resolve `Self` to the enclosing `impl`'s concrete type.
  Fixed in two places, both reusing the *existing* `mark_type_params` +
  `substitute_type_params` pipeline (treating `Self` as a one-element
  generic-parameter substitution scoped to the current impl, rather than
  writing a third, separate substitution mechanism):
  - `check_fn` — so a method body's own `Self`-typed parameters/return
    type resolve to the concrete impl type while checking that body.
  - `register_impls` — found by explicitly checking "could this same gap
    bite anywhere else", per the standing instruction, rather than
    stopping at the one call site the error pointed to: the *registered*
    method signature (what `resolve_method` hands back to a real call
    site elsewhere in the program) was **also** left with `Self`
    unresolved, since it used plain `resolve_type` independently of
    `check_fn`. Not yet exercised by any test (nothing in the current
    suite calls `.compareTo()` on a value through the registry), but the
    same bug class — fixed the same way, proactively, rather than
    leaving a known-but-unexercised gap for a future phase to rediscover
    the hard way.
- **One deliberately unaddressed, flagged consideration found along the
  way**: a *trait's own* declared signature (as opposed to an `impl`'s)
  correctly leaves `Self` unresolved (appropriate — a trait has no one
  concrete `Self` until implemented), but this means a `dyn Trait` method
  whose signature mentions `Self` in parameter position (like
  `compareTo`) can't be soundly type-checked against a real argument
  through dynamic dispatch — this mirrors real object-safety rules
  (Rust restricts exactly this class of method from `dyn` use for the
  same underlying reason). Not solved this round — no current test
  exercises calling `Self`-parameterized methods through `dyn` — but
  worth surfacing rather than leaving implicit.

Re-verified with the same standing checks (balance, AST cross-reference
script, glob-import audit — all clean) plus hand-tracing all three
previously-failing tests token-by-token through both fixes, and
re-checking all nine previously-passing tests for whether either fix
could have changed their behavior (confirmed: no — the parser fix is
purely additive for a previously-unparseable construct, and the
`types.rs` fixes are no-ops whenever no `Self` reference is actually
present, which is true for all nine).

## Phase 6 — Borrow Checker
**Status: 🟢 Complete — confirmed by real CI.** Actual downloaded CI
log: **138 tests total, 0 failures, 0 warnings** (57 lib + 16
`borrow_checks` [pre-correction count; see "Post-review correction"
below for the current, re-verified 17] + 12 `generics` + 6
`integration` + 6 `parser_doc24` + 15 `traits_impls` + 26 `types_doc5`).
Clean run, no regressions in any Phase 1–5 test file.

Per Document 25 §3 Rule 4 this is the single most foundational phase in
the roadmap, so it got the most conservative possible treatment: read
Document 6 in full plus Document 17 §4.4's Polonius/CFG guidance before
writing any code, then built `src/borrow.rs` as a **standalone pass**
(new `BorrowChecker`, run after — and independent of — `types.rs`'s
`TypeChecker`, touching zero existing Phase 1–5 code except two
one-line additions: `pub mod borrow;` in `lib.rs`, nothing else). This
was a deliberate risk-reduction choice: the already-verified Phase 3–5
`types.rs` internals are not exposed or modified, so there is no way
for Phase 6 to have regressed anything that was previously green.

### What it checks (Document 6)
1. **Move semantics** (§2, §4.1): `let b = a;` and by-value
   call-argument moves (`consume(alice)`), with use-after-move
   correctly rejected.
2. **Copy-type transitivity** (§4.2): a struct is Copy only if marked
   `@copy` (see "Flagged decision" below) AND every field is itself
   Copy, computed via fixed-point iteration so nested `@copy` structs
   compose correctly and a `@copy` struct with a `String` field is
   correctly rejected back down to non-Copy.
3. **The aliasing rule** (§3.1): any number of active shared borrows,
   or exactly one active mutable borrow, never both — checked
   symmetrically (mut-while-shared, shared-while-mut, mut-while-mut).
4. **Non-lexical borrow liveness** (§3.2) — see "The §3.1/§3.2 tension"
   below; this was the trickiest part of the whole phase.
5. **Escaping references** (§5.1): a `let` binding initialized from a
   call to a function whose signature both takes and returns
   reference-shaped values is tracked as borrowing from every
   `borrow`-argument passed at that call site; using it after any one
   of those sources has gone out of scope is rejected.

### The §3.1/§3.2 tension — a real interpretive decision, flagged for sign-off
Document 6 §3.1's own example creates `ref1`/`ref2` (immutable borrows
of `counter`) and states the following `borrow mut counter` (`ref3`)
must be **rejected** — but `ref1`/`ref2` are never read again anywhere
in that example. Taken completely literally, real-world NLL (and real
rustc) would actually **accept** this, since an unread borrow has a
zero-length live range and is dead before `ref3` is ever created — this
is the canonical example used to explain NLL. That would contradict
Document 6's own stated outcome. §3.2 then shows the opposite pull:
`ref1` *is* read once (`print(ref1)`), and that read — occurring before
`ref2`'s mutable borrow, in the *same* block — must make the code
**accepted**, i.e. borrows really do end at last-use, not block-end.

Resolution implemented (see `borrow.rs`'s module doc, "NLL
approximation" — full reasoning there): **a borrow is live from
creation until its last actual textual read within its enclosing
block; if it has no later read at all, it is conservatively treated as
live through the end of the block** (rather than dead immediately).
This is the only rule that reproduces *both* of Document 6's own
stated outcomes exactly — verified by two dedicated tests
(`doc6_s3_1_mutable_while_immutable_active_rejected` and
`doc6_s3_2_nll_borrow_ends_at_last_use`) plus a third sanity check
(`doc6_s3_2_nll_still_rejects_if_conflicting_borrow_precedes_last_use`,
confirming a still-needed-later borrow correctly still blocks). It is
conservative relative to true NLL (may reject some code real Rust would
accept — e.g. a borrow that's *truly, permanently* unused) but never
accepts a genuine aliasing violation, which is the direction that
matters for soundness. **This is a real interpretive call on a genuine
tension between two of the document's own examples, not an arbitrary
implementation detail — flagged explicitly for your sign-off**, same as
prior phases' grounded-but-not-explicitly-spelled-out decisions.

### Flagged decision: `@copy` struct-marking syntax
Documents 3/6 define what the `copy` keyword *means* but never show a
concrete syntax for marking a *struct* as Copy (only primitives are
shown as inherently Copy). Grounded in the existing, already-parsing
`@attribute` mechanism (Document 23 §14's `attribute ::= "@" IDENT
(...)?"`, which already prefixes any item generically) rather than
inventing new syntax or touching the parser at all — `@copy` on a
`struct` just works today with zero parser changes, verified by reading
`parse_attrs`/`parse_item` directly. **Flagged for explicit sign-off**,
same category as Phase 2's own flagged EBNF-gap decisions.

### Known, explicitly-documented scope limits (not silently shipped)
All spelled out in `borrow.rs`'s module doc in full, summarized here:
- **Same-block-only NLL scan.** The last-use search only looks at the
  *directly enclosing* block's own statements (recursing into
  sub-expressions, but not treating a read inside a nested
  block/closure/branch as shortening the *outer* block's liveness
  computation). Safe direction (under-counts liveness → more
  conservative, never less), just less precise than a full CFG walk.
- **Branches (`if`/`match`/loop) are checked independently, not
  merged.** A real violation *within* one branch is still caught, but
  a branch's moves/borrows don't propagate to sibling branches or to
  code after the construct (e.g. an unconditional move inside one `if`
  arm, followed by unconditional reuse after the `if`, is NOT flagged).
  None of Document 6 §8's five required cases involve branching, so
  this doesn't affect the exit criteria, but it's a real gap beyond
  them.
- **Move-checking is precise only for the exact shapes Document 6 §2/§4
  show**: bare `let b = a;` and by-value call arguments. A binding
  initialized from any other expression this pass can't syntactically
  classify (a function call's return value, a method call, a
  binary/cast expression) is left move-*unchecked* rather than guessed
  at — mirrors an existing, already-established precedent in
  `types.rs` itself (`MethodCall`'s unknown-method case explicitly
  "stays silent rather than fabricating a false error"). Concretely:
  `let a = makeString(); let b = a; print(a);` is NOT caught unless
  `makeString`'s return type is independently obvious to this pass. A
  non-Copy variable used twice as different *field values* inside one
  struct literal (`Line { a: p1, b: p1 }`) is similarly not flagged as
  a double-move. Building full move-checking would mean duplicating (or
  exposing) `types.rs`'s real inference engine — deliberately not done,
  to avoid a second, potentially-divergent type system and to avoid
  touching the verified Phase 3–5 code at all.
- **Closures**: capture-mode inference is explicitly Phase 7 territory
  (already flagged as such in `types.rs`'s own Phase 3/5 comments, and
  Document 25 lists it under Phase 7). This pass still walks into
  closure bodies to catch use-of-already-moved-outer-variable, but does
  not push a fresh scope for closure parameters — a closure parameter
  that happens to shadow an outer (moved) binding's name could
  currently produce a false positive. Not exercised by any test written
  this phase; flagged for Phase 7 to resolve properly alongside real
  capture-mode analysis.
- **Destructuring `let` patterns** (tuple/array/tuple-struct) aren't
  tracked at all — same, already-established gap `types.rs` itself
  documents for the identical case (a safe failure mode: no false
  errors, just no protection for those bindings yet).

### Self-verification performed (no compiler available)
Given zero `cargo`/`rustc` access, verification was done entirely by
hand, at two levels:
1. **Every AST shape used was checked against the actual current
   `ast.rs`**, not assumed from memory or from Document 23 — every
   `Box<T>` vs. plain `T` distinction across all ~36 `Expr` variants,
   `Stmt`'s 8 variants, `IfExpr`/`MatchExpr`/`MatchArm`/`LoopExpr`/
   `ClosureExpr`/`ElseBranch`/`ClosureBody` was individually grepped
   and cross-checked (e.g. `Stmt::Return(Option<Expr>)` is *unboxed*
   while `Expr::Return(Option<Box<Expr>>)` is boxed — confirmed both
   handled correctly in their respective call sites).
2. **A real, caught bug found this way, before any test was written**:
   the first draft ran the generic recursive expression-checker over a
   `let ref1 = borrow counter;` initializer, which hit the
   general-purpose `Expr::Borrow` handling (meant only as a fallback
   for borrows in call-argument/other positions) and registered `ref1`
   as an immediately-*expiring temporary* borrow instead of a real,
   named, persistently-tracked one — silently making Document 6 §3.1's
   required-rejection case incorrectly pass. Root-caused (not
   patched at the symptom) by introducing one shared `declare_binding`
   helper used by *both* `Stmt::Let` and `Expr::Assign` (the two call
   sites with the same underlying shape), rather than a local patch to
   one of them.
3. **A Rust-borrow-checker risk in `borrow.rs`'s own code**, found by
   inspection rather than compilation: `register_borrow` originally
   held a `&mut PlaceBorrows` (from `.entry().or_default()`) across
   calls to `self.err(...)` (which needs `&mut self`). Modern NLL very
   likely accepts this (each conflicting branch returns immediately
   after), but since this can't be compile-tested here, it was
   restructured to read everything needed into owned locals *before*
   any `self.err(...)` call — unambiguously correct under any borrow
   checker, not a bet on NLL nuance. Same treatment applied to
   `check_fn`'s construction of the per-function `FnChecker` (which
   reborrows `self`): explicitly scoped with a `{ }` block so the
   reborrow is lexically dropped before `self.errors.extend(...)` is
   called, rather than relying on field-level NLL liveness across
   `FnChecker`'s `checker` and `errors` fields.
4. **Every one of the 17 tests in `tests/borrow_checks.rs` was hand-
   traced statement-by-statement against the actual algorithm** (not
   just "should work" reasoning) — including full state traces of
   `place_borrows`/scope stacks through both the accepted and rejected
   variants of the §3.1, §3.2, and §5.1 cases. All 17 traced to the
   expected `assert_ok`/`assert_rejected` outcome.
5. Dead-code sweep: removed a `ParamRefInfo`/`FnRefShape.params` field
   that was constructed but never read (would have been a compiler
   warning, breaking the project's 0-warnings standard) once it became
   clear the escaping-reference heuristic only needs the callee's
   return-type shape, not its declared parameter shapes.
6. Brace/paren balance double-checked via script (both 0 delta) as a
   final gross-error sanity check.

### Document 6 §8's five required verification cases — mapped to tests
| §8 case | Test(s) in `tests/borrow_checks.rs` |
|---|---|
| §3.1 aliasing rule (rejected) | `doc6_s3_1_mutable_while_immutable_active_rejected` + 2 supporting (`two_shared_borrows_alone_are_fine`, `two_mutable_borrows_rejected`) |
| §3.2 NLL (accepted) | `doc6_s3_2_nll_borrow_ends_at_last_use` + 1 supporting sanity check |
| §5.1 lifetimes/escaping ref (rejected) | `doc6_s5_1_escaping_reference_used_after_source_scope_ends_rejected` |
| §5.1 in-scope use (accepted) | `doc6_s5_1_escaping_reference_used_while_source_still_in_scope_ok` |
| §4.2 Copy-struct transitivity | `doc6_s4_2_copy_struct_with_string_field_does_not_bypass_move` + 2 supporting (all-Copy-fields case, nested-Copy-of-Copy case) |
| §6 Rc/Arc never silently substituted | `doc6_s6_no_silent_rc_substitution_ordinary_move_still_rejected` |

Plus 5 supporting tests for the underlying move/borrow mechanics those
five cases depend on (§2's basic move, §4.1's call-argument move, §3's
temporary-borrow-as-argument, primitive Copy types) — **17 tests
total**, all hand-traced to their expected result. Real pass/fail
counts are what CI reports, not what's written here (per standing
instruction #2) — see "How to verify" below.

### `.github/workflows/ci.yml`
No changes needed — picks up `tests/borrow_checks.rs` automatically
(same pattern as every prior phase).

### How to verify (this environment has no cargo/rustc/network)
**Original round (confirmed):** CI reported 138 total / 0 failures / 0
warnings, with `borrow_checks` contributing 16.

**This correction round (pending):** push this commit and check the
Actions log again. Expect `cargo build --verbose` still clean (0
warnings — watch specifically for `borrow.rs`, since the `expr_reads_ident`
Spawn/Select/Query change is new code, not just edited comments), and
`cargo test --verbose` showing all other test files' counts unchanged
from the confirmed run above (57 lib / 12 generics / 6 integration / 6
parser_doc24 / 15 traits_impls / 26 types_doc5 — 122 total outside
`borrow_checks`) plus `borrow_checks` now reporting **17/17 passed** (up
from 16, per the added test). If any of the 17 fail, the most likely
culprit is the strict-NLL fallback change in `compute_last_use` — check
first whether the failing case involves a borrow that's read only
*after* a point being checked (the "full forward scan, not just before
this point" behavior that `doc6_s3_1_mutable_while_immutable_active_rejected`
and `doc6_s3_1_two_mutable_borrows_rejected` both specifically depend
on).

### Exit criteria (Document 25 §2.3) — self-assessment (pre-correction)
> "Every rejected-code example in Document 6 §8 is correctly rejected;
> every accepted example is correctly accepted (including the NLL
> example — borrows ending at last-use, not block-end)."

- All five §8 cases implemented and hand-traced to the correct outcome
  in both directions where the doc shows both.
- Two decisions genuinely required interpretive judgment beyond what's
  explicitly spelled out in the document (the §3.1/§3.2 NLL tension,
  and the `@copy` struct-marking syntax) — both resolved with reasoning
  grounded directly in the document's own text and existing project
  precedent, both **explicitly flagged above for your sign-off** rather
  than silently decided.
- Known scope limits (branch-merging, closure capture interaction,
  move-checking breadth) are real and stated plainly, not hidden —
  none of them affect the five required cases.

**Real CI result: 138 tests, 0 failures, 0 warnings.** `borrow_checks`
contributed 16 (not the 17 estimated above — a hand-trace count, not an
actual count; noted so the gap between estimate and reality is on the
record, same standard as the Phase 4 count correction).

### Post-review correction: Document 6 §3.1 itself was wrong, not just my implementation of it
After the CI confirmation above, review caught that the §3.1/§3.2
tension flagged earlier wasn't an implementation ambiguity to route
around — Document 6 §3.1's *own example* was incorrect. Its original
form (`ref1`/`ref2` created, never read, `ref3` then claimed to be a
compile error) is not what genuine NLL — which §3.2 and Document 25's
exit criteria both explicitly require ("not merely conservative") —
actually does: an unread borrow has a zero-length live range and real
rustc accepts that case. Document 6 §3.1 has been corrected upstream
(now attached separately) to add `print(ref1)`/`print(ref2)` calls that
make both borrows genuinely live at `ref3`'s creation point, with an
explanatory note on the correction itself.

**Implementation updated to match — genuine strict NLL, no conservative
fallback:**
- `compute_last_use`'s "never read anywhere in the block" case now
  returns `decl_idx` (dies immediately, at the very next statement)
  instead of `block.stmts.len()` (the old "conservatively lives to
  block end" fallback). The "found a real later read" case — including
  reads that occur *after* the point currently being checked, since
  `compute_last_use` does one full forward scan of the block at
  declaration time — is unchanged and was already correct.
- **A real, narrow soundness consequence of that change was caught and
  fixed before it shipped, not after**: under the *old* fallback,
  `expr_reads_ident` failing to find a genuine read anywhere was always
  safe (worst case, over-conservative). Under the *new* fallback,
  a missed read would make a still-live borrow look dead — a real gap
  in the *unsound* direction. Auditing `expr_reads_ident` for exactly
  this found one real instance: `Expr::Spawn`/`Select`/`Query` bodies
  (whose internals aren't analyzed anywhere else in this pass either,
  deferred to Phases 12/18/20) previously returned `false`
  unconditionally ("assume no read"). Changed to `true`
  ("conservatively assume it might read anything") specifically because
  the risk profile of that gap changed with the algorithm — not because
  the gap itself was new. No Document 6 §8 case exercises these
  constructs, and this doesn't reintroduce the old fallback's
  imprecision anywhere else.
- **A second, unrelated documentation bug was caught during the same
  audit**: the module doc's "Same-block scope only" note claimed a read
  inside a nested block/if/match-arm/loop body would NOT be found by
  the last-use scan. Re-checking `stmt_reads_ident`/`expr_reads_ident`
  against the actual code showed this was simply inaccurate — they
  already recurse arbitrarily deep into nested constructs via mutual
  recursion with `block_reads_ident`, so any syntactically-legal read
  (which, under ordinary lexical scoping, can only ever occur within
  the borrow's own enclosing block's statement tree anyway) is already
  found. The doc comment described a limitation that doesn't actually
  exist in the code; corrected to state what's actually true instead of
  leaving an inaccurate "known limitation" on the record.

**Test file updated, all 17 tests re-traced by hand (not just the ones
that changed):**
- `doc6_s3_1_mutable_while_immutable_active_rejected` — rewritten to
  match the corrected doc example exactly (`print(ref1)` before `ref3`,
  `print(ref2)` after). Re-traced: `ref1` expires normally before
  `ref3` (its real last use was before `ref3`); `ref2`'s later read is
  what the algorithm actually detects as still-active at `ref3`'s
  creation point — matching the corrected doc's own reasoning
  ("ref2... reinforcing the rejection") precisely.
- `doc6_s3_1_two_mutable_borrows_rejected` — **this one was NOT
  mentioned in the review feedback, but tracing all 15 other tests by
  hand (per the standing instruction to check for silent behavior
  changes, not just the one explicitly called out) found it would have
  silently flipped from rejected to accepted**: its `ref1` was never
  read, so under strict NLL it would expire before `ref2`'s conflicting
  mutable borrow, and real Rust does in fact accept two sequential,
  never-reused mutable borrows. Fixed by adding `print(ref1)` after
  `ref2`'s creation, mirroring the same "later read still counts" shape
  as the corrected §3.1 case, restoring a genuine conflict.
- `doc6_s3_1_unread_borrow_does_not_block_later_mutable_borrow` — new
  test, added specifically to pin down the strict-NLL behavior in the
  *accept* direction (the converse of the case above), which wasn't
  independently tested before.
- All other 14 tests re-traced statement-by-statement against the new
  algorithm and confirmed unaffected: the §3.2 pair and the two §5.1
  escaping-reference tests all rely on the "found a real read" branch
  of `compute_last_use` or (for §5.1's call-argument temporary borrows)
  don't use `compute_last_use` at all; all Copy-struct, move-checking,
  and Rc/Arc tests don't touch borrow/NLL logic whatsoever.
- Net: **17 tests** in `borrow_checks.rs` now (16 before, plus the one
  new confirming test) — **not yet independently confirmed by a real
  CI run; this correction was made in this same session, after the CI
  result reported above.** ⏳

### Exit criteria — current status
All five §8 cases (now matching the *corrected* §3.1) hand-traced
correct, including the specific "not merely conservative" requirement —
the implementation no longer has a conservative fallback anywhere in
the borrow-aliasing/NLL logic. **Needs one more real CI run** to
confirm the 17 (up from 16) `borrow_checks` tests pass and nothing
else regressed, before Phase 6 can be called fully closed on this
correction. Both decisions flagged in the original write-up above
remain **resolved** (the `@copy` syntax approved as-is; the NLL
tension turned out to be a spec bug, now fixed upstream, with the
implementation brought in line).

## Phase 7 — Functions & Closures
**Status: 🟢 Complete — confirmed by real CI.** Actual downloaded CI
log: **149 tests total, 0 failures, 0 warnings** (57 lib + 17
borrow_checks + 10 closures + 12 generics + 6 integration + 6
parser_doc24 + 15 traits_impls + 26 types_doc5). Matches the predicted
counts exactly (17, up from 16 post-NLL-correction; 10 new closures
tests). Clean run, no regressions.

Scope per Document 25 §2.3: parameters with ownership-annotated
signatures (already largely in place from Phase 6, since `check_fn`
already reads each param's `OwnershipMod`), closures with automatic
capture-mode inference (Document 10 §4.3), and `move`'s interaction
with lifetime analysis — built as a direct extension of Phase 6's
`borrow.rs`, reusing its exact machinery rather than adding a parallel
closure-specific system, per the exit criteria's explicit requirement.

### What was built
All in `mtnc/src/borrow.rs` (no other file touched except the new
`tests/closures.rs`):
- **Closures now get their own scope** for their parameters (Phase 6
  had flagged this as a known gap: without it, a closure parameter
  sharing an outer binding's name could incorrectly resolve to the
  outer one).
- **Capture tracking** (`CaptureFrame`, `capture_stack`, `lookup_scope_idx`,
  `record_capture_if_outer`): reuses the *exact same* scope-based
  `use_ident`/assignment machinery already built in Phase 6 — a closure
  body is walked completely normally, and any identifier that resolves
  to a scope outer to the closure's own is recorded as a capture (and
  as needing a mutable borrow if it's ever an assignment target,
  covering both `=` and every compound-assignment operator).
- **Capture application** (`apply_closure_captures`): for a `move`
  closure, every capture is transferred by ownership (`mark_moved`,
  skipped automatically for Copy types, same rule as everywhere else);
  for a non-`move` closure, every capture is registered as a borrow via
  `register_borrow` — the *same* function Phase 6's named `let r =
  borrow x;` uses, with the same aliasing (§3.1) and strict-NLL (§3.2)
  checks applying automatically, no separate code path.
- **Escaping closures**: a closure literal bound to a name has that
  name's `Binding.borrow_sources` set to its captured places — the
  *same* field Document 6 §5.1's escaping-reference check already
  reads. Using the closure after a captured source has gone out of
  scope is therefore caught by code that hasn't changed since Phase 6.
- A closure literal used unbound (passed directly as a call/method
  argument) gets the same analysis with its borrows treated as
  temporary (expiring at the end of the current statement), mirroring
  Document 6 §3's `printName(borrow alice)` shape.

### A real, previously-latent bug found and fixed at the root (not patched for the closure case)
Implementing mutable-capture tracking required routing every
`Expr::Assign` through code that could tell `x = y` apart from `x +=
y`. Tracing this surfaced that **the existing Phase 6 code didn't
distinguish them at all** — every assignment, including every compound
form, was routed through `declare_binding`, which fully *reclassifies*
the target from the RHS's shape (correct for plain `=`, but wrong for
`+=`/`-=`/etc., which read-modify-write the target's own existing value
and shouldn't adopt the RHS's `is_copy`/`borrow_sources` at all). This
was never caught in Phase 6 because no Phase 6 test used compound
assignment. Fixed generally: `AssignOp::Eq` still reclassifies via
`declare_binding`; every other `AssignOp` now keeps the target's own
binding state, only checking it isn't already moved and walking the
RHS. Not just a Phase 7 patch — this fixes real, if previously
unexercised, incorrect behavior for compound assignment anywhere in the
language, not only inside closures.

### A second, more serious bug found and fixed before it could ship: false-positive move on ordinary reads
Hand-tracing `|| print(count)` with a **non-Copy** type in place of
Document 10 §4.3's own `count: i32` (i.e. checking the doc's own
example pattern still holds when the captured type isn't Copy, since
nothing in the doc says it has to be) found that the closure body-walk
would incorrectly **move** the captured variable out of the outer
scope — because the ordinary by-value-call-argument move-check
(`check_call_args`, Document 6 §4.1) has no notion of "I'm currently
walking a closure body for capture analysis" and fires exactly as it
would for a plain, non-closure function call. This is not a narrow edge
case: it would misfire on any non-move closure that merely *reads* a
non-Copy variable through an ordinary (non-`borrow`-wrapped) function
call — a very natural thing to write. Root-caused and fixed directly in
`mark_moved`: while inside a closure body being walked, a move onto a
binding that resolves outer to that closure's own scope is suppressed,
deferring entirely to `apply_closure_captures`'s single, authoritative,
post-walk decision. Verified this doesn't weaken the `move`-closure
case (Test `doc10_s4_3_move_closure_moves_capture_reuse_rejected`):
`apply_closure_captures`'s own move branch runs *after* the capture
stack is popped, so its `mark_moved` call is never suppressed — the
fix only ever removes a *premature*, mid-walk move-marking, never the
final, correct one.

### A third gap found, NOT fixed (out of Phase 7's scope, flagged rather than hidden)
While designing the test for the bug above, found that **this pass
never checks a move against currently-active borrows of the same
place** at all — `let r = borrow x; /* r still needed later */ let y =
x;` moves `x` without complaint even though `r` may still be used. This
is a pre-existing Phase 6 gap (Document 6 §8's own cases don't exercise
it either), not something Phase 7 introduced or worsened. Not fixed
now — flagged in `borrow.rs`'s module doc and here, for a future phase
to close (likely: consult `place_borrows` from the move-marking paths,
symmetric to how borrow-creation already consults it).

### Document 10 §4.3 examples and exit criteria — mapped to tests (`tests/closures.rs`, 10 tests)
| Case | Test |
|---|---|
| Read-only capture (`\|\| print(count)`) | `doc10_s4_3_readonly_capture_ok` |
| Mutable capture via `+=` (`\|x: i32\| total += x`) | `doc10_s4_3_mutable_capture_via_compound_assign_ok` |
| `move` capture, reuse rejected | `doc10_s4_3_move_closure_moves_capture_reuse_rejected` |
| `move` capture of a Copy value, reuse still OK | `doc10_s4_3_move_closure_copy_capture_reuse_still_ok` |
| **Exit criteria**: non-move closure capturing a borrow that doesn't outlive its use → rejected, via §5.1's escaping-reference mechanism | `phase7_escaping_closure_rejected_after_captured_source_out_of_scope` (+ converse `..._still_in_scope_ok`) |
| **Exit criteria**, second angle: non-move closure's mutable capture conflicting with a still-live shared borrow → rejected, via §3.1's aliasing mechanism (the *same* code path that rejects `ref3` in `borrow_checks.rs`) | `phase7_closure_mutable_capture_conflicts_with_active_shared_borrow` |
| Phase 6 gap fix: closure param shadows an outer moved binding | `phase7_closure_param_shadows_outer_moved_binding` |
| Bug-fix regression pin: non-move closure reading a non-Copy capture doesn't move it | `phase7_nonmove_closure_reading_noncopy_capture_does_not_move_it` |
| Robustness: unbound closure argument doesn't error/crash | `phase7_unbound_closure_argument_does_not_error` |

All 10 hand-traced statement-by-statement against the actual algorithm
(same discipline as Phase 6), including full `capture_stack`/`scopes`/
`place_borrows` state traces through the two exit-criteria tests. All
16 (post-correction) `borrow_checks.rs` tests were also re-traced
against the `Expr::Assign`/`mark_moved` changes to confirm no
regression — neither change alters any path those tests exercise
(none use compound assignment or closures).

### How to verify (no cargo/rustc/network here)
Push and check the Actions log. Expect `cargo build --verbose` clean (0
warnings), and `cargo test --verbose` showing the confirmed Phase 6
counts unchanged (57 lib / 12 generics / 6 integration / 6
parser_doc24 / 15 traits_impls / 26 types_doc5 / 17 borrow_checks — the
last pending its own re-confirmation per the correction above) plus a
new `closures` binary reporting **10/10 passed**.

### Exit criteria — self-assessment
> "The closure capture-mode inference test suite must pass, and a
> non-`move` closure that captures a borrow which would not outlive the
> closure's own use must be correctly rejected by the borrow checker,
> not accepted by some separate, closure-specific escape hatch."

Both halves hand-traced correct, and — importantly — verified
structurally, not just by outcome: the escaping-closure rejection goes
through the identical `Binding.borrow_sources`/`is_in_scope` check
Phase 6 wrote for function-returned references, and the aliasing
rejection goes through the identical `register_borrow` conflict check
Phase 6 wrote for named borrows. No new rejection mechanism was added
anywhere in this phase — only new *callers* of Phase 6's existing ones.
**Confirmed by real CI: 149 total, 0 failures, 0 warnings** (17
borrow_checks + 10 closures, both hand-trace predictions exactly
right). Two bugs were found
and fixed at the root while implementing this (compound-assignment
reclassification; premature move during closure-body walks), and one
further pre-existing gap was found and explicitly flagged rather than
fixed (move-vs-active-borrow) — all three documented in `borrow.rs`
itself and above, not left implicit.

## Phase 8 — Full Control Flow
**Status: 🟡 Implemented, hand-traced, ⏳ pending real `cargo test`/CI confirmation** (no cargo/rustc in this environment, same as every phase)

Scope per Document 25 §2.3: `if`/`match` exhaustiveness via real
pattern-matrix decomposition (Document 9 §2.5, Document 17 §4.5's
Maranget reference), all four loop forms with labeled `break`/
`continue` (Document 9 §3), and `yield` generators lowered to an
explicit state machine (Document 9 §4). This phase touched more
existing, previously-verified code than any prior one — including a
foundational lexer gap that had been latent since Phase 1 — so extra
weight was put on tracing every change against the actual current
source before writing tests, per standing instruction #9.

### 1. A foundational, previously-latent lexer gap: no lifetime/label token existed at all
Labeled loops (Document 9 §3.5's `'outer: for ...`) and `&'a Type`
(Document 6 §5.1) both need a `'ident` token. Checking the actual
parser (not assuming) showed every loop-label field was hardcoded to
`None` specifically because the lexer had never tokenized `'ident` at
all — `lex_char` unconditionally treated a leading `'` as the start of
a char literal, so `'outer:` would have produced a hard "unterminated
char literal" lex error, not just a silently-dropped label. This had
never surfaced before because no prior phase's tests happened to write
`'a`-shaped source (Phase 6's own `longest` test deliberately used
`&str` without an explicit lifetime, which in hindsight was already
sidestepping this).

Fixed at the root, in the lexer: `lex_char` now disambiguates exactly
the way real-world Rust's lexer does — an identifier-start character
immediately followed by a second `'` is a char literal (`'a'`); an
identifier-start character NOT immediately closed is a lifetime/label
token (`'a`, `'outer`); anything else (escapes, digits, symbols) is
unaffected, taking the exact same path as before. Verified against the
one pre-existing char-literal test (preserved exactly) plus three new
inline lexer tests for the escape/digit-unaffected case and both
lifetime shapes (bare label, single-letter type-position lifetime).

Wired into every place that needed it: loop-label parsing (`'outer:`
before any loop form), `break`/`continue` labels, `&'a Type` (stores
the name in `Type::Ref.lifetime`, which already existed as a field but
was always `None`), and generic lifetime parameters (`fn
longest<'a>(...)`) — the last of these is parsed-and-discarded rather
than tracked (no `GenericParam::Lifetime` variant exists, and adding
one is out of Phase 8's scope; named lifetimes remain structurally
inert everywhere, consistent with Document 6's own established
scope through Phase 6/11) so the syntax parses instead of erroring,
which is the concrete, previously-broken thing this fixes.

**Scope cut, flagged rather than silently handled:** `LoopExpr::DoWhile`
has no `label` field in the AST at all (Document 9 never shows a
labeled do-while example either) — a label before `do` still parses
(so it doesn't error) but has no effect. Real gap, not exercised by
any Document 9 example.

### 2. Exhaustiveness checking (`src/exhaustive.rs`) — a real Maranget-style algorithm, not a shortcut
Implements genuine pattern-matrix usefulness checking: constructor
specialization, a default matrix for non-exhaustive/infinite domains,
and full recursive decomposition of nested patterns (an enum variant
containing further sub-patterns), not a "collect top-level variant
names" heuristic. Handles:
- User-declared enums (via a `EnumTable` built from `types.rs`'s
  existing `enums` registry).
- `Option`/`Result` as synthetic two-variant enums (`Some`/`None`,
  `Ok`/`Err`) — they're ordinary enums per Document 7 §3.2 but live as
  dedicated `Ty::OptionTy`/`Ty::ResultTy` variants rather than entries
  in the enum registry, so they're special-cased structurally.
- `bool` as a genuine two-constructor finite domain — exhaustive via
  `true`/`false` alone, no wildcard needed.
- Tuples, recursively over element types.
- Or-patterns, both nested (`Pattern::Or`) and this parser's actual
  representation of a `|`-combined top-level arm (`MatchArm.patterns:
  Vec<Pattern>` — confirmed by reading `parse_match_expr` directly:
  Document 9 §2.1's `HttpMethod::Put | HttpMethod::Delete => ..` is
  stored as two separate patterns on one arm, not a nested `Or`).
- Guards excluded from the coverage matrix entirely (Document 9 §2.3).

Every other type (ints, floats, strings, chars, structs, arrays) is
treated as infinite/unenumerable — literal patterns over them are real
matrix rows, but only a wildcard (or destructuring whose own parts are
all-covering) can make such a match exhaustive, matching Document 9
§2.2's own `match statusCode {200=>..,404=>..,500=>.., _=>..}` example
requiring the `_`.

**A real disambiguation problem, resolved by reading the actual
parser, not assumed:** `HttpMethod::Get` (no parens) parses as
`Pattern::Ident("HttpMethod::Get")` — structurally identical to a
genuine capturing binding like `n` in `n if n < 0 => ..`. Resolved by
checking, at exhaustiveness-analysis time, whether the pattern's name
(or its last `::`-segment) matches a registered NULLARY variant of the
scrutinee's type; only then is it treated as a real constructor,
otherwise it's a wildcard-equivalent binding exactly as it would be for
any other type.

**A real bug caught before shipping, not after:** the first draft's
`specialize` function computed a wildcard row's expansion width as a
hardcoded 0, correct only for nullary constructors — specializing by
any constructor with fields (e.g. `Some(_)`) against a matrix
containing a plain wildcard row would have produced a 0-wide expansion
instead of the correct width, desyncing that row's column count from
every other row and the query, corrupting the whole recursion. Caught
by re-reading my own draft before writing any tests (the same
discipline Phase 6/7 applied to `borrow.rs`), fixed by unifying to one
`specialize` that always takes the column's type and computes real
arity via the same `Ctx::field_types` the rest of the algorithm uses —
no parallel/inconsistent arity computation left anywhere.

**A defensive fix, not just an optimization:** made `is_useful` use
`col_tys.first()` with a `Ty::Unit` fallback instead of `col_tys[0]`
direct indexing, so a hypothetical arity-mismatched pattern (a gap
types.rs doesn't fully guard against — see PROGRESS.md's Phase 3
notes on pattern-binding types not being distributed yet) makes this
check imprecise at worst, never panics the compiler itself (Document 1
Pillar I: a crash is strictly worse than an overly-cautious
diagnostic).

Hand-traced against 5 representative cases before writing any test
(HttpMethod with a `|`-arm and a data-carrying variant; a genuinely
non-exhaustive HttpMethod match; `bool`; `i32` literals with and
without a wildcard; `Option`) — all traced to the correct result. 14
tests in `tests/exhaustiveness.rs`, each grounded in a specific
Document 9/7 example or a directly-adjacent case (nested enum
decomposition, guard-exclusion, tuple-with-catchall).

### 3. Loop type-checking (`types.rs`) — `Expr::Loop` was completely unchecked before this phase
Confirmed by reading the code directly: `Expr::Loop` (all four forms)
fell into a catch-all that did nothing — not even walking into the
body, meaning zero type errors inside ANY loop, of any form, were ever
reported. Added real checking for all four forms:
- `loop`: Document 9 §3.1's loop-as-expression form — its type is the
  unified type of every `break <value>;` that targets it specifically
  (same "no cross-branch guessing" rule Document 5 rule 5 already
  applies to `match`/`if`). Tracked via a `loop_frames` stack pushed/
  popped around each loop, with `Stmt::Break`'s handling recording a
  break's already-checked value type directly into its target frame —
  **a single pass**, not a separate collect-then-recheck pass (a first
  draft tried the latter and would have reported any real type error
  inside a `break <value>;` expression twice; caught while designing
  it, before it was ever a working version to regress from).
- `while`/`do-while`: condition checked against `bool`.
- `for`: element-type inference limited to what's directly
  determinable without a real `Iterable` trait (Phase 16 territory,
  not built) — `[T]` unwraps to `T`; a `Range` (whose own `check_expr`
  already returns the bare element type) is used as-is; anything else
  falls back to the iterator's own checked type. Covers Document 9
  §3.3's own examples; flagged, not silently wrong, for anything
  genuinely needing `Iterable` dispatch.
- Labeled `break`/`continue` (Document 9 §3.5): resolved against the
  WHOLE `loop_frames` stack (not just the innermost), so a label
  naming an outer loop from inside a nested one resolves correctly; an
  unresolvable label, or a bare `break`/`continue` outside any loop,
  is now a real error where it previously did nothing at all.

**Incidental fix, called out separately (not core Phase 8 scope):**
`Stmt::TargetBlock` (`#target(native){..}`) was also never walked into
by the type checker — noticed and fixed alongside this work since it's
the same match arm and a one-line fix, not because Document 2 §8's
directive blocks are part of Phase 8's stated scope.

10 tests in `tests/control_flow.rs`: Document 9 §3.1's own
loop-break-value example, incompatible-break-type rejection, `while`
condition/body checking (confirming bodies are now actually visited),
Document 9 §3.5's own nested-labeled-loop example, valid/invalid label
resolution in both directions, and the `&'a str` lexer-fix regression
check.

### 4. Generators (`src/generator.rs`) — a real, scoped lowering + interpreter, since no LLVM backend exists yet
Document 25 §2.3's exit criterion ("the generator state machine must
correctly resume from every yield point across multiple `.next()`
calls, preserving local state correctly between suspensions") needs
something *executable* to verify against, and Phase 10 (real codegen)
doesn't exist yet. Rather than skip this or fake it, built a genuine
state-machine lowering — Document 9 §4's own described technique
("compiling the function body into an enum representing which yield
point I'm resuming from plus the captured locals"), implemented as
real Rust data structures (`LoweredGenerator`, `GenStep`) — plus a
small interpreter (`GeneratorState::next`) that actually executes it,
so tests call `.next()` repeatedly against real lowered output and
assert on real yielded values and real preserved local state, the way
a future LLVM-generated `poll` function would behave.

**Explicitly scoped, not a general Mountain evaluator:** supports
exactly the statement/expression shapes Document 9 §4's own
`fibonacci` example uses (`let`/assignment with simple initializers,
`yield`, one enclosing `loop`, integer arithmetic) — anything outside
that is a real `LowerError`, never silent mis-lowering. Building a
general interpreter would mean re-implementing the whole language's
runtime semantics ahead of Phase 10; this proves the state-machine
*transformation* is correct for a real, representative case, which is
what the exit criterion actually asks for.

**A real parsing-shape risk, checked rather than assumed:** whether a
trailing `loop {}` with no semicolon parses as `Stmt::Expr` or as the
block's own `tail` expression matters for finding it during lowering.
Checked by reading `parser::parse_stmt` directly: a block-like
expression at the end of a block, with no following `;`, becomes
`Block.tail`, not a statement — meaning Document 9 §4's *own* example
(no semicolon after the loop's closing brace) parses via `tail`, not
`stmts.last()`. The first draft only checked `stmts.last()`, which
would have failed to lower the specification's own central example.
Fixed to check both representations.

Hand-traced the full `fibonacci` lowering and five consecutive
`.next()` calls by hand against the real algorithm before writing any
test — produced 0, 1, 1, 2, 3, matching the real Fibonacci sequence
exactly, with `a`/`b`/`next` locals correctly evolving across
suspensions. 5 tests in `tests/generators.rs`: successful lowering of
Document 9 §4's actual example, the full 10-value sequence against
real Fibonacci numbers (matching Document 24 §3's `.take(10)` usage),
explicit local-state assertions after a specific `.next()` call (not
just checking yielded values), a one-shot (non-looping) generator's
finish/re-`.next()` semantics, and an explicit-error check for an
unsupported shape (a function call inside a generator body).

### 5. The borrow-checker/yield interaction — explicitly checked, per your instruction, not assumed
You flagged this specifically: "generators suspend mid-function...
don't assume this is automatically covered — check it explicitly."
Checked by hand-tracing two concrete cases against the actual Phase
6/7 algorithm before writing anything: a borrow whose only read occurs
*after* a `yield`, and the adversarial case of a conflicting borrow
created *after* a `yield` while an earlier borrow (read still later)
remains genuinely live. **Both trace correctly with zero new code**,
because `yield` was already just an ordinary statement to
`compute_last_use`'s forward scan and `check_expr`'s walk — both
written in Phase 6, before generators were a named concern at all, and
neither ever special-cased or skipped `yield`. This is a verified
result, not an assumption: two tests
(`phase8_borrow_still_needed_after_yield_point_stays_correctly_live`,
`phase8_conflicting_borrow_after_yield_still_rejected`) were added to
`tests/borrow_checks.rs` to make the verification permanent rather
than just asserted in this report — the second one specifically rules
out the case where the first passes for the wrong reason (e.g. if
`yield` somehow reset the aliasing table).

### `.github/workflows/ci.yml`
No changes needed — picks up all five new/extended test files
automatically.

### Predicted test counts (real counts required from CI, not this table)
| Suite | Count | Note |
|---|---|---|
| lib (inline) | 60 | 57 confirmed + 3 new lexer tests |
| borrow_checks | 19 | 17 confirmed + 2 new yield-borrow tests |
| closures | 10 | unchanged |
| control_flow | 10 | new |
| exhaustiveness | 14 | new |
| generators | 5 | new |
| generics | 12 | unchanged |
| integration | 6 | unchanged |
| parser_doc24 | 6 | unchanged |
| traits_impls | 15 | unchanged |
| types_doc5 | 26 | unchanged |
| **Total** | **183** | |

### How to verify (no cargo/rustc/network here)
Push and check the Actions log. Expect `cargo build --verbose` clean (0
warnings — this phase touched `lexer.rs`/`parser.rs`/`types.rs`
directly, higher regression surface than Phase 6/7's mostly-additive
changes, so watch this closely), `cargo test --verbose` showing
**183/183** if every hand-trace in this report was correct, with the
pre-existing 57(+3) lib tests and all Phase 1–7 external test files'
counts unchanged (regression check — the lexer/loop-checking changes
specifically touch code every prior phase's tests already exercise).

### Exit criteria — self-assessment
> "The exhaustiveness checker must correctly reject every
> non-exhaustive match test case (and correctly accept exhaustive
> ones)... the generator state machine must correctly resume from
> every yield point across multiple `.next()` calls, preserving local
> state correctly between suspensions."

Both hand-traced correct, with a real algorithm (not a heuristic) for
exhaustiveness and a real, executable lowering+interpreter (not a
description) for generators, per the standing "implement for real,
verify via an executable prototype harness" guidance. The
yield/borrow-checker interaction you specifically flagged was checked
explicitly, not assumed, with the verification result pinned down by
two new tests. **Not yet independently confirmed by real CI.** ⏳ One
foundational, previously-latent gap (no lifetime token at all) was
found and fixed at the root while pursuing labeled loops specifically;
three real bugs in this phase's own new code were caught and fixed
before shipping (exhaustiveness's zero-arity `specialize`, the
double-pass break-value type-checking, and the wrong-statement-form
generator-loop detection); one scope cut is flagged (`DoWhile` has no
label field in the AST). Nothing here required a sign-off-worthy
interpretive judgment call the way Phase 6's NLL tension did — every
design decision was grounded directly in a specific Document 6/9/17
passage or an explicit, stated scope boundary.

## Phases 9–25
**Status: ⚪ Not started**

(Full phase table: see Document 25 §2.3.)
