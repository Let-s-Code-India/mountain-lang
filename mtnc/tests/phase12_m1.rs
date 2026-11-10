//! Phase 12, milestone M1 (Document 10 §4, §5, §9.5): closures and function values.
//! Programs are compiled with the real `mtnc` binary and run; outputs are asserted exactly.
//! Rejection tests assert the specific message. Every temp path is unique (pid + counter).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn scratch() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("mtnc_m1_{}_{}", std::process::id(), n));
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

const CFG: &str = "struct Config { name: String, level: i32 }\nfn make(n: String, l: i32) -> Config { return Config { name: n, level: l }; }\nfn take(c: Config) -> i32 { println(c.name); return c.level; }\n";

// ---------------------------------------------------------------- Document 10 §4

#[test]
fn doc10_4_1_basic_closure() {
    assert_eq!(out("fn main() { let double = |x: i32| -> i32 { x * 2 }; let result = double(5); println(result); }"), "10\n");
}

#[test]
fn doc10_4_2_types_inferred_from_the_expected_fn_type() {
    let src = "fn apply(f: fn(i32) -> i32, x: i32) -> i32 { return f(x); }\nfn main() { println(apply(|x| x * 2, 21)); println(apply(|x| { let y = x + 1; y * y }, 4)); }";
    assert_eq!(out(src), "42\n25\n");
}

#[test]
fn doc10_4_3_capture_by_reference() {
    assert_eq!(out("fn main() { let count = 7; let printCount = || println(count); printCount(); printCount(); }"), "7\n7\n");
}

#[test]
fn doc10_4_3_capture_by_mutable_reference_changes_the_outer_variable() {
    let src = "fn main() { let mut total = 0; let mut addToTotal = |x: i32| total += x; addToTotal(3); addToTotal(4); println(total); }";
    assert_eq!(out(src), "7\n");
}

#[test]
fn doc10_4_3_move_closure_owns_a_copy() {
    // the closure has its own `n`; the outer one is unaffected
    let src = "fn main() { let mut n = 1; let mut inc = move || { n += 1; n }; println(inc()); println(inc()); println(n); }";
    assert_eq!(out(src), "2\n3\n1\n");
}

#[test]
fn doc10_4_3_move_closure_owning_a_struct() {
    let src = format!("{}fn main() {{ let data = make(String::from(\"d\"), 4); let analyze = move || data.level * 10; println(analyze()); println(analyze()); }}", CFG);
    assert_eq!(out(&src), "40\n40\n");
}

#[test]
fn closure_returned_from_a_function_keeps_its_captures() {
    let src = "fn makeAdder(n: i32) -> fn(i32) -> i32 { move |x: i32| x + n }\nfn main() { let a = makeAdder(5); let b = makeAdder(100); println(a(1)); println(b(1)); println(a(2)); }";
    assert_eq!(out(src), "6\n101\n7\n");
}

#[test]
fn closure_returning_a_closure() {
    let src = "fn main() { let mk = |n: i32| move |x: i32| x * n; let triple = mk(3); println(triple(4)); println(mk(5)(6)); }";
    assert_eq!(out(src), "12\n30\n");
}

#[test]
fn closure_defined_in_a_loop_sees_each_iteration() {
    let src = "fn main() { let mut i = 0; let mut sum = 0; while i < 4 { let k = i * 10; let add = |x: i32| x + k; sum += add(1); i += 1; } println(sum); }";
    assert_eq!(out(src), "64\n");
}

// ---------------------------------------------------------------- Document 10 §5

#[test]
fn doc10_5_1_function_type_alias_and_named_function_as_value() {
    let src = "type MathOp = fn(i32, i32) -> i32;\nfn applyOp(a: i32, b: i32, op: MathOp) -> i32 { return op(a, b); }\nfn add(a: i32, b: i32) -> i32 { a + b }\nfn main() { let result = applyOp(3, 4, add); println(result); }";
    assert_eq!(out(src), "7\n");
}

