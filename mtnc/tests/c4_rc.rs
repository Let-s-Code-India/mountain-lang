//! Phase 11c C4: `Rc` / `Arc` / `Weak` (Doc 6 §6).
use mtnc::codegen::Options;
use mtnc::driver;
use std::process::Command;

/// Unique per call: tests run in parallel threads of one process.
static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn tmp(src: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mtn_c4_{}_{}", std::process::id(), COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
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
fn memory_example_rc_weak() {
    let (o, c) = run_file(&example("rc_weak.mtn"));
    assert_eq!(c, 0);
    assert_eq!(o, "1\n1\n2\nleaf\n3\n2\n0\n1\n1\nc\np\n");
}

#[test]
fn memory_example_arc_shared() {
    let (o, c) = run_file(&example("arc_shared.mtn"));
    assert_eq!(c, 0);
    assert_eq!(o, "2\n3\n1\ncfg\n0\n");
}

// ------------------------------------------------------------------ behaviour

#[test]
fn rc_clone_counts_and_drops() {
    ok("fn main() { let a = Rc::new(5); let b = a.clone(); println(Rc::strongCount(borrow a)); drop(b); println(Rc::strongCount(borrow a)); println(*a); }", "2\n1\n5\n");
}

#[test]
fn rc_of_a_heap_value_is_freed_with_the_last_owner() {
    ok("fn main() { let a = Rc::new(String::from(\"s\")); let b = a.clone(); let c = b.clone(); println(*c); }", "s\n");
}

#[test]
fn weak_upgrade_succeeds_while_a_strong_owner_lives_and_fails_after() {
    ok(
        "fn main() { let w: Weak<i32>; { let r = Rc::new(7); w = r.downgrade(); match w.upgrade() { Some(x) => println(*x), None => println(0) } } match w.upgrade() { Some(x) => println(*x), None => println(0) } }",
        "7\n0\n",
    );
}

#[test]
fn weak_count_counts_only_weak_handles() {
    ok("fn main() { let r = Rc::new(1); println(Rc::weakCount(borrow r)); let w = r.downgrade(); let w2 = w.clone(); println(Rc::weakCount(borrow r)); }", "0\n2\n");
}

#[test]
fn object_outlives_its_last_strong_owner_until_the_last_weak_is_gone() {
    ok("fn main() { let w: Weak<String>; { let r = Rc::new(String::from(\"x\")); w = r.downgrade(); } println(1); }", "1\n");
}

#[test]
fn arc_uses_the_same_api_and_counts() {
    ok("fn main() { let a = Arc::new(3); let b = a.clone(); println(Arc::strongCount(borrow a)); let w: Weak<i32, Arc> = a.downgrade(); drop(a); drop(b); match w.upgrade() { Some(x) => println(*x), None => println(0) } }", "2\n0\n");
}

#[test]
fn rc_inside_a_struct_and_a_growable() {
    ok("struct H { r: Rc<String> } fn main() { let s = Rc::new(String::from(\"v\")); let h = H { r: s.clone() }; let v = [s.clone(), s.clone()]; println(Rc::strongCount(borrow s)); println(*h.r); }", "4\nv\n");
}

#[test]
fn field_access_and_method_calls_go_through_rc() {
    ok("struct P { n: i32 } impl P { fn get(borrow self) -> i32 { return self.n; } } fn main() { let r = Rc::new(P { n: 9 }); println(r.n); println(r.get()); }", "9\n9\n");
}

#[test]
fn matching_a_field_behind_an_rc_does_not_move_it_out() {
    ok("struct C { o: Option<String> } fn main() { let r = Rc::new(C { o: Some(String::from(\"k\")) }); match r.o { Some(s) => println(1), None => println(0) } match r.o { Some(s) => println(1), None => println(0) } }", "1\n1\n");
}

// ------------------------------------------------------------------ rejections

#[test]
fn use_after_move_of_an_rc_is_rejected() {
    let e = rejected("fn main() { let a = Rc::new(1); let b = a; println(Rc::strongCount(borrow a)); }");
    assert!(e.contains("use of moved value"), "{}", e);
}

#[test]
fn moving_a_heap_field_out_of_an_rc_is_a_clear_error() {
    let f = tmp("struct P { s: String } fn main() { let r = Rc::new(P { s: String::from(\"x\") }); let t = r.s; println(t); }");
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("-o").arg(f.with_extension("bin")).output().unwrap();
    assert!(!out.status.success());
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("out of a borrowed value"), "{}", e);
}

#[test]
fn rc_count_needs_a_borrowed_pointer() {
    let e = rejected("fn main() { let a = Rc::new(1); println(Rc::strongCount(a)); }");
    assert!(e.contains("needs `borrow"), "{}", e);
}

// ------------------------------------------------------------------ IR

#[test]
fn arc_counts_are_atomic_and_rc_counts_are_not() {
    let arc = ir("fn main() { let a = Arc::new(1); let b = a.clone(); println(Arc::strongCount(borrow a)); }");
    assert!(arc.contains("atomicrmw add"), "{}", arc);
    assert!(arc.contains("atomicrmw sub"), "{}", arc);
    let rc = ir("fn main() { let a = Rc::new(1); let b = a.clone(); println(Rc::strongCount(borrow a)); }");
    assert!(!rc.contains("atomicrmw"), "{}", rc);
}

#[test]
fn rc_drop_glue_is_generated_once_per_type() {
    let text = ir("fn main() { let a = Rc::new(1); let b = Rc::new(2); let c = a.clone(); println(*b + *c); }");
    assert_eq!(text.matches("define void @__mtn_drop_Rc_i32_").count(), 1, "{}", text);
}
