//! Phase 13 M2 round: the `Atomic<*mut T>` ruling, tuple structs, check/build agreement, and locks (Document 14 §6, §7, §9.1, §9.5).
//! Every program is built in its own unique directory (AtomicUsize + pid), never shared between tests.
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mtn_p13m2_{}_{}_{}", std::process::id(), tag, COUNTER.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn have_valgrind() -> bool {
    Command::new("valgrind").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Builds `src` (a file) into a unique directory; Ok(exe) or Err(message of the failed build).
fn build(src: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let exe = unique_dir("exe").join("out_exe");
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(src).arg("-o").arg(&exe).output().unwrap();
    if out.status.success() && exe.exists() {
        Ok(exe)
    } else {
        Err(String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr))
    }
}

fn write_src(src: &str) -> std::path::PathBuf {
    let f = unique_dir("src").join("p.mtn");
    std::fs::write(&f, src).unwrap();
    f
}

fn run_exe(exe: &std::path::Path, workers: &str, valgrind: bool) -> (String, i32) {
    let out = if valgrind {
        Command::new("valgrind").args(["--leak-check=full", "--errors-for-leak-kinds=definite", "--error-exitcode=99", "-q"]).arg(exe).env("MTN_WORKERS", workers).output().unwrap()
    } else {
        Command::new(exe).env("MTN_WORKERS", workers).output().unwrap()
    };
    (String::from_utf8_lossy(&out.stdout).to_string(), out.status.code().unwrap_or(-1))
}

fn run_src(src: &str) -> String {
    let exe = build(&write_src(src)).unwrap_or_else(|e| panic!("build failed: {}\n{}", e, src));
    let (o, c) = run_exe(&exe, "2", false);
    assert_eq!(c, 0, "exit code\n{}", o);
    o
}

fn rejected(src: &str, needle: &str) {
    match build(&write_src(src)) {
        Ok(_) => panic!("should have been rejected (`{}`):\n{}", needle, src),
        Err(m) => assert!(m.contains(needle), "expected `{}` in:\n{}", needle, m),
    }
}

fn golden_in(dir: &str, name: &str) {
    let src = root().join(dir).join(format!("{}.mtn", name));
    let want = std::fs::read_to_string(root().join("tests/golden13").join(format!("{}.out", name))).unwrap();
    let exe = build(&src).unwrap_or_else(|e| panic!("build of {} failed: {}", name, e));
    for w in ["1", "4"] {
        let (o, c) = run_exe(&exe, w, false);
        assert_eq!((o.as_str(), c), (want.as_str(), 0), "{} at MTN_WORKERS={}", name, w);
        if have_valgrind() {
            let (o, c) = run_exe(&exe, w, true);
            assert_eq!((o.as_str(), c), (want.as_str(), 0), "{} under valgrind at MTN_WORKERS={}", name, w);
        }
    }
}

fn golden(name: &str) {
    golden_in("examples/concurrency", name)
}

// ---------------------------------------------------------------- Atomic<*mut T> Send/Sync ruling (2026-11-19)

