//! Phase 11c C1: carry-over fixes (immutable reassignment, tail calls through `borrow`, generic methods with own `<U>`).
use std::process::Command;

/// Unique per call: tests run in parallel threads of one process, so uniqueness must never come from the source text.
static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn tmp(src: &str, tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mtn_c1_{}_{}_{}", std::process::id(), tag, COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("p.mtn");
    std::fs::write(&f, src).unwrap();
    f
}

fn run_args(src: &str, args: &[&str]) -> (String, i32) {
    let f = tmp(src, "run");
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("run").arg(&f).args(args).output().unwrap();
    (String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr), out.status.code().unwrap_or(-1))
}

fn run(src: &str) -> (String, i32) {
    run_args(src, &[])
}

fn rejected(src: &str, needle: &str) {
    let (o, c) = run(src);
    assert!(c != 0 && o.contains(needle), "expected `{}`, got ({}): {}", needle, c, o);
    let f = tmp(src, "build");
    let exe = f.with_file_name("out_exe");
    let _ = std::fs::remove_file(&exe);
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(!out.status.success() && !exe.exists(), "build must fail without a binary: {:?}", out);
}

fn ok(src: &str, want: &str) {
    let (o, c) = run(src);
    assert_eq!((o.as_str(), c), (want, 0), "program output");
}

// ---------- item 2: immutable bindings ----------

#[test]
fn rejects_reassigning_a_let() {
    rejected("fn main() { let x: i32 = 1; x = 2; println(x); }", "not declared `mut`");
}

#[test]
fn rejects_compound_assignment_to_a_let() {
    rejected("fn main() { let x: i32 = 1; x += 2; println(x); }", "not declared `mut`");
}

#[test]
fn rejects_field_assignment_on_an_immutable_struct() {
    rejected("struct P { x: i32 } fn main() { let p = P { x: 1 }; p.x = 5; println(p.x); }", "not declared `mut`");
}

#[test]
fn rejects_index_assignment_on_an_immutable_array() {
    rejected("fn main() { let a: [i32; 2] = [1, 2]; a[0] = 5; println(a[0]); }", "not declared `mut`");
}

#[test]
fn rejects_borrow_mut_of_an_immutable_let() {
    rejected("fn bump(borrow mut n: i32) { n += 1; } fn main() { let x: i32 = 1; bump(borrow mut x); println(x); }", "not declared `mut`");
}

#[test]
fn rejects_assigning_a_by_value_parameter() {
    rejected("fn f(n: i32) -> i32 { n = 5; return n; } fn main() { println(f(1)); }", "not declared `mut`");
}

#[test]
fn rejects_assigning_through_an_immutable_borrow_parameter() {
    rejected("struct P { x: i32 } fn f(borrow p: P) { p.x = 5; } fn main() { let mut p = P { x: 1 }; f(borrow p); println(p.x); }", "not declared `mut`");
}

#[test]
fn rejects_assigning_a_for_loop_variable() {
    rejected("fn main() { for i in 0..3 { i = 5; println(i); } }", "not declared `mut`");
}

#[test]
fn rejects_assigning_a_match_pattern_binding() {
    rejected("fn main() { let o: Option<i32> = Some(1); match o { Some(v) => { v = 2; println(v); } None => {} } }", "not declared `mut`");
}

#[test]
fn rejects_assigning_a_destructured_let_binding() {
    rejected("fn main() { let (a, b) = (1, 2); a = 5; println(a + b); }", "not declared `mut`");
}

#[test]
fn deferred_initialisation_is_a_first_assignment() {
    ok("fn main() { let x: i32; x = 5; println(x); }", "5\n");
}

#[test]
fn deferred_initialisation_on_both_branches() {
    ok("fn main() { let c: bool = true; let x: i32; if c { x = 1; } else { x = 2; } println(x); }", "1\n");
}

#[test]
fn rejects_a_second_assignment_after_deferred_initialisation() {
    rejected("fn main() { let x: i32; x = 5; x = 6; println(x); }", "twice");
}

#[test]
fn rejects_deferred_assignment_inside_a_loop() {
    rejected("fn main() { let x: i32; for i in 0..3 { x = i; } println(1); }", "loop or closure");
}

#[test]
fn mutable_bindings_still_work() {
    ok("struct P { x: i32 } fn bump(borrow mut n: i32) { n += 1; } fn main() { let mut a: i32 = 1; a = 2; a += 3; let mut p = P { x: 1 }; p.x = 9; bump(borrow mut a); for mut i in 0..1 { i = 7; println(i); } println(a); println(p.x); }", "7\n6\n9\n");
}

// ---------- item 1: guaranteed tail call through `borrow` ----------

const SWAP_PAIRS: &str = "struct Pair { a: i64, b: i64 }
fn count(borrow x: Pair, borrow y: Pair, n: i64, acc: i64) -> i64 {
    if n == 0 { return acc; }
    return count(borrow y, borrow x, n - 1, acc + x.a);
}
fn main() {
    let p = Pair { a: 1, b: 0 };
    let q = Pair { a: 2, b: 0 };
    println(count(borrow p, borrow q, 1000000, 0));
}";

#[test]
fn tail_call_with_swapped_borrow_places_is_a_loop_in_debug() {
    ok(SWAP_PAIRS, "1500000\n");
}

#[test]
fn tail_call_with_swapped_borrow_places_is_a_loop_in_release() {
    let (o, c) = run_args(SWAP_PAIRS, &["--release"]);
    assert_eq!((o.as_str(), c), ("1500000\n", 0));
}

