//! Phase 12, milestone M0 (Document 12 §10.2, Document 25 §2.6): housekeeping that must be
//! closed before any concurrency work. Sections: move-while-borrowed (Doc 6 §9.8).

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
fn assert_moved_while_borrowed(src: &str) {
    let errs = check(src);
    assert!(
        errs.iter().any(|e| e.contains("cannot move") && e.contains("while it is borrowed")),
        "expected a move-while-borrowed error, got {:#?}\nsource:\n{}",
        errs,
        src
    );
}

const PRE: &str = "struct User { name: String }\nfn use1(u: &User) { }\nfn consume(u: User) { }\nfn two(a: &User, b: User) { }\n";

fn prog(body: &str) -> String {
    format!("{}fn main() {{\n{}\n}}\n", PRE, body)
}

#[test]
fn move_while_borrowed_let_rejected() {
    assert_moved_while_borrowed(&prog(
        "let x = User { name: \"a\" };\nlet r = borrow x;\nlet y = x;\nuse1(r);",
    ));
}

#[test]
fn move_while_borrowed_by_value_argument_rejected() {
    assert_moved_while_borrowed(&prog(
        "let x = User { name: \"a\" };\nlet r = borrow x;\nconsume(x);\nuse1(r);",
    ));
}

#[test]
fn move_while_borrowed_same_call_rejected() {
    assert_moved_while_borrowed(&prog("let x = User { name: \"a\" };\nlet r = borrow x;\ntwo(r, x);"));
}

#[test]
fn move_while_borrowed_temporary_in_same_call_rejected() {
    assert_moved_while_borrowed(&prog("let x = User { name: \"a\" };\ntwo(borrow x, x);"));
}

#[test]
fn move_while_borrowed_return_rejected() {
    assert_moved_while_borrowed(
        "struct User { name: String }\nfn use1(u: &User) { }\nfn f(c: bool) -> User {\n let x = User { name: \"a\" };\n let r = borrow x;\n if c { return x; }\n use1(r);\n return User { name: \"b\" };\n}\nfn main() { }\n",
    );
}

#[test]
fn move_while_borrowed_inside_loop_rejected() {
    assert_moved_while_borrowed(&prog(
        "let x = User { name: \"a\" };\nlet r = borrow x;\nlet mut i = 0;\nwhile i < 3 {\n  let y = x;\n  i += 1;\n}\nuse1(r);",
    ));
}

#[test]
fn move_while_borrowed_in_nested_block_rejected() {
    assert_moved_while_borrowed(&prog(
        "let x = User { name: \"a\" };\nlet r = borrow x;\nif true {\n  let y = x;\n}\nuse1(r);",
    ));
}

#[test]
fn move_while_borrowed_mutable_borrow_rejected() {
    assert_moved_while_borrowed(&prog(
        "let x = User { name: \"a\" };\nlet r = borrow mut x;\nconsume(x);\nuse1(r);",
    ));
}

#[test]
fn move_while_borrowed_by_closure_capture_rejected() {
    assert_moved_while_borrowed(&prog(
        "let x = User { name: \"a\" };\nlet r = borrow x;\nlet c = move || consume(x);\nuse1(r);",
    ));
}

#[test]
fn borrow_ended_before_the_move_is_fine() {
    // Non-lexical (Doc 6 §3.2): `r`'s last use precedes the move.
    assert_ok(&prog("let x = User { name: \"a\" };\nlet r = borrow x;\nuse1(r);\nlet y = x;\nuse1(borrow y);"));
}

#[test]
fn never_read_borrow_does_not_block_a_move() {
    assert_ok(&prog("let x = User { name: \"a\" };\nlet r = borrow x;\nconsume(x);"));
}

#[test]
fn temporary_borrow_in_earlier_statement_does_not_block_a_move() {
    assert_ok(&prog("let x = User { name: \"a\" };\nuse1(borrow x);\nconsume(x);"));
}

#[test]
fn return_after_last_use_of_borrow_is_fine() {
    assert_ok(
        "struct User { name: String }\nfn use1(u: &User) { }\nfn f(c: bool) -> User {\n let x = User { name: \"a\" };\n let r = borrow x;\n if c { use1(r); return x; }\n return User { name: \"b\" };\n}\nfn main() { }\n",
    );
}