#[test]
fn atomic_pointer_in_arc_struct_shared_by_tasks_and_a_thread() {
    golden("atomic_pointer_shared");
}
#[test]
fn atomic_pointer_to_non_send_pointee_is_rejected() {
    rejected(
        "fn main() { let mut n = Rc::new(5); let s: Arc<Atomic<*mut Rc<i32>>> = Arc::new(Atomic::new(&mut n as *mut Rc<i32>)); let c = s.clone(); let h = spawn move { 1 + { let _x = c; 0 } }; println(h.join()); }",
        "is `Send` only when the pointee `Rc<i32>` is `Send`",
    );
}
#[test]
fn struct_with_an_atomic_pointer_and_a_plain_pointer_is_not_send() {
    rejected(
        "struct S { a: Atomic<*mut i32>, b: *mut i32 }\nfn main() { let mut x = 1; let p = &mut x as *mut i32; let s = Arc::new(S { a: Atomic::new(p), b: p }); let c = s.clone(); let h = spawn move { 1 + { let _x = c; 0 } }; println(h.join()); }",
        "a raw pointer is not `Send`",
    );
}
#[test]
fn plain_raw_pointers_stay_non_send() {
    rejected("fn main() { let mut x = 1; let p = &mut x as *mut i32; let h = spawn move { let _q = p; 1 }; println(h.join()); }", "a raw pointer is not `Send`");
}
#[test]
fn atomic_pointer_to_a_node_that_holds_an_atomic_pointer_is_send() {
    // the Michael-Scott / Treiber node shape used in M5: `next` is itself an `Atomic<*mut Node>`
    let out = run_src("struct Node { v: i64, next: Atomic<*mut Node> }\nstruct Stack { head: Atomic<*mut Node> }\nfn main() { let mut n = Node { v: 3, next: Atomic::new(unsafe { null }) }; let s = Arc::new(Stack { head: Atomic::new(&mut n as *mut Node) }); let c = s.clone(); let h = spawn move { let p = c.head.load(Ordering::Acquire); unsafe { p.read().v } }; println(h.join()); }");
    assert_eq!(out, "3\n");
}

// ---------------------------------------------------------------- tuple structs (Document 7 §2.4)

#[test]
fn tuple_structs_constructor_fields_patterns_generics_drop_copy_pack() {
    golden_in("examples/memory", "tuple_structs");
}
#[test]
fn tuple_struct_basic_probe_from_the_manager() {
    assert_eq!(run_src("struct P(i32, i32); fn main() { let p = P(1, 2); println(p.0); }"), "1\n");
}
#[test]
fn tuple_struct_field_write_needs_mut_and_wrong_use_is_rejected_by_check_and_build() {
    for (src, needle) in [
        ("struct P(i32,i32); fn main(){ let p = P(1); println(p.0); }", "missing field `1`"),
        ("struct P(i32,i32); fn main(){ let p = P(1, true); println(p.0); }", "expected `i32`, found `bool`"),
        ("struct P(i32,i32); fn main(){ let p = P(1,2); p.0 = 3; println(p.0); }", "not declared `mut`"),
        ("struct P(i32,i32); fn main(){ let p = P(1,2); println(p.2); }", "has no field `2`"),
        ("struct P(i32,i32); fn main(){ let p = P(a: 1, b: 2); println(p.0); }", "named arguments are only allowed"),
    ] {
        check_and_build_agree(src, needle);
    }
}

/// `mtnc check` and `mtnc build` must give the same verdict for a type-level error.
fn check_and_build_agree(src: &str, needle: &str) {
    let f = write_src(src);
    let chk = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("check").arg(&f).output().unwrap();
    let ctext = String::from_utf8_lossy(&chk.stdout).to_string() + &String::from_utf8_lossy(&chk.stderr);
    assert!(!chk.status.success() && ctext.contains(needle), "check should reject `{}`:\n{}\n{}", needle, src, ctext);
    let m = build(&f).err().unwrap_or_else(|| panic!("build accepted:\n{}", src));
    assert!(m.contains(needle), "build should say `{}`:\n{}", needle, m);
}

#[test]
fn field_moved_as_a_method_call_argument_is_caught_by_check_too() {
    check_and_build_agree(
        "struct R { name: String, n: i32 }\nimpl Drop for R { fn drop(borrow mut self) { } }\nstruct S { }\nimpl S { fn eat(borrow self, s: String) { } fn num(borrow self, n: i32) { } }\nfn main() { let r = R { name: \"a\", n: 1 }; let s = S { }; s.num(r.n); s.eat(r.name); }",
        "cannot be taken apart",
    );
}

// ---------------------------------------------------------------- locks (Document 14 §9.5)

