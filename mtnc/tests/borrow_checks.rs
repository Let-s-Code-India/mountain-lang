//! Borrow checker tests, per Document 25 §2.3's Phase 6 exit criteria:
//! "every rejected-code example in Document 6 §8 is correctly
//! rejected; every accepted example is correctly accepted (including
//! the NLL example -- borrows ending at last-use, not block-end)."
//!
//! Document 6 §8 lists exactly five traced verification cases; each
//! gets a dedicated `#[test]` below, named after its doc section, in
//! both directions where the doc itself shows both an accepted and a
//! rejected variant. Additional tests cover the underlying mechanics
//! (move-through-call, aliasing symmetry, struct-field Copy
//! transitivity) that those five cases depend on, so a future
//! regression has a better chance of being caught precisely rather
//! than just failing one of the five headline cases.

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
// Document 6 §8, case 1 (§3.1): the aliasing rule.
// "ref1/ref2/ref3 mutable-borrow-while-immutable-active example ->
// correctly rejected."
//
// Document 6 §3.1 was itself corrected after Phase 6's first review:
// the original example had ref1/ref2 created but never read again
// anywhere, which (under genuine strict NLL, which is what §3.2 and
// Document 25's exit criteria both require) would actually be
// ACCEPTED by real NLL semantics -- an unread borrow has a
// zero-length live range. The doc's example now has `print(ref1)`
// before ref3 and `print(ref2)` after it, making both borrows
// genuinely live at ref3's creation point. This test mirrors the
// corrected example exactly.
// ============================================================

#[test]
fn doc6_s3_1_mutable_while_immutable_active_rejected() {
    // Corrected §3.1 example: ref1 is read right before ref3 (still
    // live), and ref2 is read AFTER ref3 -- `compute_last_use` finds
    // that later read via a full forward scan of the block done once
    // at ref2's declaration, so ref2 is correctly still considered
    // live at ref3's creation point too, even though its only read is
    // textually after ref3.
    assert_rejected(
        r#"
        fn main() {
            let mut counter = 0;
            let ref1 = borrow counter;
            let ref2 = borrow counter;
            print(ref1);
            let ref3 = borrow mut counter;
            print(ref2);
        }
        "#,
    );
}

#[test]
fn doc6_s3_1_unread_borrow_does_not_block_later_mutable_borrow() {
    // The direct converse of the case above, confirming genuine
    // strict-NLL semantics rather than the old conservative
    // "unread-lives-to-block-end" fallback this pass used before
    // Document 6 §3.1 was corrected: a borrow that is truly never read
    // again anywhere must NOT block a later conflicting borrow. Real
    // rustc accepts exactly this shape.
    assert_ok(
        r#"
        fn main() {
            let mut counter = 0;
            let ref1 = borrow counter;
            let ref2 = borrow mut counter;
        }
        "#,
    );
}

#[test]
fn doc6_s3_1_two_shared_borrows_alone_are_fine() {
    // Two shared borrows of the same place at once are explicitly
    // allowed by §3.1 ("multiple immutable borrows allowed") --
    // unaffected by read/unread status either way, since two shared
    // borrows never conflict with each other regardless.
    assert_ok(
        r#"
        fn main() {
            let mut counter = 0;
            let ref1 = borrow counter;
            let ref2 = borrow counter;
        }
        "#,
    );
}

#[test]
fn doc6_s3_1_two_mutable_borrows_rejected() {
    // The other half of "exactly one mutable borrow" -- two active
    // mutable borrows of the same place must also be rejected. `ref1`
    // is read (via `print(ref1)`) AFTER `ref2`'s creation, so its live
    // range genuinely extends across `ref2`'s creation point --
    // exactly the same "later read still counts" shape as the
    // corrected §3.1 example above (needed under strict NLL: without
    // that later read, `ref1` would be truly unused and real NLL would
    // accept two sequential, never-reused mutable borrows too).
    assert_rejected(
        r#"
        fn main() {
            let mut counter = 0;
            let ref1 = borrow mut counter;
            let ref2 = borrow mut counter;
            print(ref1);
        }
        "#,
    );
}

// ============================================================
// Document 6 §8, case 2 (§3.2): non-lexical borrow liveness.
// "non-lexical lifetime example -> correctly accepted, confirming
// borrows end at last-use, not block-end."
// ============================================================

#[test]
fn doc6_s3_2_nll_borrow_ends_at_last_use() {
    // Literal §3.2 example: ref1 IS read (print(ref1)) right before
    // ref2's mutable borrow -- that read is ref1's last use, so ref1
    // is dead by the time ref2 is created, even though both are in the
    // same block (not the block's textual end).
    assert_ok(
        r#"
        fn main() {
            let mut counter = 0;
            let ref1 = borrow counter;
            print(ref1);
            let ref2 = borrow mut counter;
        }
        "#,
    );
}

