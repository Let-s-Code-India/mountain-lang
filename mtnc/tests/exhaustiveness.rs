//! Match exhaustiveness tests — Phase 8 (Document 25 §2.3).
//!
//! Exit criteria: "the exhaustiveness checker must correctly reject
//! every non-exhaustive `match` test case (and correctly accept
//! exhaustive ones, including via a `_` wildcard or an exhaustive set
//! of specific patterns without one)."

use mtnc::lexer;
use mtnc::parser::parse_program;
use mtnc::types::TypeChecker;

fn check(src: &str) -> Vec<String> {
    let (tokens, lex_errs) = lexer::tokenize(src);
    assert!(lex_errs.is_empty(), "lex errors: {:?}", lex_errs);
    let (program, parse_errs) = parse_program(tokens);
    assert!(parse_errs.is_empty(), "parse errors: {:?}\nsource:\n{}", parse_errs, src);
    let mut tc = TypeChecker::new();
    tc.check_program(&program);
    tc.errors.iter().map(|e| e.message.clone()).collect()
}

fn assert_exhaustive(src: &str) {
    let errs = check(src);
    let exhaustive_errs: Vec<_> = errs.iter().filter(|e| e.contains("non-exhaustive")).collect();
    assert!(exhaustive_errs.is_empty(), "expected exhaustive, got errors: {:#?}\nsource:\n{}", exhaustive_errs, src);
}

fn assert_non_exhaustive(src: &str) {
    let errs = check(src);
    assert!(errs.iter().any(|e| e.contains("non-exhaustive")), "expected a non-exhaustive-match error, got: {:#?}\nsource:\n{}", errs, src);
}

const HTTP_METHOD: &str = r#"
    enum HttpMethod {
        Get,
        Post,
        Put,
        Delete,
        Custom(String),
    }
"#;

#[test]
fn doc9_s2_1_all_variants_plus_data_variant_exhaustive() {
    // Document 9 §2.1's own example: every variant covered explicitly,
    // including a `|`-combined pair and a data-carrying variant with a
    // binding sub-pattern.
    let src = format!(
        r#"
        {http_method}
        fn handle(httpMethod: HttpMethod) {{
            match httpMethod {{
                HttpMethod::Get => 1,
                HttpMethod::Post => 2,
                HttpMethod::Put | HttpMethod::Delete => 3,
                HttpMethod::Custom(name) => 4,
            }}
        }}
        "#,
        http_method = HTTP_METHOD
    );
    assert_exhaustive(&src);
}

#[test]
fn missing_one_variant_is_non_exhaustive() {
    let src = format!(
        r#"
        {http_method}
        fn handle(httpMethod: HttpMethod) {{
            match httpMethod {{
                HttpMethod::Get => 1,
                HttpMethod::Post => 2,
            }}
        }}
        "#,
        http_method = HTTP_METHOD
    );
    assert_non_exhaustive(&src);
}

#[test]
fn wildcard_arm_covers_missing_variants() {
    let src = format!(
        r#"
        {http_method}
        fn handle(httpMethod: HttpMethod) {{
            match httpMethod {{
                HttpMethod::Get => 1,
                _ => 0,
            }}
        }}
        "#,
        http_method = HTTP_METHOD
    );
    assert_exhaustive(&src);
}

#[test]
fn doc9_s2_2_int_match_without_wildcard_is_non_exhaustive() {
    // Document 9 §2.2's own example: literal `i32` patterns alone can
    // never be exhaustive, no matter how many are listed -- the domain
    // is unenumerable.
    assert_non_exhaustive(
        r#"
        fn statusText(statusCode: i32) {
            match statusCode {
                200 => 1,
                404 => 2,
                500 => 3,
            }
        }
        "#,
    );
}

#[test]
fn doc9_s2_2_int_match_with_wildcard_is_exhaustive() {
    assert_exhaustive(
        r#"
        fn statusText(statusCode: i32) {
            match statusCode {
                200 => 1,
                404 => 2,
                500 => 3,
                _ => 0,
            }
        }
        "#,
    );
}

#[test]
fn bool_true_false_exhaustive_without_wildcard() {
    // `bool` is a genuine two-constructor finite domain -- Document 9
    // never shows this explicitly, but it follows directly from the
    // same exhaustiveness principle applied to a type this checker CAN
    // fully enumerate, and is a natural, low-risk case to pin down.
    assert_exhaustive(
        r#"
        fn describe(flag: bool) {
            match flag {
                true => 1,
                false => 2,
            }
        }
        "#,
    );
}

#[test]
fn bool_only_true_is_non_exhaustive() {
    assert_non_exhaustive(
        r#"
        fn describe(flag: bool) {
            match flag {
                true => 1,
            }
        }
        "#,
    );
}

#[test]
fn doc7_s3_2_option_some_none_exhaustive() {
    // Document 7 §3.2: Option is an ordinary two-variant enum
    // (`Some`/`None`) in Mountain's own model -- exercised here as the
    // synthetic-enum special case `exhaustive.rs` documents.
    assert_exhaustive(
        r#"
        fn describe(user: Option<i32>) {
            match user {
                Some(u) => 1,
                None => 2,
            }
        }
        "#,
    );
}