#[test]
fn mutex_counter_threads_and_tasks_example() {
    golden("mutex_counter");
}
#[test]
fn rwlock_four_readers_and_a_writer_example() {
    golden("rwlock_readers_writer");
}
#[test]
fn spinlock_counter_example() {
    golden("spinlock_counter");
}
#[test]
fn bank_transfer_two_mutex_accounts_fixed_order_example() {
    golden("bank_transfer");
}

/// `Arc<Mutex<i64>>` incremented by 8 OS threads x 100,000, at both worker counts, exact total.
#[test]
fn mutex_eight_os_threads_by_one_hundred_thousand() {
    let src = r#"
fn bump(c: Arc<Mutex<i64>>) { let mut i: i64 = 0; while i < 100000 { let mut g = c.lock(); *g += 1; i += 1; } }
fn main() {
    let m: Arc<Mutex<i64>> = Arc::new(Mutex::new(0));
    let t0 = { let c = m.clone(); thread::spawnOS(move || { bump(c); 0 }) };
    let t1 = { let c = m.clone(); thread::spawnOS(move || { bump(c); 0 }) };
    let t2 = { let c = m.clone(); thread::spawnOS(move || { bump(c); 0 }) };
    let t3 = { let c = m.clone(); thread::spawnOS(move || { bump(c); 0 }) };
    let t4 = { let c = m.clone(); thread::spawnOS(move || { bump(c); 0 }) };
    let t5 = { let c = m.clone(); thread::spawnOS(move || { bump(c); 0 }) };
    let t6 = { let c = m.clone(); thread::spawnOS(move || { bump(c); 0 }) };
    let t7 = { let c = m.clone(); thread::spawnOS(move || { bump(c); 0 }) };
    t0.join(); t1.join(); t2.join(); t3.join(); t4.join(); t5.join(); t6.join(); t7.join();
    println(*m.lock());
}
"#;
    let exe = build(&write_src(src)).unwrap_or_else(|e| panic!("{}", e));
    for w in ["1", "4"] {
        assert_eq!(run_exe(&exe, w, false), ("800000\n".to_string(), 0), "workers {}", w);
    }
}

/// 1,000 tasks x 100 increments each.
#[test]
fn mutex_one_thousand_tasks_by_one_hundred() {
    let src = r#"
fn bump(c: Arc<Mutex<i64>>) { let mut i: i64 = 0; while i < 100 { let mut g = c.lock(); *g += 1; i += 1; } }
fn fan(c: Arc<Mutex<i64>>, n: i64) -> i64 {
    if n == 1 { bump(c); return 0; }
    let h = n / 2;
    let a = c.clone();
    let l = spawn move { fan(a, h) };
    let r = spawn move { fan(c, n - h) };
    l.join() + r.join()
}
fn main() {
    let m: Arc<Mutex<i64>> = Arc::new(Mutex::new(0));
    fan(m.clone(), 1000);
    println(*m.lock());
}
"#;
    let exe = build(&write_src(src)).unwrap_or_else(|e| panic!("{}", e));
    for w in ["1", "4"] {
        assert_eq!(run_exe(&exe, w, false), ("100000\n".to_string(), 0), "workers {}", w);
    }
}

fn run_both_workers(src: &str, want: &str) {
    let exe = build(&write_src(src)).unwrap_or_else(|e| panic!("build failed: {}\n{}", e, src));
    for w in ["1", "4"] {
        assert_eq!(run_exe(&exe, w, false), (want.to_string(), 0), "workers {}\n{}", w, src);
        if have_valgrind() {
            assert_eq!(run_exe(&exe, w, true), (want.to_string(), 0), "valgrind, workers {}\n{}", w, src);
        }
    }
}