#[test]
fn doc6_s3_2_nll_still_rejects_if_conflicting_borrow_precedes_last_use() {
    // Sanity check on the other direction: if the conflicting mutable
    // borrow comes BEFORE ref1's read, ref1 is still alive at that
    // point (its last use hasn't happened yet) -- must be rejected.
    assert_rejected(
        r#"
        fn main() {
            let mut counter = 0;
            let ref1 = borrow counter;
            let ref2 = borrow mut counter;
            print(ref1);
        }
        "#,
    );
}

// ============================================================
// Document 6 §8, case 3 (§5.1): escaping references / lifetimes.
// "the longest() example with b going out of scope before result is
// used -> correctly rejected by lifetime checking." Document 6 §5.1
// also states the in-scope use is OK.
// ============================================================

const LONGEST_FN: &str = r#"
    fn longest(x: &str, y: &str) -> &str {
        if x.len() > y.len() { return x; } else { return y; }
    }
"#;

#[test]
fn doc6_s5_1_escaping_reference_used_after_source_scope_ends_rejected() {
    let src = format!(
        r#"
        {longest}
        fn main() {{
            let a = String::from("hello");
            let result;
            {{
                let b = String::from("hi");
                result = longest(borrow a, borrow b);
                print(result);
            }}
            print(result);
        }}
        "#,
        longest = LONGEST_FN
    );
    assert_rejected(&src);
}

#[test]
fn doc6_s5_1_escaping_reference_used_while_source_still_in_scope_ok() {
    let src = format!(
        r#"
        {longest}
        fn main() {{
            let a = String::from("hello");
            let result;
            {{
                let b = String::from("hi");
                result = longest(borrow a, borrow b);
                print(result);
            }}
        }}
        "#,
        longest = LONGEST_FN
    );
    assert_ok(&src);
}

// ============================================================
// Document 6 §8, case 4 (§4.2): Copy-struct field transitivity.
// "a copy-marked struct containing a String field is rejected
// transitively -- no bit-copy of a heap-owning field is ever
// permitted."
//
// Document 3/6 never show a concrete struct-level `copy`-marking
// syntax; this pass grounds it in the existing `@attribute` mechanism
// (Document 23 §14) -- see borrow.rs's module doc. Flagged for
// sign-off; these tests pin down the exact behavior chosen.
// ============================================================

#[test]
fn doc6_s4_2_copy_struct_with_string_field_does_not_bypass_move() {
    // A struct marked @copy but containing a non-Copy `String` field
    // must NOT be treated as Copy -- so assigning it moves it, and
    // reusing the original after that move is a real error.
    assert_rejected(
        r#"
        @copy
        struct BadCopy {
            name: String,
        }
        fn main() {
            let a = BadCopy { name: "x" };
            let b = a;
            let c = a;
        }
        "#,
    );
}

#[test]
fn doc6_s4_2_copy_struct_with_all_copy_fields_is_really_copy() {
    // A struct marked @copy whose fields are ALL themselves Copy
    // (plain i32s here) is genuinely Copy -- reusing the original
    // after "moving" it into another binding must be accepted, since
    // real Mountain semantics duplicate it instead of moving it.
    assert_ok(
        r#"
        @copy
        struct Point {
            x: i32,
            y: i32,
        }
        fn main() {
            let a = Point { x: 1, y: 2 };
            let b = a;
            let c = a;
        }
        "#,
    );
}

#[test]
fn doc6_s4_2_nested_copy_struct_of_copy_struct_is_copy() {
    // Fixed-point propagation: a @copy struct containing another
    // @copy struct (whose own fields are all Copy) should compose to
    // Copy as well.
    assert_ok(
        r#"
        @copy
        struct Point {
            x: i32,
            y: i32,
        }
        @copy
        struct Line {
            a: Point,
            b: Point,
        }
        fn main() {
            let p1 = Point { x: 1, y: 2 };
            let l1 = Line { a: p1, b: p1 };
            let l2 = l1;
            let l3 = l1;
        }
        "#,
    );
}

// ============================================================
// Document 6 §8, case 5 (§6): Rc/Arc are explicit, opt-in, never
// silently substituted. This pass doesn't need to special-case
// Rc/Arc at all for that guarantee to hold -- there's simply no
// mechanism anywhere in this checker that would ever swap a plain
// owned/borrowed binding for an `Rc`/`Arc` type on the developer's
// behalf. Confirmed here by checking that ordinary move-checking
// still applies normally to code that doesn't use Rc/Arc: nothing
// about this pass would "help" a would-be-moved value survive by
// quietly wrapping it.
// ============================================================

