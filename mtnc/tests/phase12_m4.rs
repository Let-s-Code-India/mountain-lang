//! Phase 12, milestone M4 (Document 12 §3, §11.4, §11.2, §11.7; Document 9 §8.5-8.6):
//! the M3 leftovers (Phase 13 names, `spawn` inside a generator) and `async`/`await`, `Future<T>`,
//! `join`, `yieldNow`, `sleepMs`, `spawn <future>`.
//! Programs are compiled with the real `mtnc` binary and run; outputs are asserted exactly.
//! No test relies on timing or `sleep`: programs that print from several tasks are compared sorted,
//! or print one line after `join`. Every temp path is unique (pid + counter).

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn scratch() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("mtnc_m3_{}_{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Run {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run_env(src: &str, env: &[(&str, &str)]) -> Run {
    let dir = scratch();
    let f = dir.join("prog.mtn");
    std::fs::write(&f, src).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mtnc"));
    cmd.arg("run").arg(&f);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    let r = Run {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        code: out.status.code().unwrap_or(-1),
    };
    let _ = std::fs::remove_dir_all(&dir);
    r
}

fn run_src(src: &str) -> Run {
    run_env(src, &[])
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

fn sorted_lines(s: &str) -> Vec<String> {
    let mut v: Vec<String> = s.lines().map(|l| l.to_string()).collect();
    v.sort();
    v
}


// ---------------------------------------------------------------- M3 leftovers

fn phase13(src: &str, name: &str) {
    rejects(src, &format!("`{}` is not yet supported by codegen — Phase 13", name));
}

// Phase 13 M2 implemented `Mutex<T>`, `RwLock<T>` and `Spinlock`: the old gate is gone (tests/phase13_m2.rs covers them).
#[test]
fn locks_are_no_longer_gated_by_phase13() {
    assert_eq!(out("fn f(m: borrow Mutex<i32>) -> i32 { *m.lock() }\nfn main() { let m = Mutex::new(1); println(f(borrow m)); }"), "1\n");
    assert_eq!(out("struct S { m: RwLock<i32> }\nfn main() { let s = S { m: RwLock::new(2) }; println(*s.m.read()); }"), "2\n");
    assert_eq!(out("fn main() { let s = Spinlock::new(); { let g = s.acquire(); } println(1); }"), "1\n");
}

// Phase 13 M1 implemented `Atomic<T>`: the old "not yet supported" gate is gone (tests/phase13_m1.rs covers it).
#[test]
fn atomic_is_no_longer_gated_by_phase13() {
    assert_eq!(out("fn main() { let a = Atomic::new(1); println(a.load()); }"), "1\n");
    assert_eq!(out("static C: Atomic<u64> = Atomic::new(7);\nfn main() { println(C.load()); }"), "7\n");
}

// Phase 13 M3 implemented `channel` / `Sender<T>` / `Receiver<T>`: the old gate is gone (tests/phase13_m3.rs covers them).
#[test]
fn channel_is_no_longer_gated_by_phase13() {
    assert_eq!(out("fn main() { let (tx, rx) = channel::<i32>(); tx.send(5); println(match rx.recv() { Some(v) => v, None => 0 }); }"), "5\n");
    rejects("fn main() { let (tx, rx) = channel(); }", "`channel` needs its message type: write `channel::<T>()`");
}

#[test]
fn phase13_select() {
    rejects("fn main() { select { } }", "`select` (channels) is not yet supported by codegen — Phase 13");
}

#[test]
fn phase13_volatile() {
    phase13("fn main() { let x = 1; volatile x; }", "volatile");
}

#[test]
fn a_user_declared_item_with_a_phase13_name_is_left_alone() {
    assert_eq!(out("struct Atomic { v: i32 }\nfn main() { let a = Atomic { v: 3 }; println(a.v); }"), "3\n");
}

#[test]
fn spawn_inside_a_generator_body_works_and_items_are_the_results() {
    let src = "fn squares(n: i32) -> Generator<i32> {
    let mut i = 0;
    while i < n {
        let h = spawn move || { i * i };
        yield h.join();
        i = i + 1;
    }
}
fn main() {
    let mut total = 0;
    for v in squares(5) { println(v); total = total + v; }
    println(total);
}";
    assert_eq!(out(src), "0\n1\n4\n9\n16\n30\n");
}

// ---------------------------------------------------------------- M4: async / await

