//! Phase 9 (Document 11 — Error Handling) tests, per Document 25 §2.3.
//!
//! Exit criterion under test: `try`/`catch` must never be a second,
//! competing mechanism next to `?`/`Result`. Codegen doesn't exist until
//! Phase 10, so "byte-identical compiled output" cannot be checked yet;
//! the strongest pre-codegen check is `PropagationRecord` equality —
//! `try`/`catch` and the equivalent hand-written `?`-chain must resolve
//! to the same list of (kind, source error, target error, converted?)
//! edges (see `phase9_try_catch_matches_hand_written_question_chain`).
//! What remains to confirm once Phase 10 lands is listed in PROGRESS.md.
//!
//! Every test source sticks to syntax shapes already exercised by
//! earlier phases' suites or written verbatim in Document 11 / 24.

use mtnc::lexer;
use mtnc::parser::parse_program;
use mtnc::types::{builtin_variants, PropKind, PropagationRecord, Ty, TypeChecker};

fn check_full(src: &str) -> (Vec<String>, Vec<PropagationRecord>) {
    let (tokens, lex_errs) = lexer::tokenize(src);
    assert!(lex_errs.is_empty(), "lex errors: {:?}", lex_errs);
    let (program, parse_errs) = parse_program(tokens);
    assert!(parse_errs.is_empty(), "parse errors: {:?}\nsource:\n{}", parse_errs, src);
    let mut tc = TypeChecker::new();
    tc.check_program(&program);
    (tc.errors.iter().map(|e| e.to_string()).collect(), tc.propagations.clone())
}

fn check(src: &str) -> Vec<String> {
    check_full(src).0
}

fn assert_ok(src: &str) {
    let errs = check(src);
    assert!(errs.is_empty(), "expected no type errors, got: {:#?}\nsource:\n{}", errs, src);
}

fn assert_rejected(src: &str) {
    let errs = check(src);
    assert!(!errs.is_empty(), "expected type errors, got none\nsource:\n{}", src);
}

fn assert_rejected_with(src: &str, needle: &str) {
    let errs = check(src);
    assert!(
        errs.iter().any(|e| e.contains(needle)),
        "expected an error containing {:?}, got: {:#?}\nsource:\n{}",
        needle, errs, src
    );
}

/// Shared declarations. `ConfigError` has a real `From<IoError>` impl
/// (Document 11 §2's requirement for `?` to convert error types).
const PRELUDE: &str = r#"
enum IoError { NotFound }
enum ConfigError { Io, Bad }
impl From<IoError> for ConfigError {
    fn from(e: IoError) -> ConfigError {
        return ConfigError::Io;
    }
}
fn readFile(path: String) -> Result<String, IoError> {
    return Ok("data");
}
fn parseConfig(text: String) -> Result<i32, ConfigError> {
    return Ok(1);
}
fn applyConfig(c: i32) { }
fn applyDefaultConfig() { }
fn logError(m: String) { }
fn report(e: ConfigError) { }
"#;

fn with_prelude(body: &str) -> String {
    format!("{}\n{}", PRELUDE, body)
}

// ============================================================
// §1 — Result / Option as real, typed constructors
// ============================================================

#[test]
fn phase9_prelude_itself_checks_clean() {
    assert_ok(PRELUDE);
}

#[test]
fn ok_constructor_matches_declared_return_type() {
    assert_ok(&with_prelude("fn f() -> Result<i32, IoError> { return Ok(5); }"));
}

#[test]
fn err_constructor_matches_declared_return_type() {
    assert_ok(&with_prelude("fn f() -> Result<i32, IoError> { return Err(IoError::NotFound); }"));
}

#[test]
fn ok_payload_of_wrong_type_rejected() {
    assert_rejected(&with_prelude("fn f() -> Result<i32, IoError> { return Ok(\"text\"); }"));
}

