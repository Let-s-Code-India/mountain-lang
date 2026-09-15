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
// ============================================================

#[test]
fn doc6_s3_1_mutable_while_immutable_active_rejected() {
    // Literal §3.1 example: ref1/ref2 are created and never read again
    // anywhere -- per this pass's documented NLL approximation (see
    // borrow.rs module doc), an unused borrow is conservatively still
    // "live", so the following `borrow mut` must be rejected exactly
    // as Document 6 states.
    assert_rejected(
        r#"
        fn main() {
            let mut counter = 0;
            let ref1 = borrow counter;
            let ref2 = borrow counter;
            let ref3 = borrow mut counter;
        }
        "#,
    );
}

#[test]
fn doc6_s3_1_two_shared_borrows_alone_are_fine() {
    // Same shape minus the conflicting mutable borrow -- two shared
    // borrows of the same place at once are explicitly allowed by
    // §3.1 ("multiple immutable borrows allowed").
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
    // mutable borrows of the same place must also be rejected.
    assert_rejected(
        r#"
        fn main() {
            let mut counter = 0;
            let ref1 = borrow mut counter;
            let ref2 = borrow mut counter;
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