#[test]
fn tail_call_with_borrow_mut_places_writes_the_right_owner() {
    ok("struct Pair { a: i64, b: i64 }
fn fill(borrow mut x: Pair, borrow mut y: Pair, n: i64) {
    if n == 0 { return; }
    x.a += 1;
    fill(borrow mut y, borrow mut x, n - 1);
}
fn main() {
    let mut p = Pair { a: 0, b: 0 };
    let mut q = Pair { a: 100, b: 0 };
    fill(borrow mut p, borrow mut q, 1000000);
    println(p.a);
    println(q.a);
}", "500000\n500100\n");
}

#[test]
fn tail_method_call_through_borrow_self_and_borrow_argument() {
    ok("struct Pair { a: i64, b: i64 }
impl Pair {
    fn walk(borrow self, borrow other: Pair, n: i64, acc: i64) -> i64 {
        if n == 0 { return acc; }
        return other.walk(borrow self, n - 1, acc + self.a);
    }
}
fn main() {
    let p = Pair { a: 1, b: 0 };
    let q = Pair { a: 2, b: 0 };
    println(p.walk(borrow q, 1000000, 0));
}", "1500000\n");
}

#[test]
fn tail_call_with_a_field_of_a_borrow_parameter() {
    ok("struct Inner { v: i64 }
struct Outer { left: Inner, right: Inner }
fn go(borrow i: Inner, borrow o: Outer, n: i64, acc: i64) -> i64 {
    if n == 0 { return acc; }
    return go(borrow o.right, borrow o, n - 1, acc + i.v);
}
fn main() {
    let o = Outer { left: Inner { v: 3 }, right: Inner { v: 5 } };
    println(go(borrow o.left, borrow o, 1000000, 0));
}", "4999998\n");
}

#[test]
fn tail_call_passing_a_local_by_borrow_is_a_clear_error() {
    rejected("struct Pair { a: i64, b: i64 }
fn f(borrow x: Pair, n: i64) -> i64 {
    let t = Pair { a: 1, b: 2 };
    if n == 0 { return x.a; }
    return f(borrow t, n - 1);
}
fn main() { let p = Pair { a: 1, b: 0 }; println(f(borrow p, 3)); }", "guaranteed tail call");
}

#[test]
fn tail_call_passing_a_local_reference_variable_is_a_clear_error() {
    rejected("struct Pair { a: i64, b: i64 }
fn f(borrow x: Pair, n: i64) -> i64 {
    let t = Pair { a: 1, b: 2 };
    let r = borrow t;
    if n == 0 { return x.a; }
    return f(r, n - 1);
}
fn main() { let p = Pair { a: 1, b: 0 }; println(f(borrow p, 3)); }", "guaranteed tail call");
}

#[test]
fn non_tail_borrow_recursion_may_borrow_locals() {
    ok("struct Pair { a: i64, b: i64 }
fn f(borrow x: Pair, n: i64) -> i64 {
    let t = Pair { a: n, b: 0 };
    if n == 0 { return x.a; }
    return 1 + f(borrow t, n - 1);
}
fn main() { let p = Pair { a: 7, b: 0 }; println(f(borrow p, 3)); }", "4\n");
}

// ---------- item 3: generic method with its own `<U>` ----------

#[test]
fn generic_method_in_a_plain_impl_two_instances() {
    ok("struct Foo { v: i32 }
impl Foo {
    fn pick<U>(self, u: U) -> U { return u; }
}
fn main() { let f = Foo { v: 1 }; println(f.pick(2.5)); println(f.pick(7)); println(f.pick(true)); }", "2.5\n7\ntrue\n");
}

#[test]
fn generic_method_with_own_params_in_generic_impl() {
    ok("struct Box2<T> { v: T }
impl<T> Box2<T> {
    fn pairWith<U>(self, u: U) -> U { return u; }
    fn first(self) -> T { return self.v; }
}
fn main() {
    let b = Box2 { v: 1 };
    let c = Box2 { v: 2.5 };
    println(b.pairWith(2.5));
    println(b.pairWith(9));
    println(c.pairWith(true));
    println(b.first());
    println(c.first());
}", "2.5\n9\ntrue\n1\n2.5\n");
}

#[test]
fn generic_method_using_both_the_impl_and_its_own_parameter() {
    ok("struct Holder<T> { v: T }
impl<T> Holder<T> {
    fn swapIn<U>(self, u: U) -> Holder<U> { return Holder { v: u }; }
}
fn main() {
    let h = Holder { v: 1 };
    let g = h.swapIn(2.5);
    println(g.v);
}", "2.5\n");
}

#[test]
fn generic_method_with_a_bound() {
    ok("trait Scale { fn scale(self, k: i32) -> i32; }
impl Scale for i32 { fn scale(self, k: i32) -> i32 { return self * k; } }
struct Mul { k: i32 }
impl Mul {
    fn apply<U: Scale>(self, x: U) -> i32 { return x.scale(self.k); }
}
fn main() { let m = Mul { k: 3 }; println(m.apply(5)); }", "15\n");
}

#[test]
fn generic_method_bound_is_checked_at_the_call() {
    rejected("trait Scale { fn scale(self, k: i32) -> i32; }
impl Scale for i32 { fn scale(self, k: i32) -> i32 { return self * k; } }
struct Mul { k: i32 }
impl Mul {
    fn apply<U: Scale>(self, x: U) -> i32 { return x.scale(self.k); }
}
fn main() { let m = Mul { k: 3 }; println(m.apply(true)); }", "does not satisfy bound");
}

#[test]
fn generic_method_through_borrow_mut_self() {
    ok("struct Counter { n: i32 }
impl Counter {
    fn addAndGet<U>(borrow mut self, by: i32, tag: U) -> U { self.n += by; return tag; }
}
fn main() {
    let mut c = Counter { n: 1 };
    let t = c.addAndGet(4, 2.5);
    println(t);
    println(c.n);
}", "2.5\n5\n");
}