#[test]
fn borrow_in_inner_block_ended_with_the_block_does_not_block_later_move() {
    assert_ok(&prog(
        "let x = User { name: \"a\" };\nif true {\n  let r = borrow x;\n  use1(r);\n}\nconsume(x);",
    ));
}

#[test]
fn borrow_of_another_place_does_not_block_a_move() {
    assert_ok(&prog(
        "let x = User { name: \"a\" };\nlet z = User { name: \"b\" };\nlet r = borrow z;\nconsume(x);\nuse1(r);",
    ));
}

// ---------------------------------------------------------------- end-to-end helpers

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn mtnc() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mtnc"))
}
fn scratch() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("mtnc_p12m0_{}_{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
struct Run {
    stdout: Vec<u8>,
    stderr: String,
    code: i32,
}
fn run_src(src: &str) -> Run {
    let dir = scratch();
    let f = dir.join("prog.mtn");
    std::fs::write(&f, src).unwrap();
    let out = mtnc().arg("run").arg(&f).output().expect("failed to start mtnc");
    Run { stdout: out.stdout, stderr: String::from_utf8_lossy(&out.stderr).to_string(), code: out.status.code().unwrap_or(-1) }
}
fn must_fail(src: &str) -> String {
    let dir = scratch();
    let f = dir.join("bad.mtn");
    let exe = dir.join("bad_out");
    std::fs::write(&f, src).unwrap();
    let out = mtnc().arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(!out.status.success(), "build unexpectedly succeeded for:\n{}", src);
    assert!(!exe.exists(), "a binary was produced although checking failed");
    String::from_utf8_lossy(&out.stderr).to_string()
}
fn stdout_of(src: &str) -> String {
    let r = run_src(src);
    assert_eq!(r.code, 0, "exit code {} stderr:\n{}\nsource:\n{}", r.code, r.stderr, src);
    String::from_utf8_lossy(&r.stdout).to_string()
}
#[allow(dead_code)]
fn _unused(_: &Path) {}

// ---------------------------------------------------------------- by-value `mut` parameters

#[test]
fn mut_param_copy_type_caller_unaffected() {
    let out = stdout_of(
        "fn count(mut n: i32) -> i32 { n += 1; n += 1; return n; }\nfn main() { let a = 5; let b = count(a); println(a); println(b); }\n",
    );
    assert_eq!(out, "5\n7\n");
}

#[test]
fn mut_param_plain_assignment_and_loop() {
    let out = stdout_of(
        "fn sum(mut n: i32, mut acc: i32) -> i32 { while n > 0 { acc = acc + n; n = n - 1; } return acc; }\nfn main() { println(sum(4, 0)); }\n",
    );
    assert_eq!(out, "10\n");
}

#[test]
fn mut_param_owned_struct_is_moved_and_mutable() {
    let out = stdout_of(
        "struct P { x: i32 }\nfn bump(mut p: P) -> i32 { p.x = p.x + 10; return p.x; }\nfn main() { let p = P { x: 1 }; println(bump(p)); }\n",
    );
    assert_eq!(out, "11\n");
}

#[test]
fn mut_param_owned_string_dropped_without_leak_and_reassigned() {
    let out = stdout_of(
        "fn f(mut s: String) -> usize { s = String::from(\"abc\"); return 3; }\nfn main() { println(f(String::from(\"hello\"))); }\n",
    );
    assert_eq!(out, "3\n");
}

#[test]
fn mut_param_method_taking_borrow_mut_self() {
    let out = stdout_of(
        "struct C { n: i32 }\nimpl C { fn inc(borrow mut self) { self.n += 1; } }\nfn f(mut c: C) -> i32 { c.inc(); c.inc(); return c.n; }\nfn main() { println(f(C { n: 0 })); }\n",
    );
    assert_eq!(out, "2\n");
}

#[test]
fn mut_param_in_method_and_trait_impl_may_differ() {
    let out = stdout_of(
        "trait T { fn go(self, n: i32) -> i32; }\nstruct S { k: i32 }\nimpl T for S { fn go(self, mut n: i32) -> i32 { n += self.k; return n; } }\nfn main() { let s = S { k: 3 }; println(s.go(4)); }\n",
    );
    assert_eq!(out, "7\n");
}

#[test]
fn param_without_mut_cannot_be_assigned() {
    let e = must_fail("fn f(n: i32) -> i32 { n += 1; return n; }\nfn main() { println(f(1)); }\n");
    assert!(e.contains("mut"), "{}", e);
}

#[test]
fn mut_cannot_combine_with_borrow_or_move() {
    let e = must_fail("fn f(mut borrow n: i32) { }\nfn main() { }\n");
    assert!(e.contains("cannot be combined"), "{}", e);
    let e = must_fail("fn f(mut move n: i32) { }\nfn main() { }\n");
    assert!(e.contains("cannot be combined"), "{}", e);
}

#[test]
fn borrow_mut_param_and_plain_mut_param_together() {
    let out = stdout_of(
        "fn f(borrow mut a: i32, mut b: i32) { a += b; b += 100; }\nfn main() { let mut x = 1; f(borrow mut x, 2); println(x); }\n",
    );
    assert_eq!(out, "3\n");
}

// ---------------------------------------------------------------- impl vs trait signatures (Doc 7 §9.6)

use mtnc::types::TypeChecker;

fn type_errors(src: &str) -> Vec<String> {
    let (tokens, lex_errs) = lexer::tokenize(src);
    assert!(lex_errs.is_empty(), "lex errors: {:?}", lex_errs);
    let (program, parse_errs) = parse_program(tokens);
    assert!(parse_errs.is_empty(), "parse errors: {:?}", parse_errs);
    let mut tc = TypeChecker::new();
    tc.check_program(&program);
    tc.errors.iter().map(|e| e.to_string()).collect()
}

#[test]
fn impl_param_type_differs_from_trait_rejected() {
    let e = type_errors("trait T { fn f(self, x: i32) -> i32; }\nstruct S { a: i32 }\nimpl T for S { fn f(self, x: i64) -> i32 { return 0; } }\nfn main() { }\n");
    assert!(e.iter().any(|m| m.contains("parameter 1 has type `i64` but the trait declares `i32`")), "{:?}", e);
}

#[test]
fn impl_return_type_differs_from_trait_rejected() {
    let e = type_errors("trait T { fn f(self) -> i32; }\nstruct S { a: i32 }\nimpl T for S { fn f(self) -> bool { return true; } }\nfn main() { }\n");
    assert!(e.iter().any(|m| m.contains("returns `bool` but the trait declares `i32`")), "{:?}", e);
}

#[test]
fn impl_self_substitution_accepted() {
    let e = type_errors("trait T { fn merge(self, other: Self) -> Self; }\nstruct S { a: i32 }\nimpl T for S { fn merge(self, other: S) -> S { return self; } }\nfn main() { }\n");
    assert!(e.is_empty(), "{:?}", e);
}

#[test]
fn impl_self_substitution_wrong_type_rejected() {
    let e = type_errors("trait T { fn merge(self, other: Self) -> Self; }\nstruct S { a: i32 }\nstruct U { a: i32 }\nimpl T for S { fn merge(self, other: U) -> S { return self; } }\nfn main() { }\n");
    assert!(e.iter().any(|m| m.contains("parameter 1 has type `U`")), "{:?}", e);
}

#[test]
fn impl_generic_trait_arguments_substituted() {
    let ok = type_errors("trait Conv<X> { fn conv(self, x: X) -> X; }\nstruct S { a: i32 }\nimpl Conv<i32> for S { fn conv(self, x: i32) -> i32 { return x; } }\nfn main() { }\n");
    assert!(ok.is_empty(), "{:?}", ok);
    let bad = type_errors("trait Conv<X> { fn conv(self, x: X) -> X; }\nstruct S { a: i32 }\nimpl Conv<i32> for S { fn conv(self, x: bool) -> i32 { return 1; } }\nfn main() { }\n");
    assert!(bad.iter().any(|m| m.contains("parameter 1 has type `bool` but the trait declares `i32`")), "{:?}", bad);
}

#[test]
fn impl_borrow_vs_value_parameter_rejected() {
    let e = type_errors("trait T { fn f(self, x: borrow String) -> i32; }\nstruct S { a: i32 }\nimpl T for S { fn f(self, x: String) -> i32 { return 0; } }\nfn main() { }\n");
    assert!(!e.is_empty(), "{:?}", e);
}

#[test]
fn impl_mut_parameter_is_not_part_of_the_signature() {
    let e = type_errors("trait T { fn f(self, n: i32) -> i32; }\nstruct S { a: i32 }\nimpl T for S { fn f(self, mut n: i32) -> i32 { n += 1; return n; } }\nfn main() { }\n");
    assert!(e.is_empty(), "{:?}", e);
}

#[test]
fn allocator_impl_signature_still_checked() {
    let e = type_errors("struct A { n: i32 }\nimpl Allocator for A {\n fn alloc(borrow mut self, size: usize, align: usize) -> *mut u8 { return null; }\n fn dealloc(borrow mut self, ptr: *mut u8, size: i32) { }\n}\nfn main() { }\n");
    assert!(!e.is_empty(), "{:?}", e);
}

// ---------------------------------------------------------------- break/continue out of `try` (Doc 12 §11.6)

const RISKY: &str = "fn risky(n: i32) -> Result<i32, String> {\n  if n == 3 { return Err(String::from(\"three\")); }\n  return Ok(n * 10);\n}\n";

#[test]
fn try_break_and_continue_in_while() {
    let out = stdout_of(&format!(
        "{}fn main() {{\n let mut total = 0;\n let mut i = 0;\n while i < 10 {{\n  i += 1;\n  try {{\n   let v = risky(i)?;\n   if v == 20 {{ continue; }}\n   if v > 60 {{ break; }}\n   total += v;\n  }} catch (e) {{\n   println(e);\n  }}\n }}\n println(total);\n println(i);\n}}\n",
        RISKY
    ));
    assert_eq!(out, "three\n160\n7\n");
}

#[test]
fn try_break_in_for_loop() {
    let out = stdout_of(&format!(
        "{}fn main() {{\n let mut last = 0;\n for i in 1..10 {{\n  try {{\n   let v = risky(i)?;\n   last = v;\n   if v >= 50 {{ break; }}\n  }} catch (e) {{ last = -1; }}\n }}\n println(last);\n}}\n",
        RISKY
    ));
    assert_eq!(out, "50\n");
}

#[test]
fn try_labeled_break_and_continue_to_outer_loop() {
    let out = stdout_of(&format!(
        "{}fn main() {{\n let mut n = 0;\n 'outer: for a in 1..5 {{\n  for b in 1..5 {{\n   try {{\n    let v = risky(b)?;\n    if a == 2 {{ continue 'outer; }}\n    if a == 4 {{ break 'outer; }}\n    n += 1;\n   }} catch (e) {{ n += 100; }}\n  }}\n }}\n println(n);\n}}\n",
        RISKY
    ));
    // a=1: b=1,2 ok (+2), b=3 err (+100), b=4 ok (+1) -> 103 ; a=2: b=1 continue 'outer ; a=3: same as a=1 -> 103 ; a=4: b=1 break
    assert_eq!(out, "206\n");
}

#[test]
fn try_with_value_and_break() {
    let out = stdout_of(&format!(
        "{}fn main() {{\n let mut sum = 0;\n let mut i = 0;\n while i < 6 {{\n  i += 1;\n  let v = try {{\n   let r = risky(i)?;\n   if r == 40 {{ break; }}\n   r\n  }} catch (e) {{ 0 }};\n  sum += v;\n }}\n println(sum);\n}}\n",
        RISKY
    ));
    assert_eq!(out, "30\n"); // 10 + 20 + (3 -> 0) then i=4 breaks
}

#[test]
fn try_inner_loop_jumps_stay_inside_the_try() {
    let out = stdout_of(&format!(
        "{}fn main() {{\n let mut c = 0;\n let mut i = 0;\n while i < 3 {{\n  i += 1;\n  try {{\n   let v = risky(1)?;\n   let mut k = 0;\n   while true {{ k += 1; if k > 4 {{ break; }} }}\n   c += k;\n  }} catch (e) {{ }}\n }}\n println(c);\n}}\n",
        RISKY
    ));
    assert_eq!(out, "15\n");
}

#[test]
fn try_nested_try_break_to_outer_loop() {
    let out = stdout_of(&format!(
        "{}fn main() {{\n let mut i = 0;\n let mut seen = 0;\n while i < 10 {{\n  i += 1;\n  try {{\n   let z = risky(1)?;\n   try {{\n    let v = risky(i)?;\n    if v >= 40 {{ break; }}\n    seen += 1;\n   }} catch (e) {{ seen += 100; }}\n  }} catch (e2) {{ }}\n }}\n println(seen);\n println(i);\n}}\n",
        RISKY
    ));
    assert_eq!(out, "102\n4\n");
}

#[test]
fn try_without_jumps_still_lowers_as_before() {
    // no Option wrapper appears in the IR when nothing leaves the block
    let src = format!("{}fn main() {{ try {{ let v = risky(1)?; println(v); }} catch (e) {{ println(e); }} }}\n", RISKY);
    let ir = mtnc::driver::emit_ir(&src, &mtnc::codegen::Options { release: false }).unwrap();
    assert!(!ir.contains("__try_ctl"), "{}", ir);
}

#[test]
fn break_with_value_cannot_leave_try() {
    let e = must_fail(&format!(
        "{}fn main() {{ let x = loop {{ try {{ let v = risky(1)?; break v; }} catch (e) {{ }} }}; println(x); }}\n",
        RISKY
    ));
    assert!(e.contains("cannot leave a `try`"), "{}", e);
}

// ---------------------------------------------------------------- lexer rules (Doc 2 §10)

fn lex_errors(src: &str) -> Vec<(String, u32, u32)> {
    let (_, errs) = lexer::tokenize(src);
    errs.iter().map(|d| (d.message.clone(), d.span.line, d.span.col)).collect()
}
fn lex_ok(src: &str) {
    let e = lex_errors(src);
    assert!(e.is_empty(), "unexpected lex errors for {:?}: {:?}", src, e);
}

#[test]
fn identifier_ascii_forms_accepted() {
    lex_ok("let userName = _cache; let HTTPRequest2 = max_retries;");
}

#[test]
fn non_ascii_identifier_rejected_with_message_and_position() {
    let e = lex_errors("let caf\u{e9} = 1;");
    assert_eq!(e.len(), 1, "{:?}", e);
    assert!(e[0].0.contains("identifiers may use only ASCII letters, digits and `_`") && e[0].0.contains('\u{e9}'), "{:?}", e);
    assert_eq!((e[0].1, e[0].2), (1, 8));
}

#[test]
fn non_ascii_identifier_start_rejected() {
    let e = lex_errors("let \u{928}\u{92e} = 1;");
    assert_eq!(e.len(), 1, "{:?}", e);
    assert!(e[0].0.contains("ASCII"), "{:?}", e);
}

#[test]
fn non_ascii_allowed_in_strings_chars_and_comments() {
    lex_ok("// \u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947}\nlet a = \"\u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947} \u{4f60}\u{597d} \u{1F600}\"; let c = '\u{e9}'; /* \u{4f60}\u{597d} */ let d = '\u{4f60}';");
}

#[test]
fn every_documented_escape_accepted() {
    lex_ok(r#"let a = "\n \r \t \\ \" \' \0 \x41 \x7F \u{41} \u{1F600} \u{10FFFF} \u{000041}";"#);
    lex_ok(r#"let c = '\n'; let d = '\''; let e = '\\'; let f = '\x41'; let g = '\u{1F600}'; let h = '"'; let i = '\0';"#);
}

#[test]
fn unknown_escape_rejected_at_its_position() {
    let e = lex_errors("let s = \"ab\\qcd\";");
    assert_eq!(e.len(), 1, "{:?}", e);
    assert!(e[0].0.contains("unknown or malformed escape sequence `\\q`"), "{:?}", e);
    assert_eq!((e[0].1, e[0].2), (1, 12));
}

#[test]
fn malformed_hex_and_unicode_escapes_rejected() {
    for bad in [
        r#"let s = "\x";"#,
        r#"let s = "\x4";"#,
        r#"let s = "\xZZ";"#,
        r#"let s = "\x80";"#,
        r#"let s = "\xFF";"#,
        r#"let s = "\u41";"#,
        r#"let s = "\u{}";"#,
        r#"let s = "\u{41";"#,
        r#"let s = "\u{D800}";"#,
        r#"let s = "\u{110000}";"#,
        r#"let s = "\u{1234567}";"#,
        r#"let s = "\u{GG}";"#,
    ] {
        let e = lex_errors(bad);
        assert!(e.iter().any(|(m, _, _)| m.contains("unknown or malformed escape sequence")), "{} -> {:?}", bad, e);
    }
}

#[test]
fn backslash_at_end_of_line_is_an_error_and_there_is_no_line_continuation() {
    let e = lex_errors("let s = \"abc\\\nrest\";");
    assert!(e.iter().any(|(m, l, c)| m.contains("escape") && *l == 1 && *c == 13), "{:?}", e);
    assert!(e.iter().any(|(m, _, _)| m.contains("unterminated string literal")), "{:?}", e);
}

#[test]
fn strings_are_single_line() {
    let e = lex_errors("let s = \"abc\ndef\";");
    assert!(e.iter().any(|(m, _, _)| m.contains("unterminated string literal")), "{:?}", e);
}

#[test]
fn raw_strings_process_no_escapes() {
    lex_ok(r#"let r = r"C:\no\escapes\here\q";"#);
}

#[test]
fn char_literal_must_hold_exactly_one_character() {
    for bad in ["let c = '';", "let c = 'ab';", "let c = 'abc';", "let c = '\u{928}\u{92e}';", "let c = '12';", "let c = '\\n\\n';"] {
        let e = lex_errors(bad);
        assert!(e.iter().any(|(m, _, _)| m.contains("character literal must contain exactly one character")), "{} -> {:?}", bad, e);
    }
}

#[test]
fn char_literal_versus_lifetime_still_works() {
    lex_ok("let c = 'a'; fn f<'a>(x: &'a str) -> &'a str { x } 'outer: loop { break 'outer; }");
}

#[test]
fn byte_order_mark_is_ignored() {
    lex_ok("\u{feff}fn main() { }");
}

#[test]
fn invalid_utf8_source_is_a_compile_error() {
    let dir = scratch();
    let f = dir.join("bad.mtn");
    std::fs::write(&f, b"fn main() { print(\"\xff\xfe\"); }").unwrap();
    let out = mtnc().arg("check").arg(&f).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
    assert!(err.contains("utf-8") || err.contains("utf8"), "{}", err);
}

#[test]
fn hindi_and_chinese_text_round_trips_as_exact_utf8_bytes() {
    let r = run_src("// \u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947} comment\nfn main() { print(\"\u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947} \u{926}\u{941}\u{928}\u{93f}\u{92f}\u{93e} \u{4f60}\u{597d}\"); }\n");
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "नमस्ते दुनिया 你好".as_bytes());
}

#[test]
fn non_ascii_text_works_everywhere_in_a_program() {
    let r = run_src("/* \u{4f60} */ fn greet() -> String { return String::from(\"\u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947}\"); }\nfn main() {\n  let c = '\u{e9}';\n  println(greet());\n  println(\"\\u{1F600} \\x41 \\t|\");\n  // \u{4f60}\u{597d}\n}\n");
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(String::from_utf8(r.stdout).unwrap(), "नमस्ते\n😀 A \t|\n");
}

#[test]
fn bad_escape_in_a_program_gives_no_binary() {
    let e = must_fail("fn main() { print(\"a\\qb\"); }\n");
    assert!(e.contains("escape"), "{}", e);
}
