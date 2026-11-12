//! Phase 12, milestone M2 (Document 9 §4, §8.5; Document 13 §8.1): generators.
//! Programs are compiled with the real `mtnc` binary and run; outputs are asserted exactly.
//! Rejection tests assert the specific message. Every temp path is unique (pid + counter).

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn scratch() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("mtnc_m2_{}_{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Run {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run_src(src: &str) -> Run {
    let dir = scratch();
    let f = dir.join("prog.mtn");
    std::fs::write(&f, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("run").arg(&f).output().unwrap();
    let r = Run {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        code: out.status.code().unwrap_or(-1),
    };
    let _ = std::fs::remove_dir_all(&dir);
    r
}

/// Runs `src`, expects success and returns stdout.
fn out(src: &str) -> String {
    let r = run_src(src);
    assert_eq!(r.code, 0, "program failed:\n{}\nstderr: {}\nstdout: {}", src, r.stderr, r.stdout);
    r.stdout
}

/// Builds `src`, expects a compile error containing `needle`, and that no executable is left.
fn rejects(src: &str, needle: &str) {
    let dir = scratch();
    let f = dir.join("bad.mtn");
    let exe = dir.join("bad_out");
    std::fs::write(&f, src).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(!o.status.success(), "build unexpectedly succeeded for:\n{}", src);
    assert!(!exe.exists(), "a binary was produced although checking failed");
    assert!(err.contains(needle), "expected `{}` in:\n{}\nsource:\n{}", needle, err, src);
    assert!(!err.contains("panicked"), "compiler crashed: {}", err);
    let _ = std::fs::remove_dir_all(&dir);
}


const FIB: &str = "fn fibonacci() -> Generator<u64> {\n    let mut a: u64 = 0;\n    let mut b: u64 = 1;\n    loop {\n        yield a;\n        let next = a + b;\n        a = b;\n        b = next;\n    }\n}\n";

// ---------------------------------------------------------------- positive

#[test]
fn doc9_4_fibonacci_stopped_with_break_at_the_10th_value() {
    let src = format!("{}fn main() {{ let mut n = 0; for value in fibonacci() {{ println(value); n += 1; if n == 10 {{ break; }} }} }}", FIB);
    assert_eq!(out(&src), "0\n1\n1\n2\n3\n5\n8\n13\n21\n34\n");
}

#[test]
fn next_returns_option_and_resumes_where_it_left_off() {
    let src = format!(
        "{}fn main() {{ let mut g = fibonacci(); let mut i = 0; while i < 6 {{ match g.next() {{ Some(v) => println(v), None => println(\"none\") }} i += 1; }} }}",
        FIB
    );
    assert_eq!(out(&src), "0\n1\n1\n2\n3\n5\n");
}

#[test]
fn finished_generator_returns_none_forever() {
    let src = "fn two() -> Generator<i32> { yield 1; yield 2; }\nfn main() { let mut g = two(); let mut i = 0; while i < 5 { match g.next() { Some(v) => println(v), None => println(\"none\") } i += 1; } }";
    assert_eq!(out(src), "1\n2\nnone\nnone\nnone\n");
}

#[test]
fn explicit_return_ends_the_generator_early() {
    let src = "fn early(stop: bool) -> Generator<i32> { yield 1; if stop { return; } yield 2; }\nfn main() { for x in early(true) { println(x); } println(\"-\"); for x in early(false) { println(x); } }";
    assert_eq!(out(src), "1\n-\n1\n2\n");
}

#[test]
fn body_does_not_run_until_the_first_next() {
    let src = "fn lazy() -> Generator<i32> { println(\"start\"); yield 1; println(\"end\"); }\nfn main() { let mut g = lazy(); println(\"created\"); match g.next() { Some(v) => println(v), None => println(\"none\") } match g.next() { Some(v) => println(v), None => println(\"none\") } }";
    assert_eq!(out(src), "created\nstart\n1\nend\nnone\n");
}

#[test]
fn general_bodies_while_if_match_blocks_and_locals() {
    let src = r#"
enum Shape { Circle(i32), Named(String), Empty }
fn shapes() -> Generator<Shape> { yield Shape::Circle(3); yield Shape::Named(String::from("sq")); yield Shape::Empty; }
fn describe() -> Generator<String> {
    for s in shapes() {
        match s {
            Shape::Circle(r) => { yield String::from("circle"); yield String::from("again"); println(r); }
            Shape::Named(n) => { yield String::from("named"); println(n); }
            Shape::Empty => { yield String::from("empty"); }
        }
    }
}
fn main() { for s in describe() { println(s); } }
"#;
    assert_eq!(out(src), "circle\nagain\n3\nnamed\nsq\nempty\n");
}

#[test]
fn a_reference_to_a_local_stays_valid_across_a_yield() {
    let src = r#"
struct Pt { x: i32, name: String }
fn refs() -> Generator<i32> {
    let p = Pt { x: 5, name: String::from("ref") };
    let rn = &p;
    yield 10;
    println(rn.x);
    println(rn.name);
    yield p.x;
}
fn main() { for x in refs() { println(x); } }
"#;
    assert_eq!(out(src), "10\n5\nref\n5\n");
}

#[test]
fn nested_generator_calls() {
    let src = "fn evens(limit: i32) -> Generator<i32> { let mut i = 0; while i < limit { if i % 2 == 0 { yield i; } i += 1; } }\nfn outer() -> Generator<i32> { yield -1; for x in evens(7) { yield x * 10; } yield -2; }\nfn main() { for x in outer() { println(x); } }";
    assert_eq!(out(src), "-1\n0\n20\n40\n60\n-2\n");
}

#[test]
fn tuple_items_and_destructuring_in_for() {
    let src = "fn pairs() -> Generator<(i32, String)> { let mut i = 0; while i < 3 { yield (i, String::from(\"p\")); i += 1; } }\nfn main() { for (a, b) in pairs() { println(a); println(b); } }";
    assert_eq!(out(src), "0\np\n1\np\n2\np\n");
}

#[test]
fn generic_generator_function() {
    let src = "fn two<T>(a: T, b: T) -> Generator<T> { yield a; yield b; }\nfn main() { for x in two(String::from(\"u\"), String::from(\"v\")) { println(x); } for x in two(1.5, 2.5) { println(x); } for x in two(7, 8) { println(x); } }";
    assert_eq!(out(src), "u\nv\n1.5\n2.5\n7\n8\n");
}

#[test]
fn for_over_generator_supports_labels_break_continue_and_wildcard() {
    let src = r#"
fn two<T>(a: T, b: T) -> Generator<T> { yield a; yield b; }
fn main() {
    'outer: for a in two(1, 2) {
        for b in two(10, 20) {
            if b == 20 { continue 'outer; }
            println(a * 100 + b);
        }
    }
    for _ in two(String::from("w"), String::from("z")) { println("tick"); }
    for x in two(1, 2) { if x == 1 { continue; } println(x); }
}
"#;
    assert_eq!(out(src), "110\n210\ntick\ntick\n2\n");
}

#[test]
fn early_return_from_inside_a_for_over_a_generator() {
    let src = "fn first_big(limit: i32) -> i32 { for v in nat() { if v > limit { return v; } } return -1; }\nfn nat() -> Generator<i32> { let mut i = 0; loop { yield i; i += 1; } }\nfn main() { println(first_big(4)); }";
    assert_eq!(out(src), "5\n");
}

#[test]
fn parameters_by_value_owned_and_mut() {
    let src = "fn g(mut s: String, n: i32) -> Generator<i32> { s = String::from(\"z\"); println(s); yield n; yield n + 1; }\nfn main() { for x in g(String::from(\"a\"), 4) { println(x); } }";
    assert_eq!(out(src), "z\n4\n5\n");
}

#[test]
fn generator_is_a_value_it_can_be_stored_returned_and_overwritten() {
    let src = r#"
fn g(tag: i32) -> Generator<i32> { yield tag; yield tag + 1; }
struct Holder { g: Generator<i32>, tag: i32 }
fn mk() -> Holder { return Holder { g: g(10), tag: 1 }; }
fn pass() -> Generator<i32> { return g(20); }
fn main() {
    let mut h = mk();
    match h.g.next() { Some(v) => println(v), None => println("none") }
    let mut x = pass();
    x = g(30);
    for v in x { println(v); }
}
"#;
    assert_eq!(out(src), "10\n30\n31\n");
}

#[test]
fn generator_value_is_move_only() {
    let src = "fn g() -> Generator<i32> { yield 1; }\nfn main() { let a = g(); let mut b = a; match b.next() { Some(v) => println(v), None => println(\"none\") } }";
    assert_eq!(out(src), "1\n");
}

#[test]
fn works_in_release_mode_too() {
    let dir = scratch();
    let f = dir.join("prog.mtn");
    std::fs::write(&f, format!("{}fn main() {{ let mut n = 0; for value in fibonacci() {{ println(value); n += 1; if n == 10 {{ break; }} }} }}", FIB)).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("run").arg(&f).arg("--release").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout), "0\n1\n1\n2\n3\n5\n8\n13\n21\n34\n");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- rejections

