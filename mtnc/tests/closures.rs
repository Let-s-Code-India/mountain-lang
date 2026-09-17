//! Closure & function ownership-semantics tests — Phase 7 (Document 25
//! §2.3): "parameters, ownership-annotated signatures, closures with
//! capture-mode inference (Document 10 §4.3), and the `move` keyword's
//! interaction with lifetime analysis."
//!
//! Exit criteria: "the closure capture-mode inference test suite must
//! pass, and — critically — a non-`move` closure that captures a
//! borrow which would not outlive the closure's own use must be
//! correctly rejected by the borrow checker [from Phase 6], not
//! accepted by some separate, closure-specific escape hatch."
//!
//! Document 10 §4.3's own three examples (read-only capture, mutable
//! capture, `move` capture) are each mapped to a dedicated test below,
//! plus the escaping-closure exit-criteria case in both directions,
//! plus a handful of tests pinning down real bugs found and fixed
//! while implementing this (see `borrow.rs`'s module doc and
//! PROGRESS.md for the full reasoning behind each).

use mtnc::borrow::BorrowChecker;
use mtnc::lexer;
use mtnc::parser::parse_program;

fn check(src: &str) -> Vec<String> {
    let (tokens, lex_errs) = lexer::tokenize(src);
    assert!(lex_errs.is_empty(), "lex errors: {:?}", lex_errs);
    let (program, parse_errs) = parse_program(tokens);
    assert!(parse_errs.is_empty(), "parse errors: {:?}\nsource:\n{}", parse_errs, src);
    let mut bc = BorrowChecker::new();
    bc.check_program(&program);
    bc.errors.iter().map(|e| e.to_string()).collect()
}

fn assert_ok(src: &str) {
    let errs = check(src);
    assert!(errs.is_empty(), "expected no borrow errors, got: {:#?}\nsource:\n{}", errs, src);
}

fn assert_rejected(src: &str) {
    let errs = check(src);
    assert!(!errs.is_empty(), "expected borrow errors, got none\nsource:\n{}", src);
}

// ============================================================
// Document 10 §4.3's three capture-mode examples.
// ============================================================

#[test]
fn doc10_s4_3_readonly_capture_ok() {
    // "captures `count` by immutable borrow (only reads it)" -- calling
    // the closure later is itself a read of `count` via §5.1-style
    // escaping-reference tracking; both must be fine since `count`
    // never leaves scope.
    assert_ok(
        r#"
        fn main() {
            let count = 0;
            let printCount = || print(count);
            printCount();
        }
        "#,
    );
}

#[test]
fn doc10_s4_3_mutable_capture_via_compound_assign_ok() {
    // "captures `total` by mutable borrow (modifies it)" -- via `+=`,
    // exercising both the capture-mode inference AND the fix to
    // compound-assignment handling (see the `Expr::Assign` arm's doc
    // comment in borrow.rs) that this test also depends on: naively
    // reclassifying `total` from `x`'s shape on every `+=` would not
    // have broken THIS specific test (both are `i32`), but is fixed at
    // the root regardless, not just patched for this shape.
    assert_ok(
        r#"
        fn main() {
            let mut total = 0;
            let mut addToTotal = |x: i32| total += x;
            addToTotal(5);
        }
        "#,
    );
}

#[test]
fn doc10_s4_3_move_closure_moves_capture_reuse_rejected() {
    // "`move` forces ownership transfer into the closure" -- reusing a
    // non-Copy captured value after a `move` closure has captured it
    // must be rejected, the same as any other move (Document 6 §2/§4).
    assert_rejected(
        r#"
        struct Data { value: String }
        fn useData(d: Data) { }
        fn main() {
            let data = Data { value: "x" };
            let c = move || useData(data);
            let d2 = data;
        }
        "#,
    );
}

#[test]
fn doc10_s4_3_move_closure_copy_capture_reuse_still_ok() {
    // A `move` closure capturing a Copy value doesn't block reuse --
    // Copy types are duplicated, not moved, regardless of `move`
    // (Document 6 §4.2's Copy-bypasses-moves rule applies identically
    // to closure captures).
    assert_ok(
        r#"
        fn main() {
            let n = 5;
            let c = move || print(n);
            let m = n;
        }
        "#,
    );
}

// ============================================================
// Phase 7 exit criteria: a non-move closure capturing a borrow that
// would not outlive the closure's own use must be rejected by the
// SAME machinery Phase 6 built (Document 6 §3.1/§3.2 aliasing, and
// §5.1 escaping-reference tracking) -- not a separate mechanism.
// ============================================================