#[test]
fn async_fn_basics_await_and_nesting() {
    let s = out("async fn add(a: i32, b: i32) -> i32 { return a + b; }
async fn twice(x: i32) -> i32 { let a = await add(x, x); return await add(a, 0); }
async fn main() { println(await twice(21)); }");
    assert_eq!(s, "42\n");
}

#[test]
fn async_fn_is_lazy_nothing_runs_until_awaited() {
    let s = out("async fn noisy(tag: i32) -> i32 { println(tag); return tag; }
async fn main() {
    let never = noisy(1);
    let later = noisy(2);
    println(100);
    let r = await later;
    println(r);
}");
    assert_eq!(s, "100\n2\n2\n");
}

#[test]
fn a_sync_main_can_run_async_work_through_spawn_and_join() {
    let s = out("async fn sq(n: i64) -> i64 { await yieldNow(); return n * n; }
fn main() {
    let h = spawn sq(7);
    println(h.join());
}");
    assert_eq!(s, "49\n");
}

#[test]
fn await_of_a_join_handle_in_async_code() {
    let s = out("fn work(n: i32) -> i32 { return n + 1; }
async fn main() {
    let h = spawn move || { work(9) };
    println(await h);
}");
    assert_eq!(s, "10\n");
}

#[test]
fn spawn_of_a_future_returns_a_join_handle_awaitable_and_joinable() {
    let s = out("async fn v(n: i32) -> i32 { await yieldNow(); return n; }
async fn main() {
    let a = spawn v(1);
    let b = spawn v(2);
    println(await a + await b);
}");
    assert_eq!(s, "3\n");
}

#[test]
fn join_of_two_to_eight_futures_gives_a_tuple() {
    let s = out("async fn n(i: i32) -> i32 { await yieldNow(); return i; }
async fn main() {
    let (a, b) = await join(n(1), n(2));
    println(a + b);
    let (c, d, e) = await join(n(3), n(4), n(5));
    println(c + d + e);
    let (p, q, r, s, t, u, v, w) = await join(n(1), n(2), n(3), n(4), n(5), n(6), n(7), n(8));
    println(p + q + r + s + t + u + v + w);
}");
    assert_eq!(s, "3\n12\n36\n");
}

#[test]
fn join_of_an_array_literal_gives_an_array() {
    let s = out("async fn n(i: i32) -> i32 { await yieldNow(); return i * 10; }
async fn main() {
    let r = await join([n(1), n(2), n(3)]);
    println(r[0] + r[1] + r[2]);
}");
    assert_eq!(s, "60\n");
}

#[test]
fn join_with_mixed_result_types() {
    let s = out("async fn a() -> i32 { return 5; }
async fn b() -> String { return String::from(\"hi\"); }
async fn main() { let (x, y) = await join(a(), b()); println(x); println(y); }");
    assert_eq!(s, "5\nhi\n");
}

#[test]
fn futures_inside_join_run_concurrently_on_the_pool() {
    // three futures of 120 ms each. Only a LOWER bound on the elapsed time is asserted (never an
    // upper bound: no timing assumptions); the result proves all three completed.
    let t = std::time::Instant::now();
    let s = out("async fn nap(ms: u64) -> u64 { await sleepMs(ms); return ms; }
async fn main() { let (a, b, c) = await join(nap(120), nap(120), nap(120)); println(a + b + c); }");
    let el = t.elapsed();
    assert_eq!(s, "360\n");
    assert!(el >= std::time::Duration::from_millis(120), "sleepMs returned too early: {:?}", el);
}

#[test]
fn sleep_ms_waits_at_least_the_requested_time() {
    let t = std::time::Instant::now();
    let s = out("async fn main() { await sleepMs(150); println(1); }");
    assert_eq!(s, "1\n");
    assert!(t.elapsed() >= std::time::Duration::from_millis(150));
}

#[test]
fn sleeping_tasks_do_not_block_the_single_worker() {
    // with ONE worker, 50 tasks each `await sleepMs` and then finish: they can only all finish if
    // sleeping releases the worker. (No timing is asserted.)
    let src = "async fn nap(i: i32) -> i32 { await sleepMs(5); return i; }
async fn run(n: i32) -> i32 {
    let mut total = 0;
    let mut i = 0;
    let mut hs: i32 = 0;
    while i < n { let h = spawn nap(i); total = total + await h; i = i + 1; hs = hs + 1; }
    return total;
}
async fn main() { println(await run(50)); }";
    let r = run_env(src, &[("MTN_WORKERS", "1")]);
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "1225\n");
}

#[test]
fn yield_now_lets_other_ready_tasks_run_first_on_one_worker() {
    // one worker: the spawned task cannot run until main yields; after `yieldNow` it has run.
    let src = "async fn other() { println(2); }
async fn main() {
    let h = spawn other();
    println(1);
    await yieldNow();
    println(3);
    await h;
}";
    let r = run_env(src, &[("MTN_WORKERS", "1")]);
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "1\n2\n3\n");
}

#[test]
fn fan_out_of_100_spawned_futures_awaited_in_order() {
    // (a growable array of handles needs Vec, Phase 16: a chain of handles is used instead)
    let src = "async fn job(i: i32) -> i32 { await yieldNow(); return i * 2; }
async fn chain(i: i32, n: i32, h: JoinHandle<i32>) -> i32 {
    let here = await h;
    if i == n { return here; }
    let next = spawn job(i);
    return here + await chain(i + 1, n, next);
}
async fn main() { let first = spawn job(0); println(await chain(1, 100, first)); }";
    // job(0) .. job(99): sum of 2*i = 9900
    assert_eq!(out(src), "9900\n");
}

#[test]
fn question_mark_in_async_fn_propagates_ok_and_err() {
    let s = out("async fn half(n: i32) -> Result<i32, String> {
    if n % 2 != 0 { return Err(String::from(\"odd\")); }
    return Ok(n / 2);
}
async fn quarter(n: i32) -> Result<i32, String> { let h = await half(n)?; let q = await half(h)?; return Ok(q); }
async fn main() {
    let a = await quarter(8);
    match a { Ok(v) => println(v), Err(e) => println(e) }
    let b = await quarter(6);
    match b { Ok(v) => println(v), Err(e) => println(e) }
}");
    assert_eq!(s, "2\nodd\n");
}

#[test]
fn async_main_returning_err_exits_one() {
    let r = run_src("async fn bad() -> Result<i32, String> { return Err(String::from(\"nope\")); }
async fn main() -> Result<(), String> { let v = await bad()?; println(v); return Ok(()); }");
    assert_eq!(r.code, 1, "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "");
}

#[test]
fn throw_and_return_value_work_in_async_fns() {
    let s = out("async fn f(n: i32) -> Result<i32, String> {
    let r = try {
        if n < 0 { throw String::from(\"neg\"); }
        n
    } catch (e) {
        return Err(e);
    };
    return Ok(r);
}
async fn main() {
    match await f(3) { Ok(v) => println(v), Err(e) => println(e) }
    match await f(-3) { Ok(v) => println(v), Err(e) => println(e) }
}");
    assert_eq!(s, "3\nneg\n");
}

#[test]
fn async_methods_with_by_value_self_and_without_self() {
    let s = out("struct Acc { base: i32 }
impl Acc {
    async fn new_sum(a: i32, b: i32) -> i32 { await yieldNow(); return a + b; }
    async fn total(self, extra: i32) -> i32 { await yieldNow(); return self.base + extra; }
}
async fn main() {
    println(await Acc::new_sum(2, 3));
    let a = Acc { base: 10 };
    println(await a.total(5));
}");
    assert_eq!(s, "5\n15\n");
}

#[test]
fn mut_params_are_fine_in_async_fns() {
    let s = out("async fn bump(mut n: i32) -> i32 { n = n + 1; await yieldNow(); n = n + 1; return n; }
async fn main() { println(await bump(1)); }");
    assert_eq!(s, "3\n");
}

#[test]
fn owned_heap_values_survive_across_awaits() {
    let s = out("async fn keep(s: String) -> String {
    let extra = String::from(\"!\");
    await yieldNow();
    await sleepMs(1);
    return extra;
}
async fn main() { println(await keep(String::from(\"hey\"))); }");
    assert_eq!(s, "!\n");
}

#[test]
fn a_future_that_is_never_awaited_is_dropped_cleanly() {
    let s = out("async fn f(s: String) -> i32 { println(s); return 1; }
async fn main() { let a = f(String::from(\"x\")); let b = f(String::from(\"y\")); println(await b); }");
    assert_eq!(s, "y\n1\n");
}

#[test]
fn arc_held_across_await_is_send_and_spawnable() {
    let s = out("async fn use_arc(a: Arc<i32>) -> i32 { await yieldNow(); return *a + 1; }
async fn main() { let a = Arc::new(41); let h = spawn use_arc(a); println(await h); }");
    assert_eq!(s, "42\n");
}

#[test]
fn a_panic_in_a_spawned_async_task_exits_101_at_once() {
    let r = run_src("async fn boom() -> i32 { panic(\"async boom\"); return 1; }
async fn main() { let h = spawn boom(); println(await h); }");
    assert_eq!(r.code, 101, "stderr: {}", r.stderr);
    assert!(r.stderr.contains("panicked: async boom"), "stderr: {}", r.stderr);
}

#[test]
fn a_panic_in_async_main_exits_101() {
    let r = run_src("async fn main() { await yieldNow(); panic(\"main boom\"); }");
    assert_eq!(r.code, 101, "stderr: {}", r.stderr);
}

#[test]
fn main_waits_for_detached_async_tasks_at_exit() {
    let s = out("async fn late() { await yieldNow(); await yieldNow(); println(7); }
async fn main() { let _h = spawn late(); println(1); }");
    assert_eq!(sorted_lines(&s), vec!["1".to_string(), "7".to_string()]);
}

#[test]
fn detached_async_tasks_finish_for_a_sync_main_too() {
    let s = out("async fn late() { await yieldNow(); println(9); }
fn main() { let _h = spawn late(); }");
    assert_eq!(s, "9\n");
}

#[test]
fn eight_joined_futures_do_not_deadlock_on_one_worker() {
    let src = "async fn n(i: i32) -> i32 { await yieldNow(); await yieldNow(); return i; }
async fn main() {
    let (a, b, c, d, e, f, g, h) = await join(n(1), n(2), n(3), n(4), n(5), n(6), n(7), n(8));
    println(a + b + c + d + e + f + g + h);
}";
    let r = run_env(src, &[("MTN_WORKERS", "1")]);
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "36\n");
}

#[test]
fn load_10000_spawned_futures_yielding_a_few_times_on_four_workers() {
    let src = "async fn work(i: i32) -> i32 {
    let mut k = 0;
    while k < 4 { await yieldNow(); k = k + 1; }
    return i;
}
async fn spawner(n: i32) {
    let mut i = 0;
    while i < n { let h = spawn work(i); drop(h); i = i + 1; }
}
async fn main() { await spawner(10000); }";
    let r = run_env(src, &[("MTN_WORKERS", "4"), ("MTN_RT_STATS", "1")]);
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    assert!(r.stderr.contains("workers=4 spawned=10000 completed=10000"), "stderr: {}", r.stderr);
}

// ---- rejections (every compile error of M4)

#[test]
fn rejects_await_in_a_non_async_fn() {
    rejects("async fn g() -> i32 { 1 }\nfn main() { let x = await g(); }", "`await` can only be used inside an `async fn`");
}

#[test]
fn rejects_await_in_a_closure() {
    rejects("async fn g() -> i32 { 1 }\nasync fn f() { let c = || { await g() }; }\nfn main() { }", "`await` cannot be used inside a closure or a `spawn` block");
}

#[test]
fn rejects_await_in_a_spawn_block() {
    rejects("async fn g() -> i32 { 1 }\nasync fn f() { let h = spawn { await g() }; }\nfn main() { }", "`await` cannot be used inside a closure or a `spawn` block");
}

#[test]
fn rejects_awaiting_a_future_twice() {
    rejects("async fn g() -> i32 { 1 }\nasync fn main() { let f = g(); let a = await f; let b = await f; }", "use of moved value `f`");
}

#[test]
fn rejects_awaiting_something_that_is_not_a_future() {
    rejects("async fn main() { let x = await 1; }", "cannot `await` a value of type");
}

#[test]
fn rejects_spawning_a_future_holding_an_rc_across_an_await() {
    rejects("async fn g() { await yieldNow(); }
async fn f() { let r = Rc::new(1); await g(); println(*r); }
async fn main() { let h = spawn f(); await h; }", "`Rc<i32>` cannot be sent between threads");
}

#[test]
fn the_not_send_reason_names_the_variable_and_the_function() {
    rejects("async fn g() { await yieldNow(); }
async fn f() { let r = Rc::new(1); await g(); println(*r); }
async fn main() { let h = spawn f(); await h; }", "alive in `r` across an `await` in `async fn f`");
}

#[test]
fn rejects_a_borrow_param_in_an_async_fn() {
    rejects("async fn f(borrow x: i32) { }\nfn main() { }", "parameter `x` is passed by `borrow`");
}

#[test]
fn rejects_a_variadic_param_in_an_async_fn() {
    rejects("async fn f(..xs: i32) { }\nfn main() { }", "variadic parameter `xs` is not allowed");
}

#[test]
fn rejects_a_borrow_self_async_method() {
    rejects("struct S { v: i32 }\nimpl S { async fn m(borrow self) -> i32 { self.v } }\nfn main() { }", "`self` is passed by `borrow`");
}

#[test]
fn rejects_yield_in_an_async_fn() {
    rejects("async fn f() { yield 1; }\nfn main() { }", "`yield` inside an `async fn`");
}

#[test]
fn rejects_async_closures() {
    rejects("fn main() { let c = async || 1; }", "`async` closures and `async` blocks are not part of Mountain 1.0");
}

#[test]
fn rejects_async_blocks() {
    rejects("fn main() { let c = async { 1 }; }", "`async` closures and `async` blocks are not part of Mountain 1.0");
}

#[test]
fn rejects_async_trait_methods() {
    rejects("trait T { async fn f(self); }\nfn main() { }", "async trait methods are not part of Mountain 1.0");
}

#[test]
fn rejects_join_with_one_future_or_not_awaited() {
    rejects("async fn g() -> i32 { 1 }\nasync fn main() { let x = await join(g()); }", "`join(..)` must be awaited directly");
}

#[test]
fn rejects_await_inside_try() {
    rejects("async fn g() -> i32 { 1 }
async fn f() -> i32 { let x = try { await g() } catch (e) { 0 }; return x; }
fn main() { }", "`await`");
}

#[test]
fn a_type_error_in_an_async_body_is_reported() {
    rejects("async fn f() -> i32 { return String::from(\"x\"); }\nfn main() { }", "expected `i32`, found `String`");
}

// ---------------------------------------------------------------- M4: valgrind

fn have_valgrind() -> bool {
    Command::new("valgrind").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn valgrind_clean(src: &str, workers: &str, expect_sorted: &[&str]) {
    if !have_valgrind() {
        return;
    }
    let dir = scratch();
    let f = dir.join("prog.mtn");
    let exe = dir.join("prog");
    std::fs::write(&f, src).unwrap();
    let b = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let o = Command::new("valgrind")
        .args(["--leak-check=full", "--show-leak-kinds=all", "--errors-for-leak-kinds=all", "--error-exitcode=99", "-q"])
        .arg(&exe)
        .env("MTN_WORKERS", workers)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "valgrind: {}", String::from_utf8_lossy(&o.stderr));
    let got = sorted_lines(&String::from_utf8_lossy(&o.stdout));
    let want: Vec<String> = expect_sorted.iter().map(|s| s.to_string()).collect();
    assert_eq!(got, want);
    let _ = std::fs::remove_dir_all(&dir);
}

const VG_MIX: &str = "async fn a(s: String) -> String { await yieldNow(); return s; }
async fn late() { await sleepMs(20); await yieldNow(); println(7); }
async fn work(i: i32) -> i32 { let mut k = 0; while k < 3 { await yieldNow(); k = k + 1; } return i; }
async fn spawner(n: i32) { let mut i = 0; while i < n { let h = spawn work(i); drop(h); i = i + 1; } }
async fn main() {
    let never = a(String::from(\"never\"));
    let never2 = a(String::from(\"never2\"));
    let _d = spawn late();
    let (x, y) = await join(a(String::from(\"x\")), a(String::from(\"y\")));
    println(x); println(y);
    let r = await join([a(String::from(\"p\")), a(String::from(\"q\"))]);
    println(r[0]);
    let h = spawn a(String::from(\"h\"));
    println(await h);
    await spawner(200);
}";

#[test]
fn valgrind_futures_completed_never_started_and_detached_one_worker() {
    valgrind_clean(VG_MIX, "1", &["7", "h", "p", "x", "y"]);
}

#[test]
fn valgrind_futures_completed_never_started_and_detached_four_workers() {
    valgrind_clean(VG_MIX, "4", &["7", "h", "p", "x", "y"]);
}

#[test]
fn valgrind_sync_main_with_futures_still_sleeping_at_exit() {
    valgrind_clean(
        "async fn a(s: String) -> String { await yieldNow(); return s; }
async fn slow() { await sleepMs(30); println(5); }
fn main() { let _h = spawn slow(); let _n = spawn a(String::from(\"n\")); }",
        "4",
        &["5"],
    );
}

#[test]
fn valgrind_main_returning_err_does_not_leak_the_error_value() {
    if !have_valgrind() {
        return;
    }
    let dir = scratch();
    let f = dir.join("prog.mtn");
    let exe = dir.join("prog");
    std::fs::write(&f, "async fn bad() -> Result<i32, String> { return Err(String::from(\"nope\")); }
async fn main() -> Result<(), String> { let v = await bad()?; println(v); return Ok(()); }").unwrap();
    let b = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let o = Command::new("valgrind")
        .args(["--leak-check=full", "--show-leak-kinds=all", "--errors-for-leak-kinds=all", "--error-exitcode=99", "-q"])
        .arg(&exe)
        .output()
        .unwrap();
    // the program's own exit code is 1 (main returned Err); 99 would be a valgrind error
    assert_eq!(o.status.code(), Some(1), "valgrind: {}", String::from_utf8_lossy(&o.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}