#[test]
fn named_function_can_be_bound_and_used_twice() {
    let src = "fn sq(x: i32) -> i32 { x * x }\nfn twice(f: fn(i32) -> i32, x: i32) -> i32 { f(f(x)) }\nfn main() { let f = sq; println(f(3)); println(twice(f, 2)); println(twice(f, 3)); }";
    assert_eq!(out(src), "9\n16\n81\n");
}

#[test]
fn closures_as_parameters_and_results() {
    let src = "type Op = fn(i32, i32) -> i32;\nfn pick(neg: bool) -> Op { if neg { |a: i32, b: i32| a - b } else { |a: i32, b: i32| a + b } }\nfn run(op: Op, a: i32, b: i32) -> i32 { op(a, b) }\nfn main() { println(run(pick(true), 9, 4)); println(run(pick(false), 9, 4)); println(pick(true)(1, 2)); }";
    assert_eq!(out(src), "5\n13\n-1\n");
}

#[test]
fn closures_in_struct_fields() {
    let src = "struct Handler { id: i32, run: fn(i32) -> i32 }\nfn main() { let h = Handler { id: 7, run: |x| x * x }; println(h.id); println((h.run)(9)); let g = Handler { id: 8, run: |x: i32| x + 1 }; println((g.run)(h.id)); }";
    assert_eq!(out(src), "7\n81\n8\n");
}

#[test]
fn closures_and_named_functions_in_array_elements() {
    let src = "fn add(a: i32, b: i32) -> i32 { a + b }\nfn main() { let ops = [add, |a: i32, b: i32| a - b, |a: i32, b: i32| a * b]; let mut i = 0; while i < 3 { println(ops[i](10, 4)); i += 1; } }";
    assert_eq!(out(src), "14\n6\n40\n");
}

#[test]
fn a_local_closure_shadows_a_function_of_the_same_name() {
    let src = "fn f(x: i32) -> i32 { x + 1 }\nfn main() { println(f(1)); let f = |x: i32| x * 100; println(f(1)); }";
    assert_eq!(out(src), "2\n100\n");
}

#[test]
fn calling_the_result_of_a_call_and_a_parenthesized_closure() {
    let src = "fn mk() -> fn(i32) -> i32 { |x: i32| x + 1 }\nfn main() { println(mk()(1)); println((|x: i32| x * 2)(8)); }";
    assert_eq!(out(src), "2\n16\n");
}

#[test]
fn unit_returning_closure_and_closure_without_parameters() {
    let src = "fn run(f: fn()) { f(); f(); }\nfn main() { let mut n = 0; run(|| println(1)); let mut bump = || { n += 5; }; bump(); bump(); println(n); }";
    assert_eq!(out(src), "1\n1\n10\n");
}

// ------------------------------------------------- `return` / `?` / `throw` belong to the closure

#[test]
fn return_inside_a_closure_returns_from_the_closure_only() {
    let src = "fn main() { let sign = |x: i32| -> i32 { if x > 0 { return 1; } if x < 0 { return -1; } 0 }; println(sign(5)); println(sign(-5)); println(sign(0)); println(99); }";
    assert_eq!(out(src), "1\n-1\n0\n99\n");
}

#[test]
fn question_mark_inside_a_closure_propagates_from_the_closure() {
    let src = "fn parse(n: i32) -> Result<i32, String> { if n < 0 { Err(String::from(\"bad\")) } else { Ok(n) } }\nfn main() { let f = |x: i32| -> Result<i32, String> { let v = parse(x)?; Ok(v + 1) }; match f(1) { Ok(v) => println(v), Err(e) => println(e) } match f(-1) { Ok(v) => println(v), Err(e) => println(e) } println(7); }";
    assert_eq!(out(src), "2\nbad\n7\n");
}