#[test]
fn rejects_yield_outside_a_generator_function() {
    rejects("fn main() { yield 1; }", "`yield` outside a generator function");
    rejects("fn f() -> i32 { yield 1; return 2; }\nfn main() { }", "`yield` outside a generator function");
    rejects("fn main() { let f = || { yield 1; }; }", "`yield` outside a generator function");
}

#[test]
fn rejects_yield_of_the_wrong_type() {
    rejects("fn g() -> Generator<i32> { yield \"a\"; }\nfn main() { }", "`yield` in a generator that yields `i32`");
}

#[test]
fn rejects_return_with_a_value_in_a_generator() {
    rejects("fn g() -> Generator<i32> { yield 1; return 5; }\nfn main() { }", "`return` with a value inside a generator");
}

#[test]
fn rejects_generator_ending_in_a_value() {
    rejects("fn g() -> Generator<i32> { yield 1; 5 }\nfn main() { }", "the body ends with a value");
}

#[test]
fn rejects_borrow_parameters_naming_the_parameter() {
    rejects("fn g(borrow s: String) -> Generator<i32> { yield 1; }\nfn main() { }", "parameter `s` is passed by `borrow`");
    rejects("fn g(borrow mut s: String) -> Generator<i32> { yield 1; }\nfn main() { }", "parameter `s` is passed by `borrow`");
}