#[test]
fn guard_is_released_at_scope_end_early_return_question_mark_and_break() {
    run_both_workers(
        r#"
fn free(m: borrow Mutex<i32>) -> i32 { match m.tryLock() { Some(g) => 1, None => 0 } }
fn early(m: borrow Mutex<i32>, quit: bool) -> i32 { let g = m.lock(); if quit { return 7; } 8 }
fn check(bad: bool) -> Result<i32, String> { if bad { Err("bad") } else { Ok(5) } }
fn q(m: borrow Mutex<i32>, bad: bool) -> Result<i32, String> { let g = m.lock(); let v = check(bad)?; Ok(v + *g) }
fn main() {
    let m = Mutex::new(100);
    { let g = m.lock(); println(free(borrow m)); }
    println(free(borrow m));
    println(early(borrow m, true));
    println(free(borrow m));
    println(early(borrow m, false));
    println(free(borrow m));
    match q(borrow m, true) { Ok(v) => println(v), Err(e) => println(e) }
    println(free(borrow m));
    match q(borrow m, false) { Ok(v) => println(v), Err(e) => println(e) }
    println(free(borrow m));
    let mut n = 0;
    loop { let g = m.lock(); n += 1; if n == 3 { break; } }
    println(free(borrow m));
    while n < 6 { let g = m.lock(); n += 1; if n == 5 { break; } }
    println(free(borrow m));
}
"#,
        "0\n1\n7\n1\n8\n1\nbad\n1\n105\n1\n1\n1\n",
    );
}

#[test]
fn try_lock_is_none_while_held_on_the_same_thread() {
    run_both_workers(
        r#"
fn main() {
    let m = Mutex::new(1);
    let g = m.lock();
    match m.tryLock() { Some(x) => println("held? no"), None => println("none while held") }
    drop(g);
    match m.tryLock() { Some(mut x) => { *x = 2; println("some after drop") }, None => println("unexpected") }
    println(*m.lock());
    let rw = RwLock::new(5);
    let w = rw.write();
    match rw.tryRead() { Some(x) => println("bad"), None => println("no read while written") }
    match rw.tryWrite() { Some(x) => println("bad"), None => println("no write while written") }
    drop(w);
    let r = rw.read();
    match rw.tryRead() { Some(x) => println(*x), None => println("bad") }
    match rw.tryWrite() { Some(x) => println("bad"), None => println("no write while read") }
    drop(r);
    match rw.tryWrite() { Some(x) => println("write again"), None => println("bad") }
}
"#,
        "none while held\nsome after drop\n2\nno read while written\nno write while written\n5\nno write while read\nwrite again\n",
    );
}

#[test]
fn a_task_holding_a_lock_does_not_deadlock_the_pool_when_it_joins() {
    // C (queued first) locks the mutex; A holds it while joining B. If A's join ran C on A's own stack it would
    // block on a lock A holds: the join must run only B.
    run_both_workers(
        r#"
fn main() {
    let m: Arc<Mutex<i32>> = Arc::new(Mutex::new(0));
    let a = {
        let ma = m.clone();
        spawn move {
            let mc = ma.clone();
            let c = spawn move { let mut g = mc.lock(); *g += 10; 0 };
            let r = { let mut g = ma.lock(); let b = spawn move { 5 }; let x = b.join(); *g += x; x };
            c.join();
            r
        }
    };
    println(a.join());
    println(*m.lock());
}
"#,
        "5\n15\n",
    );
}

#[test]
fn writer_is_not_starved_by_four_reader_threads() {
    // the readers loop on read() until the writer has finished; no sleep, no timing: a starved writer hangs the test
    run_both_workers(
        r#"
struct Ctl { lock: RwLock<i64>, done: Atomic<i32> }
fn reader(c: Arc<Ctl>) -> i64 {
    let mut n: i64 = 0;
    while c.done.load(Ordering::Acquire) == 0 { let g = c.lock.read(); n += *g * 0 + 1; }
    n
}
fn main() {
    let c = Arc::new(Ctl { lock: RwLock::new(0), done: Atomic::new(0) });
    let t0 = { let x = c.clone(); thread::spawnOS(move || reader(x)) };
    let t1 = { let x = c.clone(); thread::spawnOS(move || reader(x)) };
    let t2 = { let x = c.clone(); thread::spawnOS(move || reader(x)) };
    let t3 = { let x = c.clone(); thread::spawnOS(move || reader(x)) };
    { let mut w = c.lock.write(); *w = 42; }
    c.done.store(1, Ordering::Release);
    t0.join(); t1.join(); t2.join(); t3.join();
    println(*c.lock.read());
}
"#,
        "42\n",
    );
}

