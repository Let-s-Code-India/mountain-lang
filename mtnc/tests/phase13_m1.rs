//! Phase 13 M1: `Atomic<T>` and `Ordering` (Document 14 §1, §2, §9.4).
//! Every program is built in its own unique directory (AtomicUsize + pid), never shared between tests.
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mtn_p13m1_{}_{}_{}", std::process::id(), tag, COUNTER.fetch_add(1, Ordering::SeqCst)));
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

/// Emits the LLVM IR of a program.
fn emit_ir(src: &std::path::Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(src).arg("--emit-ir").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// The text of one function (`define .. @name(..) { .. }`).
fn function_body<'a>(ir: &'a str, name: &str) -> &'a str {
    let start = ir.find(&format!("@{}(", name)).unwrap_or_else(|| panic!("no function {}", name));
    let start = ir[..start].rfind("define").unwrap();
    let end = ir[start..].find("\n}\n").map(|e| start + e).unwrap_or(ir.len());
    &ir[start..end]
}

// ---------------------------------------------------------------- behaviour (exact outputs)

#[test]
fn all_operations_and_widths() {
    golden("atomic_ops");
}
#[test]
fn pointer_atomic_swap_and_cas() {
    golden("atomic_pointer");
}
#[test]
fn spin_flag_handshake_and_atomic_struct_fields() {
    golden("atomic_spin_flag");
}
#[test]
fn eight_os_threads_and_eight_tasks_fetch_add_exact_total() {
    golden("atomic_counter");
}
#[test]
fn cas_retry_loops_from_eight_threads_exact() {
    golden("atomic_cas_max");
}
#[test]
fn static_atomic_from_tasks_and_os_threads() {
    golden("atomic_static");
}
#[test]
fn arc_atomic_in_a_move_closure_in_a_loop() {
    golden("atomic_arc_loop");
}
#[test]
fn atomic_inside_a_struct_in_actor_state() {
    golden("atomic_in_actor");
}

// ---------------------------------------------------------------- IR: exact orderings, no runtime call

fn probe_ir(body: &str) -> String {
    let src = format!("fn probe(a: borrow Atomic<i32>) -> i32 {{\n{}\n    return 0;\n}}\nfn main() {{ let a: Atomic<i32> = Atomic::new(0); println(probe(borrow a)); }}\n", body);
    let ir = emit_ir(&write_src(&src));
    function_body(&ir, "mtn_probe").to_string()
}

#[test]
fn ir_load_has_exactly_the_requested_ordering() {
    for (o, llvm) in [("Relaxed", "monotonic"), ("Acquire", "acquire"), ("SeqCst", "seq_cst")] {
        let f = probe_ir(&format!("    let v = a.load(Ordering::{});", o));
        assert!(f.contains(&format!("load atomic i32, ptr %", )) && f.contains(&format!(" {}, align 4", llvm)), "{}:\n{}", o, f);
    }
    // omitted ordering means SeqCst
    let f = probe_ir("    let v = a.load();");
    assert!(f.contains(" seq_cst, align 4"), "{}", f);
}
#[test]
fn ir_store_has_exactly_the_requested_ordering() {
    for (o, llvm) in [("Relaxed", "monotonic"), ("Release", "release"), ("SeqCst", "seq_cst")] {
        let f = probe_ir(&format!("    a.store(1, Ordering::{});", o));
        assert!(f.contains("store atomic i32 1, ptr %") && f.contains(&format!(" {}, align 4", llvm)), "{}:\n{}", o, f);
    }
    let f = probe_ir("    a.store(1);");
    assert!(f.contains(" seq_cst, align 4"), "{}", f);
}
#[test]
fn ir_rmw_has_exactly_the_requested_ordering() {
    for (o, llvm) in [("Relaxed", "monotonic"), ("Acquire", "acquire"), ("Release", "release"), ("AcqRel", "acq_rel"), ("SeqCst", "seq_cst")] {
        for (m, op) in [("fetchAdd", "add"), ("fetchSub", "sub"), ("fetchAnd", "and"), ("fetchOr", "or"), ("fetchXor", "xor"), ("swap", "xchg")] {
            let f = probe_ir(&format!("    a.{}(2, Ordering::{});", m, o));
            assert!(f.contains(&format!("atomicrmw {} ptr %", op)) && f.contains(&format!(" i32 2 {}, align 4", llvm)), "{} {}:\n{}", m, o, f);
        }
    }
}
#[test]
fn ir_compare_and_swap_derives_the_failure_ordering() {
    for (o, succ, fail) in [
        ("Relaxed", "monotonic", "monotonic"),
        ("Acquire", "acquire", "acquire"),
        ("Release", "release", "monotonic"),
        ("AcqRel", "acq_rel", "acquire"),
        ("SeqCst", "seq_cst", "seq_cst"),
    ] {
        let f = probe_ir(&format!("    let r = a.compareAndSwap(1, 2, Ordering::{});", o));
        assert!(f.contains(&format!("cmpxchg ptr %")) && f.contains(&format!(" i32 1, i32 2 {} {}, align 4", succ, fail)), "{}:\n{}", o, f);
    }
    let f = probe_ir("    let r = a.compareAndSwap(1, 2);");
    assert!(f.contains(" seq_cst seq_cst, align 4"), "{}", f);
}
#[test]
fn ir_atomics_are_instructions_not_calls_and_not_locks() {
    let f = probe_ir("    a.store(1, Ordering::Release);\n    a.fetchAdd(1, Ordering::AcqRel);\n    let r = a.compareAndSwap(1, 2, Ordering::SeqCst);\n    let v = a.load(Ordering::Acquire);");
    assert!(!f.contains("call "), "an atomic operation must not call the runtime:\n{}", f);
    assert!(!f.to_lowercase().contains("mutex") && !f.to_lowercase().contains("lock"), "{}", f);
}
#[test]
fn ir_bool_and_pointer_atomics_use_integer_instructions() {
    let ir = emit_ir(&write_src("fn probe(a: borrow Atomic<bool>) -> bool { return a.fetchOr(true, Ordering::Release); }\nfn main() { let a = Atomic::new(false); println(probe(borrow a)); }"));
    let f = function_body(&ir, "mtn_probe");
    assert!(f.contains("atomicrmw or ptr %") && f.contains(" i8 ") && f.contains(" release, align 1"), "{}", f);
}

