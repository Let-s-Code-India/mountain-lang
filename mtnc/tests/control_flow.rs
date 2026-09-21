//! Loop-form and labeled break/continue tests — Phase 8 (Document 25
//! §2.3). Covers the lexer fix (`'ident` now tokenizes as a lifetime/
//! label rather than erroring as an unterminated char literal) and the
//! new loop type-checking in `types.rs` (previously `Expr::Loop` was
//! entirely unchecked).

use mtnc::lexer;
use mtnc::parser::parse_program;
use mtnc::types::TypeChecker;

fn check(src: &str) -> Vec<String> {
    let (tokens, lex_errs) = lexer::tokenize(src);
    assert!(lex_errs.is_empty(), "lex errors: {:?}\nsource:\n{}", lex_errs, src);
    let (program, parse_errs) = parse_program(tokens);
    assert!(parse_errs.is_empty(), "parse errors: {:?}\nsource:\n{}", parse_errs, src);
    let mut tc = TypeChecker::new();
    tc.check_program(&program);
    tc.errors.iter().map(|e| e.message.clone()).collect()
}

fn assert_ok(src: &str) {
    let errs = check(src);
    assert!(errs.is_empty(), "expected no type errors, got: {:#?}\nsource:\n{}", errs, src);
}

fn assert_err_containing(src: &str, needle: &str) {
    let errs = check(src);
    assert!(errs.iter().any(|e| e.contains(needle)), "expected an error containing {:?}, got: {:#?}\nsource:\n{}", needle, errs, src);
}

#[test]
fn doc9_s3_1_loop_break_value_is_expression_result() {
    // Document 9 §3.1's own example: `loop { .. break i * 2; }` used
    // directly as a `let` initializer.
    assert_ok(
        r#"
        fn main() {
            let mut i: i32 = 0;
            let result: i32 = loop {
                i += 1;
                if i == 10 {
                    break i * 2;
                }
            };
        }
        "#,
    );
}

#[test]
fn loop_with_incompatible_break_value_types_rejected() {
    assert_err_containing(
        r#"
        fn main() {
            let result = loop {
                if true {
                    break 5;
                } else {
                    break "text";
                }
            };
        }
        "#,
        "incompatible types",
    );
}

#[test]
fn while_condition_must_be_bool() {
    assert_err_containing(
        r#"
        fn main() {
            let n: i32 = 5;
            while n {
                break;
            }
        }
        "#,
        "",
    );
}

#[test]
fn while_loop_body_is_actually_checked() {
    // Before Phase 8, `Expr::Loop` (all four forms) was entirely
    // unchecked -- nothing inside any loop body was visited by the
    // type checker at all. This pins down that a real type error
    // inside a `while` body is now caught, not silently skipped.
    assert_err_containing(
        r#"
        fn main() {
            let mut n: i32 = 0;
            while n < 5 {
                n = "not a number";
            }
        }
        "#,
        "",
    );
}

// ============================================================
// Document 9 §3.5: labeled break/continue. Also exercises the
// underlying lexer fix directly -- `'outer`/`'inner` previously
// caused a hard "unterminated char literal" lex error.
// ============================================================

#[test]
fn doc9_s3_5_labeled_break_continue_nested_loops_ok() {
    // Document 9 §3.5's own nested-loop example shape.
    assert_ok(
        r#"
        fn main() {
            'outer: for i in 0..5 {
                for j in 0..5 {
                    if j == 3 {
                        continue 'outer;
                    }
                    if i == 4 {
                        break 'outer;
                    }
                }
            }
        }
        "#,
    );
}

#[test]
fn break_with_label_naming_a_real_enclosing_loop_ok() {
    assert_ok(
        r#"
        fn main() {
            'x: loop {
                loop {
                    break 'x;
                }
            }
        }
        "#,
    );
}

#[test]
fn break_with_undeclared_label_rejected() {
    assert_err_containing(
        r#"
        fn main() {
            loop {
                break 'nonexistent;
            }
        }
        "#,
        "does not name an enclosing loop",
    );
}

#[test]
fn break_outside_any_loop_rejected() {
    assert_err_containing(
        r#"
        fn main() {
            break;
        }
        "#,
        "outside of any loop",
    );
}

#[test]
fn continue_with_label_targeting_outer_loop_ok() {
    assert_ok(
        r#"
        fn main() {
            'outer: loop {
                loop {
                    continue 'outer;
                }
            }
        }
        "#,
    );
}

#[test]
fn lifetime_in_reference_type_parses_and_checks() {
    // Document 6 §5.1's `&'a str` -- the same underlying lexer fix
    // (this project had no lifetime token at all before Phase 8, so
    // this would previously have been a hard lex error, not just a
    // dropped annotation).
    assert_ok(
        r#"
        fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
            return x;
        }
        "#,
    );
}