#[test]
fn spinlock_protects_a_counter_from_four_threads() {
    golden("spinlock_counter");
}

#[test]
fn mutex_string_modified_by_tasks_is_dropped_once_valgrind_clean() {
    run_both_workers(
        r#"
struct Note { text: String, edits: i64 }
fn edit(m: Arc<Mutex<Note>>, s: String) { let mut g = m.lock(); g.text = s; g.edits += 1; }
fn main() {
    let m: Arc<Mutex<Note>> = Arc::new(Mutex::new(Note { text: "x", edits: 0 }));
    let a = { let c = m.clone(); spawn move { edit(c, "a"); 0 } };
    let b = { let c = m.clone(); spawn move { edit(c, "b"); 0 } };
    let d = { let c = m.clone(); spawn move { edit(c, "c"); 0 } };
    a.join(); b.join(); d.join();
    println(m.lock().edits);
}
"#,
        "3\n",
    );
}

#[test]
fn lock_in_a_struct_field_and_in_actor_state() {
    run_both_workers(
        r#"
struct Bank { total: Mutex<i64>, log: RwLock<i64> }
actor Teller {
    state bank: Bank = Bank { total: Mutex::new(0), log: RwLock::new(0) };
    on deposit(v: i64) { let mut g = self.bank.total.lock(); *g += v; let mut l = self.bank.log.write(); *l += 1; }
    on report() { println(*self.bank.total.lock()); println(*self.bank.log.read()); }
}
fn main() {
    let t = Teller::spawn();
    t.send(Teller::deposit(5));
    t.send(Teller::deposit(7));
    t.send(Teller::report());
}
"#,
        "12\n2\n",
    );
}

#[test]
fn arc_mutex_and_arc_rwlock_shared_by_actors_tasks_and_os_threads() {
    run_both_workers(
        r#"
actor Adder {
    state m: Arc<Mutex<i64>> = Arc::new(Mutex::new(0));
    on add(v: i64) { let mut g = self.m.lock(); *g += v; }
    on show() { println(*self.m.lock()); }
}
fn main() {
    let shared: Arc<RwLock<i64>> = Arc::new(RwLock::new(1));
    let a = { let c = shared.clone(); spawn move { let mut g = c.write(); *g += 1; 0 } };
    let b = { let c = shared.clone(); thread::spawnOS(move || { let mut g = c.write(); *g += 1; 0 }) };
    a.join(); b.join();
    println(*shared.read());
    let ad = Adder::spawn();
    ad.send(Adder::add(3));
    ad.send(Adder::add(4));
    ad.send(Adder::show());
}
"#,
        "3\n7\n",
    );
}

// ---------------------------------------------------------------- rejections

