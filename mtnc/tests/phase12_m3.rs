//! Phase 12, milestone M3 (Document 12 §11.1-11.3, §11.7, §11.9; Document 17 §11.6; Document 10 §4.3):
//! `spawn`, `JoinHandle<T>`, `thread::spawnOS`, `pinToCore`, `Send`/`Sync`, and the C runtime.
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

const SHAPE: &str = "trait Shape { fn area(borrow self) -> i32; }\nstruct Sq { s: i32 }\nimpl Shape for Sq { fn area(borrow self) -> i32 { return self.s * self.s; } }\n";

// ---------------------------------------------------------------- positive: spawn / join

#[test]
fn spawn_one_task_and_join_returns_its_value() {
    assert_eq!(out("fn main() { let h = spawn move || { 40 + 2 }; println(h.join()); }"), "42\n");
}

#[test]
fn all_four_spawn_forms_are_accepted() {
    let src = "fn main() {\n let a = spawn { 1 };\n let b = spawn move { 2 };\n let c = spawn move || { 3 };\n let d = spawn || { 4 };\n println(a.join() + b.join() + c.join() + d.join());\n}";
    assert_eq!(out(src), "10\n");
}

#[test]
fn join_returns_values_from_ten_tasks() {
    let mut s = String::from("fn main() {\n");
    for i in 0..10 {
        s += &format!(" let h{i} = spawn move || {{ {i} * {i} }};\n");
    }
    s += " let mut total = 0;\n";
    for i in 0..10 {
        s += &format!(" total = total + h{i}.join();\n");
    }
    s += " println(total);\n}\n";
    assert_eq!(out(&s), "285\n");
}

#[test]
fn join_returns_values_from_a_thousand_tasks() {
    let n = 1000;
    let mut s = String::from("fn main() {\n");
    for i in 0..n {
        s += &format!(" let h{i} = spawn move || {{ {i} }};\n");
    }
    s += " let mut total: i64 = 0;\n";
    for i in 0..n {
        s += &format!(" total = total + h{i}.join() as i64;\n");
    }
    s += " println(total);\n}\n";
    assert_eq!(out(&s), "499500\n");
}

#[test]
fn a_thousand_tasks_by_divide_and_conquer_with_nested_spawn_and_join() {
    let src = "fn sum_range(lo: i64, hi: i64) -> i64 {
    if hi - lo <= 4 { let mut s: i64 = 0; let mut i = lo; while i < hi { s = s + i; i = i + 1; } return s; }
    let mid = lo + (hi - lo) / 2;
    let l = spawn move || { sum_range(lo, mid) };
    let r = spawn move || { sum_range(mid, hi) };
    return l.join() + r.join();
}
fn main() { println(sum_range(0, 4000)); }";
    assert_eq!(out(src), "7998000\n");
    // one worker: a joining worker must run other tasks instead of blocking (no deadlock)
    let r = run_env(src, &[("MTN_WORKERS", "1")]);
    assert_eq!((r.code, r.stdout.as_str()), (0, "7998000\n"), "stderr: {}", r.stderr);
}