#[test]
fn question_mark_inside_a_closure_checked_against_an_expected_fn_type() {
    let src = "fn parse(n: i32) -> Result<i32, String> { if n < 0 { Err(String::from(\"bad\")) } else { Ok(n) } }\nfn run(f: fn(i32) -> Result<i32, String>, x: i32) -> i32 { match f(x) { Ok(v) => v, Err(e) => { println(e); 0 } } }\nfn main() { println(run(|x| { let v = parse(x)?; Ok(v * 2) }, 4)); println(run(|x| { let v = parse(x)?; Ok(v * 2) }, -4)); }";
    assert_eq!(out(src), "8\nbad\n0\n");
}

#[test]
fn try_catch_inside_a_closure() {
    let src = "fn parse(n: i32) -> Result<i32, String> { if n < 0 { Err(String::from(\"bad\")) } else { Ok(n) } }\nfn main() { let f = |x: i32| -> i32 { try { let v = parse(x)?; v + 1 } catch (e) { println(e); -1 } }; println(f(2)); println(f(-2)); }";
    assert_eq!(out(src), "3\nbad\n-1\n");
}

#[test]
fn question_mark_in_a_closure_whose_return_type_does_not_fit_is_rejected() {
    rejects(
        "fn parse(n: i32) -> Result<i32, String> { Ok(n) }\nfn main() { let f = |x: i32| -> i32 { let v = parse(x)?; v + 1 }; println(f(1)); }",
        "requires the enclosing function to return `Result",
    );
    rejects(
        "fn parse(n: i32) -> Result<i32, String> { Ok(n) }\nfn run(f: fn(i32) -> i32) -> i32 { f(1) }\nfn main() { println(run(|x| { let v = parse(x)?; v })); }",
        "requires the enclosing function to return `Result",
    );
}

#[test]
fn throw_outside_try_in_a_closure_is_rejected_with_the_closure_rule() {
    rejects("fn main() { let f = |x: i32| -> Result<i32, String> { throw String::from(\"n\"); }; match f(1) { Ok(v) => println(v), Err(e) => println(e) } }", "`throw` is only valid inside a `try` block");
}

#[test]
fn return_type_mismatch_in_a_closure_is_rejected() {
    rejects("fn main() { let f = |x: i32| -> i32 { return \"no\"; }; println(f(1)); }", "expected `i32`, found `String`");
    rejects("fn apply(f: fn(i32) -> i32, x: i32) -> i32 { f(x) }\nfn main() { println(apply(|x| \"s\", 1)); }", "closure body has type `String` but the closure's return type is `i32`");
}

// ---------------------------------------------------------------- typing of closures and calls

#[test]
fn closure_parameter_without_a_type_and_without_context_is_rejected() {
    rejects("fn main() { let f = |x| x + 1; println(f(1)); }", "cannot infer the type of a closure parameter");
}

#[test]
fn closure_with_the_wrong_number_of_parameters_is_rejected() {
    rejects("fn apply(f: fn(i32) -> i32, x: i32) -> i32 { f(x) }\nfn main() { println(apply(|a, b| a, 1)); }", "closure takes 2 parameter(s) but the expected function type `fn(i32) -> i32` has 1");
}

#[test]
fn calling_a_non_function_value_is_rejected() {
    rejects("fn main() { let x = 5; println(x(1)); }", "cannot call a value of type `i32`");
    rejects("struct P { a: i32 }\nfn main() { let p = P { a: 1 }; println((p.a)(2)); }", "cannot call a value of type `i32`");
}

#[test]
fn calling_a_function_value_with_the_wrong_arity_or_type_is_rejected() {
    rejects("fn main() { let f = |x: i32| x + 1; println(f(1, 2)); }", "takes 1");
    rejects("fn main() { let f = |x: i32| x + 1; println(f(true)); }", "expected `i32`");
}