#[test]
fn option_missing_none_is_non_exhaustive() {
    assert_non_exhaustive(
        r#"
        fn describe(user: Option<i32>) {
            match user {
                Some(u) => 1,
            }
        }
        "#,
    );
}

#[test]
fn result_ok_err_exhaustive() {
    assert_exhaustive(
        r#"
        fn describe(r: Result<i32, String>) {
            match r {
                Ok(v) => 1,
                Err(e) => 2,
            }
        }
        "#,
    );
}

#[test]
fn doc9_s2_4_tuple_pattern_with_final_catchall_exhaustive() {
    // Document 9 §2.4's own pair-matching example: two specific tuples
    // plus a guarded equality arm plus a final catch-all -- exhaustive
    // because of the LAST arm's bare `(x, y)`, regardless of the
    // guarded middle arm (guards never count toward coverage, Document
    // 9 §2.3).
    assert_exhaustive(
        r#"
        fn describe(pair: (i32, i32)) {
            match pair {
                (0, 0) => 1,
                (x, y) if x == y => 2,
                (x, y) => 3,
            }
        }
        "#,
    );
}

#[test]
fn doc9_s2_3_guard_only_arms_never_count_as_exhaustive() {
    // Document 9 §2.3: "Guards do NOT count toward exhaustiveness
    // checking... the compiler still requires a fallback arm." Every
    // arm here has a guard, so despite superficially "looking like"
    // full coverage of a small bool-shaped domain, this must still be
    // rejected.
    assert_non_exhaustive(
        r#"
        fn classify(n: i32) {
            match n {
                x if x < 0 => 1,
                x if x >= 0 => 2,
            }
        }
        "#,
    );
}

#[test]
fn nested_enum_inside_tuple_struct_variant_exhaustive() {
    // Real pattern-matrix decomposition, not a top-level-only
    // shortcut: `ApiResponse::Failure` carries a nested field that
    // itself must be accounted for structurally (here just a wildcard
    // binding, but the point is the algorithm decomposes into the
    // variant's own sub-columns rather than treating `Failure(..)` as
    // one opaque unit).
    assert_exhaustive(
        r#"
        enum ApiResponse {
            Success(String),
            Failure(i32, String),
            Pending,
        }
        fn handle(response: ApiResponse) {
            match response {
                ApiResponse::Success(body) => 1,
                ApiResponse::Failure(code, msg) => 2,
                ApiResponse::Pending => 3,
            }
        }
        "#,
    );
}

#[test]
fn nested_enum_missing_one_case_is_non_exhaustive() {
    assert_non_exhaustive(
        r#"
        enum ApiResponse {
            Success(String),
            Failure(i32, String),
            Pending,
        }
        fn handle(response: ApiResponse) {
            match response {
                ApiResponse::Success(body) => 1,
                ApiResponse::Pending => 3,
            }
        }
        "#,
    );
}

// ============================================================
// Proactive fix, per the explicit instruction to check whether the
// same class of issue affects exhaustiveness anywhere else: the bug
// causing the tuple-pattern failure above (`full_signature` not
// recognizing a single-constructor product type as complete) turned
// out to affect tuple STRUCTS identically -- Document 9 §2.4's OTHER
// worked example (`struct Point(f64, f64);` with three arms, the last
// a bare `Point(x, y)` catch-all) has the exact same shape and would
// have failed the exact same way before this fix. Not previously
// covered by any test in this file; added here specifically because
// checking for this was asked for, not discovered independently.
// ============================================================

#[test]
fn doc9_s2_4_point_tuple_struct_with_final_catchall_exhaustive() {
    assert_exhaustive(
        r#"
        struct Point(f64, f64);
        fn describe(point: Point) {
            match point {
                Point(0.0, 0.0) => 1,
                Point(x, 0.0) => 2,
                Point(x, y) => 3,
            }
        }
        "#,
    );
}

#[test]
fn point_tuple_struct_missing_final_catchall_is_non_exhaustive() {
    assert_non_exhaustive(
        r#"
        struct Point(f64, f64);
        fn describe(point: Point) {
            match point {
                Point(0.0, 0.0) => 1,
                Point(x, 0.0) => 2,
            }
        }
        "#,
    );
}

#[test]
fn tuple_containing_enum_with_wildcard_catchall_exhaustive() {
    // A deeper combination than either fixed case alone: a tuple whose
    // OWN element is an enum, with only one specific-variant arm plus
    // a fully-wildcard catch-all tuple arm -- exercises `Ctor::Tuple`
    // specialization feeding into a NESTED enum-variant sub-column,
    // rather than either shape in isolation.
    let src = format!(
        r#"
        {http_method}
        fn handle(httpMethod: HttpMethod, flag: bool) {{
            match (httpMethod, flag) {{
                (HttpMethod::Get, true) => 1,
                (x, y) => 2,
            }}
        }}
        "#,
        http_method = HTTP_METHOD
    );
    assert_exhaustive(&src);
}