#[test]
fn phase7_escaping_closure_rejected_after_captured_source_out_of_scope() {
    // Direct closure analog of Document 6 §5.1's escaping-reference
    // example: `closure` is bound outside the block, captures `x`
    // (declared inside the block) by reference, and is called after
    // the block -- and therefore after `x` -- has gone out of scope.
    // This is caught by the exact same `borrow_sources`/`is_in_scope`
    // check as a function returning a reference (§5.1), reused
    // unchanged for closures via the same `Binding.borrow_sources`
    // field, not a closure-specific check.
    assert_rejected(
        r#"
        fn main() {
            let closure;
            {
                let x = 5;
                closure = || print(x);
            }
            closure();
        }
        "#,
    );
}

#[test]
fn phase7_escaping_closure_still_in_scope_ok() {
    assert_ok(
        r#"
        fn main() {
            let closure;
            {
                let x = 5;
                closure = || print(x);
                closure();
            }
        }
        "#,
    );
}

#[test]
fn phase7_closure_mutable_capture_conflicts_with_active_shared_borrow() {
    // A non-move closure that ASSIGNS to a captured variable needs a
    // mutable borrow of it (Document 10 §4.3) -- if a shared borrow of
    // that same place is still genuinely live (has a later read) when
    // the closure is created, this is a real Document 6 §3.1 aliasing
    // violation, caught via the exact same `register_borrow` path a
    // named `let ref = borrow x;` uses. This is the clearest possible
    // demonstration of the exit criteria's "not a separate escape
    // hatch" requirement: the SAME conflict-detection code path that
    // rejects `ref3` in `doc6_s3_1_mutable_while_immutable_active_rejected`
    // (borrow_checks.rs) is what rejects this closure.
    assert_rejected(
        r#"
        fn main() {
            let mut x = 5;
            let ref1 = borrow x;
            let c = |y: i32| x = y;
            print(ref1);
        }
        "#,
    );
}

// ============================================================
// Bugs found and fixed while implementing this phase (see borrow.rs's
// module doc for the full reasoning) -- each pinned down by a test so
// a future change can't silently reintroduce them.
// ============================================================

#[test]
fn phase7_closure_param_shadows_outer_moved_binding() {
    // Phase 6 gap, fixed in Phase 7: closure bodies previously weren't
    // given their own scope at all, so a closure parameter sharing an
    // outer (here, already-moved) binding's name could resolve to the
    // WRONG one. `f`'s own `x: i32` parameter must shadow the outer,
    // moved `x` completely -- referencing `x` inside `f`'s body must
    // not trigger a "use of moved value" against the unrelated outer
    // binding.
    assert_ok(
        r#"
        struct Data { value: String }
        fn consume(d: Data) { }
        fn main() {
            let x = Data { value: "a" };
            consume(x);
            let f = |x: i32| x + 1;
            let y = f(5);
        }
        "#,
    );
}

#[test]
fn phase7_nonmove_closure_reading_noncopy_capture_does_not_move_it() {
    // A real false-positive bug found while tracing this phase (fixed
    // at its root in `mark_moved`, not just documented): a non-move
    // closure reading a NON-Copy captured variable via an ordinary
    // by-value function call (the exact same shape as Document 10
    // §4.3's own `|| print(count)`, just with a non-Copy type in place
    // of `count: i32`) must NOT move that variable out of the outer
    // scope -- it's only supposed to be borrowed. Verified here by
    // reading `s` again afterward via an explicit (unambiguous, never
    // a move either way) `borrow s` -- if the bug were present, `s`
    // would already show as moved and this read would be rejected.
    assert_ok(
        r#"
        fn useStr(s: String) { }
        fn main() {
            let s = String::from("hi");
            let c = || useStr(s);
            let ref1 = borrow s;
        }
        "#,
    );
}

#[test]
fn phase7_unbound_closure_argument_does_not_error() {
    // A closure literal passed directly as a call argument (never
    // bound to a name) still gets full capture analysis via the same
    // machinery, just with no persistent binding to register a
    // last-use against -- this is a plain robustness/no-false-positive
    // check, not a specific rule from Document 10.
    assert_ok(
        r#"
        fn callWithFive(f: fn(i32) -> i32) -> i32 {
            return f(5);
        }
        fn main() {
            let result = callWithFive(|x| x + 1);
        }
        "#,
    );
}