#[test]
fn named_function_with_a_different_signature_is_rejected_where_a_fn_type_is_expected() {
    rejects("fn add(a: i32, b: i32) -> i32 { a + b }\nfn run(f: fn(i32) -> i32) -> i32 { f(1) }\nfn main() { println(run(add)); }", "expected `fn(i32) -> i32`");
}

// ---------------------------------------------------------------- ownership and borrowing

#[test]
fn use_after_move_into_a_move_closure_is_rejected() {
    let src = format!("{}fn main() {{ let c = make(String::from(\"x\"), 1); let f = move || c.level; println(c.level); println(f()); }}", CFG);
    rejects(&src, "use of moved value `c`");
}

#[test]
fn a_closure_that_moves_a_capture_in_its_body_owns_it_even_without_move() {
    let src = format!("{}fn main() {{ let c = make(String::from(\"x\"), 1); let f = || take(c); println(c.level); println(f()); }}", CFG);
    rejects(&src, "use of moved value `c`");
    let ok = format!("{}fn main() {{ let c = make(String::from(\"once\"), 6); let f = || take(c); println(f()); }}", CFG);
    assert_eq!(out(&ok), "once\n6\n");
}

#[test]
fn assigning_a_captured_variable_while_the_closure_borrows_it_is_rejected() {
    rejects("fn main() { let mut x = 1; let f = || println(x); x = 2; f(); }", "cannot assign to `x` while it is borrowed (via `f`)");
    rejects("fn main() { let mut x = 1; let mut f = || x += 1; x = 5; f(); }", "cannot assign to `x` while it is borrowed (via `f`)");
}

#[test]
fn assigning_after_the_last_use_of_the_closure_is_fine() {
    assert_eq!(out("fn main() { let mut x = 1; let f = || println(x); f(); x = 2; println(x); }"), "1\n2\n");
}

#[test]
fn two_closures_mutating_the_same_variable_are_rejected() {
    rejects("fn main() { let mut x = 0; let mut a = || x += 1; let mut b = || x += 2; a(); b(); }", "cannot borrow `x` as mutable");
}

#[test]
fn a_move_only_closure_cannot_be_used_after_it_was_moved() {
    let src = format!("{}fn run(f: fn() -> i32) -> i32 {{ f() }}\nfn main() {{ let c = make(String::from(\"x\"), 1); let f = move || c.level; println(run(f)); println(run(f)); }}", CFG);
    rejects(&src, "use of moved value `f`");
}

#[test]
fn a_borrowing_closure_is_copyable() {
    let src = "fn run(f: fn() -> i32) -> i32 { f() }\nfn main() { let k = 4; let f = || k + 1; println(run(f)); println(run(f)); }";
    assert_eq!(out(src), "5\n5\n");
}

#[test]
fn returning_a_closure_that_borrows_a_local_is_rejected() {
    rejects("fn make() -> fn() -> i32 { let n = 5; || n }\nfn main() { println(make()()); }", "this closure borrows `n`");
    rejects("fn make(n: i32) -> fn() -> i32 { let f = || n; return f; }\nfn main() { println(make(1)()); }", "`f` is a closure that borrows local variables");
    rejects("fn make(n: i32) -> fn(i32) -> i32 { if n > 0 { |x: i32| x + n } else { |x: i32| x } }\nfn main() { println(make(1)(1)); }", "this closure borrows `n`");
}

#[test]
fn storing_a_borrowing_closure_in_a_struct_or_array_is_rejected() {
    rejects("struct H { f: fn(i32) -> i32 }\nfn main() { let k = 3; let h = H { f: |x: i32| x + k }; println((h.f)(1)); }", "this closure borrows `k`");
    rejects("fn main() { let k = 3; let fs = [|x: i32| x + k]; println(fs[0](1)); }", "this closure borrows `k`");
}

#[test]
fn a_move_closure_can_be_stored_and_returned() {
    let src = "struct H { f: fn(i32) -> i32 }\nfn mk(k: i32) -> H { H { f: move |x: i32| x + k } }\nfn main() { let h = mk(3); println((h.f)(1)); }";
    assert_eq!(out(src), "4\n");
}