// ---------------------------------------------------------------- rejections

#[test]
fn unsupported_element_types_are_rejected_naming_the_allowed_ones() {
    for f in ["e1", "e2", "e3", "e4", "e5"] {
        let m = build(&root().join("tests/golden13/rej").join(format!("{}.mtn", f))).err().unwrap_or_else(|| panic!("{} accepted", f));
        assert!(m.contains("does not exist") && m.contains("`bool`, `i8`..`i64`, `u8`..`u64`, `isize`, `usize` and `*mut T`"), "{}: {}", f, m);
    }
}
#[test]
fn invalid_orderings_are_rejected_with_the_reason() {
    for (f, needle) in [
        ("o1", "`load` cannot use `Ordering::Release`"),
        ("o2", "`load` cannot use `Ordering::AcqRel`"),
        ("o3", "`store` cannot use `Ordering::Acquire`"),
        ("o4", "`store` cannot use `Ordering::AcqRel`"),
        ("o5", "cannot be a variable"),
        ("o6", "`Ordering::Weird` does not exist"),
    ] {
        let m = build(&root().join("tests/golden13/rej").join(format!("{}.mtn", f))).err().unwrap_or_else(|| panic!("{} accepted", f));
        assert!(m.contains(needle), "{}: {}", f, m);
    }
}
#[test]
fn method_misuse_is_rejected() {
    for (f, needle) in [
        ("m1", "`Atomic<bool>` has `fetchAnd`, `fetchOr` and `fetchXor` only"),
        ("m2", "no method named `frobnicate` on `Atomic<i32>`"),
        ("m3", "takes 2 value argument(s)"),
    ] {
        let m = build(&root().join("tests/golden13/rej").join(format!("{}.mtn", f))).err().unwrap_or_else(|| panic!("{} accepted", f));
        assert!(m.contains(needle), "{}: {}", f, m);
    }
}
#[test]
fn sharing_rules_are_still_enforced() {
    for (f, needle) in [
        ("t1", "this task may outlive `n`"),                  // a plain `mut` shared across spawn
        ("t2", "use of moved value `a`"),                     // an Atomic moved into two tasks without Arc
        ("t3", "is `Send` only when the pointee `Rc<i32>` is `Send`"),   // Atomic<*mut Rc<i32>> is not Send
        ("t4", "use of moved value `a`"),                     // an Atomic taken by value into two closures
    ] {
        let m = build(&root().join("tests/golden13/rej").join(format!("{}.mtn", f))).err().unwrap_or_else(|| panic!("{} accepted", f));
        assert!(m.contains(needle), "{}: {}", f, m);
    }
}
#[test]
fn atomic_in_a_packed_struct_is_rejected() {
    rejected("@pack struct P { a: i8, n: Atomic<u32> }\nfn main() { }", "is `@pack` and holds an `Atomic<T>`");
}
#[test]
fn user_ordering_enum_is_left_alone() {
    // a program that declares its own `Ordering` keeps it (the prelude one is skipped)
    assert_eq!(run_src("enum Ordering { Less, Equal, Greater }\nfn main() { let o = Ordering::Less; match o { Ordering::Less => println(1), _ => println(2) } }"), "1\n");
}
#[test]
fn atomic_is_sync_and_send_for_integers_shared_through_arc() {
    // Arc<Atomic<u64>> crosses spawn (atomic_counter above); a bare Atomic<i32> field in a struct shared via Arc too
    assert_eq!(run_src("struct S { n: Atomic<i32> }\nfn main() { let s = Arc::new(S { n: Atomic::new(1) }); let c = s.clone(); let h = spawn move { c.n.fetchAdd(1) }; println(h.join()); println(s.n.load()); }"), "1\n2\n");
}
#[test]
fn result_is_ok_and_is_err() {
    assert_eq!(run_src("fn main() { let a: Atomic<i32> = Atomic::new(1); println(a.compareAndSwap(1, 2).isOk()); println(a.compareAndSwap(1, 3).isErr()); }"), "true\ntrue\n");
}

#[test]
fn doc14_section2_cas_retry_loop_example_from_four_os_threads() {
    golden("atomic_doc14_increment");
}