#[test]
fn doc6_s6_no_silent_rc_substitution_ordinary_move_still_rejected() {
    assert_rejected(
        r#"
        struct Config {
            name: String,
        }
        fn useConfig(c: Config) { }
        fn main() {
            let cfg = Config { name: "x" };
            useConfig(cfg);
            useConfig(cfg);
        }
        "#,
    );
}

// ============================================================
// Supporting mechanics (Document 6 §2, §4) underlying the five cases
// above -- move semantics and the temporary-borrow-as-call-argument
// shape used throughout Document 6's own examples.
// ============================================================

#[test]
fn doc6_s2_basic_move_then_use_rejected() {
    // Document 6 §2's own canonical example.
    assert_rejected(
        r#"
        struct User { name: String }
        fn main() {
            let a = User { name: "Alice" };
            let b = a;
            print(a);
        }
        "#,
    );
}

#[test]
fn doc6_s2_move_then_use_the_new_binding_is_fine() {
    assert_ok(
        r#"
        struct User { name: String }
        fn main() {
            let a = User { name: "Alice" };
            let b = a;
            print(b);
        }
        "#,
    );
}

#[test]
fn doc6_s4_1_call_argument_move_rejected() {
    // Document 6 §4.1's `consume(alice)` example.
    assert_rejected(
        r#"
        struct User { name: String }
        fn consume(user: User) { }
        fn main() {
            let alice = User { name: "Alice" };
            consume(alice);
            print(alice);
        }
        "#,
    );
}

#[test]
fn doc6_s3_borrow_as_call_argument_does_not_move_and_expires_after_the_call() {
    // Document 6 §3's `printName(borrow alice); print(alice.name);`
    // example: passing `borrow alice` doesn't move `alice`, and the
    // temporary borrow doesn't linger to conflict with later use.
    assert_ok(
        r#"
        struct User { name: String }
        fn printName(user: &User) { }
        fn main() {
            let alice = User { name: "Alice" };
            printName(borrow alice);
            printName(borrow alice);
        }
        "#,
    );
}

#[test]
fn primitive_copy_types_are_never_moved() {
    // i32 (and other primitives) are Copy per Document 5 §2.1 -- using
    // the original after "assigning" it elsewhere must be fine.
    assert_ok(
        r#"
        fn main() {
            let a = 5;
            let b = a;
            let c = a;
        }
        "#,
    );
}

// ============================================================
// Phase 8 (Document 25 §2.3): "generators suspend mid-function...
// check explicitly whether a local borrow that's still live across a
// yield point gets the same soundness scrutiny a closure's captured
// borrow got in Phase 7 -- don't assume this is automatically
// covered." Explicitly verified here, both directions, rather than
// assumed: `yield` was already an ordinary statement to
// `compute_last_use`'s forward scan and `check_expr`'s walk (both
// written in Phase 6, before `yield`/generators were a named concern
// at all) -- so a borrow spanning a yield point was ALREADY correctly
// kept alive across it, and a conflicting borrow after a yield is
// ALREADY correctly rejected, with no new code needed. These two
// tests are that verification made concrete and permanent, not just
// asserted in a report.
// ============================================================

#[test]
fn phase8_borrow_still_needed_after_yield_point_stays_correctly_live() {
    // `r` is read only AFTER the `yield` -- if `yield` were somehow
    // invisible to the last-use scan, this would be indistinguishable
    // from `r` being unread, which (per the strict-NLL rule Document 6
    // §3.1 now requires) would make `r` die immediately and this
    // would accidentally still pass for the WRONG reason. The
    // adversarial test below is what actually rules that out.
    assert_ok(
        r#"
        fn gen() {
            let x = 5;
            let r = borrow x;
            yield 1;
            print(r);
        }
        "#,
    );
}

#[test]
fn phase8_conflicting_borrow_after_yield_still_rejected() {
    // The adversarial case that actually distinguishes "yield is
    // properly transparent to last-use tracking" from "the test above
    // passed for an unrelated reason": `r`'s only read is AFTER both
    // the `yield` and the conflicting `borrow mut` -- so `r` is
    // genuinely still live at the point `r2` is created, exactly
    // mirroring `borrow_checks.rs`'s own
    // `doc6_s3_1_mutable_while_immutable_active_rejected` shape, just
    // with a `yield` statement sitting in between. Must still be
    // rejected; if it weren't, that would mean a yield point was
    // somehow resetting or bypassing the aliasing check.
    assert_rejected(
        r#"
        fn gen() {
            let mut x = 5;
            let r = borrow x;
            yield 1;
            let r2 = borrow mut x;
            print(r);
        }
        "#,
    );
}