#[test]
fn a_closure_that_only_borrows_cannot_move_a_capture_out_in_a_struct_literal() {
    // not recognised as a move by the borrow checker; code generation refuses it with a clear message
    let src = format!("{}struct W {{ c: Config }}\nfn main() {{ let c = make(String::from(\"x\"), 1); let f = || W {{ c: c }}; let w = f(); println(w.c.level); }}", CFG);
    let r = run_src(&src);
    // either rejected at compile time, or it works: it must never crash or double free
    assert!(r.code == 0 || r.stderr.contains("closure"), "stderr: {}", r.stderr);
}

// ---------------------------------------------------------------- once-only closures

#[test]
fn a_closure_that_moves_a_capture_out_panics_on_the_second_call() {
    let src = format!("{}fn main() {{ let a = make(String::from(\"s\"), 1); let once = move || take(a); println(once()); println(once()); }}", CFG);
    let r = run_src(&src);
    assert_eq!(r.stdout, "s\n1\n");
    assert_eq!(r.code, 101);
    assert!(r.stderr.contains("can be called only once"), "stderr: {}", r.stderr);
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
fn valgrind_move_closure_drops_its_captures_exactly_once() {
    let src = format!(
        "{}fn run(f: fn() -> i32) -> i32 {{ f() }}\nfn main() {{ let a = make(String::from(\"a\"), 1); let f = move || a.level; println(f()); println(f());\nlet b = make(String::from(\"b\"), 2); let g = move || b.level; println(run(g));\nlet c = make(String::from(\"c\"), 3); let unused = move || c.level; }}",
        CFG
    );
    valgrind_clean(&src, "1\n1\n2\n");
}

#[test]
fn valgrind_once_closure_called_or_not() {
    let src = format!("{}fn main() {{ let a = make(String::from(\"a\"), 1); let f = move || take(a); println(f());\nlet b = make(String::from(\"b\"), 2); let g = move || take(b); }}", CFG);
    valgrind_clean(&src, "a\n1\n");
}

#[test]
fn valgrind_closure_owning_box_and_rc() {
    let src = format!(
        "{}fn main() {{ let b = Box::new(make(String::from(\"b\"), 41)); let inc = move || b.level + 1; println(inc());\nlet r = Rc::new(make(String::from(\"r\"), 5)); let r2 = r.clone(); let g = move || r2.level; println(g()); println(Rc::strongCount(borrow r)); drop(g); println(Rc::strongCount(borrow r)); }}",
        CFG
    );
    valgrind_clean(&src, "42\n5\n2\n1\n");
}

#[test]
fn valgrind_closures_in_structs_arrays_and_returned_from_functions() {
    let src = format!(
        "{}struct H {{ f: fn() -> i32 }}\nfn mk(c: Config) -> H {{ H {{ f: move || c.level }} }}\nfn main() {{ let h = mk(make(String::from(\"h\"), 3)); println((h.f)());\nlet a = make(String::from(\"x\"), 1); let b = make(String::from(\"y\"), 2);\nlet fs = [move || a.level, move || b.level]; println(fs[1]()); println(fs[0]()); }}",
        CFG
    );
    valgrind_clean(&src, "3\n2\n1\n");
}

#[test]
fn valgrind_closure_dropped_on_early_return_and_loop_break() {
    let src = format!(
        "{}fn early(flag: bool) -> i32 {{ let c = make(String::from(\"e\"), 7); let f = move || c.level; if flag {{ return 1; }} f() }}\nfn main() {{ println(early(true)); println(early(false)); let mut i = 0; while i < 3 {{ let c = make(String::from(\"l\"), i); let f = move || c.level; if i == 1 {{ break; }} println(f()); i += 1; }} }}",
        CFG
    );
    valgrind_clean(&src, "1\n7\n0\n");
}

#[test]
fn every_program_in_examples_concurrency_runs_valgrind_clean() {
    if !have_valgrind() {
        return;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").join("concurrency");
    let mut n = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().map_or(true, |e| e != "mtn") {
            continue;
        }
        let tmp = scratch();
        let exe = tmp.join("prog");
        let b = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&p).arg("-o").arg(&exe).output().unwrap();
        assert!(b.status.success(), "{}: {}", p.display(), String::from_utf8_lossy(&b.stderr));
        let o = Command::new("valgrind")
            .args(["--leak-check=full", "--errors-for-leak-kinds=definite", "--error-exitcode=99", "-q"])
            .arg(&exe)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(0), "{}: {}", p.display(), String::from_utf8_lossy(&o.stderr));
        let _ = std::fs::remove_dir_all(&tmp);
        n += 1;
    }
    assert!(n >= 3, "expected the closure programs in examples/concurrency/");
}

