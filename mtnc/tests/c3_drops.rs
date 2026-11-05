//! Phase 11c C3: drop glue, `Box<T>`, moves (Doc 6, Doc 13 §1-§2).
use mtnc::codegen::Options;
use mtnc::driver;
use std::process::Command;

/// Unique per call: tests run in parallel threads of one process.
static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn tmp(src: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mtn_c3_{}_{}", std::process::id(), COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("p.mtn");
    std::fs::write(&f, src).unwrap();
    f
}

fn have_valgrind() -> bool {
    Command::new("valgrind").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Builds `file`, runs it; when valgrind exists also asserts: no leak, no invalid access.
fn run_file(file: &std::path::Path) -> (String, i32) {
    let exe = file.with_extension("bin");
    let b = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(file).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let out = Command::new(&exe).output().unwrap();
    if have_valgrind() {
        let v = Command::new("valgrind")
            .args(["--leak-check=full", "--errors-for-leak-kinds=definite", "--error-exitcode=99"])
            .arg(&exe)
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&v.stderr).to_string();
        assert_ne!(v.status.code(), Some(99), "valgrind found a leak or memory error:\n{}", err);
        assert!(!err.contains("Invalid read") && !err.contains("Invalid write") && !err.contains("Invalid free"), "valgrind: {}", err);
    }
    (String::from_utf8_lossy(&out.stdout).to_string(), out.status.code().unwrap_or(-1))
}

fn run(src: &str) -> (String, i32) {
    run_file(&tmp(src))
}

fn ok(src: &str, want: &str) {
    let (o, c) = run(src);
    assert_eq!((o.as_str(), c), (want, 0));
}

fn rejected(src: &str) -> String {
    let f = tmp(src);
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("check").arg(&f).output().unwrap();
    assert!(!out.status.success(), "should have been rejected:\n{}", src);
    String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr)
}

fn ir(src: &str) -> String {
    driver::emit_ir(src, &Options { release: false }).unwrap_or_else(|e| panic!("codegen failed: {:?}", e))
}

fn example(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").join("memory").join(name)
}

// ------------------------------------------------------------------ examples

#[test]
fn memory_example_drop_paths() {
    let (o, c) = run_file(&example("drop_paths.mtn"));
    assert_eq!(c, 0);
    assert_eq!(o, "4\ncond\n1\na\n0\nx\nloop\ntwo\nb\n");
}

#[test]
fn memory_example_box_match_list() {
    let (o, c) = run_file(&example("box_match_list.mtn"));
    assert_eq!(c, 0);
    assert_eq!(o, "n\n1\np\n2\n0\n1\n5\n9\nw\nmoved\nfine\nkeep\n1\n-5\n0\nnew\n");
}

// ------------------------------------------------------------------ behaviour

#[test]
fn box_new_and_deref() {
    ok("fn main() { let b = Box::new(41); println(*b + 1); }", "42\n");
}

#[test]
fn box_of_struct_field_and_method_auto_deref() {
    ok("struct P { n: i32 } impl P { fn get(borrow self) -> i32 { return self.n; } } fn main() { let b = Box::new(P { n: 7 }); println(b.n); println(b.get()); }", "7\n7\n");
}

#[test]
fn deref_assignment_through_box() {
    ok("fn main() { let mut b = Box::new(1); *b = 5; println(*b); }", "5\n");
}

#[test]
fn string_from_literal_and_string() {
    ok("fn main() { let a = String::from(\"x\"); let b = String::from(a); println(b); }", "x\n");
}

#[test]
fn conditional_move_frees_exactly_once_on_both_paths() {
    for flag in ["true", "false"] {
        let src = format!("fn eat(s: String) {{ println(s); }} fn main() {{ let s = String::from(\"v\"); if {} {{ eat(s); }} }}", flag);
        let (_, c) = run(&src);
        assert_eq!(c, 0);
    }
}

