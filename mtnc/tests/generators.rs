//! Generator (`yield`) state-machine tests — Phase 8 (Document 25
//! §2.3).
//!
//! Exit criteria: "the generator state machine must correctly resume
//! from every yield point across multiple `.next()` calls, preserving
//! local state correctly between suspensions." See `generator.rs`'s
//! module doc for exactly what this prototype does and doesn't cover
//! (a real, scoped lowering + interpreter, grounded in Document 9
//! §4's own `fibonacci` example — not a general Mountain evaluator,
//! which is Phase 10's job once real codegen exists).

use mtnc::ast::{Item, ItemKind};
use mtnc::generator::{lower_generator, GeneratorState};
use mtnc::lexer;
use mtnc::parser::parse_program;

/// Parses `src` and returns the body `Block` of the top-level `fn`
/// named `name` — real source through the real lexer/parser, not a
/// hand-built AST fixture.
fn fn_body(src: &str, name: &str) -> mtnc::ast::Block {
    let (tokens, lex_errs) = lexer::tokenize(src);
    assert!(lex_errs.is_empty(), "lex errors: {:?}", lex_errs);
    let (program, parse_errs) = parse_program(tokens);
    assert!(parse_errs.is_empty(), "parse errors: {:?}\nsource:\n{}", parse_errs, src);
    for item in &program.items {
        if let ItemKind::Fn(f) = &item.kind {
            if f.name == name {
                return f.body.clone().expect("function has a body");
            }
        }
    }
    panic!("no fn named `{}` found\nsource:\n{}", name, src);
}

const FIBONACCI: &str = r#"
    fn fibonacci() -> Generator<u64> {
        let mut a: u64 = 0;
        let mut b: u64 = 1;
        loop {
            yield a;
            let next = a + b;
            a = b;
            b = next;
        }
    }
"#;

#[test]
fn doc9_s4_fibonacci_lowers_successfully() {
    // The central claim this whole module exists to check: Document 9
    // §4's own generator example -- not a simplified stand-in -- must
    // actually lower, using the real parser's actual output (which,
    // confirmed by reading `parser::parse_stmt` directly rather than
    // assuming, produces the trailing `loop {}` as the block's `tail`
    // expression here, since there's no semicolon after it -- both
    // that shape and a semicolon-terminated one are handled by
    // `lower_generator`).
    let body = fn_body(FIBONACCI, "fibonacci");
    let lowered = lower_generator(&body);
    assert!(lowered.is_ok(), "expected fibonacci to lower successfully, got: {:?}", lowered.err());
}

#[test]
fn doc9_s4_fibonacci_yields_correct_sequence_across_multiple_next_calls() {
    // The real exit-criteria claim: resume correctly from every yield
    // point, preserving state, across MULTIPLE `.next()` calls -- not
    // just "it lowers". Checked against the actual mathematical
    // Fibonacci sequence (0, 1, 1, 2, 3, 5, 8, 13, 21, 34), matching
    // Document 24 §3's own `fibonacci().take(10)` usage.
    let body = fn_body(FIBONACCI, "fibonacci");
    let lowered = lower_generator(&body).expect("should lower");
    let mut gen = GeneratorState::new();
    let expected = [0i64, 1, 1, 2, 3, 5, 8, 13, 21, 34];
    for (i, want) in expected.iter().enumerate() {
        let got = gen.next(&lowered);
        assert_eq!(got, Some(*want), "mismatch at .next() call #{}", i + 1);
    }
    // An infinite generator (Document 9 §4's own: the `loop` never
    // breaks) must never finish on its own.
    assert!(!gen.is_finished());
}

#[test]
fn generator_preserves_local_state_between_suspensions_not_just_yielded_values() {
    // Directly checks the "preserving local state correctly between
    // suspensions" half of the exit criterion, not just the yielded
    // values it happens to produce: after the 3rd `.next()` call
    // (yielding the 3rd Fibonacci number), the generator's own
    // internal `a`/`b`/`next` locals must hold the exact values a
    // correct resumption would leave them at.
    let body = fn_body(FIBONACCI, "fibonacci");
    let lowered = lower_generator(&body).expect("should lower");
    let mut gen = GeneratorState::new();
    gen.next(&lowered); // yields 0; locals: a=0, b=1
    gen.next(&lowered); // yields 1; locals: a=1, b=1, next=1
    gen.next(&lowered); // yields 1; locals: a=1, b=2, next=2
    assert_eq!(gen.local("a"), Some(1));
    assert_eq!(gen.local("b"), Some(2));
    assert_eq!(gen.local("next"), Some(2));
}

#[test]
fn one_shot_generator_without_enclosing_loop_finishes_after_its_yields() {
    // The "no loop" case `generator.rs` also supports: a generator
    // that yields a fixed sequence once and then completes, matching
    // the `Iterator`-style `next() -> Option<T>` "finished" protocol
    // (Document 9 §4 doesn't show this exact shape, but it's the
    // direct, simpler sibling case to the infinite-loop one, and
    // costs nothing extra to support given the lowering already
    // handles "no trailing loop" as a real case, not a gap).
    let src = r#"
        fn twoValues() -> Generator<i32> {
            yield 10;
            yield 20;
        }
    "#;
    let body = fn_body(src, "twoValues");
    let lowered = lower_generator(&body).expect("should lower");
    let mut gen = GeneratorState::new();
    assert_eq!(gen.next(&lowered), Some(10));
    assert!(!gen.is_finished());
    assert_eq!(gen.next(&lowered), Some(20));
    assert_eq!(gen.next(&lowered), None);
    assert!(gen.is_finished());
    // Calling `.next()` again after finishing keeps returning `None`
    // rather than restarting or panicking.
    assert_eq!(gen.next(&lowered), None);
}

#[test]
fn unsupported_generator_shape_reports_a_real_error_not_silent_mislowering() {
    // Document 25 §2.3's own standing instruction ("if you're ever
    // genuinely unsure whether a case is sound, say so explicitly")
    // applies to this prototype's own boundaries too: something
    // outside its documented subset (a function call inside a
    // generator body, here) must fail lowering with a clear error,
    // never silently produce a wrong state machine.
    let src = r#"
        fn callsSomething() -> Generator<i32> {
            let x = computeValue();
            yield x;
        }
    "#;
    let body = fn_body(src, "callsSomething");
    let lowered = lower_generator(&body);
    assert!(lowered.is_err(), "expected an explicit lowering error for an unsupported shape, got: {:?}", lowered);
}