#[test]
fn err_payload_of_wrong_type_rejected() {
    assert_rejected(&with_prelude("fn f() -> Result<i32, IoError> { return Err(5); }"));
}

#[test]
fn ok_where_option_expected_rejected() {
    assert_rejected_with(
        &with_prelude("fn f() -> Option<i32> { return Ok(1); }"),
        "builds a `Result` value",
    );
}

#[test]
fn some_and_none_match_declared_option_return() {
    assert_ok("fn f() -> Option<i32> { return Some(3); }");
    assert_ok("fn g() -> Option<i32> { return None; }");
}

#[test]
fn unit_ok_satisfies_result_of_unit() {
    // The `()` value must have type `()` (not an empty tuple type) or
    // Document 11 §5's own `return Ok(());` could never type-check.
    assert_ok(&with_prelude("fn f() -> Result<(), IoError> { return Ok(()); }"));
}

#[test]
fn constructor_as_tail_expression_uses_return_type() {
    assert_ok(&with_prelude("fn f() -> Result<i32, IoError> { Ok(1) }"));
}

#[test]
fn bare_ok_without_context_is_ambiguous_and_rejected() {
    // Document 5 inference rule 6: `Ok(5)` leaves the error type free.
    assert_rejected_with(
        &with_prelude("fn f() { let r = Ok(5); }"),
        "cannot infer the full type of `Ok`",
    );
}

#[test]
fn bare_none_without_context_is_ambiguous_and_rejected() {
    assert_rejected_with("fn f() { let n = None; }", "cannot infer the full type of `None`");
}

#[test]
fn annotated_none_and_self_determining_some_accepted() {
    assert_ok("fn f() { let n: Option<i32> = None; }");
    assert_ok("fn g() { let s = Some(5); let t: Option<i32> = s; }");
}

#[test]
fn constructor_arity_checked() {
    assert_rejected_with(
        &with_prelude("fn f() -> Result<i32, IoError> { return Ok(1, 2); }"),
        "expects 1 argument",
    );
}

// ============================================================
// §2 — the `?` propagation operator
// ============================================================

#[test]
fn question_mark_propagates_same_error_type() {
    assert_ok(&with_prelude(
        "fn f() -> Result<i32, IoError> { let t = readFile(\"a\")?; return Ok(1); }",
    ));
}

#[test]
fn question_mark_yields_the_ok_payload_type() {
    assert_ok(&with_prelude(
        "fn f() -> Result<i32, IoError> { let t: String = readFile(\"a\")?; return Ok(1); }",
    ));
    assert_rejected(&with_prelude(
        "fn g() -> Result<i32, IoError> { let n: i32 = readFile(\"a\")?; return Ok(n); }",
    ));
}

#[test]
fn question_mark_converts_error_via_real_from_impl() {
    // Document 11 §2: IoError -> ConfigError is legal because
    // `impl From<IoError> for ConfigError` exists (in PRELUDE).
    let (errs, records) = check_full(&with_prelude(
        "fn f() -> Result<i32, ConfigError> { let t = readFile(\"a\")?; return Ok(1); }",
    ));
    assert!(errs.is_empty(), "got: {:#?}", errs);
    assert_eq!(
        records,
        vec![PropagationRecord {
            kind: PropKind::ResultErr,
            source: Some(Ty::Named("IoError".into())),
            target: Some(Ty::Named("ConfigError".into())),
            converted: true,
        }]
    );
}

#[test]
fn question_mark_without_from_impl_is_a_compile_error() {
    // Document 11 §7 trace: missing `From` impl is a COMPILE error, not
    // a silent runtime failure or implicit coercion.
    assert_rejected_with(
        &with_prelude(
            "enum Other { X }\nfn f() -> Result<i32, Other> { let t = readFile(\"a\")?; return Ok(1); }",
        ),
        "no `impl From<IoError> for Other`",
    );
}

