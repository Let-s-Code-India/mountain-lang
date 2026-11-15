//! Phase 12, milestone M5 (Document 12 §8, §11.5, §11.11; Document 6 §9.11; Document 9 §8.6):
//! the three M4 fixes (F1 per-branch moves, F2 reference-typed parameters of async functions and
//! generators, F3 `spawn` of a future that is not a direct `async fn` call) and `actor`.
//! Programs are compiled with the real `mtnc` binary and run; outputs are asserted exactly.
//! No test relies on timing or `sleep`: programs that print from several tasks are compared sorted,
//! or print one line after `join`. Every temp path is unique (pid + counter).

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn scratch() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("mtnc_m5_{}_{}", std::process::id(), n));
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


// ---------------------------------------------------------------- F1: moves per branch (Doc 6 §9.11)

const EAT: &str = "fn eat(s: String) -> i32 { return 1; }\n";

fn prog(body: &str) -> String {
    format!("{}fn main() {{ {} }}", EAT, body)
}

#[test]
fn f1_if_else_may_move_the_same_variable_in_each_branch() {
    let s = out(&prog("let s = String::from(\"a\"); let c = false; let r = if c { eat(s) } else { eat(s) + 1 }; println(r);"));
    assert_eq!(s, "2\n");
}

#[test]
fn f1_a_returning_branch_does_not_poison_the_code_after_it() {
    let s = out("fn eat(s: String) -> i32 { return 1; }
fn f(n: i32, s: String) -> i32 {
    if n == 0 { return eat(s); }
    return eat(s) + 1;
}
fn main() { println(f(0, String::from(\"a\"))); println(f(1, String::from(\"b\"))); }");
    assert_eq!(s, "1\n2\n");
}

#[test]
fn f1_match_with_a_move_in_each_arm() {
    let s = out(&prog("let s = String::from(\"a\"); let n = 2; let r = match n { 1 => eat(s), 2 => eat(s) + 1, _ => 0 }; println(r);"));
    assert_eq!(s, "2\n");
}

#[test]
fn f1_else_if_chain_with_a_move_in_every_branch() {
    let s = out(&prog("let s = String::from(\"a\"); let n = 2; if n == 1 { eat(s); } else if n == 2 { eat(s); } else { eat(s); } println(n);"));
    assert_eq!(s, "2\n");
}

#[test]
fn f1_a_move_followed_by_break_leaves_the_loop_cleanly() {
    let s = out(&prog("let s = String::from(\"a\"); let mut i = 0; loop { i = i + 1; if i > 2 { eat(s); break; } } println(i);"));
    assert_eq!(s, "3\n");
}

#[test]
fn f1_a_variable_moved_and_reassigned_in_a_loop_is_fine() {
    let s = out(&prog("let mut s = String::from(\"a\"); let mut i = 0; while i < 3 { eat(s); s = String::from(\"b\"); i = i + 1; } println(i);"));
    assert_eq!(s, "3\n");
}

#[test]
fn f1_rejects_use_after_the_if_when_only_one_branch_moved() {
    rejects(&prog("let s = String::from(\"a\"); let c = true; if c { eat(s); } println(s);"), "use of moved value `s`");
}

#[test]
fn f1_rejects_use_after_a_move_in_both_branches() {
    rejects(&prog("let s = String::from(\"a\"); let c = true; if c { eat(s); } else { eat(s); } println(s);"), "use of moved value `s`");
}

#[test]
fn f1_rejects_use_after_a_match_that_moved_in_one_arm() {
    rejects(&prog("let s = String::from(\"a\"); let n = 1; match n { 1 => { eat(s); } _ => {} } println(s);"), "use of moved value `s`");
}

#[test]
fn f1_rejects_use_after_an_else_if_chain_with_one_moving_branch() {
    rejects(&prog("let s = String::from(\"a\"); let n = 1; if n == 1 { eat(s); } else if n == 2 { } else { } eat(s);"), "use of moved value `s`");
}

#[test]
fn f1_rejects_a_double_move_on_one_path() {
    rejects(&prog("let s = String::from(\"a\"); let c = true; if c { eat(s); eat(s); }"), "use of moved value `s`");
}

#[test]
fn f1_rejects_a_move_in_a_while_body() {
    rejects(&prog("let s = String::from(\"a\"); let mut i = 0; while i < 2 { eat(s); i = i + 1; }"), "use of moved value `s`");
}

#[test]
fn f1_rejects_a_move_in_a_loop_body_that_may_continue() {
    rejects(&prog("let s = String::from(\"a\"); let mut i = 0; loop { eat(s); i = i + 1; if i > 2 { break; } }"), "use of moved value `s`");
}

#[test]
fn f1_rejects_use_after_a_loop_that_moved_before_breaking() {
    rejects(&prog("let s = String::from(\"a\"); let mut i = 0; loop { i = i + 1; if i > 2 { eat(s); break; } } eat(s);"), "use of moved value `s`");
}

// ---------------------------------------------------------------- F2: reference-typed parameters

#[test]
fn f2_the_use_after_free_program_is_a_compile_error() {
    rejects("struct S { v: i64 }
async fn p(s: borrow S) -> i64 { await sleepMs(30); return s.v; }
fn go() -> JoinHandle<i64> { let s = S { v: 77 }; let h = spawn p(borrow s); return h; }
fn main() { let h = go(); println(h.join()); }", "has the borrowed type `&S`");
}

#[test]
fn f2_async_rejects_the_borrow_modifier_spelling() {
    rejects("async fn p(borrow x: i32) { }\nfn main() { }", "is passed by `borrow`");
}

#[test]
fn f2_async_rejects_borrow_mut_in_type_position() {
    rejects("struct S { v: i64 }\nasync fn p(s: borrow mut S) -> i64 { return s.v; }\nfn main() { }", "has the borrowed type `&mut S`");
}

#[test]
fn f2_async_rejects_a_reference_type() {
    rejects("async fn p(s: &i64) -> i64 { return 1; }\nfn main() { }", "has the borrowed type `&i64`");
}

#[test]
fn f2_generator_rejects_the_borrow_modifier_spelling() {
    rejects("fn g(borrow n: i32) -> Generator<i32> { yield n; }\nfn main() { }", "is passed by `borrow`");
}

#[test]
fn f2_generator_rejects_borrow_in_type_position() {
    rejects("fn g(xs: borrow [i32]) -> Generator<i32> { yield 1; }\nfn main() { }", "has the borrowed type `&[i32]`");
}

#[test]
fn f2_generator_rejects_borrow_mut_in_type_position() {
    rejects("fn g(x: borrow mut i32) -> Generator<i32> { yield 1; }\nfn main() { }", "has the borrowed type `&mut i32`");
}

#[test]
fn f2_async_and_generator_with_owned_parameters_still_work() {
    let s = out("fn g(n: i32, mut s: String) -> Generator<i32> { yield n; s = String::from(\"x\"); yield n + 1; }
async fn a(n: i32, s: String) -> i32 { await yieldNow(); return n; }
async fn main() {
    let mut it = g(1, String::from(\"s\"));
    for x in it { println(x); }
    println(await a(5, String::from(\"t\")));
}");
    assert_eq!(s, "1\n2\n5\n");
}

// ---------------------------------------------------------------- F3: spawn of a future

#[test]
fn f3_rejects_a_future_parameter_that_hides_an_rc_across_an_await() {
    rejects("async fn hold() -> i64 { let r = Rc::new(1 as i64); await yieldNow(); return *r; }
fn run(f: Future<i64>) -> i64 { let h = spawn f; return h.join(); }
fn main() { println(run(hold())); }", "pass the async call itself to `spawn`");
}

#[test]
fn f3_rejects_a_future_held_in_a_local_variable() {
    rejects("async fn add(a: i32, b: i32) -> i32 { return a + b; }
fn main() { let f = add(1, 2); let h = spawn f; println(h.join()); }", "pass the async call itself to `spawn`");
}

#[test]
fn f3_rejects_a_future_that_is_a_struct_field() {
    rejects("async fn add(a: i32, b: i32) -> i32 { return a + b; }
struct W { f: Future<i32> }
fn main() { let w = W { f: add(1, 2) }; let h = spawn w.f; println(h.join()); }", "pass the async call itself to `spawn`");
}

#[test]
fn f3_rejects_a_future_captured_by_a_spawned_closure() {
    rejects("async fn add(a: i32, b: i32) -> i32 { return a + b; }
fn main() { let f = add(1, 2); let h = spawn move { 1 + { let g = f; 1 } }; println(h.join()); }", "captured by the task given to `spawn`");
}

#[test]
fn f3_rejects_a_generator_captured_by_a_spawned_closure() {
    rejects("fn g() -> Generator<i32> { yield 1; }
fn main() { let x = g(); let h = spawn move { let mut y = x; 1 }; h.join(); }", "captured by the task given to `spawn`");
}

#[test]
fn f3_a_direct_async_call_is_still_spawnable() {
    let s = out("async fn add(a: i32, b: i32) -> i32 { return a + b; }\nfn main() { let h = spawn add(1, 2); println(h.join()); }");
    assert_eq!(s, "3\n");
}

#[test]
fn f3_a_direct_async_method_call_is_still_spawnable() {
    let s = out("struct A { n: i32 }
impl A { async fn get(self) -> i32 { await yieldNow(); return self.n; } }
async fn main() { let a = A { n: 9 }; let h = spawn a.get(); println(await h); }");
    assert_eq!(s, "9\n");
}

#[test]
fn f3_a_direct_call_holding_an_rc_across_an_await_is_still_rejected_with_the_reason() {
    rejects("async fn g() { await yieldNow(); }
async fn f() { let r = Rc::new(1); await g(); println(*r); }
async fn main() { let h = spawn f(); await h; }", "alive in `r` across an `await` in `async fn f`");
}

// ---------------------------------------------------------------- M5: actors (Doc 12 §8, §11.5)

const COUNTER_ACTOR: &str = "actor Counter {
    state total: i64 = 0;
    on add(n: i64) { self.total = self.total + n; }
    on show() { println(self.total); }
}
";

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

#[test]
fn counter_actor_receives_1000_messages_then_prints_the_total() {
    let src = format!(
        "{}fn main() {{
    let c = Counter::spawn();
    let mut i = 0;
    while i < 1000 {{ c.send(add(1)); i = i + 1; }}
    c.send(show());
}}",
        COUNTER_ACTOR
    );
    for w in ["1", "4"] {
        let r = run_env(&src, &[("MTN_WORKERS", w)]);
        assert_eq!(r.code, 0, "workers={}: {}", w, r.stderr);
        assert_eq!(r.stdout, "1000\n", "workers={}", w);
    }
}

#[test]
fn both_spellings_of_a_message_work_for_a_unique_name() {
    let src = format!("{}fn main() {{ let c = Counter::spawn(); c.send(add(2)); c.send(Counter::add(3)); c.send(Counter::show()); }}", COUNTER_ACTOR);
    assert_eq!(out(&src), "5\n");
}

#[test]
fn messages_from_one_sender_are_handled_in_the_order_sent() {
    let s = out("actor Log {
    state last: i32 = 0;
    on put(n: i32) { if n == self.last + 1 { self.last = n; } else { println(-1); } }
    on end() { println(self.last); }
}
fn main() {
    let l = Log::spawn();
    let mut i = 1;
    while i <= 500 { l.send(put(i)); i = i + 1; }
    l.send(end());
}");
    assert_eq!(s, "500\n");
}

#[test]
fn two_actors_ping_pong_n_times() {
    let src = "actor Ping {
    state count: i32 = 0;
    on start(peer: ActorRef<Pong>, me: ActorRef<Ping>, n: i32) { peer.send(ping(me, peer, n)); }
    on pong(peer: ActorRef<Pong>, me: ActorRef<Ping>, n: i32) {
        self.count = self.count + 1;
        if n > 1 { peer.send(ping(me, peer, n - 1)); } else { println(self.count); }
    }
}
actor Pong {
    state seen: i32 = 0;
    on ping(back: ActorRef<Ping>, me: ActorRef<Pong>, n: i32) {
        self.seen = self.seen + 1;
        back.send(pong(me, back, n));
    }
}
fn main() { let a = Ping::spawn(); let b = Pong::spawn(); a.send(start(b, a, 1000)); }";
    for w in ["1", "4"] {
        let r = run_env(src, &[("MTN_WORKERS", w)]);
        assert_eq!(r.code, 0, "workers={}: {}", w, r.stderr);
        assert_eq!(r.stdout, "1000\n");
    }
}

#[test]
fn string_struct_and_arc_payloads() {
    let s = out("struct Order { id: i64, who: String }
actor Book {
    state n: i32 = 0;
    on newOrder(o: Order) { self.n = self.n + 1; println(o.who); println(o.id); }
    on tag(label: String, shared: Arc<i64>) { println(label); println(*shared); }
    on done() { println(self.n); }
}
fn main() {
    let b = Book::spawn();
    let shared = Arc::new(77 as i64);
    b.send(newOrder(Order { id: 1, who: String::from(\"ann\") }));
    b.send(tag(String::from(\"t1\"), shared.clone()));
    b.send(done());
}");
    assert_eq!(s, "ann\n1\nt1\n77\n1\n");
}

#[test]
fn a_hundred_actors_each_receive_a_hundred_messages() {
    let src = "actor Acc {
    state total: i64 = 0;
    on add(n: i64) { self.total = self.total + n; }
    on report(out: ActorRef<Sink>) { out.send(got(self.total)); }
}
actor Sink {
    state sum: i64 = 0;
    state seen: i32 = 0;
    on got(n: i64) { self.sum = self.sum + n; self.seen = self.seen + 1; if self.seen == 100 { println(self.sum); } }
}
fn feed(a: ActorRef<Acc>) { let mut i = 0; while i < 100 { a.send(add(i as i64)); i = i + 1; } }
fn round(sink: ActorRef<Sink>, k: i32) {
    if k > 0 {
        let a = Acc::spawn();
        feed(a);
        a.send(report(sink));
        round(sink, k - 1);
    }
}
fn main() { let sink = Sink::spawn(); round(sink, 100); }";
    // each actor sums 0..99 = 4950; 100 actors = 495000
    for w in ["1", "4"] {
        let r = run_env(src, &[("MTN_WORKERS", w)]);
        assert_eq!(r.code, 0, "workers={}: {}", w, r.stderr);
        assert_eq!(r.stdout, "495000\n");
    }
}

#[test]
fn messages_can_be_sent_from_inside_spawned_tasks() {
    let s = out(&format!(
        "{}fn main() {{
    let c = Counter::spawn();
    let a = spawn move || {{ let mut i = 0; while i < 100 {{ c.send(add(1)); i = i + 1; }} }};
    let b = spawn move || {{ let mut i = 0; while i < 100 {{ c.send(add(2)); i = i + 1; }} }};
    a.join();
    b.join();
    c.send(show());
}}",
        COUNTER_ACTOR
    ));
    assert_eq!(s, "300\n");
}

#[test]
fn messages_can_be_sent_from_async_code() {
    let s = out(&format!(
        "{}async fn main() {{
    let c = Counter::spawn();
    let mut i = 0;
    while i < 50 {{ c.send(add(2)); await yieldNow(); i = i + 1; }}
    c.send(show());
}}",
        COUNTER_ACTOR
    ));
    assert_eq!(s, "100\n");
}

#[test]
fn a_handler_may_spawn_a_task_and_send_to_itself() {
    let s = out("actor Echo {
    state n: i32 = 0;
    on go(me: ActorRef<Echo>, k: i32) {
        self.n = self.n + 1;
        if k > 0 { me.send(go(me, k - 1)); } else { println(self.n); }
    }
    on side() { let h = spawn move || { 40 + 2 }; println(h.join()); }
}
fn main() { let e = Echo::spawn(); e.send(go(e, 9)); e.send(side()); }");
    assert_eq!(sorted_lines(&s), vec!["10".to_string(), "42".to_string()]);
}

#[test]
fn an_actor_with_no_state_works() {
    let s = out("actor Hello { on hi(n: i32) { println(n); } }\nfn main() { let h = Hello::spawn(); h.send(hi(7)); }");
    assert_eq!(s, "7\n");
}

#[test]
fn an_actor_ref_is_copyable_and_can_be_passed_around() {
    let s = out(&format!(
        "{}struct Holder {{ c: ActorRef<Counter> }}
fn twice(c: ActorRef<Counter>) {{ c.send(add(1)); c.send(add(1)); }}
fn main() {{
    let c = Counter::spawn();
    let d = c;
    let h = Holder {{ c: d }};
    twice(c);
    twice(d);
    h.c.send(add(1));
    c.send(show());
}}",
        COUNTER_ACTOR
    ));
    assert_eq!(s, "5\n");
}

#[test]
fn actor_state_with_a_string_is_dropped_at_exit() {
    valgrind_clean(
        "actor Names {
    state label: String = String::from(\"names\");
    state last: String = String::from(\"-\");
    on set(s: String) { self.last = s; }
    on show() { println(self.label); println(self.last); }
}
fn main() { let n = Names::spawn(); n.send(set(String::from(\"x\"))); n.send(set(String::from(\"y\"))); n.send(show()); }",
        "4",
        &["names", "y"],
    );
}

#[test]
fn valgrind_actors_with_payloads_and_many_messages() {
    let src = "struct Order { id: i64, who: String }
actor Book {
    state n: i32 = 0;
    state names: String = String::from(\"\");
    on newOrder(o: Order) { self.n = self.n + 1; self.names = o.who; }
    on tag(label: String, shared: Arc<i64>) { self.n = self.n + *shared as i32; }
    on done() { println(self.n); println(self.names); }
}
fn feed(b: ActorRef<Book>, k: i32, shared: Arc<i64>) {
    if k > 0 {
        b.send(newOrder(Order { id: k as i64, who: String::from(\"w\") }));
        b.send(tag(String::from(\"t\"), shared.clone()));
        feed(b, k - 1, shared);
    }
}
fn main() { let b = Book::spawn(); let shared = Arc::new(2 as i64); feed(b, 200, shared); b.send(done()); }";
    valgrind_clean(src, "1", &["600", "w"]);
    valgrind_clean(src, "4", &["600", "w"]);
}

#[test]
fn a_panic_in_a_handler_exits_101_at_once() {
    let r = run_src("actor Boom { on go() { panic(\"handler boom\"); } }\nfn main() { let b = Boom::spawn(); b.send(go()); }");
    assert_eq!(r.code, 101, "stderr: {}", r.stderr);
    assert!(r.stderr.contains("panicked: handler boom"), "stderr: {}", r.stderr);
}

#[test]
fn main_waits_for_all_queued_messages_before_exiting() {
    let s = out(&format!("{}fn main() {{ let c = Counter::spawn(); let mut i = 0; while i < 2000 {{ c.send(add(1)); i = i + 1; }} c.send(show()); }}", COUNTER_ACTOR));
    assert_eq!(s, "2000\n");
}

#[test]
fn the_stats_line_counts_every_message_as_a_task() {
    let src = format!("{}fn main() {{ let c = Counter::spawn(); c.send(add(1)); c.send(add(1)); c.send(show()); }}", COUNTER_ACTOR);
    let r = run_env(&src, &[("MTN_WORKERS", "2"), ("MTN_RT_STATS", "1")]);
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    assert!(r.stderr.contains("workers=2 spawned=3 completed=3"), "stderr: {}", r.stderr);
}

// ---- rejections (every compile error of M5)

#[test]
fn rejects_a_message_holding_an_rc() {
    rejects("actor A { on bad(r: Rc<i32>) { } }\nfn main() { let a = A::spawn(); }", "a message of actor `A` must be `Send`");
}

#[test]
fn the_rc_message_reason_names_the_type() {
    rejects("actor A { on bad(r: Rc<i32>) { } }\nfn main() { let a = A::spawn(); }", "it contains `Rc<i32>`");
}

#[test]
fn rejects_an_actor_state_that_is_not_send() {
    rejects("actor A { state r: Rc<i32> = Rc::new(1); on go() { } }\nfn main() { let a = A::spawn(); }", "the state of actor `A` must be `Send`");
}

#[test]
fn rejects_reading_a_state_field_from_outside() {
    rejects("actor A { state total: i32 = 0; on go() { } }\nfn main() { let a = A::spawn(); println(a.total); }", "the state of an actor is not accessible from outside");
}

#[test]
fn rejects_self_outside_a_handler() {
    rejects("fn f() -> i32 { return self.total; }\nfn main() { println(f()); }", "`self` is only available inside a method or an actor `on` handler");
}

#[test]
fn rejects_await_in_a_handler() {
    rejects("async fn g() -> i32 { 1 }\nactor A { on go() { let x = await g(); } }\nfn main() { }", "`await` is not allowed in an actor handler");
}

#[test]
fn rejects_a_bare_message_name_two_actors_define() {
    rejects("actor A { on go() { } }\nactor B { on go() { } }\nfn main() { let a = A::spawn(); a.send(go()); }", "the bare name is ambiguous; write `A::go(..)` or `B::go(..)`");
}

#[test]
fn the_qualified_spelling_works_when_two_actors_share_a_handler_name() {
    let s = out("actor A { on go() { println(1); } }\nactor B { on go() { println(2); } }
fn main() { let a = A::spawn(); let b = B::spawn(); a.send(A::go()); b.send(B::go()); }");
    assert_eq!(sorted_lines(&s), vec!["1".to_string(), "2".to_string()]);
}

#[test]
fn rejects_an_unknown_handler() {
    rejects("actor A { on go() { } }\nfn main() { let a = A::spawn(); a.send(A::nope(1)); }", "actor `A` has no handler `nope`");
}

#[test]
fn rejects_sending_a_non_message() {
    rejects("actor A { on go(n: i32) { } }\nfn main() { let a = A::spawn(); a.send(5); }", "needs a message of actor `A`");
}

#[test]
fn rejects_sending_another_actors_message() {
    rejects("actor A { on go() { } }\nactor B { on stop() { } }\nfn main() { let a = A::spawn(); a.send(B::stop()); }", "needs a message of actor `A`");
}

#[test]
fn rejects_calling_a_method_on_an_actor_ref() {
    rejects("actor A { on go() { } }\nfn main() { let a = A::spawn(); a.go(); }", "`ActorRef` has only `send(message)`");
}

#[test]
fn rejects_actor_generics() {
    rejects("actor A<T> { on go() { } }\nfn main() { }", "actor generics are not part of Mountain 1.0");
}

#[test]
fn rejects_actor_inheritance() {
    rejects("actor A : B { on go() { } }\nfn main() { }", "actor inheritance and trait bounds on actors are not part of Mountain 1.0");
}

#[test]
fn rejects_request_reply_handlers() {
    rejects("actor A { on go() -> i32 { 1 } }\nfn main() { }", "request/reply is not part of Mountain 1.0");
}

#[test]
fn rejects_a_borrow_message_field() {
    rejects("actor A { on go(borrow n: i32) { } }\nfn main() { }", "the fields of a message are passed by value");
}

#[test]
fn rejects_a_handler_defined_twice() {
    rejects("actor A { on go() { } on go() { } }\nfn main() { }", "defines the handler `go` twice");
}

#[test]
fn rejects_a_handler_called_spawn() {
    rejects("actor A { on spawn() { } }\nfn main() { }", "`spawn` is reserved");
}

#[test]
fn rejects_an_actor_without_handlers() {
    rejects("actor A { state n: i32 = 0; }\nfn main() { }", "has no `on` handler");
}