#[test]
fn rejected_guard_moved_into_spawn() {
    rejected("fn main() { let m = Mutex::new(1); let g = m.lock(); let h = spawn move { let _x = g; 1 }; println(h.join()); }", "cannot be sent between threads: a lock guard (`MutexGuard<i32>`)");
}
#[test]
fn rejected_future_holding_a_guard_across_await_is_spawned() {
    rejected(
        "async fn tick() -> i32 { 1 }\nasync fn f(m: Arc<Mutex<i32>>) -> i32 { let g = m.lock(); let t = await tick(); t + *g }\nfn main() { let m: Arc<Mutex<i32>> = Arc::new(Mutex::new(1)); let h = spawn f(m); println(h.join()); }",
        "not `Send`",
    );
}
#[test]
fn rejected_mutex_of_rc_in_arc_across_spawn() {
    rejected("fn main() { let m: Arc<Mutex<Rc<i32>>> = Arc::new(Mutex::new(Rc::new(1))); let c = m.clone(); let h = spawn move { let _x = c; 1 }; println(h.join()); }", "`Mutex<Rc<i32>>` is `Send` only when `Rc<i32>` is `Send`");
}
#[test]
fn rejected_assignment_through_a_read_guard() {
    rejected("fn main() { let l = RwLock::new(1); let g = l.read(); *g = 2; println(*g); }", "cannot assign through a read guard");
}
#[test]
fn rejected_use_of_the_protected_value_without_locking() {
    rejected("fn main() { let m = Mutex::new(1); println(*m); }", "protected value");
    rejected("fn main() { let m = Mutex::new(1); let x = m + 1; println(x); }", "Mutex");
}
#[test]
fn rejected_plain_mut_shared_across_spawn_still() {
    rejected("fn main(){ let mut n = 0; let a = spawn || { n += 1; 0 }; let b = spawn || { n += 1; 0 }; a.join(); b.join(); println(n); }", "this task may outlive `n`");
    rejected("fn main(){ let a = Arc::new(0); let c = a.clone(); let h = spawn move { *c = 1; 0 }; h.join(); println(*a); }", "cannot assign through an `Rc`/`Arc`");
}
#[test]
fn rejected_locking_a_moved_mutex() {
    rejected("fn main() { let m = Mutex::new(1); let n = m; let g = m.lock(); println(*g); }", "moved");
}

// ---------------------------------------------------------------- methods through guards (check and build agree)

#[test]
fn user_methods_through_write_and_read_guards() {
    run_both_workers(
        "struct C { n: i32 }\nimpl C { fn bump(borrow mut self) { self.n += 1; } fn get(borrow self) -> i32 { self.n } }\nfn main() { let l = Mutex::new(C { n: 1 }); { let mut g = l.lock(); g.bump(); println(g.get()); } let r = RwLock::new(C { n: 7 }); { let g = r.read(); println(g.get()); } { let mut w = r.write(); w.bump(); println(w.get()); } let a = Mutex::new(Atomic::new(1)); let g = a.lock(); g.fetchAdd(4); println(g.load()); }",
        "2\n7\n8\n5\n",
    );
}
#[test]
fn borrow_mut_method_through_a_read_guard_or_arc_is_rejected_by_check_and_build() {
    check_and_build_agree(
        "struct C { n: i32 }\nimpl C { fn bump(borrow mut self) { self.n += 1; } }\nfn main() { let l = RwLock::new(C { n: 1 }); let g = l.read(); g.bump(); }",
        "cannot call the `borrow mut self` method `bump` through a read guard",
    );
    check_and_build_agree(
        "struct C { n: i32 }\nimpl C { fn bump(borrow mut self) { self.n += 1; } }\nfn main() { let a = Arc::new(C { n: 1 }); a.bump(); println(a.n); }",
        "cannot call the `borrow mut self` method `bump` through an `Rc`/`Arc`",
    );
}
#[test]
fn guard_probes_static_mutex_match_arms_returned_guard_and_closure() {
    run_both_workers("static M: Mutex<i32> = Mutex::new(0);\nfn main() { { let mut g = M.lock(); *g += 5; } println(*M.lock()); }", "5\n");
    run_both_workers("fn main() { let m = Mutex::new(3); let n = 3; match n { 3 => { let mut g = m.lock(); *g += 1; println(*g); } _ => { let g = m.lock(); println(*g); } } match m.tryLock() { Some(g) => println(*g), None => println(\"none\") } println(*m.lock()); }", "4\n4\n4\n");
    rejected("fn get(m: borrow Mutex<i32>) -> MutexGuard<i32> { m.lock() }\nfn main() { let m = Mutex::new(1); let g = get(borrow m); println(*g); }", "a lock guard cannot be a return type");
}