#[test]
fn from_impl_for_a_different_source_type_does_not_count() {
    // `From<IoError> for ConfigError` exists; `From<Other> for
    // ConfigError` does not -- `trait_args` must distinguish them.
    assert_rejected_with(
        &with_prelude(
            "enum Other { X }\nfn src() -> Result<i32, Other> { return Ok(1); }\nfn f() -> Result<i32, ConfigError> { let t = src()?; return Ok(t); }",
        ),
        "no `impl From<Other> for ConfigError`",
    );
}

#[test]
fn question_mark_in_function_returning_unit_rejected() {
    assert_rejected_with(
        &with_prelude("fn f() { let t = readFile(\"a\")?; }"),
        "requires the enclosing function to return `Result",
    );
}

#[test]
fn question_mark_on_non_result_non_option_rejected() {
    assert_rejected_with(
        &with_prelude("fn f() -> Result<i32, IoError> { let x = 5; let y = x?; return Ok(1); }"),
        "can only be applied to `Result",
    );
}

#[test]
fn question_mark_on_option_in_option_function_accepted() {
    assert_ok(
        "fn find() -> Option<i32> { return Some(1); }\nfn g() -> Option<i32> { let v = find()?; return Some(v); }",
    );
}

#[test]
fn question_mark_on_option_records_none_path() {
    let (errs, records) = check_full(
        "fn find() -> Option<i32> { return Some(1); }\nfn g() -> Option<i32> { let v = find()?; return Some(v); }",
    );
    assert!(errs.is_empty(), "got: {:#?}", errs);
    assert_eq!(
        records,
        vec![PropagationRecord { kind: PropKind::OptionNone, source: None, target: None, converted: false }]
    );
}

#[test]
fn question_mark_option_in_result_function_rejected() {
    assert_rejected_with(
        &with_prelude(
            "fn find() -> Option<i32> { return Some(1); }\nfn g() -> Result<i32, IoError> { let v = find()?; return Ok(v); }",
        ),
        "requires the enclosing function to return `Option",
    );
}

#[test]
fn question_mark_result_in_option_function_rejected() {
    assert_rejected_with(
        &with_prelude("fn g() -> Option<i32> { let v = readFile(\"a\")?; return Some(1); }"),
        "requires the enclosing function to return `Result",
    );
}

#[test]
fn question_mark_on_unresolved_call_stays_silent() {
    // Stdlib calls aren't modeled until Phase 16 (Document 24 §1 uses
    // `parseJson(req.body)?`); a false "not a Result" error there would
    // be worse than staying silent, matching every other unresolved
    // construct in this checker.
    assert_ok(&with_prelude("fn f() -> Result<i32, IoError> { let x = mystery()?; return Ok(1); }"));
}

#[test]
fn question_mark_inside_closure_is_not_validated_against_enclosing_fn() {
    assert_ok(&with_prelude("fn f() { let g = || { let t = readFile(\"a\")?; }; }"));
}

// ============================================================
// §3 — try / catch (Document 11 §3, Document 23 §17.4)
// ============================================================

/// Document 11 §3's own example, verbatim shape.
const DOC11_TRY_STATEMENT: &str = r#"
fn load() {
    try {
        let contents = readFile("config.txt")?;
        let parsed = parseConfig(contents)?;
        applyConfig(parsed);
    } catch (e) {
        logError("config load failed");
        applyDefaultConfig();
    }
}
"#;

#[test]
fn doc11_section3_try_catch_example_accepted() {
    // IoError and ConfigError both flow out of the block; the wrapper's
    // error type is ConfigError (the one every source converts into),
    // reproducing Document 11 §3's `Result<(), ConfigError>`.
    assert_ok(&with_prelude(DOC11_TRY_STATEMENT));
}

#[test]
fn catch_variable_has_the_blocks_resolved_error_type() {
    assert_ok(&with_prelude(
        "fn f() { try { let c = readFile(\"a\")?; let p = parseConfig(c)?; } catch (e) { report(e); } }",
    ));
    assert_rejected(&with_prelude(
        "fn g() { try { let c = readFile(\"a\")?; let p = parseConfig(c)?; } catch (e) { let n: i32 = e; } }",
    ));
}