#[test]
fn rejects_question_mark_inside_a_generator_with_a_clear_message() {
    rejects("fn g() -> Generator<i32> { let r: Result<i32, String> = Ok(1); let v = r?; yield v; }\nfn main() { }", "`?` inside a generator is not defined yet");
}

#[test]
fn rejects_use_after_move_of_a_generator() {
    rejects("fn g() -> Generator<i32> { yield 1; }\nfn main() { let a = g(); let b = a; let mut c = a; }", "use of moved value `a`");
    rejects("fn g() -> Generator<i32> { yield 1; }\nfn main() { let g1 = g(); for x in g1 { println(x); } let mut h = g1; }", "use of moved value `g1`");
}

#[test]
fn rejects_yielding_the_same_owned_value_twice() {
    rejects("fn h() -> Generator<String> { let s = String::from(\"a\"); yield s; yield s; }\nfn main() { }", "use of moved value `s`");
}

#[test]
fn rejects_next_on_a_binding_that_is_not_mut() {
    rejects("fn g() -> Generator<i32> { yield 1; }\nfn main() { let h = g(); h.next(); }", "must be declared `mut`");
}

#[test]
fn adapters_are_reported_as_phase_16() {
    rejects("fn g() -> Generator<i32> { yield 1; }\nfn main() { for v in g().take(3) { println(v); } }", "not yet supported by codegen — Phase 16");
    rejects("fn g() -> Generator<i32> { yield 1; }\nfn main() { let mut x = g(); let y = x.take(3); }", "not yet supported by codegen — Phase 16");
}

// ---------------------------------------------------------------- IR

fn ir(src: &str) -> String {
    mtnc::driver::emit_ir(src, &mtnc::codegen::Options { release: false }).unwrap_or_else(|e| panic!("codegen failed: {:?}", e))
}

fn function_ir<'a>(ir: &'a str, name: &str) -> &'a str {
    let start = ir.find(&format!("define ptr @\"{}\"", name)).or_else(|| ir.find(&format!("define ptr @{}", name))).unwrap_or_else(|| panic!("no function {} in:\n{}", name, ir));
    let rest = &ir[start..];
    let end = rest.find("\n}\n").map(|e| e + 3).unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn ir_one_coroutine_frame_per_generator_instance_type() {
    let src = "fn two<T>(a: T, b: T) -> Generator<T> { yield a; yield b; }\nfn main() { for x in two(1, 2) { println(x); } for x in two(3, 4) { println(x); } for s in two(1.5, 2.5) { println(s); } }";
    let text = ir(src);
    // two instances (i32, f64) although three calls: mono de-duplicates
    assert_eq!(text.matches("presplitcoroutine\ndefine ptr").count(), 2, "{}", text);
    for inst in ["mtn_two$i32", "mtn_two$f64"] {
        let f = function_ir(&text, inst);
        assert_eq!(f.matches("@llvm.coro.begin").count(), 1, "{}", f);
        assert_eq!(f.matches("call ptr @__mtn_alloc").count(), 1, "frame allocated once per instance: {}", f);
        assert_eq!(f.matches("call void @__mtn_dealloc").count(), 1, "{}", f);
    }
}

#[test]
fn ir_the_frame_is_freed_on_the_destroy_path_and_never_through_raw_malloc() {
    let text = ir(&format!("{}fn main() {{ let mut g = fibonacci(); match g.next() {{ Some(v) => println(v), None => println(\"none\") }} }}", FIB));
    let f = function_ir(&text, "mtn_fibonacci");
    assert!(f.contains("@llvm.coro.free"), "{}", f);
    assert!(f.contains("@__mtn_dealloc"), "{}", f);
    assert!(!f.contains("@malloc") && !f.contains("@free"), "no raw malloc/free in the generator: {}", f);
    // the generator handle is destroyed (not leaked) when `g` goes out of scope
    assert!(text.contains("define void @__mtn_drop_Generator_u64_(ptr"), "{}", text);
    assert!(text.contains("@llvm.coro.destroy"), "{}", text);
    // the destroy path goes through the same free
    assert!(f.matches("@llvm.coro.suspend").count() >= 3, "initial + yield + final suspension: {}", f);
}