#[test]
fn spawn_move_with_owned_data() {
    let src = "struct Job { name: String, n: i32 }
fn main() {
    let name = String::from(\"mountain\");
    let a = spawn move { name };
    let job = Job { name: String::from(\"job\"), n: 7 };
    let b = spawn move { job };
    let c = spawn move { Box::new(5) };
    println(a.join());
    let j = b.join();
    println(j.name);
    println(j.n);
    println(*c.join());
}";
    assert_eq!(out(src), "mountain\njob\n7\n5\n");
}

#[test]
fn arc_i32_and_arc_string_are_read_by_several_tasks() {
    let src = "fn main() {
    let text = Arc::new(String::from(\"shared\"));
    let num = Arc::new(100);
    let n1 = num.clone(); let n2 = num.clone(); let n3 = num.clone();
    let t1 = text.clone(); let t2 = text.clone();
    let a = spawn move { *n1 + 1 };
    let b = spawn move { *n2 + 2 };
    let c = spawn move { *n3 + 3 };
    let d = spawn move { t1 };
    let e = spawn move { t2 };
    println(a.join() + b.join() + c.join());
    let x = d.join();
    let y = e.join();
    println(*x);
    println(*y);
    println(Arc::strongCount(borrow text));
}";
    assert_eq!(out(src), "306\nshared\nshared\n3\n");
}

#[test]
fn nested_spawn_inside_a_task() {
    let src = "fn main() { let outer = spawn { let inner = spawn { 7 }; inner.join() * 6 }; println(outer.join()); }";
    assert_eq!(out(src), "42\n");
}

#[test]
fn a_task_may_return_unit() {
    assert_eq!(out("fn main() { let h = spawn { let x = 1; }; h.join(); println(\"ok\"); }"), "ok\n");
}

#[test]
fn tasks_print_from_several_threads_lines_are_whole() {
    let src = "fn main() {
    let a = spawn move || { println(\"task a\"); 1 };
    let b = spawn move || { println(\"task b\"); 2 };
    let c = spawn move || { println(\"task c\"); 3 };
    let d = spawn move || { println(\"task d\"); 4 };
    println(a.join() + b.join() + c.join() + d.join());
}";
    let o = out(src);
    assert_eq!(sorted_lines(&o), vec!["10", "task a", "task b", "task c", "task d"]);
}

#[test]
fn a_dropped_handle_detaches_and_the_runtime_waits_for_the_task_at_exit() {
    let src = "fn main() { let h = spawn { println(\"detached task ran\"); }; drop(h); println(\"main done\"); }";
    assert_eq!(sorted_lines(&out(src)), vec!["detached task ran", "main done"]);
}

#[test]
fn tasks_spawned_by_tasks_and_never_joined_still_run_before_exit() {
    let src = "fn main() { let h = spawn { let g = spawn { println(\"grandchild\"); }; drop(g); println(\"child\"); }; drop(h); }";
    assert_eq!(sorted_lines(&out(src)), vec!["child", "grandchild"]);
}

#[test]
fn main_error_exit_code_is_kept_after_tasks_finish() {
    let src = "fn main() -> Result<(), String> { let h = spawn { 1 }; h.join(); return Err(String::from(\"bad\")); }";
    let r = run_src(src);
    assert_eq!(r.code, 1, "stderr: {}", r.stderr);
}

// ---------------------------------------------------------------- positive: threads

#[test]
fn thread_spawn_os_returns_a_value() {
    let src = "fn main() { let t = thread::spawnOS(move || { 6 * 7 }); println(t.join()); }";
    assert_eq!(out(src), "42\n");
}

#[test]
fn thread_spawn_os_with_owned_data_and_pin_to_core() {
    let src = "fn main() { let name = String::from(\"os\"); let t = thread::spawnOS(move || { pinToCore(0); name }); println(t.join()); pinToCore(1000000); }";
    assert_eq!(out(src), "os\n");
}

#[test]
fn an_unjoined_os_thread_is_joined_at_exit() {
    let src = "fn main() { let t = thread::spawnOS(move || { println(\"os thread ran\"); }); drop(t); }";
    assert_eq!(out(src), "os thread ran\n");
}

#[test]
fn an_os_thread_can_spawn_and_join_tasks() {
    let src = "fn main() { let t = thread::spawnOS(move || { let h = spawn { 20 }; h.join() + 1 }); println(t.join()); }";
    assert_eq!(out(src), "21\n");
}

// ---------------------------------------------------------------- panics

#[test]
fn a_panic_inside_a_task_ends_the_process_with_101() {
    let r = run_src("fn main() { let h = spawn { panic(\"boom\"); 1 }; println(h.join()); }");
    assert_eq!(r.code, 101, "stderr: {}", r.stderr);
    assert!(r.stderr.contains("panicked: boom"), "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "");
}

#[test]
fn a_panic_inside_an_unjoined_task_also_ends_the_process_with_101() {
    let r = run_src("fn main() { let h = spawn { panic(\"late\"); }; drop(h); }");
    assert_eq!(r.code, 101, "stderr: {}", r.stderr);
    assert!(r.stderr.contains("panicked: late"), "stderr: {}", r.stderr);
}

#[test]
fn a_panic_inside_an_os_thread_ends_the_process_with_101() {
    let r = run_src("fn main() { let t = thread::spawnOS(move || { panic(\"os boom\"); 1 }); println(t.join()); }");
    assert_eq!(r.code, 101, "stderr: {}", r.stderr);
    assert!(r.stderr.contains("panicked: os boom"), "stderr: {}", r.stderr);
}

// ---------------------------------------------------------------- runtime: workers and stats

#[test]
fn load_test_ten_thousand_synchronous_tasks_on_four_workers() {
    let src = "fn main() { let mut i = 0; while i < 10000 { let h = spawn move || { i * 2 }; drop(h); i = i + 1; } }";
    let r = run_env(src, &[("MTN_WORKERS", "4"), ("MTN_RT_STATS", "1")]);
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    let lines: Vec<&str> = r.stderr.lines().collect();
    assert_eq!(lines.len(), 1, "exactly one stats line expected, got: {:?}", lines);
    let l = lines[0];
    assert!(l.starts_with("rt: workers=4 spawned=10000 completed=10000 peak_live="), "{}", l);
    let peak: u64 = l.rsplit("peak_live=").next().unwrap().parse().unwrap();
    assert!((1..=10000).contains(&peak), "{}", l);
}

#[test]
fn load_test_with_joins_sums_ten_thousand_results() {
    let src = "fn main() { let mut i = 0; let mut total: i64 = 0; while i < 10000 { let h = spawn move || { i as i64 }; total = total + h.join(); i = i + 1; } println(total); }";
    let r = run_env(src, &[("MTN_WORKERS", "4"), ("MTN_RT_STATS", "1")]);
    assert_eq!((r.code, r.stdout.as_str()), (0, "49995000\n"), "stderr: {}", r.stderr);
    assert!(r.stderr.starts_with("rt: workers=4 spawned=10000 completed=10000 peak_live="), "{}", r.stderr);
}

#[test]
fn stats_count_only_spawned_tasks_and_workers_come_from_mtn_workers() {
    let src = "fn main() { let t = thread::spawnOS(move || { 1 }); let h = spawn { 2 }; println(t.join() + h.join()); }";
    let r = run_env(src, &[("MTN_WORKERS", "3"), ("MTN_RT_STATS", "1")]);
    assert_eq!((r.code, r.stdout.as_str()), (0, "3\n"), "stderr: {}", r.stderr);
    assert_eq!(r.stderr.trim_end(), "rt: workers=3 spawned=1 completed=1 peak_live=1");
}

#[test]
fn no_stats_line_without_the_environment_variable() {
    let r = run_src("fn main() { let h = spawn { 1 }; println(h.join()); }");
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
}

#[test]
fn a_program_without_tasks_is_unchanged_and_prints_no_stats() {
    let r = run_env("fn main() { println(1); }", &[("MTN_RT_STATS", "1")]);
    assert_eq!((r.code, r.stdout.as_str(), r.stderr.as_str()), (0, "1\n", ""));
}

#[test]
fn an_owning_environment_is_dropped_once_even_for_a_task_that_returns_unit() {
    // the value moved into the task is dropped on the worker; the program ends cleanly
    let src = "fn main() { let s = String::from(\"gone\"); let h = spawn move { let n = 1; }; let t = spawn move { s }; h.join(); println(t.join()); }";
    assert_eq!(out(src), "gone\n");
}

// ---------------------------------------------------------------- rejections: ownership

#[test]
fn rejects_a_non_move_task_that_mutates_a_shared_local() {
    rejects("fn main() { let mut n = 0; let h = spawn { n = n + 1; }; h.join(); }", "this task may outlive `n`");
}

#[test]
fn rejects_a_non_move_closure_that_borrows_a_local() {
    rejects("fn main() { let n = 5; let h = spawn || { n + 1 }; println(h.join()); }", "may outlive `n`: the closure given to `spawn` borrows the local `n`; write `move`");
}

#[test]
fn rejects_a_non_move_block_that_borrows_a_local() {
    rejects("fn main() { let n = 5; let h = spawn { n + 1 }; println(h.join()); }", "write `move`");
}

#[test]
fn rejects_a_non_move_os_thread_closure_that_borrows_a_local() {
    rejects("fn main() { let n = 5; let t = thread::spawnOS(|| { n + 1 }); println(t.join()); }", "closure given to `thread::spawnOS` borrows the local `n`");
}

#[test]
fn rejects_moving_a_borrow_parameter_into_a_task() {
    rejects("fn f(borrow s: String) { let h = spawn move { s }; h.join(); } fn main() { f(borrow String::from(\"a\")); }", "cannot move `s` into a closure: it is borrowed here, not owned");
}

#[test]
fn rejects_joining_a_handle_twice() {
    rejects("fn main() { let h = spawn { 1 }; h.join(); h.join(); }", "use of moved value `h`");
}

#[test]
fn rejects_using_a_moved_value_after_it_went_into_a_task() {
    rejects("fn main() { let s = String::from(\"a\"); let h = spawn move { s }; println(s); h.join(); }", "use of moved value `s`");
}

// ---------------------------------------------------------------- rejections: Send / Sync

#[test]
fn rejects_an_rc_captured_by_a_task() {
    rejects(
        "fn main() { let r = Rc::new(5); let h = spawn move { *r }; println(h.join()); }",
        "`Rc<i32>` cannot be sent between threads: its reference count is not atomic",
    );
}

#[test]
fn rejects_a_task_that_returns_an_rc() {
    rejects(
        "fn main() { let h = spawn move { Rc::new(5) }; let r = h.join(); println(*r); }",
        "`Rc<i32>` cannot be sent between threads: its reference count is not atomic (use `Arc<T>` to share between threads) (it is the result of the task",
    );
}

#[test]
fn rejects_a_struct_containing_an_rc() {
    rejects(
        "struct S { r: Rc<i32> }\nfn main() { let s = S { r: Rc::new(1) }; let h = spawn move { s }; h.join(); }",
        "`S` cannot be sent between threads: it contains `Rc<i32>`: its reference count is not atomic",
    );
}

#[test]
fn rejects_a_struct_containing_a_raw_pointer() {
    rejects(
        "struct P { p: *mut i32 }\nfn f(q: *mut i32) { let s = P { p: q }; let h = spawn move { s }; h.join(); }\nfn main() { }",
        "`P` cannot be sent between threads: it contains `*mut i32`: a raw pointer is not `Send`",
    );
}

#[test]
fn rejects_a_boxed_trait_object() {
    let src = format!("{}fn main() {{ let b: Box<dyn Shape> = Box::new(Sq {{ s: 2 }}); let h = spawn move {{ b.area() }}; println(h.join()); }}", SHAPE);
    rejects(&src, "`Box<dyn Shape>` cannot be sent between threads: it contains `dyn Shape`: `dyn Shape` carries no `+ Send` guarantee");
}

#[test]
fn rejects_mutation_through_an_arc() {
    rejects(
        "fn main() { let a = Arc::new(1); let b = a.clone(); let h = spawn move { *b = 5; }; h.join(); }",
        "cannot assign through an `Rc`/`Arc`",
    );
}

#[test]
fn rejects_an_arc_of_an_rc() {
    rejects(
        "fn main() { let a = Arc::new(Rc::new(1)); let h = spawn move { a }; h.join(); }",
        "`Arc<T>` needs `Rc<i32>` to be `Send + Sync`",
    );
}

#[test]
fn rejects_capturing_a_borrowed_reference() {
    rejects(
        "fn f(r: &i32) { let h = spawn move { r }; h.join(); } fn main() { let x = 1; f(&x); }",
        "may outlive the data it borrows",
    );
}

#[test]
fn an_os_thread_rejects_an_rc_too() {
    rejects(
        "fn main() { let r = Rc::new(5); let t = thread::spawnOS(move || { *r }); println(t.join()); }",
        "`Rc<i32>` cannot be sent between threads",
    );
}

#[test]
fn spawn_needs_a_closure_or_block() {
    rejects("fn main() { let h = spawn 5; }", "`spawn`");
}