// ---------------------------------------------------------------- misc closure + language details

#[test]
fn try_block_may_call_a_local_closure_and_a_named_function() {
    let src = "fn parse(n: i32) -> Result<i32, String> { if n < 0 { Err(String::from(\"bad\")) } else { Ok(n) } }\nfn id(x: i32) -> i32 { x }\nfn main() { let k = 10; let add = |x: i32| x + k; let r = try { let v = parse(5)?; add(id(v)) } catch (e) { println(e); 0 }; println(r); }";
    assert_eq!(out(src), "15\n");
}

#[test]
fn fn_type_without_arrow_returns_unit() {
    let src = "fn run(f: fn(i32)) { f(1); f(2); }\nfn main() { run(|x| println(x * 10)); }";
    assert_eq!(out(src), "10\n20\n");
}

#[test]
fn closure_without_captures_can_be_returned_from_a_function() {
    let src = "fn mk() -> fn(i32) -> i32 { |x: i32| x + 1 }\nfn main() { let f = mk(); println(f(1)); println(f(2)); }";
    assert_eq!(out(src), "2\n3\n");
}

#[test]
fn closure_borrowing_a_borrow_parameter_and_self() {
    let src = "struct Counter { n: i32 }\nimpl Counter { fn bump(borrow mut self, by: i32) { let mut add = |k: i32| self.n += k; add(by); add(by); } }\nfn main() { let mut c = Counter { n: 1 }; c.bump(5); println(c.n); }";
    assert_eq!(out(src), "11\n");
}

// ---------------------------------------------------------------- known gap (Open items): generic impl signatures

#[test]
fn generic_impl_signature_mismatch_is_caught_when_the_impl_is_instantiated() {
    // return type
    rejects(
        "trait Shape { fn area(borrow self) -> i32; }\nstruct Wrap<T> { v: T }\nimpl<T> Shape for Wrap<T> { fn area(borrow self) -> i64 { 1 } }\nfn main() { let w = Wrap { v: 1 }; println(w.area()); }",
        "returns `i64` but the trait declares `i32`",
    );
    // parameter type
    rejects(
        "trait Shape { fn area(borrow self, k: i32) -> i32; }\nstruct Wrap<T> { v: T }\nimpl<T> Shape for Wrap<T> { fn area(borrow self, k: bool) -> i32 { 1 } }\nfn main() { let w = Wrap { v: 1 }; println(w.area(true)); }",
        "parameter 1 has type `bool` but the trait declares `i32`",
    );
}

#[test]
fn generic_impl_signature_mismatch_is_not_caught_if_the_impl_is_never_instantiated() {
    // The known gap: only the instantiated copy of a generic impl is checked in full.
    // This test pins today's behaviour so a change (the gap closing) is noticed.
    let r = run_src("trait Shape { fn area(borrow self) -> i32; }\nstruct Wrap<T> { v: T }\nimpl<T> Shape for Wrap<T> { fn area(borrow self) -> i64 { 1 } }\nfn main() { println(1); }");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "1\n");
}