#[test]
fn partial_move_drops_only_the_rest() {
    ok("struct Pair { a: String, b: String } fn main() { let p = Pair { a: String::from(\"1\"), b: String::from(\"2\") }; let x = p.a; println(x); }", "1\n");
}

#[test]
fn explicit_drop_then_scope_end_is_single_free() {
    ok("fn main() { let s = String::from(\"d\"); drop(s); println(1); }", "1\n");
}

#[test]
fn early_return_break_and_continue_release_owned_values() {
    ok(
        "fn f(n: i32) -> i32 { let s = String::from(\"s\"); if n > 0 { return n; } return 0; }
         fn main() { let mut i = 0; while i < 4 { let t = String::from(\"t\"); i = i + 1; if i == 1 { continue; } if i == 3 { break; } } println(f(2) + f(0) + i); }",
        "5\n",
    );
}

#[test]
fn question_mark_releases_locals_on_the_error_path() {
    ok(
        "fn p(n: i32) -> Result<i32, i32> { if n < 0 { return Err(n); } return Ok(n); }
         fn f(n: i32) -> Result<i32, i32> { let keep = String::from(\"k\"); let v = p(n)?; return Ok(v + 1); }
         fn main() { match f(1) { Ok(x) => println(x), Err(e) => println(e) } match f(0 - 1) { Ok(x) => println(x), Err(e) => println(e) } }",
        "2\n-1\n",
    );
}

#[test]
fn reassignment_drops_the_old_value() {
    ok("fn main() { let mut s = String::from(\"a\"); s = String::from(\"b\"); println(s); }", "b\n");
}

#[test]
fn growable_of_strings_frees_every_element() {
    ok("fn main() { let v = [String::from(\"a\"), String::from(\"b\")]; println(v[0]); }", "a\n");
}

#[test]
fn growable_of_structs_with_heap_fields() {
    ok("struct P { n: String } fn main() { let v = [P { n: String::from(\"a\") }, P { n: String::from(\"b\") }]; println(v[1].n); }", "b\n");
}

#[test]
fn recursive_list_through_box_is_freed_deeply() {
    ok(
        "struct N { v: i32, next: Option<Box<N>> }
         fn build(n: i32) -> Option<Box<N>> { if n == 0 { return None; } return Some(Box::new(N { v: n, next: build(n - 1) })); }
         fn main() { let l = build(200); match l { Some(b) => println(b.v), None => println(0) } }",
        "200\n",
    );
}

#[test]
fn match_on_owned_enum_moves_the_payload_and_drops_the_rest() {
    ok(
        "enum E { A(String), B(String, String), C }
         fn f(e: E) -> i32 { match e { E::A(s) => { println(s); 1 } E::B(x, y) => { println(y); 2 } E::C => 3 } }
         fn main() { println(f(E::A(String::from(\"a\")))); println(f(E::B(String::from(\"x\"), String::from(\"y\")))); println(f(E::C)); }",
        "a\n1\ny\n2\n3\n",
    );
}

#[test]
fn match_through_borrow_binds_references_and_keeps_ownership() {
    ok(
        "enum E { A(String), C }
         fn peek(e: borrow E) -> i32 { match e { E::A(s) => 1, E::C => 2 } }
         fn main() { let e = E::A(String::from(\"keep\")); println(peek(borrow e)); println(peek(borrow e)); }",
        "1\n1\n",
    );
}

#[test]
fn tail_recursion_with_an_owned_parameter_does_not_leak() {
    ok("fn acc(n: i32, s: String) -> i32 { if n == 0 { return 0; } return acc(n - 1, s); } fn main() { println(acc(5000, String::from(\"t\"))); }", "0\n");
}

#[test]
fn unused_owned_result_is_dropped() {
    ok("fn mk() -> String { return String::from(\"tmp\"); } fn main() { mk(); let _ = mk(); println(1); }", "1\n");
}

#[test]
fn owned_temporary_lent_as_str() {
    ok("fn show(s: &str) { println(s); } fn mk() -> String { return String::from(\"lent\"); } fn main() { show(mk()); }", "lent\n");
}

// ------------------------------------------------------------------ rejections

#[test]
fn use_after_move_is_rejected() {
    let e = rejected("fn main() { let s = String::from(\"x\"); let t = s; println(s); }");
    assert!(e.contains("use of moved value"), "{}", e);
}

#[test]
fn use_after_explicit_drop_is_rejected() {
    let e = rejected("fn main() { let s = String::from(\"x\"); drop(s); println(s); }");
    assert!(e.contains("use of moved value"), "{}", e);
}

#[test]
fn use_after_move_into_call_of_boxed_value_is_rejected() {
    let e = rejected("fn eat(b: Box<i32>) {} fn main() { let b = Box::new(1); eat(b); eat(b); }");
    assert!(e.contains("use of moved value"), "{}", e);
}

#[test]
fn recursive_struct_without_box_is_rejected() {
    let e = rejected("struct A { a: A, n: i32 } fn main() {}");
    assert!(e.contains("infinite size") && e.contains("Box"), "{}", e);
    let e = rejected("enum L { Cons(i32, L), Nil } fn main() {}");
    assert!(e.contains("infinite size"), "{}", e);
    let e = rejected("struct P { a: Option<P> } fn main() {}");
    assert!(e.contains("infinite size"), "{}", e);
}

#[test]
fn recursive_struct_through_box_is_accepted() {
    ok("struct A { a: Option<Box<A>>, n: i32 } fn main() { let x = A { a: None, n: 1 }; println(x.n); }", "1\n");
}

#[test]
fn copy_struct_with_heap_field_is_rejected() {
    let e = rejected("@copy\nstruct P { name: String } fn main() {}");
    assert!(e.contains("cannot own heap memory"), "{}", e);
}

// ------------------------------------------------------------------ IR

#[test]
fn drop_glue_is_generated_once_per_type() {
    let text = ir("struct P { a: String, b: String } fn main() { let x = P { a: String::from(\"1\"), b: String::from(\"2\") }; let y = P { a: String::from(\"3\"), b: String::from(\"4\") }; println(x.a); println(y.b); }");
    assert_eq!(text.matches("define void @__mtn_drop_P(").count(), 1, "{}", text);
    assert_eq!(text.matches("define void @__mtn_drop_String(").count(), 1, "{}", text);
}

#[test]
fn drop_glue_frees_every_heap_field_once() {
    let text = ir("struct P { a: String, b: String } fn main() { let x = P { a: String::from(\"1\"), b: String::from(\"2\") }; println(x.a); }");
    let start = text.find("define void @__mtn_drop_P(").expect("glue for P");
    let body = &text[start..];
    let end = body.find("\n}\n").unwrap();
    let glue = &body[..end];
    assert_eq!(glue.matches("call void @__mtn_drop_String").count(), 2, "{}", glue);
    let sstart = text.find("define void @__mtn_drop_String(").unwrap();
    let sbody = &text[sstart..];
    let sglue = &sbody[..sbody.find("\n}\n").unwrap()];
    assert_eq!(sglue.matches("call void @__mtn_dealloc").count(), 1, "{}", sglue);
}

#[test]
fn box_new_goes_through_the_one_allocation_interface() {
    let text = ir("fn main() { let b = Box::new(5); println(*b); }");
    assert!(text.contains("call ptr @__mtn_alloc"), "{}", text);
    assert!(text.contains("@__mtn_drop_Box_i32_"), "{}", text);
}

#[test]
fn types_without_heap_have_no_glue() {
    let text = ir("struct P { a: i32 } fn main() { let x = P { a: 1 }; println(x.a); }");
    assert!(!text.contains("__mtn_drop_"), "{}", text);
}
