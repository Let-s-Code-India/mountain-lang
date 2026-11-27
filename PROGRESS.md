# Mountain (`mtnc`) — Build Progress

Tracks execution of the 25-phase roadmap defined in Document 25 §2.3.
Updated at the end of every phase per Document 25 §2.2, point 5.

## Status Legend
- 🟢 Complete and verified (tests passing in a real CI run)
- 🟡 Complete, logic-verified, **pending real-toolchain confirmation**
- ⚪ Not started

## How this project is run (read this first if you are taking over)

**Three roles.**
1. **Owner** — has no coding background. Moves files between the other two roles and GitHub; makes the decisions that the standing rules below send to them.
2. **Manager** (a separate chat) — writes one prompt per phase, then reviews the coding assistant's report, zip and the CI log against the 25 specification documents, and either approves the phase or sends the assistant back to fix it.
3. **Coding assistant** (the role that wrote this file) — writes all the code for a phase and returns **one zip of the whole repository tree** (including this `PROGRESS.md`) plus a written report.

**The loop.** Manager prompt → coding assistant builds the phase → owner uploads the zip to GitHub (`Let-s-Code-India/mountain-lang`) → GitHub Actions CI runs (`.github/workflows/ci.yml`) → the owner brings the CI log to the manager → the manager approves (phase becomes 🟢) or sends fixes. **Nothing is 🟢 until the manager has seen a real CI log.**

**Testing.** Through Phase 9 the coding assistant had no Rust/LLVM toolchain. From Phase 10 on its sandbox can install one (`apt install rustc-1.91 cargo-1.91 llvm-18-dev libpolly-18-dev libzstd-dev zlib1g-dev libffi-dev`, then `export PATH=/usr/lib/rust-1.91/bin:$PATH LLVM_SYS_181_PREFIX=/usr/lib/llvm-18`), so the assistant now **runs the complete test suite before every delivery** and reports real numbers; CI is still the final authority.

**Standing rules for the coding assistant.**
1. Follow the plan (the 25 documents + the phase prompt) exactly. No invented features, no restructuring. Any deviation, however small, is **asked for and approved first**, stating what, why and the alternative; deviations that were already made are listed as "flagged" and need sign-off.
2. Fix ordinary bugs at the root and sweep for sibling instances; do not ask for help with normal syntax/logic errors.
3. Check the logic 2–3 times before delivering (hand-trace new tests, re-read changed functions, cross-check against the AST).
4. Every number in a report comes from a fresh command, never from memory.
5. Anything not supported yet gives a clear compile error ("… not yet supported by codegen — Phase N"), never a crash or silent miscompile.
6. The final reply lists every unresolved choice and pending approval.
7. The documents are the single source of truth (Doc 11 §8 and Doc 23 §17 are approved and authoritative; Doc 6 §3.1 is the corrected version). A real contradiction is reported, not silently resolved.

---

## Current state at a glance (consolidated by the manager, 2026-11-16)

**Phases 1–12 are 🟢 (real CI logs). Phase 13 is in progress (M0–M5 done). Phases 14–25 are ⚪ not started.** Latest confirmed CI run (Phase 12 final, 2026-11-16): **937 tests passed, 0 failed, 0 warnings, 36 `test result:` lines (35 test binaries + doc-tests); smoke tests green (`mtnc 1.0.0-dev (Phase 12 — closures, generators, async, concurrency)`, `checked 67 file(s), 0 errors`, `Hello, Mountain!`); valgrind steps green on all 9 `examples/memory/` and all 15 `examples/concurrency/` programs.**

| Phase | Status | Confirmed by (real CI) |
|---|---|---|
| 1 Scaffold + lexer | 🟢 | first green run: 29 unit + 6 integration |
| 2 Parser | 🟢 | 57 lib + 6 integration + 6 `parser_doc24` |
| 3 Core type system | 🟢 | Phase 4 log (110 tests) and every later log |
| 4 Traits and `impl` | 🟢 | 110 tests |
| 5 Generics | 🟢 | Phase 6 log (138) and later; exit criteria re-proven in 11b |
| 6 Borrow checker | 🟢 | 138; correction confirmed in the Phase 7 log (17/17) |
| 7 Functions and closures | 🟢 | 149 |
| 8 Control flow | 🟢 | owner reported 186; verified through the Phase 9 log |
| 9 Error handling | 🟢 | 243 |
| 10 LLVM codegen + native backend | 🟢 | 278 |
| 11a / 11b / 11c Memory management + codegen foundations | 🟢 | 308 / 413 / 603 (+ valgrind step) |
| 12 Closures, generators, async, concurrency | 🟢 | 660 (M0) / 713 (M1) / 754 (M2) / 800 (M3) / 857 (M4) / 922 (M5) / **937 (final)**; valgrind on 9 + 15 programs; 10,000-task load test green in CI |
| 13 Language completions, atomics, locks, channels, select, lock-free | 🟡 in progress: M0–M5 (lock-free stack and queue in Mountain, ABA test, stress, sanitizers) confirmed by real CI: 1115 tests, 0 warnings, 42 result lines, valgrind 19 memory + 36 concurrency programs, TSAN 3/3 clean; M6 (volatile, audit, banner, final-13.zip) to do | — |
| 14–25 | ⚪ | — |

**Session practice (owner decision 2026-11-08):** one fresh chat per phase, and the previous phase's chat is deleted after the manager has approved that phase. The coding assistant keeps this file current while it works; the manager does one final consolidation per phase (this section and the two registers below). Per-phase work files (`WORKLOG_*.md`) are scratch notes: this file is the record.

