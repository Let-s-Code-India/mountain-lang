//! Integration tests exercising the lexer against realistic snippets
//! pulled directly from the specification documents, not just isolated
//! unit cases. Per Document 25 §2.2 point 2: tests must also prove
//! deliberately-invalid input is correctly rejected, not just that valid
//! input lexes.

use mtnc::lexer::tokenize;
use mtnc::manifest::Manifest;
use mtnc::token::TokenKind;

#[test]
fn lexes_document1_style_function_with_doc_comment() {
    let src = r#"
/// documentation comment — attaches to the next declaration
fn calculateInterest(principal: f64, rate: f64) -> f64 {
    return principal * rate;
}
"#;
    let (tokens, errors) = tokenize(src);
    assert!(errors.is_empty(), "expected no lex errors, got: {:?}", errors);
    let has_doc_comment = tokens.iter().any(|t| matches!(&t.kind, TokenKind::DocComment(_)));
    assert!(has_doc_comment, "expected a DocComment token to be preserved");
}

#[test]
fn lexes_document6_ownership_example() {
    let src = r#"
fn example() {
    let a = User::new("Alice");
    let b = a;
    print(b.name);
}
"#;
    let (_tokens, errors) = tokenize(src);
    assert!(errors.is_empty(), "expected no lex errors, got: {:?}", errors);
}

#[test]
fn lexes_document20_query_example() {
    let src = r#"let adults: [User] = query Users where age >= 18 orderBy name;"#;
    let (_tokens, errors) = tokenize(src);
    assert!(errors.is_empty(), "expected no lex errors, got: {:?}", errors);
}

#[test]
fn lexes_document2_target_directive_block() {
    let src = r#"
#target(native) {
    // native-only code, e.g. direct OS syscalls
}
#target(wasm) {
    // WASM-only code, e.g. direct DOM/browser API interop
}
"#;
    let (_tokens, errors) = tokenize(src);
    assert!(errors.is_empty(), "expected no lex errors, got: {:?}", errors);
}

#[test]
fn deliberately_invalid_source_is_rejected_not_silently_accepted() {
    // A stray '$' character has no meaning anywhere in Documents 1-25's
    // grammar. This must surface as a real, non-empty error list — an
    // empty error list here would mean the lexer silently swallowed bad
    // input, which Document 25's roadmap discipline explicitly forbids.
    let src = "let x = 5 $ ; \u{2603} let y;"; // stray '$' and a snowman
    let (_tokens, errors) = tokenize(src);
    assert!(!errors.is_empty(), "expected lex errors for invalid characters");
    assert_eq!(errors.len(), 2, "expected exactly two invalid-character errors");
}

#[test]
fn manifest_and_lexer_integrate_on_a_realistic_mountain_toml() {
    let toml_src = r#"
[package]
name = "example-app"
version = "0.1.0"
authors = ["Someone"]
edition = "2026"

[targets]
native = true
wasm = true
"#;
    let manifest = Manifest::parse(toml_src).expect("manifest should parse");
    assert_eq!(manifest.get_str("package", "name"), Some("example-app"));
    assert_eq!(manifest.get_bool("targets", "wasm"), Some(true));
}