#[test]
fn try_block_with_unrelated_error_types_and_no_from_rejected() {
    assert_rejected_with(
        &with_prelude(
            "enum Other { X }\nfn src() -> Result<i32, Other> { return Ok(1); }\nfn f() { try { let a = readFile(\"a\")?; let b = src()?; } catch (e) { } }",
        ),
        "none of them is a type all the others convert into",
    );
}

#[test]
fn try_block_with_no_error_source_is_ambiguous_and_rejected() {
    assert_rejected_with(
        &with_prelude("fn f() { try { applyDefaultConfig(); } catch (e) { } }"),
        "cannot infer the error type of this `try` block",
    );
}

#[test]
fn doc24_section1_try_as_expression_with_return_in_catch() {
    // Document 24 §1 / Document 23 §17.4: `let body = try { .. } catch (e)
    // { return ..; };`. `parseJson` is unresolved stdlib, so the block's
    // error type is unknown (silent, not an error); the catch block ends
    // in `return`, so it has type `!` and unifies with the try value.
    assert_ok(
        "fn f() -> i32 { let body = try { parseJson(\"x\")? } catch (e) { return 0; }; return 1; }",
    );
}

#[test]
fn try_as_expression_yields_its_value_type() {
    assert_ok(&with_prelude(
        "fn readNum() -> Result<i32, IoError> { return Ok(1); }\nfn g() -> i32 { let v = try { readNum()? } catch (e) { 0 }; return v; }",
    ));
}

#[test]
fn try_and_catch_value_types_must_agree() {
    assert_rejected_with(
        &with_prelude(
            "fn readNum() -> Result<i32, IoError> { return Ok(1); }\nfn g() { let v = try { readNum()? } catch (e) { \"text\" }; }",
        ),
        "`try` block and `catch` block have incompatible types",
    );
}

#[test]
fn throw_inside_try_is_an_error_source() {
    assert_ok(
        "enum ValErr { Bad }\nfn h(x: i32) { try { if x < 0 { throw ValErr::Bad; } } catch (e) { } }",
    );
}

#[test]
fn throw_value_type_becomes_the_catch_variables_type() {
    assert_ok(
        "enum ValErr { Bad }\nfn take(e: ValErr) { }\nfn h(x: i32) { try { if x < 0 { throw ValErr::Bad; } } catch (e) { take(e); } }",
    );
}

#[test]
fn throw_outside_try_rejected() {
    assert_rejected_with(
        &with_prelude("fn f() { throw IoError::NotFound; }"),
        "only valid inside a `try` block",
    );
}

#[test]
fn return_directly_inside_try_is_flagged_as_ambiguous() {
    // Document 11 §3's wrapper-function desugaring would make this
    // `return` exit the wrapper. The spec doesn't say which is meant, so
    // it is rejected rather than silently guessed (PROGRESS.md flagged
    // interpretation 2).
    assert_rejected_with(
        &with_prelude("fn f() -> i32 { try { let c = readFile(\"a\")?; return 1; } catch (e) { } return 2; }"),
        "`return` directly inside a `try` block is ambiguous",
    );
}

#[test]
fn option_question_mark_inside_try_rejected() {
    assert_rejected_with(
        "fn find() -> Option<i32> { return Some(1); }\nfn f() { try { let v = find()?; } catch (e) { } }",
        "inside a `try` block is not supported",
    );
}

// ---- THE PHASE 9 EXIT CRITERION (pre-codegen form) ----