// ---------------------------------------------------------------- memory: valgrind

fn have_valgrind() -> bool {
    Command::new("valgrind").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn valgrind_clean(src: &str, expect_stdout: &str) {
    if !have_valgrind() {
        eprintln!("valgrind not installed: skipping");
        return;
    }
    let dir = scratch();
    let f = dir.join("prog.mtn");
    let exe = dir.join("prog");
    std::fs::write(&f, src).unwrap();
    let b = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let o = Command::new("valgrind")
        .args(["--leak-check=full", "--errors-for-leak-kinds=definite", "--error-exitcode=99", "-q"])
        .arg(&exe)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "valgrind: {}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(String::from_utf8_lossy(&o.stdout), expect_stdout);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn valgrind_stop_midway_holding_string_and_box() {
    let src = r#"
fn words(n: i32) -> Generator<String> {
    let owned = Box::new(n);
    let mut i = 0;
    while i < *owned {
        let keep = String::from("kept");
        yield keep;
        let s = String::from("item");
        yield s;
        i += 1;
    }
}
fn main() {
    let mut g = words(3);
    match g.next() { Some(s) => println(s), None => println("none") }
    match g.next() { Some(s) => println(s), None => println("none") }
    match g.next() { Some(s) => println(s), None => println("none") }
    for s in words(5) { println(s); break; }
}
"#;
    valgrind_clean(src, "kept\nitem\nkept\nkept\n");
}

#[test]
fn valgrind_partially_consumed_rc_is_released_exactly_once() {
    let src = r#"
fn counter(rc: Rc<i32>) -> Generator<i32> { let mine = rc; let mut i = 0; loop { yield i + *mine; i += 1; } }
fn main() {
    let shared = Rc::new(100);
    println(Rc::strongCount(borrow shared));
    let c = counter(shared.clone());
    println(Rc::strongCount(borrow shared));
    let mut k = 0;
    for v in c { println(v); k += 1; if k == 3 { break; } }
    println(Rc::strongCount(borrow shared));
    let never = counter(shared.clone());
    println(Rc::strongCount(borrow shared));
    drop(never);
    println(Rc::strongCount(borrow shared));
    let mut started = counter(shared.clone());
    match started.next() { Some(v) => println(v), None => println("none") }
    println(Rc::strongCount(borrow shared));
}
"#;
    valgrind_clean(src, "1\n2\n100\n101\n102\n1\n2\n1\n100\n2\n");
}

#[test]
fn valgrind_generator_in_struct_overwritten_and_dropped() {
    let src = r#"
fn g(s: String, n: i32) -> Generator<i32> { let t = s; yield n; yield n + 1; println(t); }
struct H { g: Generator<i32>, tag: String }
fn main() {
    let mut h = H { g: g(String::from("a"), 1), tag: String::from("tag") };
    match h.g.next() { Some(v) => println(v), None => println("none") }
    h.g = g(String::from("b"), 5);
    for v in g(String::from("c"), 9) { println(v); }
    let unused = g(String::from("d"), 0);
}
"#;
    valgrind_clean(src, "1\n9\n10\nc\n");
}

#[test]
fn valgrind_temporaries_and_nested_generators_dropped_mid_expression() {
    let src = r#"
fn inner(tag: String) -> Generator<String> { yield tag; yield String::from("more"); }
fn outer() -> Generator<String> {
    for s in inner(String::from("x")) {
        yield s;
    }
    yield String::from("tail");
}
fn main() {
    let mut o = outer();
    match o.next() { Some(s) => println(s), None => println("none") }
    for s in outer() { println(s); break; }
}
"#;
    valgrind_clean(src, "x\nx\n");
}

#[test]
fn valgrind_release_build_is_clean_too() {
    if !have_valgrind() {
        return;
    }
    let dir = scratch();
    let f = dir.join("prog.mtn");
    let exe = dir.join("prog");
    std::fs::write(&f, "fn words(n: i32) -> Generator<String> { let b = Box::new(n); let mut i = 0; while i < *b { yield String::from(\"w\"); i += 1; } }\nfn main() { for s in words(4) { println(s); if true { break; } } let mut g = words(2); g.next(); }").unwrap();
    let b = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("-o").arg(&exe).arg("--release").output().unwrap();
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let o = Command::new("valgrind").args(["--leak-check=full", "--errors-for-leak-kinds=definite", "--error-exitcode=99", "-q"]).arg(&exe).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "w\n");
    let _ = std::fs::remove_dir_all(&dir);
}