### Approval log (flagged items) — manager decisions
Every item the coding assistant flagged "for sign-off" in Phases 1–12 has been decided. Phase 12 (all accepted, with the manager's rulings recorded in Doc 12 §11.10–11.12, Doc 9 §8.6, Doc 10 §9.6, Doc 6 §9.8–9.11): flow-insensitive move-while-borrowed; by-value `mut` parameters; generators `?`/`throw` rejected permanently; `borrow fn` callable; `await` and `yield` not allowed inside `try`; panic in any task exits 101; the process waits for tasks, OS threads and actor mailboxes at exit; `ActorRef` copyable and `Send`; futures reach `spawn` only as direct `async fn` calls; function values and `Future` variables are not `Send`; stats line unchanged (an actor message is one task); `f::<T>` as a function value needs all type arguments. Plus, Approved as implemented unless a later phase replaced it: Phase 2 deviations 1–9 (now Document 23 §4 / §17); Phase 3 bidirectional checker; Phase 4 orphan-rule approximation (revisit in Phase 14); Phase 5 matmul special rule and unification hybrid (generalised in 11b); Phase 6 NLL tension (spec bug, Doc 6 §3.1 corrected) and `@copy` (Doc 23 §17.8); Phase 9 flagged interpretations (Doc 11 §8, §8.9); Phase 10 seven flagged items (Doc 17 §10); Phase 11a/11b/11c flagged interpretations (see their entries; 11c items: zero-slot ownership instead of drop flags, by-reference `match` binding, `Weak<T>`/`Weak<T, Arc>`, invented names `upgrade`/`strongCount`/`weakCount`, no user `Drop` trait, `dyn T` in value position = owned `Box<dyn T>`, `Box::newIn` taking a shared `borrow` allocator).

### Open items register (carry forward — nothing here is hidden)
1. ✅ **CLOSED in Phase 12 M0** (see Phase 12 entry). **Move while borrowed (Phase 7 gap, Doc 6): a move is not checked against active borrows of the same place** (`let r = borrow x; let y = x; use(r)` is accepted). Memory-safety relevant. **Manager decision: close it in the first milestone of Phase 12**, with positive and rejection tests.
2. ✅ **CLOSED in Phase 12 M0** (Doc 23 §17.11). **By-value parameter `mut`:** Doc 3 says `mut` may mark a function parameter, Doc 23's parameter grammar has no `mut x: T`; today by-value parameters are immutable. The manager will rule with a Doc 23 edit before Phase 12 starts.
3. ✅ **CLOSED in Phase 12 M0** (Doc 7 §9.6). **impl method signatures vs the trait's:** only names and parameter counts are compared, not parameter and return types (Doc 7). To be scheduled by the manager (suggested: Phase 12 housekeeping).
4. **Orphan rule** uses a one-file "package" approximation (Doc 7 §5) until the package system exists: revisit in **Phase 14**.
5. **Language features that need later phases:** (closures, generators, `async`, `spawn`, `thread`, `actor`, `Send`/`Sync`, `Rc` rejected as non-`Send`: DONE in Phase 12); `channel`, `select`, `Atomic`, `Mutex`/`RwLock`/`Spinlock`, `volatile`, interior mutability through `Rc`/`Arc`, `dyn Trait + Send` (Phase 13); modules (14); `wasm32` target and the size-class allocator, Doc 13 §3.1 (15); the real prelude, `for` over growable arrays and `Iterable`, `.len()`/`push`/string operations, printing `&i32`/128-bit integers, array patterns `[a, b]`/`[first, ..rest]`, `Option`/`Result`/`Box`/`Rc` as library source (16); lint for non-tail recursion (23).
6. **(Decided on 2026-11-08 and written into the documents; implemented items marked: ASCII identifiers + escapes DONE in Phase 12 M0, `break`/`continue` out of `try` DONE; the rest is Phase 13 M0 / Phase 16 M0.) Formerly unspecified in the 25 documents:** pointer arithmetic and pointer methods; a user `Drop` trait; Unicode identifier rules and string-escape validation (Phase 1); `dyn` behind `Rc`/`Arc` and `dyn Trait<T>`; `break`/`continue` leaving a `try` block. Each needs a document decision before it is built.
7. **Smaller known limits (see the Phase 11b and 11c entries):** const expressions inside type arguments (`Matrix<f64, R*C, 1>`); later-use inference for const parameters; same-named associated types in two traits; `Stack<i32>::Item`; `impl<T> Foo<Vec<T>>`; temporaries live to the end of their statement; no move out of an array by indexing.
8. **Document follow-ups for the manager:** merge into the documents the interpretations that are now implemented (`Weak<T, Arc>`, `dyn T` as a value type, `Rc`/`Arc` API names, `Allocator`/`Box<T, A>`) — Doc 6 §6, Doc 7 §4.5, Doc 13 §3.2.
9. **Repo housekeeping (DONE 2026-11-14):** `.gitignore` contains `/mtnc/examples/**/*.bin`; the stray `gitignore` file is gone; `PROGRESS.md` entries for Phases 1–8 keep their original narrative (including the CI failures and fixes) as history; the status lines above are authoritative.
10. **Move-while-borrowed is flow-insensitive and conservative (known limitation, Phase 12 M0; manager decision).** `let r = borrow x; if c { return x; } use(r);` is rejected although only one path conflicts. It never accepts an unsound program.
11. **`break <value>` cannot leave a `try` (language restriction, Phase 12 M0; manager decision).** Plain `break`/`continue` (also labelled) can; a value-carrying `break` gives the clear error "cannot leave a `try`".
12. **Known gap: generic impl signatures are only fully checked after instantiation (Doc 7 §9.6).** The impl-vs-trait comparison of parameter and return types skips types that mention the impl's own type parameters; the instantiated copy (`impl Shape for Wrap$i32`) IS checked in full at mono time. An `impl<T>` that is never instantiated and has a wrong signature is accepted. Tests in `tests/phase12_m1.rs` (`generic_impl_signature_mismatch_is_caught_when_the_impl_is_instantiated`, `..._is_not_caught_if_the_impl_is_never_instantiated`).
13. **Known limitation (M1, manager-accepted): a `move` closure is move-only even when it only captures Copy values** (it owns its environment). Use a plain closure when sharing is wanted.
14. **Higher-order library (Doc 10 §5.2) `.map/.filter/.reduce/.any/.sortBy` → Phase 16** (needs the real prelude/iterators).
15. **Known gap (M1): a closure that moves a capture out in a form the borrow analysis does not see** is refused at codegen with "cannot move `x` out of a closure that only borrows it" (test in `tests/phase12_m1.rs`). The struct-literal-field form is now seen by the analysis.
16. **New behaviour (M1): "cannot assign to `x` while it is borrowed (via `f`)"** — a closure that borrows `x` keeps `x` frozen while the closure is alive; this also applies to plain borrows.
17. **Function values (M1 follow-up):** calling through `borrow f` auto-derefs; passing a function parameter by value moves it (hint: "pass `borrow f` to keep using the function value"); functions with variadic or default parameters cannot be function values (permanent restriction, message says to wrap the call in a closure); `borrow`/`borrow mut`/`borrow dyn`/immutable `borrow [T]` parameters work through function values. Also fixed: moves out of struct-literal fields, tuple and array elements are now tracked.
18. **`for` over arrays → Phase 16 (M2, manager-accepted).** `for x in arr` on a local array variable currently counts as a *move* of `arr` in the borrow checker; array iteration itself is decided with the library (codegen for it is "not yet supported — Phase 16").
19. **Restriction (M2): `yield` inside a `try { }` block is rejected** (the `try` body is lowered to a separate function; Doc 11 §8.8). Move the `yield` out of the `try`.
20. **Permanent (M2, manager decision, Doc 9 §8.6): `?` and `throw` cannot be used inside a generator body** (its result is a `Generator`); yield the error instead (`match r { Ok(v) => yield Ok(v), Err(e) => { yield Err(e); return; } }`, tested).
21. **FIXED in M5 (was: moves in separate branches rejected).** Document 6 §9.11 is implemented: `if`/`else` branches and `match` arms are alternative paths; a variable is moved after the join if it is moved in any branch that does not end in `return`; a move inside a loop body that is not re-assigned is an error (the body is checked a second time from the end-of-iteration state; paths that leave by `break`/`return` do not loop back, their moves flow to the code after the loop). Conservative: it may reject valid code, never accepts a use after a move on some path.
22. **`join(array)` returns a fixed-size array and only for an array LITERAL (M4).** `await join([f(), g(), h()])` gives `[T; N]`; joining a growable `[Future<T>]` needs `Vec`/`push` (Phase 16). Registered as a known gap.
23. **Restriction (M4): `await` cannot be used inside a `try { }` block** (the body is lowered to a separate function, like `yield`, item 19). Put the `await` before the `try`, or inside the function the `try` calls.
24. **Known limitation (M4): a `borrow self` async method, `borrow`/`borrow mut`/variadic async parameters, async trait methods, `async` closures and `async` blocks are rejected** with clear messages (Doc 12 §11.4; the future owns all its parameters).
25. **Panic message (M3/M4):** a panic inside a task still prints `thread 'main'` in the message (exit code 101 is right).
26. **Actors (M5), decisions:** `ActorRef<A>` is a COPYABLE pointer (no `.clone()` needed) and is `Send`/`Sync`; actors are never freed before the process exits (no `stop`/kill in 1.0), so an actor created in a loop stays allocated until exit. A mailbox is unbounded (no back-pressure). Actor items are lowered only at the top level (modules are Phase 14).
27. **Actors (M5), visible leaks of the lowering:** the message type is an ordinary enum named like the actor (`A::h(..)` is a variant constructor; a program may `match` on it), the state is the private struct `__ActorState_A` and handlers are the methods `__on_h`; compile errors raised inside a handler body or in `A::spawn` are reported "in `__on_h`" / "in `spawn`". Cosmetic, to be polished with the diagnostics phase (Phase 21/22).
28. **Actors (M5), limits:** state and handler bodies cannot use `Vec`/`push`/`retain` (Phase 16), so the Doc 12 §8 `OrderBook` example (`self.orders.push(order)`) cannot be written yet; examples use scalars/`String`. An actor cannot obtain its own `ActorRef` (pass it in a message, as in the ping-pong example).
29. **`f::<T>` as a function value (M6, Doc 23 §17.2).** Works when EVERY type argument is written (`pair::<i32, String>`); a partial list and a bare generic name (`apply(identity, 3)`) are errors with clear messages; inference of the missing arguments from the expected function type is not done.
30. **Sandbox/CI note (M6; passed in the real CI run of 2026-11-16 with `peak_live=10000`):** the load test's 2.5 s sleep is its timing margin (see the M6 entry); it asserts only lower bounds on time.
31. **Carry-forward to Phase 13 (manager, 2026-11-16).** Phase 13 M0 = language completions: user `Drop` trait (Doc 6 §9.9), pointer methods (Doc 23 §17.13), `dyn Trait<T>`, `Rc<dyn T>`/`Arc<dyn T>`/`Weak<dyn T>` and `dyn T + Send + Sync` (Doc 7 §9.8–9.10; `dyn` is never `Send` until then); then `channel`, `select`, `Atomic<T>`, `Mutex`/`RwLock`/`Spinlock`, `volatile` (they already give the message "not yet supported by codegen — Phase 13"). Also to consider there: a way to stop an actor, a lock-free scheduler queue (all deques share one lock), per-task panic message naming the task (item 25).
32. **Known limitation (M5, F3 side effect):** `await join(a, b)` over future *variables* is rejected (`join` runs them on the pool and Send is judged only for direct `async fn` calls); write `await join(fa(), fb())`. Revisit in Phase 16 (track where a future variable came from).
33. **(Phase 13 M0, FIXED in the M1 round) Moving a field / pattern-binding out of a `Drop` type** is now reported by `mtnc check` as well as by `build`, with the same message ("cannot be taken apart", Doc 6 §9.9 rule 5), by the new post-check pass `src/dropmoves.rs`. Covered contexts: `let` initializer, assignment right-hand side, struct-literal field, by-value argument of a user function, `return`, block tail, and a pattern that binds an owning field of an owned `Drop` value (`let`/`match`). Not covered at check time (still caught by `build`): a field moved in other positions, e.g. as an argument of a method call. Kept the codegen rule as the backstop.
34. **(Phase 13 M0) `impl Drop` must cover every instantiation** (`impl<T> Drop for W<T>`): a specialised impl, extra bounds or a `where` clause are rejected (conservative reading of Doc 6 §9.9 rule 1). An unpolled future that already holds a `Drop` value mid-suspension is not user-reachable today.
35. **(Phase 13 M0) `Rc<dyn T>` / `Arc<dyn T>` cannot be dereferenced into a value** (`*rc`), only called through trait methods; `borrow mut self` trait methods are rejected through `Rc`/`Arc` (shared access, as for concrete types). `dyn Trait<A>` coercion of a *generic* type needs an impl whose trait arguments match exactly.
36. **(Phase 13 M0) `Sync` for `&dyn T`:** a `dyn T` without `+ Sync` is not `Sync` (was: ignored). `dyn T` without `+ Send` is not `Send`; `dyn T + Send` coerces to `dyn T` but never the reverse.
37. **(Phase 13 M1) Spec gap: struct patterns do not exist.** Doc 23's pattern grammar has tuple-struct patterns only (`Name(a, b)`), so `let W { name } = w;` is a parse error, not a `Drop` rule violation. The pattern rule is checked for enum variants and tuple-struct patterns. Struct patterns are therefore a Phase 16 point (grammar decision). The tuple-struct constructor bug found here is FIXED in the M2 round (see M2 below).
38. **(Phase 13 M1) `isOk()` / `isErr()` on `Result` added** because Doc 14 §2's CAS retry-loop example calls `.isOk()` and the library methods arrive only in Phase 16. They are compiler-known, take no arguments and do not move the receiver. `isSome`/`isNone`/`unwrap`... are NOT added.
39. **(Phase 13 M1) The `Ordering` argument must be written literally** (`Ordering::Acquire`), never a variable or expression: the generated instruction carries exactly that ordering. `Ordering` itself is an ordinary enum injected like `Drop` (skipped when the program declares its own `Ordering`).
40. **(Phase 13 M1) `Atomic<*mut T>` is never `Send`** (a raw pointer is not `Send`, Doc 12 §11.7) and is `Sync` only when `T: Send`, per the manager's ruling; so `Arc<Atomic<*mut T>>` cannot cross `spawn`. This will block M5 (a Treiber stack / Michael-Scott queue shared by 8 threads) unless the structure is shared another way: ruling requested before M5 (see the M1 report). **Manager ruling (2026-11-19): CHANGED — `Atomic<*mut T>` is `Send` and `Sync` when `T` is `Send` (Doc 14 §9.4), needed for the M5 lock-free structures; implemented as the first item of the M2 round (tests `atomic_pointer_*` in `tests/phase13_m2.rs`; plain `*mut T` / `*const T` stay non-`Send`; `Atomic<*mut T>` with a non-`Send` pointee stays non-`Send`/non-`Sync`; a struct with an `Atomic<*mut T>` field and a plain `*mut T` field is non-`Send`). Note for M5: Doc 14 §4's `Node<T>` with a plain `*mut Node<T>` `next` field would make the node non-`Send`; the M5 nodes use `next: Atomic<*mut Node>`.**
41. **(Phase 13 M1) Atomic details:** `Atomic<T>` is move-tracked but owns no heap memory; an `Atomic` field in an `@pack` struct is rejected (atomics need natural alignment); `Arc::new(Atomic::new(0))` infers `i32` (the literal type does not flow backwards from a later use) so write `Arc<Atomic<u64>>` on the binding; pointer atomics are done with integer instructions on the address (`ptrtoint`/`inttoptr`).
42. **(Phase 13 M2) `Spinlock` has only `acquire()`** (no `tryAcquire`): the manager's API list gives no try form; kept conservative. A `Spinlock` never parks: it spins with `sched_yield` every 65,536 iterations. Hold it for a few instructions only.
43. **(Phase 13 M2) A guard keeps its lock borrowed until the end of the block that declared it** (`let g = m.lock();`), for the borrow checker only. `drop(g)` releases the lock at run time but does not end the borrow, so `drop(g); let n = m;` (moving the lock in the same block) is rejected although it would be sound. Conservative false positive; temporaries (`*m.lock()`) hold the borrow for their statement only.
44. **(Phase 13 M2) Guards cannot escape:** a type that mentions a guard (`MutexGuard<T>`, `Option<RwLockReadGuard<T>>`, ...) is rejected as a function return type, a struct field, an enum payload and a `static` ("a lock guard cannot be a return type ... pass the lock itself (`borrow Mutex<T>`) and lock inside the callee"); parameters are allowed. A guard captured by a `move` closure that is NOT spawned is accepted (the guard is released when the closure is dropped; hand probe, runs); a spawned one is rejected because the guard is not `Send`.
45. **(Phase 13 M2) Lock waiters block their worker thread** (a task waiting for a lock occupies one pool worker: hold locks briefly). Anti-deadlock rule at `MTN_WORKERS=1`: a worker that holds a lock and joins a task may run ONLY the awaited task (never an arbitrary queued task), and a lock waiter never helps, so a queued task that needs the held lock cannot be run on the holder's stack (test `a_task_holding_a_lock_does_not_deadlock_the_pool_when_it_joins`, `MTN_WORKERS` 1 and 4).
46. **(Phase 13 M2) Locks are not re-entrant: locking a lock already held by the same thread is a deadlock, not an error** (Doc 14 §9.5; `tryLock` returns `None` instead). There is deliberately no test that deadlocks. No poisoning.
47. **(Phase 13 M2, check-vs-build, by design until Phase 16) Unknown library names and methods pass `mtnc check` and fail `mtnc build`:** `foo(1)`, `x.foo()`, `Vec::new()`, `s.len()`, `v[i]` on a `Vec` give "no method named ... (standard-library methods arrive with the prelude in Phase 16)" at build (index on `Vec`: at check). The checker stays silent on unresolved library names until the prelude exists. Everything else found by the M2 probes now agrees between `check` and `build` (tuple-struct named arguments; a `borrow mut self` method through a read guard or an `Rc`/`Arc` is now a CHECK error; a field moved as a method-call argument is a check error).
48. **(Phase 13 M2) Literal types do not flow backwards from a later use:** `Arc::new(Mutex::new(0))` is `Arc<Mutex<i32>>`; write the type on the binding (`let m: Arc<Mutex<i64>> = Arc::new(Mutex::new(0));`) and it flows into `Mutex::new`. Likewise `let mut i = 0` is `i32`.
49. **(Phase 13 M2) "Phase 13" in user-facing strings:** the three messages "(interior mutation needs `Atomic`/`Mutex`, Phase 13)" (codegen/expr.rs, codegen/dynamic.rs, statics) were reworded to cite Doc 12/14. One remains: the gate in `driver.rs` (`... not yet supported by codegen — Phase 13`) for `channel`, `select`, `volatile`; it goes away as M3, M4 and M6 land (the M6 test scans the source for the string).
50. **(Phase 13 M2) Helgrind reports "possible data race" on the lock examples** (`mutex_counter`, `rwlock_readers_writer`, `spinlock_counter`, `bank_transfer`, `MTN_WORKERS=4`): the flagged accesses are the protected values themselves (in `mtn_transfer`, `mtn_shuttle`, ...), exact totals every run. Helgrind only understands pthread primitives, not the C11 atomics (acquire/release ticket words) the runtime locks are built from, so it cannot see the happens-before edge; I could not prove the reports false with helgrind and no ThreadSanitizer is available for generated code. Reported, not claimed clean. Valgrind (memcheck) is clean on all four.
51. **(Phase 13 M3) DEVIATION, APPROVED by the manager 2026-11-24 (compensation, not helping; documented in Doc 12 by the manager): a pool worker blocked in a synchronous channel wait is COMPENSATED, not "helped".** The ruling says a blocked worker runs other queued tasks "as `join` does". That provably deadlocks the mandatory single-worker tests (a) and (b): the consumer run on top of the blocked producer's stack sits ABOVE the producer's frame, so when the producer's `send` can finally complete, the consumer must first return, and it cannot return until the producer sends (stack inversion). Implemented instead: a worker that blocks in `send`/`recv` is replaced by an extra worker thread (invariant `extra >= blocked`, cap 2048, extras live until shutdown); async `await tx.send(..)` / `await rx.recv()` use the waker lists and hold no worker at all. Tests (a), (b), (c) pass at `MTN_WORKERS` 1, 2 and 4. Consequence: `MTN_WORKERS` bounds RUNNABLE workers, not total threads; each task blocked in a sync channel wait holds one thread (beyond the cap of 2048 it just sleeps).
52. **(Phase 13 M3 hazard, FIXED in M4; manager approved 2026-11-24) `join` no longer helps with arbitrary queued tasks.** If the awaited task is still queued, `join` runs exactly that task on the joiner's stack (`take_specific`); if it is already running elsewhere, the joiner waits with the same compensation as item 51 (`comp_block_locked` / `comp_unblock_locked`, one extra worker while it waits) and never runs an unrelated queued task. The main thread just waits. Test `join_inversion_inv2_does_not_hang` (the former hang `inv2.mtn`, with a watchdog so a regression FAILS instead of hanging) at `MTN_WORKERS` 1, 2 and 4; `join_of_a_running_task_waits_and_deep_recursive_joins_work` (recursive fib(18) and a 300-deep join chain: no stack growth problem). All Phase 12 and Phase 13 test binaries were re-run 20x debug and 20x release: 0 failures; Phase 12 examples stay valgrind clean.
53. **(Phase 13 M3) `channel` details.** Capacity is `usize` (a signed variable is a compile error; 0 panics "channel capacity must be at least 1", exit 101); default 1024; the message type is required (`channel::<T>()`; no inference from later use). Bounded = Vyukov MPMC ring with `max(cap, 2)` cells (with one cell `pos + cap` equals `pos + 1`) plus an exact logical limit; unbounded = Michael-Scott queue with hazard-pointer reclamation (no mutex). Every memory order has a comment in `mtn_rt.c`. Wake-ups are broadcasts (all waiters re-check). A value being sent when the task holding it is destroyed while suspended in `await tx.send(..)` is not dropped (frame destruction is not run for a cancelled future; no cancellation exists yet).
54. **(Phase 13 M3) Pre-existing gap fixed at the root:** names bound by a destructuring `let (a, b) = ..` were unknown to the borrow pass, so a `move` closure could not capture them ("undefined variable") and moves of them were not tracked. They are now declared, each tuple element move-tracked by its own type (`tests: a_destructured_handle_pair_can_be_captured_by_a_closure`).
55. **(Phase 13 M3) Language limits met by the tests:** `server` is a keyword (not usable as a function name); a struct literal cannot be a `match` scrutinee (bind it first); `Ok(_)`/`Err(_)` need arms of one type (`0` then `v` fails: write the typed arm first).
56. **(Phase 13 M3, extended in M4; APPROVED) ThreadSanitizer / ASan:** `bash scripts/tsan_runtime.sh` compiles `mtn_rt.c` with `-fsanitize=thread` plus a C driver (`tests/tsan/rt_harness.c`: locks; channels of capacity 1, 3, 64 and unbounded with 1/1, 4/4, 8/8 producers/consumers; close by drop; leftover drop counting; try operations; **M4: `__mtn_chan_select` with 2 and 3 channels, 1/4/8 producers per channel, source order and all-closed checks**). Determinism check before adding it to CI: the whole script (3 TSAN runs + 1 ASan/UBSan run) was run 20 times locally: 20/20 exit 0, 60 TSAN runs with 0 warnings, 20 ASan/UBSan runs clean. So the step was added to `.github/workflows/ci.yml` (after the valgrind steps; the only change to `ci.yml` in M4). The async `__mtn_chan_select_poll` path is covered by the generated-code tests, not by the C harness (it needs a task frame).
57. **(Phase 13 M4) `select` shape.** `select { case v = <-rx: body, ... }` is an expression; every arm waits on a `Receiver<T>` (also through `&`, `Box`, `Rc`, `Arc`, a struct field) and binds the WHOLE `Option<T>` to a name or `_` (`Some(v)` = a value, `None` = that channel is closed and empty). Other patterns (`case Some(v) = <-rx`) are rejected ("binds the whole `Option<T>`"); arms share one type like `match` arms (a diverging arm is ignored); an arm borrows its receiver (it is not moved, a loop may select again). Rejected with clear messages: a non-`Receiver` arm (a `Sender` gets a hint), zero arms, arms of different types, `select` inside an `async fn` without `await`. Parse errors: a send arm (`case tx <- v: ..`) and a `default` arm, both "not supported in Mountain 1.0" (Doc 12 §11.13 items 5-6); no timeout arm.
58. **(Phase 13 M4) Source order and closed channels (Doc 12 §11.13).** When several arms are ready the FIRST in source order runs; arms that lose consume nothing. A closed channel is "ready" with `None` forever (after its pending values were taken), so a closed EARLIER arm starves later arms: the idiom is to stop selecting on the first `None` and drain the others with `recv` (used by the examples and tests). All arms closed: arm 0 runs with `None`. This is the specified behaviour, not a bug; I found it by writing an example that spun forever (my example's bug, not the runtime).
59. **(Phase 13 M4) Blocking and async.** A synchronous `select` on a pool worker is compensated like a blocked `recv` (item 51: an extra worker thread while it sleeps); it first spins briefly, then registers on every channel (`SelReg` list per channel, under the channel mutex), re-scans (seq_cst, the same Dekker handshake as `recv`, so no lost wake-up) and sleeps; the notifier that publishes a value wakes the registered selector. `await select` in an `async fn` registers wakers instead and holds NO worker (test `async_select_holds_no_worker_many_selecting_tasks_with_one_worker`, 50 selecting tasks at `MTN_WORKERS=1`). The registrations live in the async frame; a task destroyed while suspended in `await select` would leave them registered, but no cancellation exists in 1.0 (same limit as item 53).
60. **(Phase 13 M4) Grammar check (Doc 23 §9 vs Doc 12 §11.13).** Doc 23 §9 has a `select` production (called `select_stmt` there) with arms `case <pattern> = <-expr : expr`; §11.13 uses `select` as an expression whose value is the chosen arm. They agree with what is implemented; the only restriction is item 57 (pattern = name or `_`), which is the minimum that makes the `Option<T>` binding unambiguous. No grammar was invented.
61. **(Phase 13 M4) Test-suite housekeeping.** The two old tests that expected `select { }` to hit the Phase-13 gate (`tests/e2e.rs` unsupported-features test, `phase12_m4::phase13_select`) were replaced (`volatile` is the remaining gated name; `select_is_no_longer_gated_by_phase13`). `driver.rs` has no select gate (it had none in code, only the stale test and doc comment). Language limits met again while writing the tests: `server` is a keyword, a struct literal cannot be a `match` scrutinee, `println` takes one argument, `Option`/`Result` helpers other than `isOk`/`isErr` need `match`, moves inside loops that are not provably left are rejected by the borrow checker (use `break` inside the arm).
62. **(Phase 13 M5) Tagged word, decided WITHOUT a new feature (flagged for the manager).** Doc 14 §4 sketches `head: Atomic<*mut Node<T>>`; §5 asks for "a generation counter packed alongside the pointer, compared atomically as one combined word" and §9.4 has no 128-bit atomics. So the stack's heads are `Atomic<usize>`: the low 48 bits hold the node address, the high 16 bits the tag, packed with `as usize` casts and shifts; every successful head change bumps the tag (`TAG_STEP`). Nothing was added to the language. Limits, written in the example: addresses must fit in 48 bits (asserted for every node: `check_address` panics otherwise), and the tag wraps after 65,536 head changes between one thread's load and its CAS. The queue uses `Atomic<*mut Node<T>>` as in the spec (item 64).
63. **(Phase 13 M5) Stack reclamation: recycle, free at Drop.** A popped node goes onto the stack's own second tagged list and the next push reuses it, so a node's memory stays valid for any thread still holding its address: a stale pop reads `next` of a recycled node and fails its CAS on the tag; no use-after-free. `next` is an `Atomic<usize>` (so concurrent stale reads are not a data race), the value is written before the push CAS and read after the pop CAS by the winner. Drop pops both lists, drops the values still on the stack (`dropInPlace`) and frees every node. `LockFreeStack<T>` carries an unused `marker: Atomic<*mut Node<T>>` field: without it `T` would not appear in the fields (heads are plain words) and a stack of non-`Send` values (`Rc`) would wrongly be `Send`; with it `Rc` payloads are rejected (test). There is no `PhantomData` in the language, hence the marker.
64. **(Phase 13 M5) Queue reclamation: never free until Drop, so no ABA.** No node of the Michael-Scott queue is freed or reused while the queue lives (a dequeued dummy stays linked behind `head`; Drop walks the whole chain from the first node, drops the values still queued and frees everything). An address never reappears, so ABA cannot happen and no tag is needed. Cost: memory grows with the number of enqueues until the queue is dropped (a recycling queue would need tags or hazard pointers). The helping step is implemented in both `enqueue` (tail lags: swing it, retry) and `dequeue` (head == tail but next != null: swing tail first). The value is read after the head CAS succeeded, so non-`Copy` payloads work (tested with a `Drop` payload).
65. **(Phase 13 M5) ABA tests.** `aba_sequence()` in the example runs the classic interleaving exactly on the raw list: list A→B→C, a stale pop saw (A, next B), another thread pops A, pops B, pushes A back, the stale CAS runs. With the tag: CAS fails, list A→C (output `false`, `3012` = sum 301 x 10 + 2 nodes). Negative control in the test: the same program with `TAG_STEP = 0` prints `true`, `3202`: the CAS succeeds, A is lost and the already popped B is back on the list. Honest note about the stress tests: against the untagged stack, 3 runs of the 8-thread mixed stress gave 2 passes and 1 hang (corrupted list, killed by the watchdog), i.e. the ABA window is narrow and the stress test alone would not reliably catch a missing tag; the deterministic sequence is the proof, the stress test is the load check.
66. **(Phase 13 M5) Stress tests (all generated programs, no timing assertions, times to stderr only).** Stack, 8 threads x 100,000: (a) each thread pushes 100,000 unique values then pops up to 100,000, (b) 100,000 mixed operations each (push or pop chosen by a per-thread generator). Queue: 4 producers + 4 consumers x 100,000 enqueues (FIFO per producer checked by each consumer), 1 producer + 7 consumers, 7 producers + 1 consumer, and 8 threads x 100,000 mixed operations (each thread checks FIFO per producer on what it dequeues itself, the main thread drains the rest and checks too). Oversubscribed: threads = 4 x cores with a minimum of 16 (this sandbox has 2 cores: 16 threads), 800,000 operations in all, for stack (both workloads), queue producers/consumers and queue mixed. Every run checks: pushed count == popped count (exact expected count where fixed), checksum of values, sum of squares, FIFO violations == 0. Unique values: `thread * 1,000,000 + k`.
67. **(Phase 13 M5) Sanitizers, honestly.** (a) Valgrind (memcheck, leaks): both examples clean at `MTN_WORKERS` 1 and 4, and as part of the concurrency script (36 programs clean). Payload tests (stack and queue with a `Drop` payload, left-over values included) are exact and valgrind clean. (b) Helgrind on the examples, 2 workers: stack 7 reports, queue 3 reports, all of two classes, both because Helgrind does not model C11/LLVM atomics (same class as item 50): (i) offset 0 of a node = the `next` field, accessed with Relaxed/Acquire atomic loads and stores that are plain `mov` instructions on x86-64 (stack: 4 reports in `list_snapshot`/`list_push`; queue: 2 in `enqueue`/`dequeue`); (ii) offset 8 = the node `value`, written by the pusher / read by the winner of the pop CAS, and rewritten after recycling: the ordering is the Release/Acquire CAS chain Helgrind cannot see (stack: 3 reports in `push`; queue: 1 in `dequeue`). No lock-order or other report. (c) Whole-program ThreadSanitizer WAS possible: `bash scripts/tsan_program.sh target/debug/mtnc file.mtn...` takes the IR of `mtnc build --emit-ir`, adds the `sanitize_thread` attribute to every function, lets `clang-18 -fsanitize=thread -c` insert the `__tsan_*` calls (including `__tsan_atomic64_*`) and links with `cc -fsanitize=thread` plus the runtime (clang-18 has no TSAN runtime library here, GCC's libtsan is used; same interface). Positive control `tests/tsan/race_control.mtn` (a deliberate data race) is reported (exit 66, 2 warnings); the two examples and the four stress programs (8 threads x 20,000) are clean, 3 runs of the `--ignored` test `whole_program_threadsanitizer_on_the_stress_programs`: 0 warnings. It is NOT in CI (needs clang-18 and libtsan, not asked for). `scripts/tsan_runtime.sh` (in CI) still passes: exit 0, 0 warnings.
68. **(Phase 13 M5) Send/Sync.** A raw pointer sent through `spawn` or `thread::spawnOS` is rejected ("a raw pointer is not `Send`"); a struct with a plain `*mut` field inside an `Arc` is rejected ("cannot be sent between threads"); `Arc<LockFreeStack<i64>>` and `Arc<LockFreeQueue<i64>>` cross `spawn` and `spawnOS` (Doc 14 §9.4 ruling); a stack or queue of `Rc<i64>` does not.
69. **(Phase 13 M5) A flaky test found by the 20x runs and fixed (test bug, not a runtime bug).** The negative-control test for ABA ran the whole example with the tag switched off, including the example's own 4-thread part, which on an untagged stack can corrupt the list and hang (1 of 20 release runs hit the 300 s watchdog; the deterministic output before the hang was correct). The control now runs only `aba_sequence()`. After the fix `phase13_m5` ran 20x debug and 20x release: 40/40 passed.
70. **(Phase 13 M5) Language limits met (unchanged, worked around).** `println` takes one argument; `Option`/`Result` helpers other than `isOk`/`isErr` need `match`; no `PhantomData` (marker field, item 63); integers become pointers only through `as usize as *mut T`; a `borrow` argument is written `borrow x` at the call site but passed on without it inside; threads are spawned by recursion (no `Vec`/`for`).

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
   *(History note: that real run came back green — see the Status line.)*

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
- **Confirmed by a real CI run** (see the Status line at the top of this phase). ✅

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

### Flagged deviations from Document 23 — ✅ APPROVED by the manager (2026-11-08); formalised in Document 23 §4 and §17
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
   those examples. ✅ Approved by the manager (2026-11-08), like the others above.
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

### Known open gap — ✅ RESOLVED: Document 23 §4 now defines `tensor_type` (`tensor<f32>[784, 128]`)
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
  parse cleanly — **confirmed by the real CI run (all 6 `parser_doc24` tests pass)**.
- Document 24 example 3: expected to now correctly isolate *only* the
  flagged tensor-shape gap (the earlier `layers::Dense` multi-segment
  path failure that was masking this is fixed) — still blocked on your
  guidance for that one specific construct.
- **Confirmed by a real CI run** (see the Status line at the top of this phase). ✅

### `.github/workflows/ci.yml`
`actions/checkout@v4` → `actions/checkout@v7` (see Housekeeping above).
Otherwise no changes needed — `cargo test --verbose` already picks up
the new `tests/parser_doc24.rs` file and the new unit tests in `parser.rs`
automatically; the workflow itself doesn't need to know about individual
test files.

---

## Phase 3 — Core Type System

**Status: 🟢 Complete — confirmed by real CI.** The Phase 4 CI log (110 tests) already contains the Phase 3 suites (`types_doc5` 26 + the type-checker unit tests in `lib`), all passing, and every later real CI log up to the latest (603 tests) re-runs them green. *(Original status when written: 🟡 pending CI.)*

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
functions. Flagged for sign-off like every other deviation this phase. **✅ Approved by the manager (2026-11-08); the checker was later extended with real inference variables in Phase 11b.**

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
  everything. **Confirmed by real CI.**
- Ambiguous empty-array literal: produces a real, specific diagnostic
  (`"cannot infer type of empty array literal []..."`), not a silent
  default — tested both for the exact Document 5 §7 phrasing and a
  second, differently-shaped call site to confirm it's a general rule,
  not a pattern-matched special case.
- **Confirmed by a real CI run** (see the Status line at the top of this phase). ✅

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
against directly. Flagged for sign-off, same as every deviation before it. **✅ Approved by the manager (2026-11-08); to be revisited with the real package system in Phase 14 (Open items register).**

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
  subject to the rule at all. **Confirmed by real CI.**
- Static dispatch: resolves through concrete `impl` blocks (inherent and
  trait-provided), checks argument types, rejects undefined methods,
  and correctly orders inherent-over-trait precedence.
- `dyn` dispatch: resolves through the trait's own declared signature
  (not any concrete impl), checks argument types, rejects undefined
  methods, and is confirmed (via a side-by-side test) to genuinely use a
  different resolution path than static dispatch, not the same one.
- **Confirmed by a real CI run** (see the Status line at the top of this phase). ✅

### `.github/workflows/ci.yml`
No changes needed — picks up `tests/traits_impls.rs` automatically.

## Phase 5 — Generics

**Status: 🟢 Complete — confirmed by real CI.** The Phase 6 CI log (138 tests) contains `generics` 12 passing; every later real CI log re-runs them green. The two Phase 5 exit criteria were re-proven by the general mechanism in Phase 11b: matmul inner-dimension mismatch rejected at compile time (`tests/const_generics.rs::matmul_with_mismatched_inner_dimension_is_rejected`) and no dynamic dispatch in generic-only code (`tests/mono.rs::generic_only_code_has_no_dynamic_dispatch`). *(Original status when written: 🟡 pending CI.)*

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
nothing — but a real scope decision, flagged for sign-off. **✅ Approved by the manager (2026-11-08); generalised in Phase 11b M4 (`matmul<const R, K, C>`), the special rule stays.**

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
- *(Closed in Phase 11b M3.)* **Associated types are registered structurally but not semantically
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
  multiplication is rejected). **Confirmed by real CI.**
- Zero-`dyn`-dispatch: demonstrated via `satisfies_bound`/`ty_lookup_name`
  having no code path that could succeed for `Ty::DynTrait` — a generic
  call's bound-check only passes when the substitution produced a
  concrete, registry-lookupable type, which is direct, checkable
  evidence the substitution never used the `dyn` machinery Phase 4 built.
- **Confirmed by a real CI run** (see the Status line at the top of this phase). ✅

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

### The §3.1/§3.2 tension — a real interpretive decision, flagged for sign-off — ✅ RESOLVED (it was a spec bug; Doc 6 §3.1 was corrected, see the post-review correction below)
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

### Flagged decision: `@copy` struct-marking syntax — ✅ APPROVED (recorded in Document 23 §17.8)
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

**This correction round (confirmed by the Phase 7 CI log, `borrow_checks` 17/17):** push this commit and check the
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
  result reported above.** ✅ Confirmed afterwards by the Phase 7 CI log (`borrow_checks` 17/17, no regressions).

### Exit criteria — current status
All five §8 cases (now matching the *corrected* §3.1) hand-traced
correct, including the specific "not merely conservative" requirement —
the implementation no longer has a conservative fallback anywhere in
the borrow-aliasing/NLL logic. **Confirmed: the Phase 7 CI log (149 tests) shows
`borrow_checks` 17/17 and no regressions, so Phase 6 is fully closed on this
correction.** Both decisions flagged in the original write-up above
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

### A third gap found, NOT fixed (out of Phase 7's scope, flagged rather than hidden) — ⚠️ STILL OPEN (Open items register #1)
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
parser_doc24 / 15 traits_impls / 26 types_doc5 / 17 borrow_checks — all confirmed by the Phase 7 CI log) plus a
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
**Status: 🟢 Reported confirmed by the project owner: 186 tests, 0 failures, 0 warnings, run with `--no-fail-fast`** *(recorded at the start of Phase 9 from the owner's report; the raw CI log was not available in the Phase 9 session, so this line was attributed at the time; **the manager has since verified it independently: the Phase 9 CI log (243 tests, green) contains every Phase 8 suite (exhaustiveness 17, generators 5, control_flow 10, borrow_checks 19, ...).** The static `#[test]` count of the Phase 8 tree, 186, matches the reported figure.)*

> **Correction recorded in Phase 9:** the paragraph below says `--no-fail-fast` was added to `ci.yml` permanently. The `ci.yml` in the Phase 8 zip did **not** contain it (its test step was plain `cargo test --verbose`). It was actually added in Phase 9 (see below).

*Original status line at time of writing:* 🟡 First CI round: 1 real failure found and fixed at the root, plus one additional, proactively-identified instance of the same bug — pending re-confirmation (no cargo/rustc in this environment, same as every phase)

**Real CI result, round 1:** `borrow_checks` (19), `closures` (10),
`control_flow` (10) all green; `exhaustiveness` reported **13
passed, 1 failed** (`doc9_s2_4_tuple_pattern_with_final_catchall_exhaustive`)
before `cargo test`'s default fail-fast stopped the remaining targets
— **not a sign of additional problems, just genuinely unconfirmed**
until this round is repushed. `--no-fail-fast` added to
`ci.yml` permanently (see below) so every future round gets full
results in one pass rather than needing to fix-and-repush
target-by-target.

### The bug, traced in the actual code before touching anything
Failing source — Document 9 §2.4's own canonical example:
```mtn
match pair {
    (0, 0) => 1,
    (x, y) if x == y => 2,
    (x, y) => 3,
}
```
Genuinely exhaustive (the final bare `(x, y)` matches everything), but
reported non-exhaustive. Traced `is_useful`/`full_signature` against
this exact input by hand before forming any hypothesis: the guard
exclusion itself (`check_exhaustiveness` skipping the middle arm
entirely) was confirmed correct in isolation — the middle arm
contributes zero rows regardless, exactly as it should. **The real
bug: `full_signature` had no case for `Ty::Tuple` at all**, so a tuple
scrutinee always fell through to "not enumerable," which routes
through the default-matrix path. But `(x, y)` — even with every
sub-pattern a wildcard — lowers to `Ctor(Tuple, [Wildcard, Wildcard])`,
never to a bare `CtorPat::Wildcard`; the default-matrix path only ever
picks up rows that ARE literally `Wildcard`. So the all-wildcard tuple
catch-all could never be recognized as covering anything, regardless
of what the guarded arm did or didn't do — the mechanism the review
hypothesized (a guarded row incorrectly marking its shape as "handled")
was a reasonable place to look but wasn't what was actually happening;
the guard-handling code path was never reached for this failure at
all, since the bug was one level up, in how tuples were (not) recognized
as a complete-signature type in the first place.

### Fixed generally, not patched for this one shape
Added `Ty::Tuple(_) => Some(vec![Ctor::Tuple])` to `full_signature`, so
a tuple's single, always-present constructor now goes through the
*real* constructor-specialization path (the one already proven correct
by the other 13 tests for enums/bool/Option/Result) instead of the
infinite-domain fallback. Verified by hand-tracing the fix against the
exact failing input: now correctly reports exhaustive.

### Checked for the same class of issue elsewhere, as asked — found one more real instance, fixed proactively
A tuple is a single-constructor product type; the same "no `Some(..)`
case in `full_signature`" gap applied identically to **tuple structs**
— Document 9 §2.4's *other* worked example in the same section
(`struct Point(f64, f64);` with a final bare `Point(x, y)` catch-all)
has the exact same shape and would have failed the exact same way,
untested until checked for specifically. Fixed alongside the tuple fix:
`full_signature`/`field_types` now also recognize a registered tuple
struct's single constructor as complete, via a new `Ctx::struct_fields`
lookup sourced from `types.rs`'s existing `self.structs` registry
(mirroring how `self.enums` already feeds `EnumTable`) — plumbed
through as a new `StructTable` parameter to `check_exhaustiveness`.
Named-field structs are structurally unaffected either way, since
`Pattern` has no variant for destructuring one at all.

Also traced (not just asserted) a deeper combination — a tuple whose
element is itself an enum, with one specific-variant arm plus a
fully-wildcard catch-all (`match (httpMethod, flag) { (HttpMethod::Get,
true) => .., (x, y) => .. }`) — by hand, through the full recursive
specialization across both the `Tuple` and nested `HttpMethod`
constructor sets, confirming the fix composes correctly under nesting,
not just at the top level.

**A real lifetime bug caught in this fix's own first draft, before
compiling anything:** `StructTable` was initially typed as
`HashMap<String, &'a [Ty]>` (mirroring `EnumTable`), but unlike
`EnumShape.variants` (which already exists as a persistent
`Vec<(String, Vec<Ty>)>` field `EnumTable` can borrow a slice from
directly), a struct's field TYPES have to be freshly extracted from
`StructShape.fields`'s `(name, Ty)` pairs — an unavoidable allocation.
Borrowing a slice from that freshly-built, function-local `Vec` would
have been a dangling reference the moment the building closure
returned. Caught by re-reading the draft before it was ever compiled;
fixed by making `StructTable` own its data (`HashMap<String, Vec<Ty>>`)
instead of borrowing, since there was no actual need for
`EnumTable`-style zero-copy borrowing here in the first place.

**Verified no regression:** re-traced all 13 previously-passing tests
against the fix by inspection — none reference `Ty::Tuple` or an
unregistered-as-enum `Ty::Named` type at any point `full_signature` is
consulted, so the fix is additive and isolated to the cases it's meant
to correct.

### Tests added (net +3, `tests/exhaustiveness.rs` now 17)
- `doc9_s2_4_point_tuple_struct_with_final_catchall_exhaustive` +
  `point_tuple_struct_missing_final_catchall_is_non_exhaustive` — the
  proactively-found Point case, both directions, so this doesn't stay
  a silent fix.
- `tuple_containing_enum_with_wildcard_catchall_exhaustive` — the
  nested-combination confidence check described above.
- The original failing test (`doc9_s2_4_tuple_pattern_with_final_catchall_exhaustive`)
  is unchanged — it was a correct test catching a real bug, not a test
  that needed fixing.

`.github/workflows/ci.yml` also got `--no-fail-fast` added to the test
step permanently, so a future failure anywhere doesn't hide results
from every target after it — see the "How to verify" and "Predicted
test counts" sections at the end of this Phase 8 entry for the current,
consolidated numbers and re-verification instructions (this correction
section only covers what changed in this round; the original write-up
immediately below is otherwise unchanged from before CI ran).

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
No changes needed here specifically — the `--no-fail-fast` addition is
covered in the correction section at the top of this Phase 8 entry.

### Predicted test counts (post-fix; real counts required from CI)
| Suite | Count | Note |
|---|---|---|
| lib (inline) | 60 | 57 confirmed + 3 new lexer tests |
| borrow_checks | 19 | 17 confirmed + 2 new yield-borrow tests |
| closures | 10 | unchanged |
| control_flow | 10 | confirmed green in CI round 1 |
| exhaustiveness | 17 | **14 confirmed (13 passed/1 failed) → fixed, +3 new tests → 17** |
| generators | 5 | unconfirmed (fail-fast stopped before this target ran) |
| generics | 12 | unconfirmed (fail-fast stopped before this target ran) |
| integration | 6 | unconfirmed (fail-fast stopped before this target ran) |
| parser_doc24 | 6 | unconfirmed (fail-fast stopped before this target ran) |
| traits_impls | 15 | unconfirmed (fail-fast stopped before this target ran) |
| types_doc5 | 26 | unconfirmed (fail-fast stopped before this target ran) |
| **Total** | **186** | |

### How to verify (no cargo/rustc/network here)
Push and check the Actions log — `--no-fail-fast` is now in `ci.yml`
(see the correction section above), so this round should cover every
target in one pass regardless of any individual failure. Expect
`cargo build --verbose` clean (0 warnings), and `cargo test --verbose`
showing **186/186**: the 3 previously-confirmed-green suites unchanged
(borrow_checks 19, closures 10, control_flow 10), `exhaustiveness` now
at 17/17 (up from the reported 13/14), and the six suites that never
got to run this round (generators, generics, integration,
parser_doc24, traits_impls, types_doc5) reporting their real counts
for the first time.

### Exit criteria — self-assessment
> "The exhaustiveness checker must correctly reject every
> non-exhaustive match test case (and correctly accept exhaustive
> ones)... the generator state machine must correctly resume from
> every yield point across multiple `.next()` calls, preserving local
> state correctly between suspensions."

Real CI already found one genuine exhaustiveness bug this hand-trace
process missed the first time around — Document 9 §2.4's own tuple
example, reported non-exhaustive when it wasn't — now fixed at the
root (a missing `Ty::Tuple` case in `full_signature`, not a
guard-handling bug as first hypothesized) and, per the explicit
instruction to check for the same class of issue elsewhere, found and
fixed one more real instance proactively (tuple structs, Document 9
§2.4's *other* worked example) before it could cause a second,
separate failure report. The generator lowering+interpreter and the
yield/borrow-checker interaction are untouched by this fix and remain
hand-traced-only at the time; those suites have since run green in CI (Phase 9 log, 243 tests).
**Confirmed by real CI (186 reported by the owner; independently verified through the Phase 9 log).** ✅ Full accounting of
every bug found and fixed this phase (lexer, loop type-checking,
exhaustiveness ×2) is in the correction section at the top of this
entry and in `exhaustive.rs`'s own doc comments. One scope cut remains
flagged (`DoWhile` has no label field in the AST).

## Phase 9 — Error Handling
**Status: 🟢 Confirmed by a real CI log (reviewed by the manager): 243 tests passed, 0 failed** — lib 60, borrow_checks 19, closures 10, control_flow 10, error_handling 57, exhaustiveness 17, generators 5, generics 12, integration 6, parser_doc24 6, traits_impls 15, types_doc5 26. Only warning was the unused import `Item` in `tests/generators.rs:12` (removed in Phase 10). *(The paragraph below is the original Phase 9 hand-off text, kept verbatim.)* Original note: No `cargo`/`rustc`/network in that environment.

### 0. Claims in the hand-off message, checked against the actual zip
| Claim | Result |
|---|---|
| Phase 8: 186 tests | Static `#[test]` count of the Phase 8 tree = **186** (lib 60 + integration 126). Matches. (A naive `grep -c` says 187 because one line in `borrow_checks.rs` and one in `generator.rs` merely *mention* `#[test]` in comments; counted here only when the attribute starts a line.) |
| `--no-fail-fast` already permanent in `ci.yml` | **False for this zip.** Absent; test step was `cargo test --verbose`. PROGRESS.md's Phase 8 entry claimed it had been added — that claim was wrong. Added now (`cargo test --verbose --no-fail-fast`). |
| Doc 23 has §17 (eight grammar clarifications) | **True** in the project-knowledge copy (§17.1–17.8 present, incl. §17.4 `try_expr`). |
| `mtnc check` runs the pipeline | **Not true**, and worth knowing: `main.rs` only runs the **lexer** (banner still says "Phase 1"). Parser, type checker and borrow checker are exercised only by tests, not the CLI. Unchanged in Phase 9; flagged for whoever wires the CLI (Phase 10 is the natural point). |

### 1. Scope delivered (Document 25 §2.3, Phase 9; Document 11 §§1–5)
- **`Result`/`Option` constructors, typed** — `Ok(x)`, `Err(e)`, `Some(x)`, `None` (previously `None` was an *undefined variable* and `Ok(..)` fell into the untyped fallback). One variant table, `types::builtin_variants`, now feeds constructor typing, pattern binding **and** `exhaustive.rs` (which used to carry a private copy).
- **`?` operator** (§2) — on `Result<T,E2>` yields `T` and routes `E2` to the innermost target; on `Option<T>` yields `T` and routes `None`. Error conversion requires equal types or a real `impl From<E2> for E` (recorded with its trait arguments in the new `ImplRecord.trait_args`, so `From<IoError>` and `From<ParseError>` on one target are distinguishable). Missing impl = compile error, per §2/§7.
- **`try`/`catch`** (§3, Doc 23 §17.4 expression position) — checked natively as another propagation target. **`throw`** contributes its value type as an error source (§3.1).
- **`panic`, `assert`, `ensure`** (§4, §5) — `panic(String) -> !`, `assert(bool) -> ()`, `ensure(bool, E) -> Result<(), E>`.
- **`return` is now checked against the enclosing function's declared return type** (Phase 8 flagged this as never done; needed so `return Err(..)`/`return Ok(())` have a type to check against).

### 2. Design point behind the exit criterion (Pillar I)
`try` is **not** a second mechanism. The checker keeps one stack of *propagation targets* (`PropTarget`: enclosing `fn` return type | `try` block | closure-opaque). `?` in a function body and `?` inside a `try` block are routed by the **same** function, `route_result_error`; a `try` block just collects the sources and resolves its error type at the end. Every resolved edge is logged as a `PropagationRecord { kind, source, target, converted }`.

### 3. Exit criterion — what is and is not proven
> "`try`/`catch`'s desugaring must produce byte-identical compiled output to the equivalent hand-written `?`-chain" — verify structural equivalence at the AST/type-checking level for now.

- **Proven (CI-confirmed):** `phase9_try_catch_matches_hand_written_question_chain` checks Document 11 §3's own example written both ways (hand-written `fn __try_block`-style function + `match`, and `try`/`catch`). Both must type-check clean and yield **identical `PropagationRecord` lists** (same sources, same target error type, same `From` conversions, same order) — 4 records, first 2 == last 2, with `IoError→ConfigError` converted and `ConfigError→ConfigError` identity. The decision logic was also executed in a Python model (`resolve_try_error_type`, 9 cases incl. the equivalence) — all agreed with the hand-traces.
- **Was NOT proven in Phase 9 — closed in Phase 10 (literal `try` desugaring + identical-IR test, CI-confirmed):** (a) that the *generated IR* is byte-identical (no codegen exists); (b) I did **not** build a literal AST→AST desugaring pass (`try` → nested `fn` + `match`). The checker treats `try` natively and relies on the shared routing code + the record-equality test. Phase 10 should either lower `try` by literally desugaring before IR generation or diff the two IR outputs; either closes the criterion. Until then this criterion is *structurally supported*, not *confirmed*.

### 4. Bugs / gaps found by tracing (each fixed at its root, then swept for siblings)
1. **Numeric literal never checked against a non-numeric expected type** (latent since Phase 3): `check_expr` exempts int/float literals assuming `check_literal` adopted `expected`, true only for numeric `expected`. `let s: String = 5;` and `Err(5)` against `IoError` passed silently. Found by hand-tracing my own test `err_payload_of_wrong_type_rejected`. Fixed in `check_expr`; `Ty::TypeParam` deliberately exempt (an unannotated generic struct literal passes placeholder types — see `check_struct_lit`).
2. **`()` typed as an empty tuple, not `Ty::Unit`** — `Ok(())` could never satisfy `Result<(), E>` (Doc 11 §5's own example). Fixed in the `Tuple` arm.
3. **`!` was not a bottom type** — `check_compatible` rejected `!` vs anything; `if`/`match` arms of type `!` conflicted with value arms (Doc 9 §2.3's `panic` arm; Doc 24 §1's `_ => return ..`). Fixed in `check_compatible`, `check_if`, `check_match`.
4. **A block ending in `return`/`throw`/`panic(..);` typed as `()`**, not `!` — would falsely reject Doc 24 §1's `catch (e) { return ..; }`. `check_block` now returns `!` for such blocks (all `check_block` callers audited: loop bodies ignore the result; `if`/`match`/try use it correctly).
5. **Match-arm pattern bindings were never typed** — Doc 11 §1.1's `Ok(contents) => print(contents)` reported `contents` undefined. `bind_pattern` now distributes types for `Ok/Err/Some/None`, user enum variants, tuples, tuple structs. Unknown scrutinee ⇒ variables bound as `()` (existing convention) instead of falsely "undefined".
6. **No auto-deref through references** — `balance: borrow mut Account` then `balance.funds` silently typed `()` (Doc 11 §5's own example). `strip_refs` now applied to field access, method receivers, indexing.
7. **Exhaustiveness falsely rejected a constructor-only `match` over an unresolved call** (Doc 24 §5: `match rx.recv() { Ok(..) => .., Err(_) => .. }`): `()` is an "unenumerable domain". Skipped when the scrutinee is `()` *and* unresolved.
8. **Duplicate Option/Result variant table** (`exhaustive.rs` vs everywhere else) → single `builtin_variants`.
9. **PROGRESS.md/`ci.yml` mismatch about `--no-fail-fast`** (section 0).

### 5. Flagged spec interpretations — ✅ APPROVED by the manager; now Document 11 §8 / §8.9
Written up as a proposed **Document 11 §8** in the delivered `11-error-handling.md` (project-knowledge copy is read-only here; please replace it). Summary:
1. **`return` directly inside `try`** → compile error (Doc 11 §3's wrapper-fn desugaring would make it exit the wrapper; surface syntax suggests otherwise). Alternative: define it as returning from the enclosing function.
2. **A `try` block's error type** when its `?`s differ → the first source type all others equal-or-`From`-convert into (reproduces §3's example). `From` is not transitive. No source at all → compile error (Doc 5 rule 6).
3. **`?` on `Option` inside `try`** → compile error (wrapper returns `Result`).
4. Not a real ambiguity but a decision: `Ok`/`Err`/`None` with no expected type are compile errors (rule 6); `Some(x)` is self-determining.
5. `Option`/`Result` remain dedicated `Ty` variants (single shared variant table) rather than literal source-level enums until the Phase 16 prelude.

### 6. Known gaps (deliberately not closed)
- `.into()` as a user-written method is not typed (only the `?` conversion path uses `From`). Bare `return;` in a non-unit function is not rejected (generators/async make the rule non-trivial).
- `let`-destructuring still binds only plain identifiers (only *match arms* got pattern binding).
- `?`/`return`/`throw` inside **closures** are not validated (closure return types untracked) — silent, not fabricated errors. `?` on an *unresolved* stdlib call is silent (Doc 24 §1's `parseJson(..)?`); a `try` whose only `?`s are unresolved gets no error-type check.
- `assert` debug-only stripping / `ensure` always-present: type-level only; the runtime distinction is Phase 10 (nothing to strip yet). `recover` (Doc 11 §4) is Phase 12 (needs `spawn`/`actor`).
- Nested item declarations inside blocks are still not checked (Phase 3 gap, unchanged).
- *(Status update 2026-11-08: `.into()` was closed in Phase 11b M5; nested items in Phase 11a / 11b-1. Still open: `?`/`return`/`throw` inside closures (Phase 12); `let`-destructuring beyond plain identifiers and bare `return;` rules — see the Open items register.)*

### 7. Files changed
`mtnc/src/types.rs` (bulk), `mtnc/src/exhaustive.rs` (shared table, 1 hunk), `mtnc/tests/error_handling.rs` (new), `.github/workflows/ci.yml` (`--no-fail-fast`), `PROGRESS.md`. **No AST/parser/lexer changes.**

### 8. Tests
57 new `#[test]`s in `tests/error_handling.rs` (constructors/Option/Result ×13, `?` ×13, `try`/`catch`/`throw` ×13 incl. the exit-criterion test, `panic`/`assert`/`ensure` ×11, pattern bindings ×5, return-checking ×1, variant-table guard ×1 = 57; per-section counts computed from the file, grouped by section header). **Expected total = 186 + 57 = 243. This is a static count of `#[test]` attributes, NOT a test run** — the real number comes from the CI log.

### 9. Where regressions could appear if CI is red (existing behavior I changed)
Every existing test that uses `return X;` (types_doc5, generics ×6, traits_impls ×8, control_flow, closures) was hand-traced against the new return-type check and should pass. Other behavior changes that could, in principle, newly reject a previously-accepted program: (1) literal-vs-non-numeric check (§4.1); (2) `!` handling in `if`/`match`; (3) `check_block` returning `!`. If any pre-Phase-9 suite fails, look at those three first. Also watch the compiler output for warnings (target: 0) — new private items were swept for dead code (every new fn/field has ≥1 use) but that is a grep, not `rustc`.

### 10. What to push / where to look
Push the whole tree (keep the `mtnc/` layout). In GitHub → Actions → the run for this push → job `build-and-test` → step **Run test suite** → download the raw log. Check: (a) per-target `test result:` lines — `error_handling` should say `57 passed`; (b) grand total 243; (c) no `warning:` lines from `rustc`; (d) with `--no-fail-fast` a red run lists **all** failing tests at once, so send that whole log rather than fixing one at a time.

## Phase 10 — LLVM IR codegen + native backend
**Status: 🟢 Confirmed by a real CI log (reviewed by the manager): 278 tests passed, 0 failed, 0 warnings** (243 earlier + 35 e2e); smoke tests passed (`hello.mtn` prints `Hello, Mountain!`). **The seven "flagged for sign-off" items in §5 below are APPROVED and now authoritative** (recorded in Doc 17 §10 and Doc 11 §8.9).

### 0. Housekeeping done first
- Phase 9 marked 🟢 above (243 passed, from the confirmed CI log).
- Unused import `Item` removed from `tests/generators.rs` (build now shows **0 warnings**).
- Stale banners updated: `main.rs` (rewritten, see §2) and the `Cargo.toml` description/"zero external crates" comment.

### 1. Scope delivered (Document 25 Phase 10; Doc 17 §2–7)
Lexer → Parser → Type check (+ exhaustiveness) → Borrow check → *[desugar `try`/`catch`]* → LLVM IR → object file → system `cc` link → native executable.
- **CLI (Doc 17 §8).** `mtnc check [path]` runs the *whole* front end (lexer, parser, type checker incl. exhaustiveness, borrow checker), prints all diagnostics, exits non-zero on error (closes the Phase 9 §0 gap). `mtnc build <file|dir> [-o out] [--release] [--emit-ir]` = check + codegen + link; **no executable or object file is left behind when anything fails** (tested). `mtnc run <file|dir> [--release]` builds into a temp file, runs it, and forwards the exit code. `--emit-ir` prints the LLVM IR (a small debugging aid, **flagged**, see §5).
- **Codegen (`src/codegen.rs`, `src/codegen/expr.rs`, `src/codegen/print.rs`)** with `inkwell 0.10` / LLVM 18. IR generation is **target-agnostic**: pointer width and data layout come from the `TargetMachine` the driver passes in; nothing names a CPU (tested). Phase 15 reuses it with a `wasm32` machine.
- **Language subset that compiles and runs:** non-generic `fn`s (params, returns, recursion, early `return`), `let`/`let mut`/shadowing, assignment and compound assignment; `i8…i128, u8…u128, isize, usize, f32, f64, bool, char`; arithmetic, comparison, short-circuit `&&`/`||`, bitwise, shifts, `**`, unary `-`/`!`/`~`, `??`, `as` casts; `if`/`else` (statement and expression), `loop` with `break value`, `while`, `do/while`, `for` over `a..b`/`a..=b` integer ranges, labeled `break`/`continue`; structs (stack, by value), tuple structs, enums incl. data-carrying variants (tagged union `{ i32 tag, [N x i64] payload }`), tuples (`t.0`, nested `t.1.0`, `let (a, b) = …`), `match` on ints/bool/char/enums/tuples with guards and or-patterns; `Option`/`Result`, `Some/None/Ok/Err`, `?` (same error type), `try`/`catch`/`throw`, `panic`, `assert`, `ensure`.
- **Match lowering.** Matches whose arms are all literal / unit-variant / data-variant-with-irrefutable-fields / catch-all with no guards become one LLVM **`switch`** (on the enum tag or the scalar); anything else (guards, tuples, floats, nested refutable patterns) becomes a chain of conditional branches. (Doc 17 §6 says "`switch` where dense"; I use `switch` whenever it is possible and let LLVM pick jump table vs. compares — flagged, §5.)
- **Overflow / arithmetic (Doc 5 §2.1, Doc 4 §1, Doc 11 §5).** Debug (default): `+ - *` and unary `-` use `llvm.{s,u}{add,sub,mul}.with.overflow` and panic ("attempt to add with overflow", …); shifts check the amount. Release (`--release`): wrapping, shift amount masked, `assert` stripped. Division/remainder by zero always panic; signed `MIN / -1` panics; a **literal** zero divisor is a compile error (Doc 4 §1). Float→int casts saturate (`llvm.fptosi.sat`/`fptoui.sat`). `ensure` is always present.
- **`panic`** writes `thread 'main' panicked: <message>` to stderr and exits with **code 101** (no source location yet — spans are not carried on expressions; Doc 22 backtraces are Phase 23).
- **`fn main()`** may return `()` or `Result<(), E>` (Err → exit code 1; Doc 19/24 show Result-returning mains). The C `main` is a generated wrapper around `mtn_main`; all user functions are emitted as `mtn_<name>` so they cannot collide with libc.
- **Not supported → clear error "… not yet supported by codegen — Phase N":** closures, generic functions/structs/enums, `impl`/`trait`/methods, `dyn`, references/`borrow` params, `Box`/`Rc`/heap types, arrays/`[T]`/indexing, `String` operations, `const`/`static`/`type` items, nested items in fns, `yield`, `async`/`await`/`spawn`/`select`, `table`/`query`, `ui`/`component`, `server`, `actor`, tensors, modules/`use`/`import`, named/default/variadic arguments, `for` over anything but an integer range, `?` with error-type conversion (`From`), matching on string literals, printing 128-bit integers. **The Phase numbers in these messages are my assignment** (the prompt asked for "Phase N" without a table): closures/generics/traits/refs/heap/arrays/consts → 11, async/spawn/actor → 12, modules → 14, std-dependent (`for` over iterables, string match, printing i128) → 16, server → 17, table/query → 18, ui → 19. **Known gap:** the guaranteed tail-call optimization (Doc 10 §6) is **not** implemented (LLVM may still optimize in release builds, but nothing is guaranteed).

### 2. Typed AST (Doc 17 §1 "TYPED AST") — the design
`TypeChecker` previously returned only errors. Smallest mechanism that gives codegen types: **`TypeChecker::expr_types: HashMap<usize, Ty>`**, filled in `check_expr`, keyed by the address of the `Expr` node (`&Expr as *const Expr as usize`; helper `type_of`). The AST is never mutated or moved after parsing (all nodes live in `Vec`/`Box` heap storage), so addresses are stable identities and no `id`/type slot had to be added to the AST. Codegen reads types with `ty_of(expr)`. A second table `try_info` records, per `try` expression, the resolved `(value type, error type)` of Doc 11 §3's wrapper. **Risk to know about:** this relies on node addresses; any pass that clones or rebuilds expressions must re-run the type checker afterwards (the desugar step does exactly that).

### 3. `try`/`catch` is lowered by literal desugaring (Doc 11 §3, §3.1, §8; `src/desugar.rs`)
Before IR generation `try { B } catch (e) { C }` becomes `match __try_block_N(<captured locals>) { Ok(_) => {}, Err(e) => { C } }` and a synthesized `fn __try_block_N(<captured>) -> Result<V, E> { B'; return Ok(<tail or ()>); }`, where `B'` has every `throw x` rewritten to `return Err(x)`. `V`/`E` come from the checker's `try_info` (Doc 11 §8.2 rule). The lowered program is type-checked **again** (it is ordinary source).
- **Normalization rule for the IR-identity test:** the wrapper of the N-th `try` expression (0-based, source/pre-order) is named exactly `__try_block_N` and is inserted immediately before the item containing the `try`. The hand-written test program spells the same wrapper under that name and in that position; everything else is byte-identical, so `emit_ir` of both is compared with `assert_eq!` (debug and release). **This closes the Phase 9 §3 "NOT proven" item (a) and (b).** Test: `try_catch_ir_is_identical_to_the_hand_written_question_mark_chain` (+ a runtime-equivalence test).
- **Captured variables (flagged interpretation).** Doc 11 §3's wrapper is a free function and never mentions captures. Locals of the enclosing function that the block *reads* are passed as by-value parameters (found by a scoped free-variable analysis). A block that **assigns to / mutably borrows** an outer local, or contains a `break`/`continue` that leaves the block, is rejected with "not yet supported by codegen — Phase 11" (needs by-reference capture). `return` inside `try` was already a type error (Doc 11 §8.1).

### 4. Bugs found and fixed at the root while building this phase (front-end gaps the codegen programs exposed)
1. **Parser: block-like statement did not end the statement.** `while c { .. } -1` parsed as `(while …) - 1`. A block-like expression (`if`/`match`/`loop`/`while`/`for`/`do`/`unsafe`/`try`/`{}`) in statement position now ends the statement (Rust rule). Tests: `block_like_statement_ends_the_statement`.
2. **Parser: positional field access** `pair.0` (Doc 7 §2.4, Doc 5 §3.2) did not parse; `t.1.0` lexes as `t . 1.0` (a float) and is now split into two accesses.
3. **Checker: tuple field typing** — `t.0` silently typed `()`; now typed, and an out-of-range index is an error.
4. **Checker: `let (a, b) = pair;`** (Doc 5 §3.2, Doc 10 §3.3) reported `a`/`b` undefined; destructuring `let` now binds through `bind_pattern`.
5. **Checker: `a ?? b`** checked the fallback against `Option<T>` instead of `T`, rejecting `first_even(3,8) ?? 0`; also `Result<T,_> ?? x` now yields `T`.
6. **Checker: float literal suffix** `0.5f32` was ignored (typed `f64`); now honoured (Doc 2 §6.2).
Sweeps: all existing `check_expr`/`check_binary` callers re-read; the full prior suite (243 tests) was re-run after each change.

### 5. Flagged deviations / choices — ✅ all seven APPROVED (recorded in Document 17 §10)
1. **`print`/`println`/`eprint`/`eprintln` as compiler intrinsics** until the Phase 16 prelude (as the prompt asked): ints, floats (printed with C `%g`), `bool`, `char` (UTF-8), and string values. A `String` is a static `{ptr, len}` view — no heap, no `+`, no `Display`. Marked in `codegen.rs`/`print.rs`; replace in Phase 16.
2. **`mtnc build --emit-ir`** — a CLI flag the spec does not list; added so the IR can be inspected/tested. Remove if unwanted.
3. **`fn main() -> Result<(), E>`** accepted (Err → exit 1); spec only shows it in Doc 19/24 examples.
4. **`switch` whenever possible** (see §1), and **`panic` message has no file:line** (no spans on expressions).
5. **Captured-variable handling for `try`** (§3) and the **Phase numbers in "not yet supported" messages** (§1) are my choices.
6. **Debug is the default build mode; `--release` selects wrapping + `assert` stripping + `default<O2>`** — Doc 5 §2.1 defines the two behaviours but no CLI flag name.
7. **Linking:** object file → system `cc` (no LTO). The CI runner provides `cc`.

### 6. Files changed
New: `src/codegen.rs`, `src/codegen/expr.rs`, `src/codegen/print.rs`, `src/desugar.rs`, `src/driver.rs`, `tests/e2e.rs`, `examples/{recursion,loops,shapes,results,trycatch,panics,overflow,numeric,mixed,control}.mtn`. Changed: `src/main.rs` (new CLI), `src/lib.rs` (modules), `src/types.rs` (typed table, try_info, §4 fixes), `src/parser.rs` (§4.1–4.2), `Cargo.toml` (inkwell), `examples/hello.mtn` (now a real hello world), `tests/generators.rs` (import), `.github/workflows/ci.yml`, `PROGRESS.md`.

### 7. Tests (counts from `cargo test` in the sandbox, not from memory)
Existing 243 unchanged and passing. New `tests/e2e.rs`: **35** (hello, recursion, loops/labels, structs/enums/match/tuples, Option/Result/`?`/`??`/`ensure`, `try`/`catch`, numerics, mixed, diverging control flow; panic → 101; debug overflow → 101; release wraps; div/mod by zero; `MIN / -1`; literal zero divisor = compile error; `assert` debug-only / `ensure` always; shift check/mask; exit-code forwarding; **rejection: type error, borrow error, non-exhaustive `match` → build fails, non-zero exit, no binary/object**; `check` runs the full front end; unsupported features give clear errors; IR: dense/enum `match` → `switch`, guarded `match` → no `switch`, debug uses overflow intrinsics and release does not, unsigned intrinsic, target-agnostic IR, no heap calls for structs; **`try`/`catch` IR == hand-written IR**, runtime equivalence, captured locals, outer-assignment rejected, nested `try`). **Expected total: 243 + 35 = 278.**

### 8. What the manager should read in the CI log
Job `build-and-test` (runs on `ubuntu-24.04`):
1. **Install LLVM 18 development packages** — must succeed.
2. **Build** — no `warning:`/`error:` lines (target: 0 warnings).
3. **Run test suite** — per-target `test result:` lines: lib 60, borrow_checks 19, closures 10, control_flow 10, **e2e 35**, error_handling 57, exhaustiveness 17, generators 5, generics 12, integration 6, parser_doc24 6, traits_impls 15, types_doc5 26 → **grand total 278, 0 failed**. (e2e invokes the real `mtnc` and the system `cc`.)
4. **Smoke test — mtnc --version** — prints `mtnc 0.1.0 (Phase 10 — LLVM IR codegen + native backend)` *(from Phase 11 the banner is `mtnc 1.0.0-dev (...)`)*.
5. **Smoke test — mtnc check on the examples directory** — prints `mtnc check: checked 11 file(s), 0 errors`.
6. **Smoke test — mtnc run examples/hello.mtn** — `expected: Hello, Mountain!` and `actual:   Hello, Mountain!`; the step fails on any mismatch.

### 9. Exit criterion (Doc 25, Phase 10)
> A real `.mtn` program using only Phase 1–9 features compiles to a native binary and produces correct output when executed.
Met locally: `mtnc run examples/hello.mtn` and the other 9 examples compile to native executables and print the asserted output. ✅ Confirmed by the real CI run (278 tests, smoke tests green).

## Phase 11 — Memory management internals + codegen foundations (delivered in three parts: 11a, 11b, 11c)
The owner/manager split Phase 11 (it is too large for one delivery) into **11a** (foundations A), **11b** (generics and calls, itself split into **11b-1**, **11b-2** generics, **11b-3** raw pointers) and **11c** (drop glue, `Box`/`Rc`/`Arc`/`Weak`, `dyn`, allocators, valgrind CI step, arena benchmark). **Phase 11 is 🟢: all three parts have real green CI logs (11a 308, 11b 413, 11c 603, and the final 11c run also has the valgrind step green).** Each part is a separate zip + CI run + manager review.

### Phase 11a — references, arrays/slices, impl blocks, const/static/type, nested items, named/default/variadic arguments
**Status: 🟢 Confirmed by a real CI log (reviewed by the manager): 308 tests passed, 0 failed, 0 warnings** (lib 60, borrow_checks 19, closures 10, control_flow 10, e2e 50, error_handling 57, exhaustiveness 17, generators 5, generics 12, integration 6, phase11a 15, parser_doc24 6, traits_impls 15, types_doc5 26); smoke tests passed. The manager's decisions on the 11a flagged items: items 2, 3, 6 are fixed in 11b-1; item 1 (per-thread tables) accepted as an implementation detail; item 4 stays Phase 11c; item 5: Doc 23 §3 is ground truth for the variadic spelling (`...values: i32`), Doc 10 was corrected.

**Housekeeping done:** Phase 10 marked 🟢 (above). Version label `1.0.0-dev` in `Cargo.toml` and the banner `mtnc 1.0.0-dev (Phase 11 — memory management + codegen foundations)` (the `"0.1.0"` strings in `src/manifest.rs` tests and `tests/integration.rs` are sample `mountain.toml` data and were left alone). "Not yet supported — Phase N" messages audited against Doc 25 §2.5 (corrected: closures/generators/`await`/`spawn`/function values/`actor` → 12, `select` → 13, struct update `..base` → 16; the rest already matched: generics/`dyn`/growable `[T]`/raw pointers/`null`/`?`-with-`From`/heap types/`try` capture → 11 (11b/11c), modules → 14, `for` over iterables/string match/i128 printing/`str` → 16, `server` 17, `table` 18, `ui` 19).

**What 11a delivers (all with positive tests):**
1. **References.** `borrow x: T` / `borrow mut x: T` parameters are an opaque `ptr`; inside the function the parameter name IS the caller's value (its local slot is the incoming pointer, no copy), so `n += 1` on a `borrow mut n: i32` writes the caller's variable. Call sites write `borrow x` / `borrow mut x` (Doc 6 §3); anything else for a borrow parameter is a clear error. Locals of reference type (`let r = borrow c;`) hold the pointer and auto-dereference for field access and method calls. Method receivers `borrow self` / `borrow mut self` / `self` (Doc 7 §2.3).
2. **Arrays and slices.** `[T; N]` is an LLVM `[N x T]` value; slices `&[T]` are `{ptr, len}`. Bounds-checked indexing for reads, writes and compound assignment (`a[i] = v`, `a[i] += v`, `grid[1][0]`), element index may be any integer type (a negative signed index fails the check). Out-of-bounds prints `thread 'main' panicked: index out of bounds: the length is L but the index is I` and exits 101 (Doc 22 §8 wording). `borrow a` where a slice is expected unsizes a fixed array; `borrow a[lo..hi]` / `a[lo..=hi]` make sub-slices with a range check (`slice range out of bounds`). Arrays work as parameters, return values, struct fields, nested arrays, arrays of structs.
3. **`impl` blocks (static dispatch).** Inherent methods, associated functions (`Type::new`, `Self::new`, `Self { .. }`), trait methods, impls on primitive types (`impl Trait for i32`). Symbols are `mtn_<Type>::<method>` / `mtn_<Type>::<Trait>::<method>`. **Default trait methods** are copied into each implementing `impl` before codegen (`desugar::inline_default_methods`, then the program is re-checked) so each type gets its own copy checked with `Self` = that type (the checker never checked trait default bodies themselves).
4. **`const` / `static` / `type` items, nested items.** `const` is inlined at every use (Doc 3). `static` is one LLVM global with a *constant* initializer (literals, tuples, arrays, struct literals, other consts; anything else is a clear error); statics are read-only (the AST has no `static mut`; `Atomic<T>` is Phase 13). `type` aliases are expanded wherever a type is written. Items declared inside function bodies (fn, struct, enum, impl, const, ...) are hoisted to one shared namespace (`desugar::all_items`).
5. **Named, default and variadic arguments (Doc 10 §2.1–2.3).** Positional then named (any order); defaults evaluated at the call site; unknown/duplicate/missing/extra/positional-after-named are compile errors. A variadic parameter (`fn f(...values: i32)`, Doc 23 syntax) is passed as a stack-allocated array viewed as a slice `&[T]` (no heap), so inside the function it is indexable (`values[0]`); `.len()` and `for` over it are Phase 16.

**Type-checker changes (needed by the above; all 278 earlier tests still pass):** new `Ty::Fixed(T, N)` (size is a literal, a `const`, or constant arithmetic of those; any other size expression such as a const-generic one still resolves to the unsized `Array` as before); array literal checked against `[T; N]` (length mismatch is an error); `a[lo..hi]` types as the unsized slice place; `borrow` of a fixed array where `&[T]` is expected is the one unsizing coercion; `&mut T` accepted where `&T` is expected; a string literal is accepted as `&str`; `borrow` parameters have type `&T`/`&mut T` for *callers* (but plain `T` inside the function); variadic parameter is `&[T]` inside; parameter default values are type-checked; `const`/`static` are globals and their initializers are checked; type aliases expanded; associated-function calls `Type::f(..)`/`Self::f(..)` resolve; `Self { .. }` literals; trait default methods and primitive-type impls resolve in `resolve_method`; the range `1..n` takes its type from `n`. Type aliases and constant sizes live in per-thread tables filled by `register_item_context` (because `resolve_type` is a free function) — flagged below.

**Flagged interpretations (11a) — all decided by the manager (see the Status line above):**
1. **Per-thread alias/const tables** behind `resolve_type` (see above). Alternative: thread an environment through every `resolve_type` caller (large change).
2. **Nested items share one namespace**; two functions that each declare a nested item with the same name are rejected ("defined more than once") instead of scoping them per function.
3. **Default trait methods are inlined into top-level `impl` blocks only** (an `impl` nested inside a function does not get its defaults).
4. **`&[T]` of a growable `[T]`, growable `[T]` literals, `String` operations** stay "not yet supported — Phase 11" (11c: they need drop glue). An unannotated array literal `[1,2,3]` is the growable `[i32]` per Doc 5 §1, so in 11a write `let a: [i32; 3] = [1, 2, 3];`.
5. **Doc 10 §2.3 vs Doc 23 §3 variadic spelling:** Doc 10 shows `values: ...i32`; the parser follows the ground-truth Doc 23 (`...values: i32`). Doc 10's example could be corrected.
6. `static` initializers are limited to constant literals (no calls, no arithmetic) — Doc 3 only requires a `const`-evaluable initializer; richer constant folding can come later.

**Known gaps / not in 11a:** generics, `dyn`, `?` with `From`, tail-call guarantee, `unsafe`/raw pointers, `try` blocks that assign outer locals (→ 11b); growable `[T]`/`String`, drop glue, `Box`/`Rc`/`Arc`/`Weak`, `Allocator`/arena, `@pack`/`@align(N)`, valgrind CI step, arena benchmark (→ 11c). Default allocator = system `malloc`/`free` behind one internal interface (no allocation exists yet in 11a); Doc 13 §3.1's size-class allocator is now scheduled in Phase 15 (Doc 25 §2.5) — known gap against Doc 13 §3.1.

**Files changed (11a):** new `src/desugar.rs` additions (`all_items`, `inline_default_methods`), `tests/phase11a.rs`, `examples/{refs,arrays,methods,items,args,combined}.mtn`; changed `src/codegen.rs`, `src/codegen/expr.rs`, `src/codegen/print.rs`, `src/types.rs`, `src/driver.rs`, `src/main.rs`, `Cargo.toml`, `tests/e2e.rs`, `PROGRESS.md`.

**What the manager should read in the CI log (11a):** job `build-and-test` as in Phase 10 (CI file unchanged). Expected `test result:` lines: lib 60, borrow_checks 19, closures 10, control_flow 10, **e2e 50**, error_handling 57, exhaustiveness 17, generators 5, generics 12, integration 6, **phase11a 15**, parser_doc24 6, traits_impls 15, types_doc5 26 → **grand total 308, 0 failed, 0 warnings**. Smoke step output: `mtnc --version` → `mtnc 1.0.0-dev (Phase 11 — memory management + codegen foundations)`; `mtnc check examples` → `checked 17 file(s), 0 errors`; hello.mtn → `Hello, Mountain!`.

### Phase 11b-1 — 11a follow-ups, `?` with `From`, guaranteed tail calls, `try` with outer-local mutation
**Status: 🟢 Confirmed by a real CI log (reviewed by the manager): 322 tests passed, 0 failed, 0 warnings.** (11b-2 = generics, 11b-3 = `unsafe`/raw pointers are separate later deliveries; the manager decided the split after the assistant reported that the front end did not yet support `impl<T>` generics/associated types and that Doc 23/Doc 4 had no raw-pointer grammar.)

**What 11b-1 delivers (each with positive tests; examples in `examples/`):**
1. **Nested items scoped per function** (`src/scope.rs`, run right after parsing, before type checking). A nested item whose name is declared more than once in the program (counting top-level items) is renamed `<enclosing fn path>$<name>` and every reference inside the enclosing function (including inside other nested items) is rewritten; a local variable of the same name shadows it. Names that do not collide are never touched. A nested item is visible in the whole enclosing function body (not just its block); two same-named items in ONE function remain a "defined more than once" error. Example `nested_scope.mtn`.
2. **Default trait methods for nested `impl` blocks**: `desugar::inline_default_methods` now walks the whole item tree (`scope::for_each_item_mut`), so traits and impls nested in functions behave like top-level ones.
3. **`static` constant folding** (`fold` in `src/codegen/expr.rs`): arithmetic (`+ - * / % << >> & | ^ **`), comparisons, `&&`/`||`/`!`, unary `-`/`~`, `as` casts on literals and `const`s (ints, floats, bool, char). Constant overflow, division by zero and a bad shift amount are compile errors; calls and other `static`s are "not constant". To make `static B: u8 = 200 + 55;` type correctly the checker now flows the expected type into the left operand of arithmetic/bitwise operators (Doc 5 rule 3) — a general checker improvement (was: literals defaulted to `i32` and the declaration was rejected). Example `const_fold.mtn`.
4. **`?` with `From` conversion (Doc 11 §2)**: `impl From<Src> for Target` (the one trait with a type argument supported before 11b-2) is registered as `Target::From<Src>::from` and recorded in `from_impls`; `expr?` calls it when the error types differ (no conversion when they are equal; a missing impl is a compile error — front end and codegen both report it). Works inside `try` blocks (the wrapper is an ordinary function, Doc 11 §8.2 picks the block's error type) and `Option?` is unchanged. Several `From` impls for one target are told apart by argument type, both for `?` and for explicit `Target::from(x)` calls (checker and codegen). Example `from_conv.mtn`.
5. **Guaranteed self tail-call (Doc 10 §6)** (`src/codegen/tail.rs` + `call_fn`): the call expressions in tail position (function-body tail, `return f(..)`, inside `if`/`match`/block tails, a trailing `f(x);` statement of a unit function) that call the current function — free function, associated function or method — store the new arguments into the parameter slots and branch to a `tail_loop` block, in debug **and** release. Depth 1,000,000 runs in a debug build (tests). `borrow`/`borrow mut` parameters can only be passed through unchanged (their local IS the caller's pointer); a tail call that passes a different place falls back to an ordinary call. Non-tail recursion (`n * f(n-1)`) is an ordinary call. Example `tailcall.mtn`.
6. **`try` blocks that assign or mutably borrow outer locals (Doc 11 §8.9)**: such locals become `borrow mut` parameters of the synthesized wrapper (call site `borrow mut x`), locals only read stay by-value parameters. `return` inside `try` stays a compile error (§8.1). The Phase 10 IR-equality test (try/catch vs. hand-written `?` chain) still passes. Example `trymut.mtn`.

**Other changes:** checker: `1..n` / expected-type flow (see above); `Ty` helpers unchanged. Removed the obsolete e2e tests `duplicate_nested_function_names_are_rejected` and `try_block_assigning_an_outer_local_is_a_clear_unsupported_error` (their behaviour was intentionally changed) and replaced them with the tests below.

**Flagged interpretations (11b-1) — accepted by the manager; items 3 and 5 were closed later (tail call through `borrow` in 11c C1, `.into()` in 11b M5):**
1. A nested item is visible in the **whole enclosing function body** (not only its own block); renaming happens **only on a name collision**.
2. Constant folding is by the compiler in Rust (i128/f64, f32 results rounded to f32) rather than by LLVM's constant folder; `static` cannot read another `static` (only `const`s) — a `static` is a memory location, not a constant.
3. A tail call whose `borrow`-parameter argument is a different place than the parameter itself is **not** turned into a jump (silently an ordinary call). Such calls are rare; making them loops needs re-bindable reference locals.
4. **Known gap (agreed in the prompt):** no lint warning for non-tail recursion (Doc 10 §6) — the existing diagnostics mechanism has no lint channel yet (Phase 23). **For 11c:** once drop glue exists, a tail call must happen *after* the drops of the current frame — `call_fn`'s tail-call path will need to run the pending drops first.
5. `.into()` as an explicit method is not implemented (only `?` and `Type::from(x)` use `From`).

**Files changed (11b-1):** new `src/scope.rs`, `src/codegen/tail.rs`, `examples/{tailcall,trymut,from_conv,nested_scope,const_fold}.mtn`; changed `src/codegen.rs`, `src/codegen/expr.rs`, `src/desugar.rs`, `src/driver.rs`, `src/lib.rs`, `src/types.rs`, `tests/e2e.rs`, `PROGRESS.md`.

**What the manager should read in the CI log (11b-1):** job `build-and-test` (CI file unchanged). Expected `test result:` lines: lib 60, borrow_checks 19, closures 10, control_flow 10, **e2e 64**, error_handling 57, exhaustiveness 17, generators 5, generics 12, integration 6, phase11a 15, parser_doc24 6, traits_impls 15, types_doc5 26 → **grand total 322, 0 failed, 0 warnings**. Smoke: `mtnc --version` → `mtnc 1.0.0-dev (Phase 11 — memory management + codegen foundations)`; `mtnc check examples` → `checked 22 file(s), 0 errors`; hello → `Hello, Mountain!`.

### Phase 11b (remaining) — generics, associated types, inference, const generics, generic traits, raw pointers (delivered as M1–M7 in one delivery)
**Status per milestone (🟢 only where the manager has seen a real CI log):**
- 11b-1: 🟢 322 tests.
- M1/M2 (generic functions, structs, enums, impls, bounds, monomorphization): 🟢 341.
- M3 (associated types, specialised impls) + item 2b (later-use inference): 🟢 357.
- M4 (const generics, `[x; N]`): 🟢 375; smoke `checked 36 file(s)`.
- M5 (generic traits, `From`/`Into`): 🟢 387; smoke `checked 38 file(s), 0 errors`.
- M6 (`unsafe`, raw pointers, `alloc`/`dealloc`) and M7/11b as a whole: 🟢 **413 tests passed, confirmed by a real CI log (reviewed by the manager)**; smoke `checked 39 file(s), 0 errors`. (The first M6 CI run had 2 failures caused by a test-helper bug, fixed in checkpoint-M6b — see below.)
- **Phase 11 as a whole: 🟢 (11c confirmed by a real CI log, see below).**

**What was delivered**
1. **Generics via AST-level monomorphization** (`src/mono.rs`): generic fns, structs, enums, impls, inline + `where` bounds, turbofish (Doc 23 §17.2), explicit and inferred type arguments. A fixpoint loop in `driver::check_source` (depth 32, cap 4096 instances; infinite instantiation is an error) clones and substitutes per instance (`pick$i32`, `Stack$i32`, `identity$3`, `Buf$f64_2`); codegen never sees generics. Only the first check reports errors on the original program; intermediate re-checks ignore transient errors; the final check reports.
2. **Associated types** (Doc 8 §5), specialised impls (`impl Trait for Stack<i32>`) as template impls.
3. **Later-use inference** for generic type parameters (`Ty::Infer`; deferred bound checks in `finalize_inference`).
4. **Const generics** (Doc 8 §8): `const N: usize` params on fns/structs/enums/impls, inference from argument types, turbofish and expected type, `matmul<const R, K, C>`.
5. **`[x; N]`** (array repeat): **Doc 23 §17.10 is the authority** (the Project copy of Doc 23 now contains §17.9 and §17.10).
6. **Generic traits** (`trait Source<T>`), several impls of one trait told apart by trait arguments, parameterised bounds, default methods; **`From<T>` as a general mechanism**, **`Into<U>`/`.into()`** as a built-in blanket over `From`.
7. **`unsafe` blocks, raw pointers `*const T` / `*mut T`, `alloc`/`dealloc`** (Doc 23 §17.9, Doc 3 Category C, Doc 13 §5): prefix `*`/`&`/`&mut` bind tighter than `as`; deref and `alloc`/`dealloc` only in `unsafe`; `*mut`→`*const` is the only implicit conversion; writes through `*const` rejected; cast table pointer↔pointer, `&T`→`*const T`, `&mut T`→`*mut/*const T`, pointer↔`usize`/`isize`, everything else an error; `null` as a pointer only in `unsafe`; pointer arithmetic, pointer methods and indexing give "not yet supported -- decided with the library that needs it". Codegen: opaque pointers, loads/stores with the pointee type, `ptrtoint`/`inttoptr`; `alloc`/`dealloc` go through ONE internal interface (`__mtn_alloc`/`__mtn_dealloc`: `malloc`, `aligned_alloc` when align > 16, `free`) so the allocator can be swapped later.

**Bugs found and fixed (each at its root)**
- Parser: `>>` closing nested generics (`Option<Option<i32>>`) did not parse; `expect_gt` now splits `>>`, `>=`, `>>=` (M5).
- Const parameter on a generic enum lost its const argument in variant/pattern types; fixed, with a test (`const_parameter_on_a_generic_enum`) (M4/M5).
- Deferred bound check skipped because fresh inference variables were inserted after the bound loop (M3b).
- Smaller M4 fixes: `[]` against a const-sized struct field, explicit const turbofish (`Buf::<f64,2>::new`), a generic body calling another generic with a bare const param, spurious "expected [f64], found [f64; 20]" for `[x; N]`.
- **Parallel-test temp-file collision (M6b):** the test helpers in `assoc_types`, `const_generics`, `generic_traits`, `infer`, `mono`, `raw_pointers` derived the temp dir from `process::id()` + `src.len()`. Cargo runs tests as threads of one process, so two tests with equal-length sources shared one `p.mtn` and each printed the other's result (CI: `doc13_alloc_write_read_dealloc`, `rejects_deref_of_non_pointer`). Fixed with a per-file `AtomicUsize` counter in the path; swept: `e2e.rs` already used a counter, no other test writes temp files.

**Flagged interpretations (manager-accepted unless marked)**
- Generic impl bodies are checked only after instantiation (Doc 8 §2).
- `[]` keeps the eager Phase-3 error; later-use inference applies to generic type parameters only.
- Phase 5 matmul: `matmul<const R, K, C>` is the general mechanism; the `Matrix * Matrix` special rule stays.
- A const-param-sized struct field is checked for its element type in round 0; the exact length is checked on the instance.
- `Into` is built in over `From` and not user-implementable; the orphan rule stays (`impl From<Celsius> for f64` is rejected).
- Item 2c (a method with its own `<U>` inside a generic impl): "not yet supported by codegen — Phase 11" (Doc 25 §2.5); known gap — **closed in 11c C1/C1b**.
- Printing `&i32` is Phase 16. An integer-literal `let` binds `i32` at its first use when the variable is unbound; operators/field access on a still-unbound inference variable may reject.
- **M6 — approved by the manager; the "pre-existing gap" (assignment to immutable bindings, parameters, pattern bindings) was closed in 11c C1:** `&mut x` / `borrow mut x` of a non-`mut` `let` binding is rejected (Mountain had no such rule; plain assignment to an immutable binding is still accepted — a pre-existing gap). Parameters and pattern bindings are not tracked. Pointer `==` was left as it was (untested). No `unsafe fn`.

**Known limits / gaps**
- Const expressions inside type arguments (`Matrix<f64, R*C, 1>`) are not parsed; no later-use inference for const params.
- M3: same-named associated types in two traits implemented for one type (first impl wins); `Stack<i32>::Item` with generic args before `::`; `impl<T> Foo<Vec<T>>` (type parameter nested in an argument).
- M5: `dyn Trait<T>` unsupported; generic-trait impls with the trait argument nested in another type only partly covered; a parameterised bound on an unresolved inference variable is not deferred.
- **Pre-existing gap vs Doc 7:** impl method signatures are not compared with the trait's (names only).
- Raw pointers: no arithmetic, methods or indexing (deliberately, until a library needs them); `&mut` mutability check does not cover parameters.

**Files changed (11b remaining):** new `src/mono.rs`; new tests `tests/{mono,assoc_types,infer,const_generics,generic_traits,raw_pointers}.rs`; new examples `generic_fn, generic_bounds, generic_struct_pair, generic_nested_impls, generic_enum, generic_turbofish_sigs, assoc_container, assoc_generic_impl, infer_later_use, array_repeat, const_generic_fns, const_generic_container, const_generic_matmul, const_generic_assoc, generic_traits, from_into, raw_pointers` (17); changed `src/{ast,parser,types,borrow,scope,desugar,codegen,driver,lib}.rs`, `src/codegen/expr.rs`, `tests/e2e.rs`, `PROGRESS.md`, new `WORKLOG_11B.md`. CI file unchanged.

**What the manager should read in the CI log (11b remaining)** — job `build-and-test`, step "Run tests" (`cargo test`) and the smoke step. Expected `test result:` lines: lib 60, assoc_types 8, borrow_checks 19, closures 10, const_generics 19, control_flow 10, e2e 64, error_handling 57, exhaustiveness 17, generators 5, generic_traits 11, generics 12, infer 8, integration 6, mono 19, parser_doc24 6, phase11a 15, **raw_pointers 26**, traits_impls 15, types_doc5 26 → **grand total 413, 0 failed, 0 warnings** (re-checked: 4 consecutive full runs in the sandbox, no flakiness). Smoke: `mtnc --version` → `mtnc 1.0.0-dev (Phase 11 — memory management + codegen foundations)`; `mtnc check examples` → **`checked 39 file(s), 0 errors`**; hello → `Hello, Mountain!`. A real CI log for M6/M7 is what turns M6 and 11b green.

### Phase 11c — 🟢 COMPLETE (C0–C7), confirmed by a real CI log including the valgrind step (see `WORKLOG_11C.md`)
**Status: 🟢 Confirmed by a real CI log, reviewed by the manager (2026-11-08): 603 tests passed, 0 failed, 0 warnings across 29 test binaries; CI step "Memory examples under valgrind" passed (`valgrind clean`) for all 9 programs in `examples/memory/`; smoke: `checked 51 file(s), 0 errors`, `Hello, Mountain!`.** Delivered as checkpoints C1, C2, C4, C5, C6 (all ACCEPTED by the manager) and `final-11c.zip`.
- **Manager verification record.** (1) The tree uploaded to GitHub is byte-identical to `final-11c.zip` (apart from `ci.yml`, see next point). (2) The first CI run after `final-11c` was green on tests but had no valgrind step (`ci.yml` was not in the zip; the valgrind test in `tests/c7_final.rs` skips itself when valgrind is not installed) — so the exit criterion was not yet proven by CI. (3) The manager prepared a `ci.yml` with `valgrind` in the apt list and a step `bash scripts/valgrind_memory_examples.sh target/debug/mtnc`; the owner installed it; the next CI run (all green) is the confirming run. `bash` is used because a script uploaded through the GitHub web UI loses its executable bit. (4) Independently, the manager built the tree with LLVM 18, ran all 9 programs under `valgrind --leak-check=full --show-leak-kinds=all`: every allocation freed, 0 errors; the full suite gave 603 passed; the arena benchmark gave default allocator 10.8 ms vs arena 5.0 ms.
- **Housekeeping that remains for the owner:** the test helpers leave build outputs (`*.bin`) in `mtnc/examples/memory/`; they must never be committed. The `.gitignore` in the repo root now needs the line `/mtnc/examples/**/*.bin` (a ready file was supplied by the manager).
- **C0:** statuses above updated (11b-1, M1–M6 all 🟢; 413 confirmed by CI).
- **C1 (carry-over fixes), fresh sandbox numbers: 441 passed, 0 failed, 0 warnings** (lib 60, assoc_types 8, borrow_checks 19, **c1_carryover 29 (new)**, closures 10, const_generics 19, control_flow 10, e2e 64, error_handling 57, exhaustiveness 17, generators 5, generic_traits 11, generics 12, **infer 7 (−1)**, integration 6, mono 19, parser_doc24 6, phase11a 15, raw_pointers 26, traits_impls 15, types_doc5 26); `mtnc check examples` → `checked 41 file(s), 0 errors`.
  1. **Guaranteed tail call through `borrow`/`borrow mut`/`borrow self`** — `borrow` parameters are rebound through a phi at the loop head; depth 1,000,000 tested in debug and release (swapped places, `borrow mut` writes, method receiver, field of a borrow parameter). A tail call whose reference argument may point into the current frame is a clear compile error (no silent ordinary call). Flagged reading: the check is conservative (see WORKLOG_11C).
  2. **Immutable bindings (Doc 3 `let`/`mut`)** — reassignment, compound assignment, field/index assignment, and `borrow mut` of a non-`mut` binding are rejected, for `let`, destructuring/pattern bindings, `for` variables, match bindings, by-value parameters and `borrow` (non-`mut`) parameters. **Exact reading of deferred initialisation:** `let x: T;` followed by one plain `x = v;` is the first assignment (allowed, tracked per control-flow path: each `if`/`else` branch and each `match` arm may initialise once); a second assignment, any compound assignment, and an assignment inside a loop or closure body that does not also contain the declaration are errors. Writing through a reference or raw pointer held in an immutable binding is allowed. **No earlier test relied on the old behaviour** (all 413 passed unchanged; one old test was removed: `infer::generic_method_with_own_params_in_generic_impl_is_clear_error`, which asserted the gap that item 3 closes). Flagged: Doc 3 says `mut` may mark a function parameter, but Doc 23's parameter grammar has no `mut x: T` form, so by-value parameters are simply immutable (no way to make one mutable) — needs the manager's ruling.
  3. **Generic method with its own `<U>`** — works in plain and generic impls (inference, bounds, several instances); implemented in checker + mono (`method_instances`). Limits: methods of a trait / trait impls, explicit turbofish on a method call.
- **C1b (manager-required leftovers), delivered with checkpoint-C2:** turbofish on method calls (`x.pick::<f64>(..)`) and generic methods declared in traits / trait impls / trait default methods, with positive and rejection tests; the misleading "missing required method" message is gone (a really missing generic trait method still gets the correct message).
- **C2 (growable `[T]` and heap `String` handles, Doc 13 §1.3), delivered with checkpoint-C2: 468 passed, 0 failed, 0 warnings** (c1_carryover 40, c2_handles 16, rest as before; `checked 42 file(s), 0 errors`). `String` / `[T]` are `{ptr, len, cap}`; string literals in `String` context allocate on the heap through `__mtn_alloc`; as a `&str` they stay static views. Growable literals, indexing with bounds check (exit 101), slice views `borrow v` / `borrow v[lo..hi]`; `for` over them stays Phase 16; `push`/`len`/concatenation stay Phase 16. **Until C3 these buffers are never freed.**
- **C3 (drop glue + `Box<T>`) and C4 (`Rc`/`Arc`/`Weak`), delivered with checkpoint-C4: 514 passed, 0 failed, 0 warnings** (c3_drops 30, c4_rc 16; `checked 46 file(s), 0 errors`; valgrind-clean `examples/memory/`). Details, the flagged mechanism choice (zero-on-move instead of per-variable flags), the new by-reference `match` binding mode, the `Weak<T>` / `Weak<T, Arc>` decision, the invented API names (`upgrade`, `strongCount`, `weakCount`) and the carried gaps are in WORKLOG_11C.md "C3/C4 as built". **No user `Drop` trait: not specified in Doc 6/13.**
- **C5 (`dyn Trait`) done, plus manager-required item 5 (assignment/`borrow mut` through `Rc`/`Arc` is a compile error) and a loop-move test: 535 passed, 0 failed, 0 warnings; `checked 47 file(s), 0 errors`.** Fat `{data, vtable}` pointers, vtable `{drop, size, align, methods..}`. Doc 9 says nothing about matching through a reference (by-reference binding is an accepted interpretation on Doc 9 §2.4 + Doc 6); Doc 6 §6 writes one `Weak<T>` (we split `Weak<T>`/`Weak<T, Arc>`; Phase 16 may unify); invented names `upgrade`/`strongCount`/`weakCount`; carried gaps: temporaries live to end of statement, no move out of an array by index, `for` over growable `[T]` = Phase 16, `Rc` not rejected as non-`Send` (Phase 12/13). See WORKLOG_11C.md.
- **C6 done (plus the manager's `dyn`-as-value decision): 575 passed, 0 failed, 0 warnings; `checked 49 file(s), 0 errors`.** `Allocator` trait (compiler-known), `Box<T, A>` / `Box::newIn(value, borrow allocator)` freeing through its own allocator (also `Box<dyn T, A>`), `@pack` / `@align(N)` with layout tests. `dyn Trait` as a value type = owned `Box<dyn Trait>` (reading recorded: Doc 13 §1 point 4, Doc 7 §4.5, Doc 8 §6, Doc 3 F, Doc 11 §6 `Option<borrow dyn Error>`). Flags: `newIn` takes a shared `borrow` allocator although `alloc` is `borrow mut self` (the spec example does exactly this); a box does not track the arena's lifetime; Doc 13's `buffer: [u8]` arena needs Phase 16, the example uses `alloc`/`dealloc` intrinsics; new impl-vs-trait parameter-count check. Details in WORKLOG_11C.md.
- **C7 (final), fresh numbers: 603 passed, 0 failed, 0 warnings, 29 test binaries** (lib 60, assoc_types 8, borrow_checks 19, c1_carryover 40, c2_handles 16, c3_drops 31, c4_rc 20, c5_dyn 27, c6_alloc 29, **c7_final 28 (new)**, rest unchanged); `mtnc check examples` -> **`checked 51 file(s), 0 errors`**.
  - `examples/memory/` (9 programs — the earlier text said 11, a counting typo confirmed by the coding assistant — all valgrind-clean in CI): nested_owned (nested owned structs, Box in struct, growable `[T]` of structs), drop_paths (conditional/partial/loop move, explicit drop), early_exits (`return`/`break`/`continue`/`?` with owned values), box_match_list, rc_weak (clone/drop, Weak upgrade -> None), arc_shared, dyn_shapes, dyn_values (`Box<dyn>` over several types, `[dyn T]`), arena_alloc (`Box::newIn`). Run by `tests/c7_final.rs` under valgrind (when installed) and by `scripts/valgrind_memory_examples.sh`.
  - **valgrind CI step: ADDED to `.github/workflows/ci.yml`** (see the manager verification record above) and green in CI. `CI_SNIPPET_VALGRIND.md` in the repo is now superseded.
  - **Arena benchmark** (`c7_final::arena_allocator_beats_the_default_allocator...`): release build, 1,000,000 allocations, median of 15 runs, measured by the Rust harness; assistant's sandbox: default allocator 9.2 ms vs arena 4.4 ms; manager's machine: 10.8 ms vs 5.0 ms (the test prints both and asserts arena < default).
  - IR tests: drop glue once per type, free once per heap field (c3), mono de-dup (struct and function), self tail call is a loop (no self call in the IR), vtable layout, `Box::newIn` through the `Allocator` trait impl, plain `Box` still uses `__mtn_dealloc`. Exit-code tests: bounds panic 101 (growable and fixed), explicit panic 101, allocator out-of-memory 101, `?` with `From` with owned values. Rejection tests: use after move, `@copy` with a heap field, recursive struct without `Box`, unsafe-only operations outside `unsafe`, use after `drop(x)`, assignment to an immutable binding, assignment through `Rc`, wrong `Allocator` impls, bad `@pack`/`@align`.
  - **"not yet supported — Phase 11" audit: none left.** Messages that were really internal invariants ("generic reached codegen") are now `internal error:` text; user-facing ones were rewritten (`null` without a pointer type, invalid assignment target, named arguments to a non-function, unknown `Rc::`/`Arc::` function). A genuine limitation that was hidden behind "Phase 11" got FIXED: an `i128` inside an enum payload now works (payload alignment follows the strictest variant). Remaining "not yet supported" messages name phases from Doc 25 §2.5 (12, 13, 14, 16, 17, 18, 19).
  - **Limitations the spec does not schedule (flagged for the manager to place):** array patterns (`[a, b]`, `[first, ..rest]`; clear error, proposed with Phase 16 slices; note Doc 9's pattern grammar does not list them but Doc 23 does); `break`/`continue` leaving a `try` block (the `try` body is a separate function); an `impl` whose type parameter is nested inside an argument of a generic type; `.len()` on growable arrays (Phase 16, so examples pass counts explicitly); `dyn` behind `Rc`/`Arc` (Doc 13 §1.4 gap); `dyn Trait<T>` (spec shows none).
  - **Full list of 11c interpretations / flags** (each also in WORKLOG_11C.md): zero-slot ownership instead of drop flags; by-reference `match` binding mode (Doc 9 silent; rests on Doc 9 §2.4 + Doc 6); `Weak<T>` / `Weak<T, Arc>` (Doc 6 §6 writes one `Weak<T>`; Phase 16 may unify); invented names `upgrade`/`strongCount`/`weakCount` (+ camelCase API names); no user `Drop` trait (not specified); `dyn T` in value position = owned `Box<dyn T>` with implicit boxing of concrete values (manager decision); `Box::newIn` takes a shared `borrow` allocator although `alloc` is `borrow mut self` (the spec example), a box does not track the arena's lifetime; arena example uses `alloc`/`dealloc` intrinsics (Doc 13's `buffer: [u8]` needs Phase 16); impl-vs-trait parameter-count check added; field order is not reordered under default layout (Doc 13 §4.1 allows it); Doc 13 §3.1 size-class allocator = Phase 15 (the single `__mtn_alloc`/`__mtn_dealloc` interface is in place).
  - **Carried gaps:** temporaries live to the end of their statement; no move out of an array by indexing; `for` over growable `[T]` = Phase 16; `Rc` not rejected as non-`Send` (Phase 12/13); interior mutation through `Rc`/`Arc` arrives with `Atomic`/`Mutex` (Phase 13).
- **Open items from C1 — status:** (a) `ci.yml`: resolved (valgrind step added by the owner); (b) by-value parameter `mut` (Doc 3 says `mut` may mark a parameter; Doc 23 has no `mut x: T` form): still open, see the Open items register; (c) generic methods in traits: done in C1b.

## Phase 12 — 🟢 COMPLETE — confirmed by the real CI run of 2026-11-16 (937 tests) and approved by the manager. Work notes: `WORKLOG_12.md`

Phase 12 is ONE phase delivered through checkpoint zips (M0 … M6) and one `final-12.zip`. **Manager verification (2026-11-16):** every checkpoint M0–M5 and the final tree were checked against a real CI log and an independent build + hand probes; the final CI log shows 937 passed / 0 failed / 0 warnings, both valgrind steps green (9 + 15 programs), smoke tests green, banner updated, and the 10,000-simultaneously-suspended-tasks load test passing with `peak_live=10000` on the GitHub runner. The Phase 12 exit criteria of Doc 25 are met: the unguarded-`mut` data race is rejected at compile time; 10,000+ concurrent tasks run on 4 workers; every construct of §2.5 row 12 compiles and runs with positive tests; no "Phase 12" codegen error is left.

### M0 — start-up and housekeeping (checkpoint-M0)
**Fresh sandbox numbers: 660 passed, 0 failed, 0 warnings, 30 test binaries** (start tree 603/29 reproduced first; +57 new tests in the new binary `tests/phase12_m0.rs`, plus one old e2e test rewritten, see 5). `mtnc check examples` -> `checked 51 file(s), 0 errors`; hello -> `Hello, Mountain!`; `scripts/valgrind_memory_examples.sh` clean on the 9 programs. Full suite run 3 times in a row: 660/0 each time.
Per-binary counts: lib 60, assoc_types 8, borrow_checks 19, c1_carryover 40, c2_handles 16, c3_drops 31, c4_rc 20, c5_dyn 27, c6_alloc 29, c7_final 28, closures 10, const_generics 19, control_flow 10, e2e 64, error_handling 57, exhaustiveness 17, generators 5, generic_traits 11, generics 12, infer 7, integration 6, mono 19, parser_doc24 6, phase11a 15, **phase12_m0 57 (new)**, raw_pointers 26, traits_impls 15, types_doc5 26 (main 0, doc 0).
1. **Environment recipe:** `apt install llvm-18-dev libpolly-18-dev zlib1g-dev libzstd-dev valgrind`; `export LLVM_SYS_181_PREFIX=/usr/lib/llvm-18; cd mtnc; cargo test` (Rust 1.97 in the sandbox).
2. **Move while borrowed (Doc 6 §9.8; Open item #1 closed).** `borrow.rs`: every registered borrow now also records the block instance (serial + nesting depth) it was declared in; a stack of "current statement index per open block" lets a move inside a nested block (`if`, loop body, closure body) be compared with the borrow's last-use index in the declaring block. A move (`let y = x;`, by-value argument, `move` closure capture) is an error when a borrow of the place has its last use **at or after** the moving statement (same-statement use counts: `two(r, x)`, `two(borrow x, x)`). `return x;` is checked with "strictly after" (the function ends, so `if r.is_empty() { return x; }` stays valid), and does not mark `x` moved. Borrows of blocks that already ended are ignored. Message: "cannot move `x` while it is borrowed (via `r`) -- the borrow is still used later (Document 6 §9.8)". Tests: 15 (let, by-value arg, same call, temporary in same call, return, loop, nested block, `borrow mut`, `move` closure; accepted: borrow ended before the move, never-read borrow, earlier temporary, return after last use, inner-block borrow ended, other place). **Flow-insensitive by design** (as the spec text "its last use is later" says): `let r = borrow x; if c { return x; } use(r);` is rejected although only one path conflicts. No earlier test changed.
3. **By-value `mut` parameters (Doc 23 §17.11; Open item #2 closed).** `ast::Param.is_mut`; parser accepts `mut x: T` and rejects `mut borrow`/`mut move` ("cannot be combined"); the checker makes the parameter assignable (like `borrow mut`), caller unaffected; codegen needed no change (by-value parameters already live in a local slot, owned values use the zero-slot mechanism). `mut` is not part of an impl-vs-trait signature. 9 tests (copy type, loop, owned struct, owned `String` reassigned, `borrow mut self` method call, trait impl may add `mut`, assignment without `mut` rejected, combination rejected, `borrow mut` + `mut` together).
4. **impl-vs-trait signature check (Doc 7 §9.6; Open item #3 closed).** Parameter and return types of each impl method are compared with the trait's after substituting `Self` and the trait's generic arguments (`Ref` vs value is part of the type). Types that are still open (type parameters of a generic impl, unresolved `Self::Assoc`) are skipped rather than guessed. `Allocator` keeps its stricter existing check. 8 tests incl. generic traits.
5. **`break`/`continue` out of `try` (Doc 12 §11.6).** `desugar.rs`: jumps that leave the `try` block (incl. labelled ones and from nested `try`s) become `{ __try_ctl_N = k; return Ok(None); }`; the wrapper then returns `Result<Option<V>, E>` and receives `borrow mut __try_ctl_N: i32`; after the call a `match` arm `Ok(None)` dispatches (`if __try_ctl_N == k { break 'l; }` ...). A `try` without such jumps lowers exactly as before (IR-identity test with hand-written wrappers still passes; a new test asserts no `__try_ctl` appears). Language restriction decided here and flagged: **`break <value>` cannot leave a `try`** (clear error "cannot leave a `try`"). The old test `break_leaving_a_try_block_is_still_a_clear_error` (asserted the gap) was rewritten to `break_leaving_a_try_block_works_since_phase_12` — the only earlier test whose meaning changed. 7 new tests (while, for, labelled break/continue to outer loop, value-yielding try, inner loop jumps stay inside, nested try, no-jump IR).
6. **Lexer (Doc 2 §10).** Identifiers ASCII-only (non-ASCII letter -> error naming the character and its position, token becomes an error token); BOM ignored; all string/char escapes validated (`\n \r \t \\ \" \' \0`, `\xHH` <= 7F, `\u{1..6 hex}` scalar value) with the error at the backslash ("unknown or malformed escape sequence `\q`"); strings stay single-line, no line continuation; char literals must hold exactly one scalar value (`''`, `'ab'`, `'\n\n'` -> "character literal must contain exactly one character"); `'a'` vs lifetime `'a` unchanged; raw strings unchanged. Invalid UTF-8 files already failed with a UTF-8 message (test added). End-to-end test: `print("नमस्ते दुनिया 你好")` produces exactly those UTF-8 bytes; non-ASCII in comments, chars and `\u{..}` escapes works. 17 tests.
**Flagged / interpretations (M0):** (a) move-while-borrowed is flow-insensitive as the spec words it; `return x` uses "strictly later" (see 2); (b) `break <value>` out of `try` is a compile error (5); (c) the impl-vs-trait comparison skips open types (4). **Known gaps after M0:** none new. **Files changed:** `src/borrow.rs`, `src/ast.rs`, `src/parser.rs`, `src/types.rs`, `src/desugar.rs`, `src/lexer.rs`, `src/desugar.rs`; tests `tests/phase12_m0.rs` (new), `tests/e2e.rs` (one test rewritten).

### M1 — closures and function values (checkpoint-M1)
**Fresh sandbox numbers: 713 passed, 0 failed, 0 warnings, 31 test binaries** (660 + 53 new in `tests/phase12_m1.rs`; one old test in `tests/error_handling.rs` rewritten, one old case removed from `tests/e2e.rs`, see 6). Full suite run 3 times in a row: 713/0 each time. `mtnc check examples` -> `checked 54 file(s), 0 errors`. `examples/concurrency/` (new, 3 closure programs) runs valgrind-clean with `scripts/valgrind_concurrency_examples.sh` (new; the CI step is extended at M6).
Per-binary counts: lib 60, assoc_types 8, borrow_checks 19, c1_carryover 40, c2_handles 16, c3_drops 31, c4_rc 20, c5_dyn 27, c6_alloc 29, c7_final 28, closures 10, const_generics 19, control_flow 10, e2e 64, error_handling 57, exhaustiveness 17, generators 5, generic_traits 11, generics 12, infer 7, integration 6, mono 19, parser_doc24 6, phase11a 15, phase12_m0 57, **phase12_m1 53 (new)**, raw_pointers 26, traits_impls 15, types_doc5 26 (main 0, doc 0).
1. **Representation (Doc 10 §9.5).** A function value is `{ code, env }`; `code(env, args..)`. A named function used as a value goes through a generated thunk with a null `env`. A closure with no captures also has a null `env`. Environment layout `{ drop_fn, called, captures.. }`. An *owning* closure (`move`, or a capture its body moves out) has a heap environment with a drop function and is **move-only**; dropping it drops each moved-in capture once, then frees the block (zero-slot mechanism, valgrind-checked). A closure that only borrows has its environment on the creating function's frame (`drop_fn` null) and is freely copyable. Reading a function value by value zeroes the source slot only when it owns a heap environment.
2. **Captures** (`borrow.rs`, the Phase 7 analysis reused; no second analysis): `Ref`, `MutRef` (writes the outer variable itself), `Move`. A non-`move` closure whose body moves a non-`Copy` capture out (call argument, `let`, `return`, block tail) captures it by `Move` (Doc 10 §9.5 "non-`Copy` captures that must be owned"). The analysis is exported per closure literal (`ClosureInfo`) and recomputed on the final (mono'd, desugared) program for code generation.
3. **`once` closures.** A closure whose body moves a capture out can run once: a second call panics ("a closure that moves a captured value out can be called only once", exit 101) instead of using a zeroed capture.
4. **Typing** (`types.rs`): closure parameters/return from the expected `fn` type or annotations (message "cannot infer the type of a closure parameter"); arity mismatch against the expected type; body type vs return type; `return`, `?`, `throw`/`try` belong to the closure (a `?` needs the closure's return type to be annotated or expected: message names that); function items are values of `fn(..) -> ..`; calling a non-function is an error ("cannot call a value of type `i32`"); calling works on locals, fields `(h.f)(x)`, array elements `ops[i](x)`, call results `mk()(1)` and parenthesized closures. A local of function type shadows a function of the same name.
5. **New rejections** (`borrow.rs`): (a) assigning a variable while a borrow of it (a `borrow` binding or a closure that captured it) is still used later: "cannot assign to `x` while it is borrowed (via `f`)" — this rule did not exist for plain borrows either (Doc 6 §3.1); (b) a closure that borrows locals cannot be returned, or stored in a struct/array/tuple/enum payload, or passed to a call whose result is a function value: "this closure borrows `n` ... write `move`"; (c) use after move into a `move` closure, and use of a moved (owning) closure binding.
6. **Other fixes needed for closures.** `try`/`catch` inside closure bodies (the desugar pass now walks closure bodies; locals of function type called inside a `try` are captured); the parser accepts `fn(A)` without `-> R` (returns `()`; Doc 10 §1 says an omitted return type is `()`); the borrow checker no longer clones AST nodes (closure info is keyed by node address); `ty_to_type` handles function types. Old tests changed: `error_handling::question_mark_inside_closure_is_not_validated_against_enclosing_fn` (asserted the pre-Phase-12 gap) became `question_mark_inside_closure_belongs_to_the_closure`; the "closures are unsupported" case was removed from `e2e::unsupported_features_give_clear_errors`.
**Flagged / interpretations (M1, need the manager's approval):** (a) by-value use of a closure/function *parameter* is a move (borrow checker treats `fn` types as non-`Copy` by type), so `g(f); g(f);` with `f` a parameter is rejected; a local bound to a borrowing closure or a named function is `Copy`. (b) a `move` closure is move-only even if it captures only `Copy` values (only a closure with **no** captures is `Copy`). (c) a borrowing closure inside a struct/array literal is rejected (write `move`), a conservative rule that never accepts a dangling environment. (d) A named function with `borrow`/variadic parameters cannot be used as a value yet: "not yet supported by codegen — Phase 12" (phase number to be confirmed). (e) **Doc 10 §5.2 `nums.map/filter/reduce/any/sortBy` do not exist**: they belong to the standard library (Doc 16, Phase 16), so the tests use user-written higher-order functions instead. (f) A closure that moves a capture out in a form the analysis does not see (e.g. a struct literal field) and is not `move` is refused at code generation with "cannot move `c` out of a closure that only borrows it". **Known gaps:** known-gap #12 test added; a `by-ref` closure passed through a user function that stores it in a longer-lived place is not tracked (only direct return/store forms are rejected).
**Files changed:** `src/borrow.rs`, `src/types.rs`, `src/desugar.rs`, `src/parser.rs`, `src/driver.rs`, `src/codegen.rs`, `src/codegen/{closure.rs (new),drops.rs,expr.rs}`; tests `tests/phase12_m1.rs` (new), `tests/error_handling.rs`, `tests/e2e.rs`; `examples/concurrency/` (new); `scripts/valgrind_concurrency_examples.sh` (new). `mtnc --version` banner unchanged until M6.

### M2 — generators (checkpoint-M2; includes the M1 follow-up)
**Fresh sandbox numbers (run 3 times, identical): 754 passed, 0 failed, 0 warnings (debug and release builds), 34 test-result lines** (713 + 8 M1 follow-up tests + 33 new in `tests/phase12_m2.rs`; the interpreter prototype's `tests/generators.rs` (5) is unchanged and green). Per-binary counts: lib 60, main 0, assoc_types 8, borrow_checks 19, c1_carryover 40, c2_handles 16, c3_drops 31, c4_rc 20, c5_dyn 27, c6_alloc 29, c7_final 28, closures 10, const_generics 19, control_flow 10, e2e 64, error_handling 57, exhaustiveness 17, generators 5, generic_traits 11, generics 12, infer 7, integration 6, mono 19, parser_doc24 6, phase11a 15, phase12_m0 57, phase12_m1 61, phase12_m2 33, raw_pointers 26, traits_impls 15, types_doc5 26, doc-tests 0.
**M1 follow-up (done first):** calling through `borrow f` auto-derefs; by-value reuse of a function parameter stays rejected with the hint "pass `borrow f` to keep using the function value"; moving an owning closure out of a borrow is rejected; variadic/default-parameter functions cannot be function values (permanent, message says to wrap the call in a closure); `borrow`/`borrow mut`/`borrow dyn`/immutable `borrow [T]` parameters work through function values; struct-literal-field move form is analysed (and moves out of struct/tuple/array literals are now tracked in general). Open items 13–17 above.
**Generators (Doc 9 §4, §8.5; Doc 13 §8.1).** `fn g(..) -> Generator<T>` whose body contains `yield` is compiled to an LLVM *coroutine* (switched-resume ABI; `src/codegen/generator.rs`): calling `g` runs only the ramp (frame allocated through `__mtn_alloc`, parameters copied into it, lazy start); the frame is the heap object of Doc 13 §8.1, never moves, and every parameter/local is an ordinary owning slot in it, so references to locals stay valid across `yield`. `Generator<T>` is one owning pointer, move-only (borrow-checked), storable in struct fields/returnable/overwritable; its drop glue calls `coro.destroy`: the destroy edge of every suspension point drops exactly the locals live there (same drop frames `return` uses) and frees the frame; a finished body has already dropped everything. `g.next()` -> `Option<T>` (`None` forever after the body ended or after `return;`); `for x in g(..)` (identifier or call, patterns incl. tuples/`_`, labels, `break`/`continue`/early `return`, unfinished generator dropped). General bodies (loops, `if`, `match` with bindings across `yield`, owned `String`/`Box`/`Rc`/struct locals, nested `for x in other() { yield x; }`). Type checker: `yield` outside a generator / in a closure / in a `try` block, wrong item type, `return value;`, a body ending in a value, `borrow`/`borrow mut`/variadic parameters (error names the parameter and says to pass by value). Adapters (`.take(n)`, ...) -> "not yet supported by codegen — Phase 16". The driver runs the coroutine passes (`default<O0>` in debug builds that contain a generator, `default<O2>` in release). Valgrind: `examples/concurrency/generators_drops.mtn`, `generators_fibonacci.mtn` (covered by `scripts/valgrind_concurrency_examples.sh`, 5/5 clean) and 5 valgrind tests in `phase12_m2.rs` (String/Box held while stopping midway, partially consumed `Rc` with exact counts, never-started generator, generator in a struct overwritten/dropped, nested generators, release build). IR tests: one coroutine function (one `coro.begin`, one `__mtn_alloc`, one `__mtn_dealloc` on the `coro.free` path) per generator *instance* type although there are several calls; no raw `malloc`/`free` in a generator.
**Flagged / interpretations (M2, need the manager's approval):** (a) **`?`/`throw` inside a generator body**: DECIDED by the manager (Doc 9 §8.6): permanently rejected, message suggests yielding the error (`match` idiom, tested). (b) **`return value;` in a generator** is rejected ("its items are produced with `yield`; use `return;`"); a body whose last expression has a value type is rejected too. (c) `g.next()` requires `let mut g` (like a `borrow mut self` method); Doc 9 does not say. (d) `for x in g` moves a generator variable into the loop (later use is "use of moved value"). (e) By the same rule `for x in arr` on a local array variable now counts as a move of `arr` (the borrow checker does not know the type there); codegen for arrays in `for` was already unsupported (Phase 16). (f) `yield` inside a `try` block is rejected (the block is lowered to a separate function). (g) Doc 9 §4's own example `fibonacci().take(10)` cannot run until the adapters of Phase 16; the same program with `break` at the 10th value runs (test + example). (h) The frame is allocated with alignment 16.
**Known gaps (M2):** printing an `Option<T>` still needs Phase 16 (`match` on `next()` instead); generic generators as methods with `self` by value are untested.
**Files changed:** `src/types.rs`, `src/borrow.rs`, `src/generator.rs` (`block_has_yield`), `src/driver.rs`, `src/codegen.rs`, `src/codegen/{generator.rs (new),drops.rs,expr.rs}`; tests `tests/phase12_m2.rs` (new); examples `examples/concurrency/generators_{drops,fibonacci}.mtn` (new); `PROGRESS.md`, `WORKLOG_12.md`.

### M3 — runtime, `spawn`, `thread`, `Send`/`Sync` (checkpoint-M3)
**Fresh sandbox numbers (full suite run 3 times in a row with `--no-fail-fast`, identical): 800 passed, 0 failed, 0 warnings, 35 test-result lines** (754 at M2 acceptance + 3 M2 follow-up tests added to `tests/phase12_m2.rs` (33 -> 36) + 43 new in `tests/phase12_m3.rs`). Per-binary counts: lib 60, main 0, assoc_types 8, borrow_checks 19, c1_carryover 40, c2_handles 16, c3_drops 31, c4_rc 20, c5_dyn 27, c6_alloc 29, c7_final 28, closures 10, const_generics 19, control_flow 10, e2e 64, error_handling 57, exhaustiveness 17, generators 5, generic_traits 11, generics 12, infer 7, integration 6, mono 19, parser_doc24 6, phase11a 15, phase12_m0 57, phase12_m1 61, phase12_m2 36, **phase12_m3 43 (new)**, raw_pointers 26, traits_impls 15, types_doc5 26, doc-tests 0. `tests/phase12_m3.rs` was also run 20 times in a row in debug and 20 times with the release-built compiler: 0 failures in all 40 runs. The three new example programs were run 20 times each built in debug and with `--release`, with `MTN_WORKERS` from 1 to 5: identical output every time.
**Valgrind:** `scripts/valgrind_concurrency_examples.sh` (all 8 programs in `examples/concurrency/`) is clean with `--leak-check=full --errors-for-leak-kinds=definite --error-exitcode=99`; the three M3 programs are also clean with `--show-leak-kinds=all` (nothing even "still reachable"). **Helgrind** (`valgrind --tool=helgrind`, informational) reports no errors for the three M3 programs. ThreadSanitizer was not run.
1. **Runtime (`mtnc/runtime/mtn_rt.c`, embedded with `include_str!`).** Compiled at link time with the system `cc -O2 -pthread -Wall -Wextra -Werror` into a private temp directory (pid + counter), only for programs whose IR references the runtime; every other program links exactly as before (no runtime, no `-pthread`). The `main` wrapper calls `__mtn_rt_init()` before `main` and `__mtn_rt_shutdown()` right after it returns (so `main`'s exit code is kept).
2. **Scheduler.** `MTN_WORKERS` threads (default: online CPUs, minimum 1), each with its own deque: the owner pops the back, other workers steal from the front; idle workers sleep on a condition variable (no spinning). Worker and OS-thread stacks are 8 MiB, set explicitly. **All deques are guarded by one mutex** (the manager allowed mutex deques); scalability work belongs to Phase 13+. A worker that calls `join` does not block: it runs other queued tasks until the awaited one is done ("helping join"), so nested `spawn` + `join` works even with `MTN_WORKERS=1` (tested with a 4,000-element divide-and-conquer sum).
3. **Tasks and handles.** `spawn <closure>` evaluates the operand (always a closure expression: the parser wraps a `{ .. }` block) to an ordinary M1 function value `{code, env}`; a thunk generated per result type calls it, stores the result in the task's slot and drops the closure's environment, so the environment is dropped exactly once, on the worker. `JoinHandle<T>` is the runtime task pointer (a heap-generic type with drop glue): `h.join()` consumes the handle (move checked by the borrow checker: a second `join` is "use of moved value"), returns `T` and frees the task; dropping an unjoined handle detaches (the task still runs; the runtime waits for it at exit and frees an untaken result with the type's drop glue).
4. **Forms.** `spawn { .. }` (non-move), `spawn move { .. }`, `spawn || { .. }`, `spawn move || { .. }`; `thread::spawnOS(closure)`; `pinToCore(n)` (`sched_setaffinity`, a no-op on failure, on non-Linux and for out-of-range `n`). A non-`move` task closure that borrows a local is rejected: "this task may outlive `n`: the closure given to `spawn` borrows the local `n`; write `move` (...)". `move` of a `borrow` parameter is rejected by the M1 rule ("cannot move `s` into a closure: it is borrowed here, not owned"; raised by codegen, not by `mtnc check`).
5. **Send/Sync (Doc 12 §11.7), structural.** `Rc`, `Weak` of `Rc`, raw pointers, `dyn Trait` (no `+ Send` form exists yet), function values, and any struct/enum/tuple/array/`Option`/`Result`/`Box` containing them are not `Send`; `Arc<T>` needs `T: Send + Sync`; `borrow T` is `Send` iff `T` is `Sync`; a captured value of borrowed-reference type is rejected ("may outlive the data it borrows"). The task's result type must be `Send` too. Every message names the type and the reason, e.g. "`Rc<i32>` cannot be sent between threads: its reference count is not atomic (use `Arc<T>` to share between threads)" and "`S` cannot be sent between threads: it contains `Rc<i32>`: its reference count is not atomic". Mutation through `Arc` is the existing "cannot assign through an `Rc`/`Arc`" rejection.
6. **Panics.** A panic in any task or OS thread prints the usual message and exits the whole process with code 101 (`__mtn_panic` calls `exit(101)`; the message still says `thread 'main'`, as before M3).
7. **Stats.** `MTN_RT_STATS=1` prints exactly one stderr line at exit: `rt: workers=<n> spawned=<n> completed=<n> peak_live=<n>` (`spawned`/`completed` count `spawn`ed tasks only; `peak_live` = most tasks existing, queued or running, at once). Load test: 10,000 tasks on `MTN_WORKERS=4` -> `workers=4 spawned=10000 completed=10000`, exit 0.
8. **Parser/other.** `spawn` is now `Expr::Spawn(Box<Expr>)` (the old `SpawnBody` is gone); the stale `e2e` case "`spawn` is unsupported" now uses `await`.
**Flagged / interpretations (M3, need the manager's approval):** (a) **No collection of handles is possible yet:** growable `[T]` has no `push`, `.len()` or `for` over arrays until Phase 16, so tests/examples create 10 / 1,000 tasks with generated explicit `let hN = spawn ..` source and with divide-and-conquer recursion. (b) A function-typed *variable* is treated as not `Send` (give the task the closure literal); `spawn f` with the *name* of a zero-argument function is accepted. (c) `Generator<T>` and `JoinHandle<T>` are `Send` iff `T` is (approximation; a generator frame may hold more). (d) `dyn Trait` is never `Send` until a `dyn Trait + Send` form exists (Doc 7 §9.10 is the only mention). (e) `spawn <call returning Future<T>>` is deferred to M4 and its error says so. (f) Doc 12 §11.1-11.3 do not say what happens to a *detached* task's panic or to a task still running when `main` panics; chosen: any panic exits 101 immediately. (g) `Mutex`, `Atomic`, `channel`, `select` are not present at all: they give the ordinary "no associated function / unknown function" errors rather than a "Phase 13" message. (h) `scripts/valgrind_concurrency_examples.sh` is not run by `.github/workflows/ci.yml` (only the memory script is): adding one CI step is proposed but NOT done without approval. (i) The peak of `peak_live` and the order of printed lines depend on scheduling; tests never rely on them.
**Known gaps (M3):** all deques share one lock; `pinToCore` has no test beyond "does not crash/does not change the result"; ThreadSanitizer not run; `spawn` inside a generator body is untested.
**Files changed:** `mtnc/runtime/mtn_rt.c` (new), `src/ast.rs`, `src/parser.rs`, `src/types.rs`, `src/borrow.rs`, `src/desugar.rs`, `src/driver.rs`, `src/codegen.rs`, `src/codegen/{task.rs (new),drops.rs,expr.rs}`; tests `tests/phase12_m3.rs` (new), `tests/e2e.rs`; examples `examples/concurrency/{spawn_join_basics,arc_sharing,os_threads}.mtn` (new); `PROGRESS.md`, `WORKLOG_12.md`. `mtnc --version` banner unchanged until M6.

### M4 — `async`/`await`, `Future<T>`, `join`, `yieldNow`, `sleepMs` (checkpoint-M4; includes the M3 leftovers)
**Fresh sandbox numbers (full suite run 3 times in a row with `--no-fail-fast`, identical): 857 passed, 0 failed, 0 warnings, 34 test-result lines** (33 test binaries incl. the lib/main unit-test binaries + doc-tests; 800 at M3 + 57 new in `tests/phase12_m4.rs`). Per-binary counts: lib 60, main 0, assoc_types 8, borrow_checks 19, c1_carryover 40, c2_handles 16, c3_drops 31, c4_rc 20, c5_dyn 27, c6_alloc 29, c7_final 28, closures 10, const_generics 19, control_flow 10, e2e 64, error_handling 57, exhaustiveness 17, generators 5, generic_traits 11, generics 12, infer 7, integration 6, mono 19, parser_doc24 6, phase11a 15, phase12_m0 57, phase12_m1 61, phase12_m2 36, phase12_m3 43, **phase12_m4 57 (new)**, raw_pointers 26, traits_impls 15, types_doc5 26 (doc 0). `mtnc check examples` -> `checked 63 file(s), 0 errors`.
**Repetition:** every concurrency test binary (`phase12_m1`, `m2`, `m3`, `m4`) was run **20 times with the debug compiler and 20 times with the release compiler** (`cargo test --release`): 160 binary-runs, 0 failures. **Valgrind:** `scripts/valgrind_concurrency_examples.sh` is clean on all 12 programs in `examples/concurrency/`; extra programs (futures completed, never started, detached at exit, still sleeping at exit, 200 detached futures; `MTN_WORKERS=1` and `4`) are clean with `--show-leak-kinds=all` (4 tests in `phase12_m4.rs`, skipped when valgrind is missing). **Helgrind** (informational, once, `MTN_WORKERS=4`): no reports. ThreadSanitizer was not run.
**Load test (M6 criterion, done now):** 10,000 spawned futures, each `await yieldNow()` 4 times, `MTN_WORKERS=4`: `rt: workers=4 spawned=10000 completed=10000 peak_live=3144` (debug build; the release build printed `peak_live=8383`), exit 0, peak RSS **13.7 MB** (measured with Python `resource.getrusage` because `/usr/bin/time` is not installed in the sandbox). No thread per task. Test `load_10000_spawned_futures_yielding_a_few_times_on_four_workers`.
**M3 leftovers.** (g) `Mutex`, `RwLock`, `Spinlock`, `Atomic`, `channel`, `select`, `volatile` give "`X` is not yet supported by codegen — Phase 13 (Document 12 §11.8)" in every position (token scan; skipped when the program declares an item of that name); one test per name. (h) CI step `Concurrency examples under valgrind (no leaks, no invalid memory access)` added to `.github/workflows/ci.yml` after the memory step (**the owner must upload `ci.yml` by hand**); the stale comment in the script fixed. One test of `spawn` inside a generator body. `.gitignore` has `/mtnc/examples/**/*.bin`; the stray `gitignore` file is gone.
1. **Design.** An `async fn f(..) -> T` is compiled like a generator (LLVM switched-resume coroutine, heap frame that never moves, lazy start) whose promise holds the result; its type is `Future<T>` (one owning pointer, move-only, heap generic). Dropping a never-started future drops only its parameters; the destroy edge of each suspension point drops exactly the locals alive there, once.
2. **`await`.** `await <Future>` is an inline loop in the awaiting frame: result ready? -> resume the sub-future -> ready? -> otherwise suspend the awaiting task (it is requeued by the waker); no worker is ever blocked. `await <JoinHandle>` registers the task as a waiter of the handle and suspends. `h.join()` still works in sync code. `?`, `throw` (inside a `try`, which itself may not contain `await`) and `return value;` work in async fns.
3. **Runtime (`mtn_rt.c`, rewritten scheduler).** One mutex over per-worker deques, one condvar; task kinds sync/future/OS; future states queued/running/notified/idle (a wake while running is remembered and the task is requeued when the poll returns); waiter lists for handles; a lazily started timer thread with a min-heap of deadlines for `sleepMs` (the sleeping task is parked, the worker is free). `yieldNow()` requeues the task behind the other ready ones. `spawn <future>` schedules the future as a task and returns `JoinHandle<T>`. `async fn main` is run to completion by the main thread via `__mtn_rt_block_on` (not counted in `spawned`/`completed`), then the runtime drains detached tasks (same drain-at-exit rule as sync main) and the exit code follows the usual rules.
4. **`join`.** `await join(f1, .., fN)` (2 to 8 futures) is desugared before type checking into spawns of every future followed by a tuple of awaits, so the futures run concurrently on the pool (`let (a, b) = await join(fa(), fb());`); `await join([f(), g()])` over an array literal gives an array. Skipped when the program declares its own `fn join`. Any other use of `join(..)` is the error "`join(..)` must be awaited directly and takes 2 to 8 futures ... or one array literal of futures".
5. **`yieldNow()` / `sleepMs(ms: u64)`** are prelude `async fn`s, injected only when the source mentions them (and the user declares no function of that name).
6. **Send (Doc 12 §11.7).** A future is `Send` iff everything alive across any of its `await`s is `Send`; the analysis follows direct async calls (dependency DFS across async fns). `spawn` of a non-`Send` future is rejected with the type and the reason, e.g. "the future of `async fn f` cannot be spawned: it is not `Send`: `Rc<i32>` cannot be sent between threads ... (alive in `r` across an `await` in `async fn f`; Document 12 §11.7)".
7. **New compile errors (each has a test):** `await` outside an `async fn`; `await` in a closure or a `spawn` block; `await` of a non-future; awaiting a future twice ("use of moved value"); `await` inside `try`; `yield` inside an async fn; `borrow`/`borrow mut`/variadic parameters and a `borrow self` method of an async fn; `async` trait methods; `async` closures and blocks ("not part of Mountain 1.0"); misuse of `join(..)`.
8. **Panic** in an async task or async main exits 101 at once. **Pre-existing leak fixed on the way:** a `main` returning `Err(String)` leaked the error value (valgrind showed 11 bytes lost); the main wrapper now drops the result before computing the exit code (test `valgrind_main_returning_err_does_not_leak_the_error_value`).
9. **Examples (CI-run now):** `examples/concurrency/{async_basics_join,async_fan_out,async_sleep_yield,async_errors}.mtn`.
**Flagged / interpretations (M4, need the manager's approval):** (a) `await f()?` means `(await f())?`. (b) `join` is only accepted directly after `await`, and the array form returns `[T; N]` for an array literal only (register 22). (c) `await` inside a `try` block is rejected (register 23). (d) "Dropped while suspended" is implemented (the destroy edges, valgrind-checked through the generator machinery) but cannot be reached from user code today: a suspended future only lives inside a running task, a detached handle keeps running, and the runtime waits for all tasks at exit; there is no user-level `poll`. (e) The Send analysis is exact for direct `async fn` calls; a future reached through a function value or generic indirection is judged by its result type only. (f) The panic message still names `thread 'main'` (register 25). (g) Sleep tests assert only a lower bound (`>= 150 ms`); the concurrency test for `join` of three 120 ms sleeps asserts only that the total sleep time is respected (no upper bound is asserted anywhere). (h) The "fan-out of 100 futures awaited in order" uses a chain of handles (no `Vec`, as in M3(a)). (i) Found, not fixed: moves in separate branches (register 21).
**Known gaps (M4):** `join` over a growable `[Future<T>]` (needs `Vec`, Phase 16); all deques still share one lock; user-level drop-while-suspended is not testable (flagged (d)); `/usr/bin/time` unavailable in the sandbox (RSS measured with `getrusage`).
**Files changed:** `mtnc/runtime/mtn_rt.c` (rewritten), `src/driver.rs`, `src/parser.rs`, `src/desugar.rs`, `src/types.rs`, `src/borrow.rs`, `src/codegen.rs`, `src/codegen/{expr.rs,closure.rs,drops.rs,task.rs,generator.rs}`; tests `tests/phase12_m4.rs` (new), `tests/e2e.rs` (the stale "await unsupported" case now uses `Mutex`); examples `examples/concurrency/async_*.mtn` (4 new); `scripts/valgrind_concurrency_examples.sh` (comment), `.github/workflows/ci.yml` (new step), `.gitignore`; `PROGRESS.md`, `WORKLOG_12.md`. `mtnc --version` banner unchanged until M6.

### M5 — actors (checkpoint-M5; includes the three M4 fixes F1–F3)
**Fresh sandbox numbers (full suite run 3 times in a row with `--no-fail-fast`, identical): 922 passed, 0 failed, 0 warnings, 35 test-result lines** (34 test binaries incl. the lib/main unit-test binaries + doc-tests; 857 at M4 + 65 new in `tests/phase12_m5.rs`). Per-binary counts: lib 60, main 0, assoc_types 8, borrow_checks 19, c1_carryover 40, c2_handles 16, c3_drops 31, c4_rc 20, c5_dyn 27, c6_alloc 29, c7_final 28, closures 10, const_generics 19, control_flow 10, e2e 64, error_handling 57, exhaustiveness 17, generators 5, generic_traits 11, generics 12, infer 7, integration 6, mono 19, parser_doc24 6, phase11a 15, phase12_m0 57, phase12_m1 61, phase12_m2 36, phase12_m3 43, phase12_m4 57, **phase12_m5 65 (new)**, raw_pointers 26, traits_impls 15, types_doc5 26 (doc 0). `mtnc check examples` -> `checked 66 file(s), 0 errors`. The old suites ran unchanged (no test was edited for F1–F3).
**Repetition:** every concurrency test binary (`phase12_m1`..`m5`, including all actor tests) was run **20 times with the debug compiler and 20 times with the release compiler**: 200 binary-runs, 0 failures. I did this. **Valgrind:** `scripts/valgrind_concurrency_examples.sh` is clean on all 15 programs of `examples/concurrency/` (12 + 3 new); the actor tests with `String`/struct/`Arc` payloads and 200+ messages are valgrind-clean at `MTN_WORKERS` 1 and 4 (in the test suite). **Helgrind** (informational, once per actor example, `MTN_WORKERS=4`): no reports. ThreadSanitizer not run.
**F1 (Document 6 §9.11).** See register item 21 (FIXED). 14 tests (accepted: `if`/`else`, `return`-branch, `match`, `else if` chain, move then `break`, move + re-assign in a loop; rejected: use after a one-branch move, move in both branches then use, one-arm `match` move, one moving `else if` branch, double move, move in `while`, move in a `loop` that may continue, use after a loop that moved before `break`).
**F2 (reference parameters).** An `async fn` and a generator function now reject EVERY parameter of a borrowed type, in both spellings (`borrow x: T` and `x: borrow T` / `x: borrow mut T` / `&T`), with "... has the borrowed type `&S`; ... owns all of its parameters, so pass it by value"; the use-after-free program of the manager is a compile error. 8 tests (async and generators, both spellings, `borrow mut`, `&T`, owned parameters still work).
**F3 (`spawn` of a future).** `spawn` accepts a future only when the operand is a direct `async fn` call (judged exactly as before); a `Future<T>` that is a variable, parameter, field, array element or function-value result is rejected with "pass the async call itself to `spawn`"; a `Future`/`Generator` variable captured by a `spawn`ed closure is rejected the same way. The `Rc`-in-a-future program of the manager is a compile error. 8 tests incl. the positive `spawn add(1, 2)` and an async method call. Note: `await join(a, b)` over future VARIABLES (not calls) is now rejected for the same reason (the desugaring spawns them).
**Actors.** `actor Name { state f: T = init; on h(params) { body } }` is lowered by `desugar::desugar_actors` (before type checking) into ordinary items: a private state struct `__ActorState_Name`, one method per handler (`borrow mut self`), a message ENUM named `Name` with one variant per handler (the fields are the handler parameters, by value), a dispatch function, `Name::spawn()` and — when the handler name is unique in the program and no user item has that name — a bare function `h(..) -> Name`, so `r.send(h(..))` and `r.send(Name::h(..))` both work. If two actors define `h`, the bare name is the error "the message name `h` is defined by several actors (A, B): the bare name is ambiguous; write `A::h(..)` or `B::h(..)`". `ActorRef<A>` is a compiler-known copyable pointer; `r.send(msg)` is typed (`msg` must be a message of that actor) and `Name::spawn()` checks that the state and the message type are `Send` (error names the type and the reason). `r.field` / other methods on an `ActorRef`, `self` outside a method/handler, `await` in a handler, unknown handler, `borrow`/variadic/default message fields, duplicate handler or state, a handler called `spawn`, an actor without handlers, actor generics, actor inheritance and request/reply (`on h() -> T`) all have clear errors and tests.
**Runtime (`mtn_rt.c`).** An actor = a state block + a FIFO mailbox (mutex `g_mu`; each message is a malloc'd node copy of the compiler's message value) + its own embedded scheduler task (kind `K_ACTOR`). `send` appends and, if the actor is idle, makes it runnable; a worker takes messages one at a time and calls the compiler-generated thunk -> dispatch function, which owns the message and runs the handler (an actor never runs two handlers at once; messages of one sender keep their order; after 64 messages the actor requeues itself behind the other ready tasks). A scheduled actor counts in `g_pending`, so `__mtn_rt_shutdown` waits until every mailbox is empty and no handler runs (Doc 12 §11.2/§11.10), then drops each actor's state once and frees it. A panic in a handler exits 101 at once. **Stats line unchanged; every message counts as one `spawned`/`completed` task** (test `the_stats_line_counts_every_message_as_a_task`).
**Examples (CI-run):** `examples/concurrency/{actor_counter,actor_ping_pong,actor_bank}.mtn`; the bank's output order is fixed by the mailboxes (checked 5 times).
**Flagged / needs the manager's decision (M5):** (a) `ActorRef` copyable, actors live until exit (register 26). (b) The message type is an enum named like the actor (register 27) — Doc 12 §8 does not say what type `newOrder(..)` has; an enum with one variant per handler was the smallest design that gives both spellings. (c) Every message counts as a spawned/completed task (the manager allowed it). (d) The Doc 12 §8 example uses `[Order]`/`push`/`retain` (Phase 16); not buildable yet (register 28). (e) `await join(a, b)` over variables is rejected by F3. (f) Cosmetic names in error contexts (register 27).
**Known gaps (M5):** unbounded mailbox; no actor stop; no `ActorRef` to self inside a handler; actors only at top level; one global lock still guards deques and mailboxes.
**Files changed (M5):** `mtnc/runtime/mtn_rt.c`, `src/borrow.rs` (F1 + `ActorRef` copy), `src/types.rs` (F2, F3, `ActorRef`, `send`, `__actor_new`, messages), `src/desugar.rs` (`desugar_actors`), `src/driver.rs`, `src/parser.rs` (clear errors for actor generics/inheritance/request-reply), `src/codegen.rs`, `src/codegen/actor.rs` (new), `src/codegen/expr.rs`, `src/codegen/drops.rs`; tests `tests/phase12_m5.rs` (new); examples `examples/concurrency/actor_*.mtn` (3 new); `PROGRESS.md`, `WORKLOG_12.md`. `mtnc --version` banner unchanged until M6.

### M6 — final integration (`final-12.zip`)
**Fresh sandbox numbers: 937 passed, 0 failed, 0 warnings** (922 at M5 + 15 new in `tests/phase12_m6.rs`; one old test in `tests/c7_final.rs` got a different example program, see 2). `cargo test` prints 36 genuine `test result:` lines (lib, main, 33 integration-test files, doc-tests); two older tests also print the words "test result" as part of their own output, so a log search may show 38 lines — read the `ok. N passed` ones. Per-binary counts: lib 60, main 0, assoc_types 8, borrow_checks 19, c1_carryover 40, c2_handles 16, c3_drops 31, c4_rc 20, c5_dyn 27, c6_alloc 29, c7_final 28, closures 10, const_generics 19, control_flow 10, e2e 64, error_handling 57, exhaustiveness 17, generators 5, generic_traits 11, generics 12, infer 7, integration 6, mono 19, parser_doc24 6, phase11a 15, phase12_m0 57, phase12_m1 61, phase12_m2 36, phase12_m3 43, phase12_m4 57, phase12_m5 65, **phase12_m6 15 (new)**, raw_pointers 26, traits_impls 15, types_doc5 26, doc-tests 0. `mtnc check examples` -> **`checked 67 file(s), 0 errors`** (one new example `examples/generic_fn_value.mtn`). `mtnc --version` -> **`mtnc 1.0.0-dev (Phase 12 — closures, generators, async, concurrency)`**.
**Repetition (I did this):** all seven Phase 12 binaries (`phase12_m0`..`m6`, which include the new load test and the IR tests) were run **20 times with the debug compiler and 20 times with the release compiler**: 280 binary-runs, 0 failures. **Valgrind:** `scripts/valgrind_concurrency_examples.sh` clean on all 15 programs of `examples/concurrency/` at `MTN_WORKERS=1` and again at `MTN_WORKERS=4`. **Helgrind** (informational, `MTN_WORKERS=4`): no reports on spawn_join_basics, async_fan_out, actor_bank, arc_sharing, os_threads. **ThreadSanitizer: ran** (the clang-18 tsan runtime is not installed in the sandbox, but GCC's `libtsan` is: example IR -> `opt-18 -passes='default<O0>'` -> `llc-18` object -> linked with `mtn_rt.c` compiled by `gcc -fsanitize=thread`, `MTN_WORKERS=4`): 0 ThreadSanitizer warnings and the expected output on the same five programs. Limit: only the C runtime is instrumented, not the code generated by LLVM (so it checks the scheduler, mailboxes, timers and handles — the shared-state code — not user code).
1. **(a) "Phase 12" audit.** `ItemKind::Actor` in `codegen.rs` was dead (actors are lowered by `desugar_actors` before codegen); it now reports an *internal error* with no phase. **`f::<T>` as a function value is implemented** (Doc 23 §17.2): `let f = identity::<i32>; f(5)` and `apply(identity::<i32>, 7)` work; the checker types it as the instantiated `fn(..) -> ..` (all type arguments must be written: "needs all N type argument(s) written out"), checks the `where` bounds, records the instance at the expression's own address, and `mono.rs::rewrite_expr` replaces the bare path by the mangled name, so the instance is the SAME one a call `identity::<i32>(2)` uses (test c8). A bare generic name without turbofish stays an error with the existing clear message ("generic function `identity` cannot be used as a value: call it, or wrap it in a closure that fixes its type arguments"). **Audit of every "not yet supported"/"Phase N" message in `src/` (no message names Phase 12; each number matches Doc 25 §2.5):** modules / `use` / `import` → 14 (`codegen.rs`); `table`/`index`/`schema` → 18; `ui`/`component` → 19; `server` → 17; `Mutex`/`RwLock`/`Spinlock`/`Atomic`/`channel`/`select`/`volatile` → 13 (`driver.rs`, Doc 12 §11.8); generic fallback `not yet supported by codegen — Phase {N}` in `codegen.rs` (callers pass 16 for `for` over iterables, 16 for comparing non-scalar values and `match` on string literals, 13 for interior mutation through `Rc`/`Arc` and for writing to a `static`); prelude/library-dependent messages → 16 (`print.rs`, `expr.rs`: `println` of non-`Display` types, unknown standard-library functions and methods). Messages that name NO phase on purpose: raw-pointer indexing/methods/arithmetic ("decided with the library that needs it"), array patterns, `?` on `Option` inside `try`, `return` directly inside `try`, `dyn Trait<..>` in the parser. Nothing is scheduled for Phases 15, 20 or 22 yet because no checked construct needs a message for them today (`wasm32`, `tensor`, `extern`/`abi` are not parsed into constructs that reach codegen). Test `a3_no_phase_12_in_user_facing_errors` scans all of `src/` and fails if a string literal contains "Phase 12" (comments and the version banner are exempt).
2. **Cosmetic.** Actor message `Send` rejection now reads "the message `bad(..)` of actor `A` must be `Send` (messages are handed to another thread): `Rc<i32>` cannot be sent between threads: ..." (test `a4`; two `phase12_m5` expectations updated for the new wording; `c7_final::no_user_visible_message_blames_phase_11` used `f::<i32>` as a rejected example — it is valid now, so the example is a two-parameter function given one type argument).
3. **(b) Load test (Doc 25 Phase 12 exit criterion).** `b1_load_10000_simultaneously_suspended_async_tasks_on_four_workers`: 10,000 `spawn`ed async tasks whose first `await` is `sleepMs(2500)`, then two `yieldNow()` and `sleepMs(5)`; asserts exit 0, exactly one stats line, `workers=4`, `spawned>=10000`, `completed==spawned`, **`peak_live>=10000`**, and only a lower bound on wall time (>= 1 s). Real numbers: `rt: workers=4 spawned=10000 completed=10000 peak_live=10000`, **wall 2.5–2.8 s, peak RSS 13.5 MB** (release build, `getrusage`); i.e. about 1.3 KB per suspended task, no thread per task. The 10,000-synchronous-task test (`phase12_m3`) and the 10,000 future-yield test (`phase12_m4`) are kept. Caveat to know: the 2.5 s sleep is the margin that lets the spawner finish creating all 10,000 tasks before the first one wakes; on a machine so slow that creating 10,000 tasks takes longer than 2.5 s the test would report `peak_live<10000`.
4. **(c) IR tests** (`phase12_m6.rs`, read the IR printed by `mtnc build --emit-ir`): c1 closure env layout `{ ptr, i64, i32 }` allocated as 24 bytes; c2 an owning environment with a `String` has exactly one drop-glue function that drops the string once and frees 40 bytes once; c3 every function is defined once, one `_drop` per closure environment type; c4 generator/async frames: `coro.id`/`coro.size`, one allocation through the internal allocator and one `coro.free` + dealloc, no direct `malloc`/`free` anywhere except inside `__mtn_alloc`/`__mtn_dealloc`, the promise slot has the result type; c5 `Future<String>` promise is a `String`, no duplicate future drop glue; c6 two calls `id::<i32>` -> one async instance, `i32` + `i64` -> two; c7 closures inside a generic function: same types -> one closure + one drop glue, two types -> two; c8 a generic function used as a value and as a call shares one instance. Note: a closure that captures nothing has no environment and therefore no drop glue (tests capture a variable for that reason).
5. **(e)** Banner changed (see above); nothing in the tests or in `ci.yml` read the old banner except the PROGRESS text of earlier phases (kept as history). **(f)** README has a Status section; `examples/concurrency/` holds 15 programs (closures_basic, closures_once, closures_ownership, generators_fibonacci, generators_drops, spawn_join_basics, arc_sharing, os_threads, async_basics_join, async_fan_out, async_sleep_yield, async_errors, actor_counter, actor_ping_pong, actor_bank), all run by the CI valgrind step. **(h)** `.gitignore` is correct (`/mtnc/target`, `Cargo.lock`, `/mtnc/examples/**/*.bin`); stray `*.bin` files produced by the valgrind scripts and `target/` are excluded from the zip; `ci.yml` is **unchanged** (the new tests need no CI step; the existing concurrency valgrind step already covers the examples). A clean unzip + `cargo build` + `cargo test` was run on the zip contents (937/0, see the delivery message).
**(d) Phase 12 positive/rejection matrix — rebuilt by the assistant; the manager compared it with the original prompt §6 on 2026-11-16 and found every line covered (no test missing).** The original PHASE12_PROMPT §6 text is not available to me any more (it only survives as the bullet lists copied into the milestone summaries); the table below is built from those lists plus every milestone requirement. If the manager remembers a §6 line that is missing, it needs one more test. ✅ = covered by the named test (file prefix `phase12_mN::`).
| Requirement | Test |
|---|---|
| Rejected: unguarded `mut` shared across `spawn` | m3::rejects_a_non_move_task_that_mutates_a_shared_local; m3::rejects_using_a_moved_value_after_it_went_into_a_task |
| Rejected: `move` of a `borrow` into a task | m3::rejects_moving_a_borrow_parameter_into_a_task; m3::rejects_capturing_a_borrowed_reference |
| Rejected: `Rc` captured / returned / inside `Arc` | m3::rejects_an_rc_captured_by_a_task; m3::rejects_a_task_that_returns_an_rc; m3::rejects_an_arc_of_an_rc; m3::an_os_thread_rejects_an_rc_too |
| Rejected: struct containing `Rc` / raw pointer | m3::rejects_a_struct_containing_an_rc; m3::rejects_a_struct_containing_a_raw_pointer |
| Rejected: `Box<dyn Trait>` | m3::rejects_a_boxed_trait_object |
| Rejected: mutation through `Arc` | m3::rejects_mutation_through_an_arc |
| Rejected: non-move closure/block/thread borrowing a local | m3::rejects_a_non_move_closure_that_borrows_a_local; ..._block_...; m3::rejects_a_non_move_os_thread_closure_that_borrows_a_local |
| Rejected: joining twice; `spawn` of a non-closure | m3::rejects_joining_a_handle_twice; m3::spawn_needs_a_closure_or_block |
| Positive: `Arc<i32>` / `Arc<String>` read by several tasks | m3::arc_i32_and_arc_string_are_read_by_several_tasks |
| Positive: `spawn move` with owned data; all four spawn forms | m3::spawn_move_with_owned_data; m3::all_four_spawn_forms_are_accepted |
| Positive: `join()` returning values from 1 / 10 / 1,000 tasks | m3::spawn_one_task_and_join_returns_its_value; join_returns_values_from_ten_tasks; join_returns_values_from_a_thousand_tasks |
| Positive: `thread::spawnOS` returning a value; pinning | m3::thread_spawn_os_returns_a_value; thread_spawn_os_with_owned_data_and_pin_to_core |
| Positive: nested `spawn` | m3::nested_spawn_inside_a_task; a_thousand_tasks_by_divide_and_conquer_with_nested_spawn_and_join |
| Panic in a task / unjoined task / OS thread -> exit 101 | m3::a_panic_inside_a_task_ends_the_process_with_101; ..._unjoined_task...; ..._os_thread...; m4::a_panic_in_a_spawned_async_task_exits_101_at_once; m5::a_panic_in_a_handler_exits_101_at_once |
| Load: 10,000 synchronous tasks on 4 workers | m3::load_test_ten_thousand_synchronous_tasks_on_four_workers |
| Load: 10,000 async tasks on 4 workers | m4::load_10000_spawned_futures_yielding_a_few_times_on_four_workers |
| **Load: 10,000 SIMULTANEOUSLY SUSPENDED tasks (exit criterion)** | **m6::b1_load_10000_simultaneously_suspended_async_tasks_on_four_workers** |
| Data race (unguarded `mut` shared across `spawn`) rejected at compile time (Doc 25 exit criterion) | m3::rejects_a_non_move_task_that_mutates_a_shared_local |
| Closures: capture by ref / mut ref / move, returned, in structs, `?`/`return`/`throw`, wrong arity/type rejected, use-after-move, escaping borrow rejected | m1 (61 tests, e.g. doc10_4_3_*, returning_a_closure_that_borrows_a_local_is_rejected, use_after_move_into_a_move_closure_is_rejected) |
| Closure environment drop exactly once (runtime) | m1::valgrind_move_closure_drops_its_captures_exactly_once and the other `valgrind_*` tests; m3::an_owning_environment_is_dropped_once_even_for_a_task_that_returns_unit |
| Closure env layout + drop glue once per env type (IR) | m6::c1, c2, c3 |
| Generators: Doc 9 §4 example, `next()` resume across yields, finished = `None`, early return, drops on early stop, rejections (yield outside, wrong type, `?`, borrow params) | m2 (36 tests) |
| Generator/async frame layout, no leaked `malloc` (IR) | m2::ir_*; m6::c4, c5 |
| async/await: lazy start, join (2–8 and array), sleep lower bound only, yield order, `?`/`throw`, Err main, panic, rejected `await` placements, Send of futures | m4 (57 tests) |
| Mono de-duplicates async and closure instances (IR) | m6::c6, c7, c8 |
| Actors: ordering, 1000 messages, ping-pong, 100 actors, payloads, spawn from handler, stateless actor, rejects (Rc message/state, unknown handler, ambiguous name, generics, request/reply...) | m5 (65 tests) |
| F1 per-branch moves; F2 reference params; F3 `spawn` of a direct async call only | m5::f1_* (14); f2_* (8); f3_* (8) |
| `f::<T>` as function value; bare generic name still rejected | m6::a1, a2, a5 |
| No "Phase 12" text in any user-facing error; banner | m6::a3, e1 |
| Valgrind-clean (concurrency) | CI step + m1/m4/m5 `valgrind_*` tests + this entry (1 and 4 workers) |
**What was built, per milestone (for a different AI):** M0 housekeeping (move-while-borrowed Doc 6 §9.8, `mut` by-value parameters Doc 23 §17.11, impl-vs-trait signature check Doc 7 §9.6, `break`/`continue` out of `try` Doc 12 §11.6, ASCII identifiers and escape validation Doc 2 §10); M1 closures `{code, env}` (Doc 10 §4, §9.5); M2 generators as LLVM switched-resume coroutines (Doc 9 §4, §8.5–8.6); M3 C runtime (`mtn_rt.c`), `spawn`/`JoinHandle`/`thread::spawnOS`/`pinToCore`, `Send`/`Sync` (Doc 12 §11.1–11.3, §11.7); M4 `async`/`await`/`Future<T>`/`join`/`yieldNow`/`sleepMs`, timers, Phase-13 token scan (Doc 12 §11.4, §11.8); M5 actors via `desugar_actors`, F1–F3 (Doc 6 §9.11, Doc 12 §11.5, §11.12); M6 audit, `f::<T>` values, load/IR tests, banner, docs. **Bugs found and fixed at the root during Phase 12:** moves in separate branches (F1); borrowed parameters in `async fn`/generators (F2, a use-after-free program); `spawn` of a future held in a variable hiding an `Rc` across an `await` (F3); a `main` returning `Err(String)` leaked the error (M4); stateless actors had no value for their state (hidden `__pad` field); runtime not linked for actor-only programs; `ActorRef` parameter treated as moved on reuse; ambiguous bare message names cascaded errors. **Decisions (all recorded by the manager in Docs 6 §9.11, 9 §8.6, 12 §11.11–11.12):** message type = enum named like the actor; `ActorRef<A>` copyable and `Send`; every message counts as one task in the stats line; Doc 12 §8 `OrderBook` example waits for Phase 16; `await join(a, b)` over future variables is rejected; unbounded mailbox, no actor stop, no own `ActorRef`, actors top level only (until Phase 14). **Known gaps (register items 21–30):** all deques and mailboxes share one lock; `join` over growable `[Future<T>]` needs Phase 16; user-level drop-while-suspended is not reachable; the panic message says `thread 'main'`; ThreadSanitizer instruments only the runtime.
**Exact CI expectations (job `build-and-test`):** step "Run tests": 36 `test result: ok` lines, grand total **937 passed, 0 failed**, 0 warnings; smoke step `cargo run -- --version` prints `mtnc 1.0.0-dev (Phase 12 — closures, generators, async, concurrency)`; `mtnc check examples` -> `checked 67 file(s), 0 errors` and hello -> `Hello, Mountain!`; "Memory examples under valgrind": 9 programs clean; "Concurrency examples under valgrind": 15 programs, each line `valgrind clean: examples/concurrency/<name>.mtn`. Load-test numbers to look for are printed only locally (the test is silent in CI unless run with `--nocapture`): `rt: workers=4 spawned=10000 completed=10000 peak_live=10000`.
**Flagged for the manager (M6):** (1) §6 matrix: checked by the manager, complete. (2) The load test relies on a 2.5 s first sleep (see 3); it passed in the real CI run. (3) ThreadSanitizer covers the runtime only. (4) Manager decision 2026-11-16: Phase 12 is 🟢. **Files changed (M6):** `src/types.rs` (`f::<T>` value, actor message wording), `src/mono.rs`, `src/codegen.rs`, `src/main.rs` (banner); tests `tests/phase12_m6.rs` (new), `tests/phase12_m5.rs` (2 expectations), `tests/c7_final.rs` (1 example); `examples/generic_fn_value.mtn` (new); `README.md`, `PROGRESS.md`, `WORKLOG_12.md`. `ci.yml` unchanged.

## Phase 13 — 🟡 IN PROGRESS (M0–M5 accepted by the manager on 2026-11-27; M6 next; docs v11). Work notes: `WORKLOG_13.md`

### M0 — language completions (checkpoint-M0)
1. **Baseline:** 937 tests passed, 0 failed, 0 warnings before any change (fresh `cargo test`).
2. **`Drop` trait (Doc 6 §9.9).** `trait Drop { fn drop(borrow mut self); }` is compiler-known (`src/dropcheck.rs`, pre-mono AST pass, plus checker and codegen rules). Types with a `Drop` impl carry a hidden `live` byte (set to 1 on construction, cleared before the user `drop` runs) so an all-zero value is told apart from a moved-out slot. Drop order: locals in reverse, a type's own `drop` first and then its fields; early `return`, `?`, `break`/`continue`, `drop(x)`, reassignment, `Box`/`Rc`/`Arc`, closures, generators and `spawn move` all verified under valgrind. Rejected with clear messages: reserved name `Drop`, non-local target type, `@copy`, trait args, specialised / bounded / `where` impls, wrong receiver, return type, async/generic `drop`, missing `drop`, `x.drop()` and `Drop::drop(..)`, moving a field or pattern binding out of a `Drop` type, use after move.
3. **Pointer methods (Doc 23 §17.13):** `isNull` (safe), `add`, `sub`, `offset`, `offsetFrom`, `read`, `write` (only `*mut`), intrinsics `copy`, `copyNonOverlapping`, `dropInPlace`, and `sizeOf::<T>()` / `alignOf::<T>()`; all unsafe-only except `isNull` and `sizeOf`/`alignOf`. Raw pointer comparison (`==`, `<`, ...) is now supported.
4. **`dyn` extensions:** `Ty::DynTrait(name, trait_args, auto_bits)`. `dyn Trait<A,B>` (vtable keyed by the trait label, method signatures substituted); `Rc/Arc/Weak<dyn T>` as a fat handle `{object, vtable}` with the value offset computed from the vtable alignment (tested with `@align(32)`), clone / downgrade / upgrade / counts, drop through the vtable; `dyn T + Send`, `+ Sync`, `+ Send + Sync` with coercion rules (a type must satisfy every bound it becomes, bounds can be dropped never added) and the Send/Sync effect (`Box<dyn T + Send>` is `Send`, `Arc<dyn T + Send + Sync>` is shareable).
5. **Tests:** `tests/phase13_m0.rs` (22 tests: golden runs of 8 examples at `MTN_WORKERS=1` and `4`, plain and under valgrind; 12 Drop, 6 pointer and 10 dyn rejection checks). Two stale tests were rewritten because they asserted the *old* "not yet supported" errors that this milestone removes: `c5_dyn::generic_trait_object_is_now_supported` and `raw_pointers::pointer_methods_and_indexing_not_yet_supported` (indexing a raw pointer stays rejected). Fresh numbers: **959 passed, 0 failed, 0 warnings** (937 + 22). `phase13_m0` run 20x debug and 20x release: 0 failures.
6. **New examples (valgrind-clean, picked up by the CI script):** `examples/memory/{drop_trait,drop_containers,drop_tasks,pointer_methods,dyn_generic_trait,dyn_send_box,dyn_rc,dyn_arc_send_sync}.mtn`; expected output in `tests/golden13/`.
7. **Open-items register:** new items 33–36 (renumbered to the manager's numbering; items 31–32 are the manager's own and are not in this copy of the file).

### M1 — `Atomic<T>` + `Ordering` and the M0 follow-up (checkpoint-M1)
1. **M0 follow-up (manager's item 33):** `mtnc check` now reports a field move / pattern move out of a `Drop` type (`src/dropmoves.rs`, run after the type checker). The hidden `live` byte is documented here: `sizeOf::<T>()`/`alignOf::<T>()` of a `Drop` type report the real layout including the byte (`struct{a:i64}` is 8, the same struct with `impl Drop` is 16; a `@pack` struct of 5 bytes becomes 6), and `@pack` / `@align(32)` on a `Drop` struct work (example `examples/memory/drop_layout.mtn`, valgrind clean; tests `drop_layout_*`, `check_reports_*`, `check_and_build_agree_*`).
2. **`Atomic<T>` (Doc 14 §1, §2, §9.4):** `Atomic<bool>`, `i8..i64`, `u8..u64`, `isize`, `usize`, `*mut T` (anything else: a clear error naming the allowed types, also in struct fields, parameters, statics). `Atomic::new`, `load`, `store`, `swap`, `fetchAdd`, `fetchSub`, `fetchAnd`, `fetchOr`, `fetchXor` (`bool`: And/Or/Xor only; wrapping), `compareAndSwap(expected, new, o) -> Result<T, T>` (`Ok(previous)` / `Err(actual)`); all `borrow self`; ordering argument optional (SeqCst). Invalid: `load` with Release/AcqRel, `store` with Acquire/AcqRel. Each operation is one LLVM `load atomic` / `store atomic` / `atomicrmw` / `cmpxchg` with exactly the requested ordering and the derived failure ordering (AcqRel->Acquire, Release->Relaxed): no lock, no runtime call (IR tests for every ordering). Works as a `static` (`static C: Atomic<u64> = Atomic::new(0);`, a non-constant global), as a struct field, inside `Arc`, in a `move` closure through an `Arc`, and in an actor's state. `Atomic<T>` is `Sync`, and `Send` when `T` is (pointer atomics: see register 40).
3. **Tests:** `tests/phase13_m1.rs`, 23 tests (the 23rd runs the literal Doc 14 §2 `lockFreeIncrement` loop from 4 OS threads: exactly 80000): eight examples with exact output at `MTN_WORKERS=1` and `4`, plain and under valgrind (8 OS threads x 100,000 `fetchAdd` and 8 tasks x 100,000 on one counter = exactly 800000 then 1600000; the Doc 14 §2 CAS retry loop from 8 threads: max 8999, bounded increment exactly 10000), an IR matrix (load 3 orderings, store 3, six RMW operations x 5 orderings, CAS x 5 with derived failure ordering), and rejections. Rewritten stale test: `phase12_m4::atomic_is_no_longer_gated_by_phase13` (it asserted the old "not yet supported" gate).
4. **Examples (valgrind-clean, run by the CI script):** `examples/concurrency/atomic_{ops,pointer,spin_flag,counter,cas_max,static,arc_loop,in_actor}.mtn`.
5. **Fresh numbers:** 986 passed, 0 failed, 0 warnings, 38 `test result:` lines (corrected from 41 by the manager's count) (959 + 4 M0 follow-up + 23 M1; per binary: phase13_m0 26, phase13_m1 23). `phase13_m0`, `phase13_m1` and `phase12_m1`..`m6` run 20x debug and 20x release: 0 failures in all. The first release repeat of `phase13_m1` HUNG: root cause was my own `atomic_spin_flag` example (a task spinning on a flag can occupy the only worker with `MTN_WORKERS=1` while the producer task waits in the queue); fixed by using OS threads, comment in the example.

### M2 — Atomic<*mut T> ruling, tuple structs, locks (checkpoint-M2)
Spec gaps / choices at the top: (a) the guards' `Drop` is INTERNAL drop glue (no user-visible `Drop` impl; one pointer to the lock, null once moved out); (b) `Spinlock` has no try form (register 42); (c) guards cannot be returned/stored (register 44); (d) struct patterns remain a Phase 16 grammar point (register 37).
1. **`Atomic<*mut T>` ruling (Doc 14 §9.4):** `Send` and `Sync` when `T` is `Send`; plain `*mut T`/`*const T` stay non-`Send`; non-`Send` pointee or a struct that also holds a plain pointer stays non-`Send`. Tests: `atomic_pointer_in_arc_struct_shared_by_tasks_and_a_thread` (example `examples/concurrency/atomic_pointer_shared.mtn`), `atomic_pointer_to_non_send_pointee_is_rejected`, `struct_with_an_atomic_pointer_and_a_plain_pointer_is_not_send`, `plain_raw_pointers_stay_non_send`, `atomic_pointer_to_a_node_that_holds_an_atomic_pointer_is_send`. Register 40 updated. The rejection fixture `tests/golden13/rej/t3.mtn` now uses the `Rc` pointee program.
2. **Tuple structs (Doc 7 §2.4, Doc 10 §2) fixed:** a call `P(1, 2)` of a tuple-struct name is lowered (`desugar_tuple_ctors`, after `desugar_join`) to a struct literal with fields `0`, `1`, ... Verified: constructor, `.0/.1` read and write (`mut` required, `+=`), patterns in `let` and `match`, generic `struct Wrap<T>(T);`, nested, parameters/returns, `impl` blocks, `Drop` on a tuple struct (order checked), `@copy`, `@pack` (`sizeOf` 5). Named arguments to a tuple-struct constructor are a check error. `check` and `build` agree (test `check_and_build_agree`). Example `examples/memory/tuple_structs.mtn` (valgrind clean). Also: a field moved as a method-call argument (`a.take(b.name)`) is now seen by `dropmoves`/the check pass (test `field_moved_as_a_method_call_argument_is_caught_by_check_too`).
3. **Locks (Doc 14 §9.1, §9.5):** `Spinlock::new()` + `acquire()`; `Mutex::new(v)` + `lock()`; `RwLock::new(v)` + `read()` / `write()`; `tryLock()`, `tryRead()`, `tryWrite()` return `Option<guard>`; no poisoning. Guards deref like `Box` (`*g`, `*g = v`, `g.field`, `g.method()` including user methods and `Atomic` methods; a guard binding needs no `mut` to write through it); a read guard gives shared access only: `*g = ..`, `borrow mut` and a `borrow mut self` method call through it are check errors ("cannot assign through a read guard: `RwLock::read` gives shared access only; take `write()`"). The protected value is reachable only through a guard (`*m`, `m + 1`, `m.field` on a `Mutex` are errors). Statics work (`static M: Mutex<i32> = Mutex::new(0);`, `Mutex::new(literal)`).
4. **Auto traits:** `Mutex<T>` / `RwLock<T>` are `Send` + `Sync` when `T` is `Send` (`Mutex<Rc<i32>>` neither); `Spinlock` is `Send` + `Sync`; guards are neither `Send` nor `Sync`, so a guard moved into `spawn` and a future holding a guard across `await` that is spawned are rejected with the reason ("must be released by the thread that took the lock ... a future that holds one across an `await` is therefore not `Send`"). `Arc<Mutex<T>>` / `Arc<RwLock<T>>` work across `spawn`, `thread::spawnOS`, async tasks and actor state (tests).
5. **Layout and drop:** `Mutex<T>` / `RwLock<T>` = `{ [8 x i32] lock words (32 bytes, zero-initialised), T }`, `Spinlock` = `i32`, a guard = one pointer to the lock (null once moved out). Guard drop glue (internal) releases the lock at scope end, early `return`, `?`, `break` and, for temporaries (`*m.lock()`) at the end of the statement; the `Mutex`/`RwLock` drop glue drops the value once (`Mutex<Note>` with a `String` field, valgrind clean). The borrow checker ties a guard to its lock (the lock cannot be moved while a guard is alive, register 43).
6. **Runtime (`runtime/mtn_rt.c`), documented in its header:** `Mutex` = ticket lock (FIFO): a ticket by `fetch_add`, bounded spin (300 iterations with a CPU relax hint), then park on a global 64-bucket parking lot (pthread mutex + condition variable, hashed by lock address, broadcast wake; a `parked` handshake with sequentially consistent ordering so a wake-up cannot be lost). `RwLock` = ticket gate + reader count: readers pass through the gate, so once a writer waits new readers queue behind it (a writer is not starved), and the writer waits until the reader count is zero. `Spinlock` = test-and-test-and-set. The runtime is linked whenever a program uses any lock.
7. **Tests** (`tests/phase13_m2.rs`, 33 tests): `Arc<Mutex<i64>>` incremented by 8 OS threads x 100,000 = exactly 800,000 and by 1,000 tasks x 100 (recursive fan-out, no `Vec` yet) = exactly 100,000, at `MTN_WORKERS` 1 and 4; four examples with exact output, plain and under valgrind at both worker counts; RwLock 4 readers + writer; writer not starved by four reader threads looping on `read()` until the writer's flag (no sleep); guard released at scope end / early `return` / `?` / `break` (in `loop` and `while`); `tryLock`/`tryRead`/`tryWrite` None while held on the same thread, Some after `drop`; the `MTN_WORKERS=1` join rule; lock in a struct field and in actor state; `Mutex<Note{String}>` valgrind clean; rejections: guard moved into spawn, future holding a guard across `await` spawned, `Mutex<Rc<i32>>` across spawn, assignment through a read guard, protected value used without locking, plain `mut` shared across spawn, locking a moved Mutex, guard returned from a function. Stale tests rewritten: `phase12_m4` three `phase13_*_path_call_and_type` gate tests (now `locks_are_no_longer_gated_by_phase13`) and `e2e::unsupported_features_give_clear_codegen_errors_not_crashes` (now uses `select`).
8. **Examples (in the CI valgrind script):** `examples/concurrency/{mutex_counter,rwlock_readers_writer,spinlock_counter,bank_transfer,atomic_pointer_shared}.mtn`; `bank_transfer` locks two `Mutex` accounts in a fixed order (lowest id first); goldens in `tests/golden13/`.
9. **Hand probes (rule 12):** a guard held across a call that uses the same `Arc<Mutex>` clone: `mtnc check` ACCEPTS it (not run: it is a deadlock by register 46). `static M: Mutex<i32> = Mutex::new(0);` works (prints 5). Guards inside `match` arms work (the guard is released at the end of the arm). A guard returned from a function is REJECTED with the clear message of register 44. `g.bump()` through a read guard: found a mismatch (check accepted, build gave a misleading "through an `Rc`/`Arc`" message): fixed, now a check error. A method call on a guard (`g.bump()`, `g.get()`, `g.fetchAdd(4)`) did not build before this round: fixed. A guard captured by a `move` closure runs (register 44). Moving a lock while a guard is alive is rejected ("cannot move `m` while it is borrowed (via `g`)").
10. **Fresh numbers:** **1017 passed, 0 failed, 0 warnings, 39 `test result:` lines** (986 + 31: `phase13_m2` 33 tests, minus 3 old gate tests replaced by 1, and 0 other change; `phase13_m0` 26, `phase13_m1` 23, `phase13_m2` 33, `phase12_m0` 57, `m1` 61, `m2` 36, `m3` 43, `m4` 55, `m5` 65, `m6` 15). Repeats: `phase12_m1`..`m4`, `phase13_m0`, `phase13_m1`, `phase13_m2` each run 20x in debug and 20x in release (`cargo test --no-run`, then the test executable directly): 0 failures in all 280 runs (`phase12_m5`/`m6` were not repeated this round: they are not touched by M2). Valgrind scripts (`bash scripts/valgrind_memory_examples.sh`, `...concurrency...`): 19 memory and 29 concurrency programs clean, exit 0. Helgrind: register 50.

### M3 — `channel` (checkpoint-M3)
Spec contradictions / approvals needed, at the top: **(1) register 51** (compensation instead of helping for blocked synchronous channel waits: pure helping provably deadlocks the mandatory single-worker tests); **(2) register 52** (a probe found that the existing `join` helping can deadlock with channels; proposed fix needs approval); **(3) register 56** (adding the TSAN script to CI).
1. **Creation and types:** `channel::<T>()` (bounded 1024), `channel::<T>(n)` (`n: usize`; 0 panics "channel capacity must be at least 1", exit 101), `channelUnbounded::<T>()`; the message type is required (clear error otherwise). `Sender<T>` / `Receiver<T>` are one pointer to the runtime channel (null once moved), move-tracked, `.clone()`, `Send` + `Sync` when `T: Send` (`Rc<i32>`, `Box<dyn Shape>` payloads rejected across `spawn` / `spawnOS` / actor messages with type and reason; `Box<dyn Shape + Send>` fine). API exactly Doc 12 §11.13: `send -> Result<(), T>`, `recv -> Option<T>`, `trySend -> Result<(), T>`, `tryRecv -> Option<T>`. Inside `async fn`: `await tx.send(v)` / `await rx.recv()` suspend via the waker lists (no worker held); `send`/`recv` without `await` there is a compile error that tells the user to write `await`; the `try` forms are fine.
2. **Runtime (`runtime/mtn_rt.c`, design header + a comment on every memory order):** bounded = Vyukov MPMC ring; unbounded = Michael-Scott queue with hazard pointers; atomic sender/receiver counts; `send` fails when no receiver is left (value handed back as `Err(v)`), `recv` gives `None` only when empty and no sender is left; values are moved into queue slots with a per-channel monomorphised drop function, so a `Drop` payload is dropped exactly once when received, left in the queue, or returned by a failed send (valgrind). Blocking waits: spin briefly, then park; every close / send event wakes all waiters (lost-wakeup-free handshake, documented). Blocked pool workers are compensated (register 51).
3. **Single-worker liveness (mandatory tests), all exact at `MTN_WORKERS` 1, 2 and 4:** (a) producer and consumer task, `channel(1)`, 10,000 values (also with the spawn order swapped); (b) 100 producer tasks + 1 consumer, `channel(1)`, exact sum and count; (c) async producer + synchronous OS-thread consumer; plus 20 async producers + 1 async consumer on one worker.
4. **Other tests (`tests/phase13_m3.rs`, 49):** producers x consumers 1/4/8 x 1/4/8 on bounded and on unbounded channels, with tasks and with OS threads (unique-id checksum, sum of squares and count summed after join); capacity 1 with 8x8; back-pressure by counts (`sent - received <= capacity + 1` at every receive, capacities 1, 2, 7); per-sender order; close by dropping all senders / all receivers (same value back); blocked sender / blocked receiver woken by close; `trySend` on full / `tryRecv` on empty; default capacity 1024; unbounded never full; payloads `String`, struct, `Box<dyn Shape + Send>`, `Arc<i64>`, nested `Sender<Sender<i64>>`; `Drop` payload on every path (valgrind); `Sender` cloned into 8 tasks; actor sends into a channel and an actor holding a channel in its state; 1,000,000 messages (exact count and sum, times printed on stderr, no time assertion); 11 rejections (non-`Send` payload x3, `Box<dyn>` without `Send`, use after send, moved handle x2, `recv`/`send` without `await`, wrong element type, missing type, bad methods/argument counts, capacity 0 and a `usize` zero variable, signed capacity). Rewritten stale test: `phase12_m4::phase13_channel_with_and_without_turbofish` is now `channel_is_no_longer_gated_by_phase13`. The `Keyword::Channel` gate in `driver.rs` is removed (`select` stays until M4, `volatile` until M6).
5. **Examples (valgrind clean, in the CI script):** `examples/concurrency/channel_pipeline.mtn` (producer, 4 workers, collector: 1000, 1000, 333833500, 1000), `channel_close.mtn`, `channel_async_bridge.mtn`; goldens in `tests/golden13/`.
6. **Hand probes (rule 12):** worker pool from tasks (`channel_pipeline`) and from OS threads (test `pipeline_with_a_worker_pool_of_os_threads`); a `Receiver` cloned and moved into a `spawn move` closure in a loop; a `Sender` dropped inside a `match` arm (receiver then sees `None`); channel of channels (works: `Sender<Sender<i64>>`); polling two channels with `tryRecv` in a loop; a channel in actor state (`state pair: (Sender<i64>, Receiver<i64>) = channel::<i64>(8);`, works). Found and fixed: tuple-`let` names invisible to closures (register 54). Found, NOT fixed (approval): the `join` hang (register 52).
7. **Sanitizers:** `bash scripts/tsan_runtime.sh`: ThreadSanitizer 0 warnings in 3 runs, ASan + UBSan exit 0 (locks and channels, caps 1/3/64/unbounded, 1/4/8 threads). Helgrind on the channel examples (`MTN_WORKERS=4`): `channel_pipeline` 4 and `channel_async_bridge` 2 reports, `channel_close` 0: the same class as register 50 (Helgrind does not model C11 atomics: the ring cells and counters are acquire/release atomics); TSAN, which does, is clean.
8. **Fresh numbers (this sandbox):** **1066 passed, 0 failed, 0 warnings, 43 `test result:` lines** (1017 + 49 new, 1 old gate test replaced by 1). Per binary: `phase12_m0` 57, `m1` 61, `m2` 36, `m3` 43, `m4` 55, `m5` 65, `m6` 15; `phase13_m0` 26, `m1` 23, `m2` 33, `m3` 49. `phase12_m1`..`m4` and `phase13_m0`..`m3` each run 20x debug and 20x release (the test executables directly): 320 runs, 0 failures. Valgrind scripts: 19 memory and 32 concurrency programs clean, exit 0. `mtnc check examples`: 94 files, 0 errors (91 + 3 new).
9. **Files to upload by hand (new / changed beyond the zip's tree):** nothing outside the zip; `scripts/tsan_runtime.sh` and `tests/tsan/rt_harness.c` are new files in the zip; `.github/workflows/ci.yml` is unchanged.

### M4 — `select` and the `join` fix (checkpoint-M4)
Spec contradictions / approvals needed, at the top: **none new.** The three M3 requests were approved on 2026-11-24 (registers 51, 52, 56) and are implemented (52 first, 56 last). Doc 23 §9 has a select production that agrees with Doc 12 §11.13, nothing invented (register 60). Open for the manager's eye: register 58 (a closed earlier arm starves later arms, per spec) and register 59 (registrations of an async select are not cleaned up if a task were cancelled; no cancellation exists).
1. **Register 52 first:** `join` runs exactly the awaited task if it is still queued, otherwise waits with compensation; never an unrelated task. `inv2` is a real test with a watchdog (a hang fails the test) at `MTN_WORKERS` 1, 2, 4.
2. **`select`** across all layers: parser (send/`default` arms rejected with clear messages), checker (`check_select`), borrow pass (arms borrow receivers; arm bindings declared by copy-ness), desugar walkers, codegen (`gen_select`: sync call `__mtn_chan_select`; async loop over `__mtn_chan_select_poll` with `gen_suspend`), runtime (`sel_scan` in source order, `SelReg` lists, `ch_wake_sel`). Details: registers 57-61.
3. **Mandatory liveness at `MTN_WORKERS` 1, 2 and 4 (all exact: 1000999000):** (a) select consumer over two channels with two producer tasks; (b) select fed by an OS thread and a task; (c) async `await select` with async producers. Also: no lost wake-up (a message arriving on the SECOND channel only, after the select blocked, 3000 rounds, sync and async; both channels at once), 50 async selecting tasks on one worker.
4. **Stress (no timing assumptions, unique-id checksum of count, sum, sum of squares):** 2 and 3 channels x 1, 4 and 8 producer tasks per channel, 100,000 messages in each of the 6 programs, at `MTN_WORKERS` 1, 2, 4.
5. **Semantics tests:** first-ready in source order, all closed, pending values before `None`, winner not lost or duplicated, losing arms consume nothing, receivers reusable after `select` (also as struct fields, 3 arms, `_` pattern), a `Drop` payload dropped exactly once (valgrind). Rejections: non-`Receiver` arm (and `Sender`), zero arms, wrong binding type, missing `await` in async, `Receiver` used after being moved into a task, different arm types, send arm, `default` arm, non-simple pattern (each checked with both `check` and `build`).
6. **Examples (valgrind clean, run by the existing glob in `scripts/valgrind_concurrency_examples.sh`, goldens in `tests/golden13/`):** `select_two_sources.mtn` (task + OS thread, totals 5050 and 1275) and `select_async.mtn` (two async producers, 300 / 19900 / 104950).
7. **Sanitizers and CI:** TSAN harness extended with `__mtn_chan_select`; script run 20x locally, 20/20 clean; step added to `ci.yml` (register 56).
8. **Fresh numbers (this sandbox):** **1095 passed, 0 failed, 0 warnings, 41 `test result:` lines** (1066 + 29 new in `phase13_m4`; two gate tests replaced one for one). Note: the M3 report said 43 lines; I count 41 now for 38 integration test files + 2 unit-test binaries + doc-tests, and cannot reproduce 43 for M3 (it would have been 40), so treat the M3 figure as miscounted. Per binary: `phase12_m0` 57, `m1` 61, `m2` 36, `m3` 43, `m4` 55, `m5` 65, `m6` 15; `phase13_m0` 26, `m1` 23, `m2` 33, `m3` 49, `m4` 29. ALL of these 12 binaries ran 20x debug and 20x release (the test executables directly): 480 runs, 0 failures. Valgrind scripts: 19 memory and 34 concurrency programs clean, exit 0. `mtnc check examples`: 96 files, 0 errors (94 + 2 new).
9. **Files to upload by hand (beyond the zip's tree):** `.github/workflows/ci.yml` (one new step, `bash scripts/tsan_runtime.sh`, after the valgrind steps). Delete the stale `PROGRESS .md` (with a space) on GitHub if it is still there.

### M5 — lock-free structures written in Mountain (checkpoint-M5)
Spec contradictions / approvals needed, at the top: **one decision for the manager (register 62) — APPROVED 2026-11-27:** Doc 14 §4 sketches the stack head as `Atomic<*mut Node<T>>`, but a pointer plus a tag in one word needs a packed integer; I used `Atomic<usize>` (48-bit address + 16-bit tag), which Doc 14 §5 describes ("packed alongside the pointer") and which needs no new language feature. The queue uses `Atomic<*mut Node<T>>` as specified. No other contradiction: Doc 14 §3.2 (help the lagging tail), §4, §5, §9.6 and the Doc 25 Phase 13 exit criterion are implemented as written. Nothing was invented.
1. **`examples/concurrency/lockfree_stack.mtn`:** Treiber stack, tagged word, recycling free list, `Drop`; every memory ordering has a comment saying why it is enough; the classic ABA sequence is a function (`aba_sequence`) with a negative control (tag off: corruption).
2. **`examples/concurrency/lockfree_queue.mtn`:** Michael-Scott queue on `Atomic<*mut Node<T>>` with the lagging-tail helping step in `enqueue` and `dequeue`; never frees until Drop (so no ABA); every ordering commented.
3. **Reclamation:** stated in both examples and in registers 63, 64.
4. **Tests (`tests/phase13_m5.rs`, 20 run by default + 1 manual):** goldens (examples run 3x, valgrind), ABA positive and negative, tag pack/wrap, 4 stack and 5 queue stress tests (8 threads x 100,000; oversubscribed 16 threads on this 2-core sandbox), `Drop` payloads exactly once (stack and queue, valgrind), Send/Sync accept and reject tests. Register 66.
5. **Sanitizers:** register 67 (valgrind clean, Helgrind 7 + 3 reports explained, whole-program TSAN possible and clean with a positive control, runtime TSAN in CI unchanged and green).
6. **Fresh numbers (this sandbox):** **1115 passed, 0 failed, 1 ignored, 0 warnings, 42 `test result:` lines** (1095 + 20 new; the ignored one is the manual whole-program TSAN test). Per binary: `phase12_m0` 57, `m1` 61, `m2` 36, `m3` 43, `m4` 55, `m5` 65, `m6` 15; `phase13_m0` 26, `m1` 23, `m2` 33, `m3` 49, `m4` 29, `m5` 20. **All 13 concurrency binaries ran 20x debug and 20x release: 520 runs; one failure was found (register 69, a test bug in the ABA negative control), fixed, and `phase13_m5` re-run 20x debug + 20x release: all passed.** Valgrind scripts: 19 memory and 36 concurrency programs clean (34 + the 2 new). `mtnc check examples`: 98 files, 0 errors (96 + 2). `scripts/tsan_runtime.sh`: exit 0, 0 warnings.
7. **Not done on purpose:** nothing added to CI (no new step needed or asked), M6 (volatile, audit, banner) not started. The two examples stay Mountain programs; they move into `std` in Phase 16.
8. **Files to upload by hand:** none outside the zip's tree (`ci.yml` is unchanged in M5). Delete the stale `PROGRESS .md` (with a space) on GitHub if it is still there.

## Phases 14 – 25 — ⚪ Not started
(Phase 13 is in progress, see above.) Next after it is **Phase 14: modules and packages** (Doc 25 §2.3 and §2.5). The full phase table is in Document 25 §2.3.