#[test]
fn phase9_try_catch_matches_hand_written_question_chain() {
    // Document 11 §3's desugaring: a `try { .. } catch (e) { .. }` is a
    // nested `fn __try_block() -> Result<(), E> { ..; return Ok(()); }`
    // plus a `match` on it. Here the hand-written form and the `try`
    // form are checked side by side; they must produce IDENTICAL
    // propagation edges (same sources, same target error type, same
    // conversions, same order) and no errors. Both go through the one
    // `route_result_error` routine -- this test is the guard that keeps
    // it that way.
    let src = with_prelude(
        r#"
fn tryBlockManual() -> Result<(), ConfigError> {
    let contents = readFile("config.txt")?;
    let parsed = parseConfig(contents)?;
    applyConfig(parsed);
    return Ok(());
}
fn viaHandWritten() {
    match tryBlockManual() {
        Ok(_) => {},
        Err(e) => { logError("config load failed"); applyDefaultConfig(); }
    }
}
fn viaTryCatch() {
    try {
        let contents = readFile("config.txt")?;
        let parsed = parseConfig(contents)?;
        applyConfig(parsed);
    } catch (e) {
        logError("config load failed");
        applyDefaultConfig();
    }
}
"#,
    );
    let (errs, records) = check_full(&src);
    assert!(errs.is_empty(), "got: {:#?}", errs);
    assert_eq!(records.len(), 4, "2 edges from the hand-written fn + 2 from try/catch: {:#?}", records);
    let (manual, via_try) = records.split_at(2);
    assert_eq!(manual, via_try, "try/catch propagation must equal the hand-written ?-chain");
    // And the edges are the ones Document 11 §2/§3 describe:
    assert_eq!(manual[0].source, Some(Ty::Named("IoError".into())));
    assert_eq!(manual[0].target, Some(Ty::Named("ConfigError".into())));
    assert!(manual[0].converted, "IoError -> ConfigError runs a From conversion");
    assert!(!manual[1].converted, "ConfigError -> ConfigError is the identity");
}

// ============================================================
// §4 — panic  /  §5 — assert, ensure
// ============================================================

#[test]
fn panic_with_string_message_accepted() {
    assert_ok("fn f() { panic(\"boom\"); }");
}

#[test]
fn panic_with_non_string_rejected() {
    assert_rejected("fn f() { panic(5); }");
}

#[test]
fn panic_with_concatenated_message_accepted() {
    // Document 11 §4's own example shape.
    assert_ok("fn f(index: usize) { panic(\"index out of bounds: \" + index as String); }");
}

#[test]
fn panic_arm_does_not_conflict_with_value_arms() {
    // Document 9 §2.3's own guard example: the `panic` arm has type `!`.
    assert_ok(
        r#"
fn classify(age: i32) -> String {
    return match age {
        n if n < 0 => panic("invalid age"),
        n if n < 18 => "minor",
        _ => "adult",
    };
}
"#,
    );
}

#[test]
fn panic_in_if_branch_does_not_conflict_with_value_branch() {
    assert_ok("fn f(c: bool) -> i32 { return if c { panic(\"x\") } else { 5 }; }");
}

#[test]
fn return_arm_in_match_does_not_conflict_with_value_arms() {
    // Document 24 §1's `_ => return ...` shape.
    assert_ok("fn f(x: i32) -> i32 { let y = match x { 0 => return 1, _ => 5 }; return y; }");
}

#[test]
fn assert_requires_bool_condition() {
    assert_ok("fn f(a: f64) { assert(a > 0.0); }");
    assert_rejected("fn g() { assert(5); }");
}

#[test]
fn ensure_yields_result_of_unit_and_composes_with_question_mark() {
    let (errs, records) = check_full(
        r#"
enum BankError { Insufficient }
fn check(funds: f64, amount: f64) -> Result<(), BankError> {
    ensure(funds >= amount, BankError::Insufficient)?;
    return Ok(());
}
"#,
    );
    assert!(errs.is_empty(), "got: {:#?}", errs);
    assert_eq!(records.len(), 1);
    assert!(!records[0].converted);
}

#[test]
fn ensure_error_type_without_from_rejected() {
    assert_rejected_with(
        &with_prelude(
            "enum Other { X }\nfn f(a: i32) -> Result<(), ConfigError> { ensure(a > 0, Other::X)?; return Ok(()); }",
        ),
        "no `impl From<Other> for ConfigError`",
    );
}

#[test]
fn ensure_requires_bool_condition() {
    assert_rejected(
        "enum BankError { Insufficient }\nfn f() -> Result<(), BankError> { ensure(5, BankError::Insufficient)?; return Ok(()); }",
    );
}

#[test]
fn doc11_section5_withdraw_example_accepted() {
    // Verbatim shape of Document 11 §5, including `balance: borrow mut
    // Account` followed by `balance.funds` (field access through a
    // reference used to silently type as `()`).
    assert_ok(
        r#"
struct Account { funds: f64 }
enum BankError { InsufficientFunds }
fn withdraw(balance: borrow mut Account, amount: f64) -> Result<(), BankError> {
    assert(amount > 0.0);
    ensure(balance.funds >= amount, BankError::InsufficientFunds)?;
    balance.funds -= amount;
    return Ok(());
}
"#,
    );
}

// ============================================================
// Pattern bindings (needed by Document 11 §1.1 / §1.2's own examples)
// ============================================================

#[test]
fn doc11_section1_1_match_on_result_binds_payloads() {
    assert_ok(&with_prelude(
        "fn show(outcome: Result<String, IoError>) { match outcome { Ok(contents) => print(contents), Err(e) => print(\"failed\") } }",
    ));
}

#[test]
fn result_pattern_binding_carries_the_real_payload_type() {
    assert_rejected(&with_prelude(
        "fn show(outcome: Result<String, IoError>) { match outcome { Ok(contents) => { let n: i32 = contents; }, Err(e) => { } } }",
    ));
}

#[test]
fn doc11_section1_2_match_on_option_binds_payload() {
    assert_ok("fn f(user: Option<i32>) -> i32 { return match user { Some(u) => u, None => 0 }; }");
    assert_rejected("fn g(user: Option<i32>) -> i32 { return match user { Some(u) => u, None => \"zero\" }; }");
}

#[test]
fn user_enum_variant_payloads_bind_in_match_arms() {
    assert_ok(
        "enum Shape { Circle(f64), Square(f64) }\nfn area(s: Shape) -> f64 { return match s { Shape::Circle(r) => r, Shape::Square(w) => w }; }",
    );
}

#[test]
fn pattern_bindings_on_unresolved_scrutinee_do_not_report_undefined_variables() {
    assert_ok("fn f() { match mystery() { Ok(v) => print(v), Err(e) => print(e) } }");
}

// ============================================================
// Return-type checking (Phase 8 flagged this as never done)
// ============================================================

#[test]
fn return_value_checked_against_declared_return_type() {
    assert_ok("fn f() -> i32 { return 1; }");
    assert_rejected("fn g() -> i32 { return \"x\"; }");
}

// ============================================================
// Single-source-of-truth guard for Option/Result's variants
// ============================================================

#[test]
fn builtin_variant_table_describes_option_and_result() {
    let opt = builtin_variants(&Ty::OptionTy(Box::new(Ty::I32))).expect("Option has variants");
    let names: Vec<&str> = opt.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, vec!["Some", "None"]);
    assert_eq!(opt[0].1, vec![Ty::I32]);
    assert!(opt[1].1.is_empty());

    let res = builtin_variants(&Ty::ResultTy(Box::new(Ty::I32), Box::new(Ty::Bool))).expect("Result has variants");
    let names: Vec<&str> = res.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, vec!["Ok", "Err"]);
    assert_eq!(res[0].1, vec![Ty::I32]);
    assert_eq!(res[1].1, vec![Ty::Bool]);

    assert!(builtin_variants(&Ty::I32).is_none());
}
