//! Phase 13 M6: `volatile` (Document 23 §17.17), the audit of "Phase 13" in user-facing messages, the IR tests
//! (atomic orderings, guard drop, `dyn` vtable drop slot), the load test and the banner.
//! Every program is built in its own unique directory and every run has a watchdog (a hang fails the test).
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mtn_p13m6_{}_{}_{}", std::process::id(), tag, COUNTER.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn have_valgrind() -> bool {
    Command::new("valgrind").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn mtnc() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mtnc"))
}

fn write_src(src: &str) -> std::path::PathBuf {
    let f = unique_dir("src").join("p.mtn");
    std::fs::write(&f, src).unwrap();
    f
}

fn build(src: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let exe = unique_dir("exe").join("out_exe");
    let out = mtnc().arg("build").arg(src).arg("-o").arg(&exe).output().unwrap();
    if out.status.success() && exe.exists() {
        Ok(exe)
    } else {
        Err(String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr))
    }
}

fn run_exe_limit(exe: &std::path::Path, workers: &str, valgrind: bool, limit: u64) -> (String, i32) {
    let mut cmd = if valgrind {
        let mut c = Command::new("valgrind");
        c.args(["--leak-check=full", "--errors-for-leak-kinds=definite", "--error-exitcode=99", "-q"]).arg(exe);
        c
    } else {
        Command::new(exe)
    };
    let mut child = cmd.env("MTN_WORKERS", workers).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let t1 = std::thread::spawn(move || {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut so, &mut s).ok();
        s
    });
    let t2 = std::thread::spawn(move || {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut se, &mut s).ok();
        s
    });
    let start = Instant::now();
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            let out = t1.join().unwrap();
            let _ = t2.join();
            return (out, st.code().unwrap_or(-1));
        }
        if start.elapsed() > Duration::from_secs(limit) {
            let _ = child.kill();
            let _ = child.wait();
            let out = t1.join().unwrap_or_default();
            panic!("HANG: the program did not finish within {}s at MTN_WORKERS={} (partial output: {:?})", limit, workers, out);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn run_exe(exe: &std::path::Path, workers: &str) -> (String, i32) {
    run_exe_limit(exe, workers, false, 120)
}

fn run(src: &str, want: &str) {
    let exe = build(&write_src(src)).unwrap_or_else(|e| panic!("build failed: {}\n{}", e, src));
    let (o, c) = run_exe(&exe, "2");
    assert_eq!((o.as_str(), c), (want, 0), "{}", src);
}

fn run_valgrind(src: &str, want: &str) {
    let exe = build(&write_src(src)).unwrap_or_else(|e| panic!("build failed: {}\n{}", e, src));
    let (o, c) = run_exe(&exe, "2");
    assert_eq!((o.as_str(), c), (want, 0), "{}", src);
    if have_valgrind() {
        let (o, c) = run_exe_limit(&exe, "2", true, 600);
        assert_eq!((o.as_str(), c), (want, 0), "under valgrind\n{}", src);
    }
}

/// `mtnc build` and `mtnc check` must both reject the program with `needle` in the message
fn rejected(src: &str, needle: &str) {
    match build(&write_src(src)) {
        Ok(_) => panic!("build should have rejected (`{}`):\n{}", needle, src),
        Err(m) => assert!(m.contains(needle), "build: expected `{}` in:\n{}\nprogram:\n{}", needle, m, src),
    }
    let f = write_src(src);
    let chk = mtnc().arg("check").arg(&f).output().unwrap();
    let all = String::from_utf8_lossy(&chk.stdout).to_string() + &String::from_utf8_lossy(&chk.stderr);
    assert!(!chk.status.success(), "check should reject:\n{}", src);
    assert!(all.contains(needle), "check: expected `{}` in:\n{}\nprogram:\n{}", needle, all, src);
}

fn emit_ir(src: &str) -> String {
    let f = write_src(src);
    let out = mtnc().arg("build").arg(&f).arg("--emit-ir").output().unwrap();
    assert!(out.status.success(), "emit-ir failed: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).to_string()
}

// ---------------------------------------------------------------- volatile: positive

/// (type, value literal, expression that prints as the same text)
const SCALARS: &[(&str, &str, &str, &str)] = &[
    ("i8", "-5", "v", "-5"),
    ("i16", "-300", "v", "-300"),
    ("i32", "-70000", "v", "-70000"),
    ("i64", "-5000000000", "v", "-5000000000"),
    ("isize", "-123456", "v", "-123456"),
    ("u8", "200", "v", "200"),
    ("u16", "60000", "v", "60000"),
    ("u32", "4000000000", "v", "4000000000"),
    ("u64", "10000000000000000000", "v", "10000000000000000000"),
    ("usize", "123456", "v", "123456"),
    ("f32", "1.5", "v", "1.5"),
    ("f64", "2.25", "v", "2.25"),
    ("bool", "true", "v", "true"),
    ("char", "'x'", "v as u32", "120"),
];

fn scalar_program(ty: &str, lit: &str, shown: &str, via_const: bool) -> String {
    let read_ptr = if via_const { "let c = p as *const @T@; let v: @T@ = volatile *c;" } else { "let v: @T@ = volatile *p;" };
    format!(
        "fn main() {{\n    unsafe {{\n        let p = alloc(16, 16) as *mut @T@;\n        volatile *p = @L@;\n        {}\n        println(@S@);\n        dealloc(p as *mut u8, 16, 16);\n    }}\n}}\n",
        read_ptr
    )
    .replace("@T@", ty)
    .replace("@L@", lit)
    .replace("@S@", shown)
}

#[test]
fn volatile_write_and_read_round_trip_for_every_scalar_type_through_mut_pointers() {
    for (ty, lit, shown, want) in SCALARS {
        run(&scalar_program(ty, lit, shown, false), &format!("{}\n", want));
    }
}

#[test]
fn volatile_read_through_const_pointers_for_every_scalar_type() {
    for (ty, lit, shown, want) in SCALARS {
        run(&scalar_program(ty, lit, shown, true), &format!("{}\n", want));
    }
}

#[test]
fn volatile_128_bit_integers_and_raw_pointers_are_scalars_too() {
    run(
        "fn main() { unsafe { let p = alloc(16, 16) as *mut i128; volatile *p = 123456789012345678; let v: i128 = volatile *p; println(v as i64); dealloc(p as *mut u8, 16, 16); } }",
        "123456789012345678\n",
    );
    // a pointer stored through a pointer: `*mut *mut i32`
    run(
        "fn main() { unsafe { let cell = alloc(8, 8) as *mut *mut i32; let target = alloc(4, 4) as *mut i32; volatile *target = 7; volatile *cell = target; let got = volatile *cell; println(volatile *got); println(got == target); dealloc(target as *mut u8, 4, 4); dealloc(cell as *mut u8, 8, 8); } }",
        "7\ntrue\n",
    );
}

#[test]
fn volatile_in_a_loop_through_an_allocated_buffer() {
    let src = "fn main() {\n    unsafe {\n        let buf = alloc(80, 8) as *mut i64;\n        let mut i: usize = 0;\n        while i < 10 {\n            let slot = buf.add(i);\n            volatile *slot = (i as i64) * 3;\n            i = i + 1;\n        }\n        let mut sum: i64 = 0;\n        let mut j: usize = 0;\n        while j < 10 {\n            sum = sum + volatile *buf.add(j);\n            j = j + 1;\n        }\n        println(sum);\n        dealloc(buf as *mut u8, 80, 8);\n    }\n}\n";
    run_valgrind(src, "135\n");
}

#[test]
fn volatile_reads_a_static_through_a_const_pointer_and_a_polled_flag_works() {
    // a read of an immutable static; and a "device register" polled in a loop and changed by another thread
    run(
        "static LIMIT: i32 = 99;\nfn main() { unsafe { let p = &LIMIT as *const i32; println(volatile *p); } }",
        "99\n",
    );
    let src = r#"fn setter(addr: usize) -> i64 {
    unsafe { let p = addr as *mut i32; volatile *p = 1; }
    0
}
fn main() {
    unsafe {
        let reg = alloc(4, 4) as *mut i32;
        volatile *reg = 0;
        let addr = reg as usize;
        let t = thread::spawnOS(move || setter(addr));
        // an atomic-free poll would be undefined behaviour in general (volatile is not atomic, Doc 23 §17.17);
        // the thread is joined first, so this only shows that the read is emitted and returns the written value
        t.join();
        println(volatile *reg);
        dealloc(reg as *mut u8, 4, 4);
    }
}
"#;
    run(src, "1\n");
}

#[test]
fn volatile_inside_expressions_and_as_a_statement_value() {
    run(
        "fn main() { unsafe { let p = alloc(8, 8) as *mut i32; volatile *p = 5; let x = (volatile *p) * 2 + 1; println(x); println(-(volatile *p)); dealloc(p as *mut u8, 8, 8); } }",
        "11\n-5\n",
    );
}

// ---------------------------------------------------------------- volatile: the IR

#[test]
fn ir_volatile_accesses_are_volatile_and_naturally_aligned_and_plain_ones_are_not() {
    // (type, expected LLVM type, alignment)
    let cases = [("i8", "i8", 1), ("i16", "i16", 2), ("i32", "i32", 4), ("i64", "i64", 8), ("u32", "i32", 4), ("f32", "float", 4), ("f64", "double", 8)];
    for (ty, llty, align) in cases {
        let src = format!(
            "fn main() {{ unsafe {{ let p = alloc(16, 16) as *mut {ty}; volatile *p = 1 as {ty}; let v = volatile *p; println(v as i64); dealloc(p as *mut u8, 16, 16); }} }}\n"
        );
        let ir = emit_ir(&src);
        assert!(ir.contains(&format!("store volatile {} ", llty)) && ir.contains(&format!(", align {}\n", align)), "{}: no volatile store in\n{}", ty, ir);
        assert!(ir.lines().any(|l| l.contains(&format!("load volatile {}, ptr", llty)) && l.trim_end().ends_with(&format!("align {}", align))), "{}: no `load volatile {}` with align {}:\n{}", ty, llty, align, ir);
        // the same program without `volatile` has none
        let plain = src.replace("volatile *p", "*p");
        let ir2 = emit_ir(&plain);
        assert!(!ir2.contains("volatile"), "{}: plain accesses must not be volatile:\n{}", ty, ir2);
    }
    // a pointer is stored/loaded as `ptr`, a bool as `i1` or `i8`, both volatile
    let ir = emit_ir("fn main() { unsafe { let cell = alloc(8, 8) as *mut *mut i32; let t = alloc(4, 4) as *mut i32; volatile *cell = t; let q = volatile *cell; println(q == t); dealloc(t as *mut u8, 4, 4); dealloc(cell as *mut u8, 8, 8); } }\n");
    assert!(ir.contains("store volatile ptr ") && ir.contains("load volatile ptr, ptr"), "{}", ir);
    let ir = emit_ir("fn main() { unsafe { let b = alloc(1, 1) as *mut bool; volatile *b = true; let v = volatile *b; println(v); dealloc(b as *mut u8, 1, 1); } }\n");
    assert!(ir.lines().any(|l| l.contains("store volatile i1 ") || l.contains("store volatile i8 ")), "{}", ir);
    assert!(ir.lines().any(|l| l.contains("load volatile i1,") || l.contains("load volatile i8,")), "{}", ir);
}

#[test]
fn ir_two_volatile_reads_of_the_same_pointer_are_both_kept() {
    // a plain optimiser would merge them; two `load volatile` instructions must be in the IR
    let ir = emit_ir("fn main() { unsafe { let p = alloc(4, 4) as *mut i32; volatile *p = 3; let a = volatile *p; let b = volatile *p; println(a + b); dealloc(p as *mut u8, 4, 4); } }\n");
    assert_eq!(ir.matches("load volatile i32").count(), 2, "{}", ir);
}

// ---------------------------------------------------------------- volatile: rejections (check and build agree)

#[test]
fn volatile_rejections() {
    // outside `unsafe`
    rejected("fn main() { let p = 0 as usize as *mut i32; let v = volatile *p; println(v); }", "only allowed inside an `unsafe` block");
    rejected("fn main() { let p = 0 as usize as *mut i32; volatile *p = 1; }", "only allowed inside an `unsafe` block");
    // the operand must be a raw pointer
    rejected("fn main() { unsafe { let x: i32 = 5; let v = volatile *x; println(v); } }", "needs a raw pointer");
    rejected("fn main() { unsafe { let x: i32 = 5; let r = &x; let v = volatile *r; println(v); } }", "needs a raw pointer");
    // the pointee must be a scalar: struct, array, String
    rejected("struct P { a: i32 }\nfn main() { unsafe { let p = alloc(8, 8) as *mut P; let v = volatile *p; println(v.a); } }", "scalar types");
    rejected("fn main() { unsafe { let p = alloc(16, 8) as *mut [i32; 4]; let v = volatile *p; println(1); } }", "scalar types");
    rejected("fn main() { unsafe { let p = alloc(24, 8) as *mut String; let v = volatile *p; println(1); } }", "scalar types");
    rejected("struct P { a: i32 }\nfn main() { unsafe { let p = alloc(8, 8) as *mut P; volatile *p = P { a: 1 }; } }", "scalar types");
    // a write needs `*mut T`
    rejected("fn main() { unsafe { let p = alloc(4, 4) as *mut i32; let c = p as *const i32; volatile *c = 1; } }", "cannot write through a `*const T` pointer");
    // compound assignment
    rejected("fn main() { unsafe { let p = alloc(4, 4) as *mut i32; volatile *p += 1; } }", "compound assignment");
    rejected("fn main() { unsafe { let p = alloc(4, 4) as *mut i32; volatile *p -= 1; } }", "compound assignment");
    // not a variable or field modifier; `volatile` is a keyword and cannot name anything
    rejected("fn main() { let volatile = 1; println(volatile); }", "volatile");
    rejected("fn f(volatile: i32) -> i32 { volatile }\nfn main() { println(f(1)); }", "volatile");
    rejected("fn main() { unsafe { let p = alloc(4, 4) as *mut i32; let v = volatile p; } }", "`volatile *p`");
    rejected("fn main() { let x = volatile 5; }", "`volatile *p`");
    // a wrong value type is reported by the ordinary assignment check
    rejected("fn main() { unsafe { let p = alloc(4, 4) as *mut i32; volatile *p = true; } }", "expected `i32`");
}

// ---------------------------------------------------------------- audit: no "Phase 13" in user-facing messages, banner

fn collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
            out.push(p);
        }
    }
}

#[test]
fn a1_no_phase_13_in_user_facing_errors() {
    let mut files = Vec::new();
    collect_rs(&root().join("src"), &mut files);
    assert!(!files.is_empty());
    let mut bad = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        for (i, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            // only string literals count; the version banner in main.rs is not an error
            if line.contains("Phase 13") && line.contains('"') && !line.contains("mtnc 1.0.0-dev") {
                bad.push(format!("{}:{}: {}", f.display(), i + 1, t));
            }
        }
    }
    assert!(bad.is_empty(), "\"Phase 13\" must not appear in a user-facing error string:\n{}", bad.join("\n"));
}

#[test]
fn a2_remaining_gates_name_a_later_phase() {
    // every "not yet supported by codegen — Phase N" in the compiler names a phase after 13
    let mut files = Vec::new();
    collect_rs(&root().join("src"), &mut files);
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if let Some(pos) = line.find("not yet supported by codegen — Phase ") {
                let n: String = line[pos + "not yet supported by codegen — Phase ".len()..].chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(n) = n.parse::<u32>() {
                    assert!(n > 13, "{}:{}: a gate names Phase {} which is implemented", f.display(), i + 1, n);
                }
            }
        }
    }
}

#[test]
fn a3_version_banner() {
    let o = mtnc().arg("--version").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "mtnc 1.0.0-dev (Phase 13 — atomics, locks, channels, select)");
    assert!(o.status.success());
}

// ---------------------------------------------------------------- IR: atomic orderings, one test per ordering

/// Builds `probe(a: borrow Atomic<u64>)` with `body` and returns the IR of `mtn_probe`.
fn probe_ir(body: &str) -> String {
    let src = format!("fn probe(a: borrow Atomic<u64>) -> u64 {{\n{}\n    return 0;\n}}\nfn main() {{ let a: Atomic<u64> = Atomic::new(0); println(probe(borrow a)); }}\n", body);
    let ir = emit_ir(&src);
    function_body(&ir, "mtn_probe")
}

fn function_body(ir: &str, name: &str) -> String {
    let needle = format!("@{}(", name);
    let at = ir.lines().scan(0usize, |pos, l| { let p = *pos; *pos += l.len() + 1; Some((p, l)) })
        .find(|(_, l)| l.starts_with("define") && l.contains(&needle)).map(|(p, _)| p).unwrap_or_else(|| panic!("no function {}", name));
    let start = at;
    let end = ir[start..].find("\n}\n").map(|e| start + e).unwrap_or(ir.len());
    ir[start..end].to_string()
}

/// Every operation family that accepts `ord` carries exactly `llvm` (and the CAS failure ordering `fail`).
fn check_ordering(ord: &str, llvm: &str, fail: &str, load_ok: bool, store_ok: bool) {
    if load_ok {
        let f = probe_ir(&format!("    let v = a.load(Ordering::{});", ord));
        assert!(f.contains(&format!("load atomic i64, ptr %")) && f.contains(&format!(" {}, align 8", llvm)), "load {}:\n{}", ord, f);
    }
    if store_ok {
        let f = probe_ir(&format!("    a.store(1, Ordering::{});", ord));
        assert!(f.contains("store atomic i64 1, ptr %") && f.contains(&format!(" {}, align 8", llvm)), "store {}:\n{}", ord, f);
    }
    for (m, op) in [("fetchAdd", "add"), ("fetchSub", "sub"), ("fetchAnd", "and"), ("fetchOr", "or"), ("fetchXor", "xor"), ("swap", "xchg")] {
        let f = probe_ir(&format!("    a.{}(2, Ordering::{});", m, ord));
        assert!(f.contains(&format!("atomicrmw {} ptr %", op)) && f.contains(&format!(" i64 2 {}, align 8", llvm)), "{} {}:\n{}", m, ord, f);
    }
    let f = probe_ir(&format!("    let r = a.compareAndSwap(1, 2, Ordering::{});", ord));
    assert!(f.contains("cmpxchg ptr %") && f.contains(&format!(" i64 1, i64 2 {} {}, align 8", llvm, fail)), "cas {}:\n{}", ord, f);
}

#[test]
fn ir_ordering_relaxed_on_every_operation_family() {
    check_ordering("Relaxed", "monotonic", "monotonic", true, true);
}
#[test]
fn ir_ordering_acquire_on_every_operation_family() {
    check_ordering("Acquire", "acquire", "acquire", true, false);
}
#[test]
fn ir_ordering_release_on_every_operation_family() {
    check_ordering("Release", "release", "monotonic", false, true);
}
#[test]
fn ir_ordering_acqrel_on_every_operation_family() {
    check_ordering("AcqRel", "acq_rel", "acquire", false, false);
}
#[test]
fn ir_ordering_seqcst_on_every_operation_family_and_as_the_default() {
    check_ordering("SeqCst", "seq_cst", "seq_cst", true, true);
    let f = probe_ir("    let v = a.load();\n    a.store(1);\n    a.fetchAdd(1);\n    let r = a.compareAndSwap(1, 2);");
    assert!(f.contains("load atomic i64, ptr %") && f.contains(" seq_cst, align 8"), "{}", f);
    assert!(f.contains("store atomic i64 1, ptr %") && f.contains("atomicrmw add ptr %"), "{}", f);
    assert!(f.contains(" seq_cst seq_cst, align 8"), "{}", f);
}

// ---------------------------------------------------------------- IR: lock guard drop, dyn vtable drop slot

#[test]
fn ir_mutex_guard_drop_calls_unlock_exactly_once() {
    let ir = emit_ir("fn probe(m: borrow Mutex<i32>) -> i32 {\n    let g = m.lock();\n    return *g;\n}\nfn main() { let m = Mutex::new(1); println(probe(borrow m)); }\n");
    let f = function_body(&ir, "mtn_probe");
    assert_eq!(f.matches("@__mtn_lock_mutex_lock(").count(), 1, "{}", f);
    assert_eq!(f.matches("@__mtn_drop_MutexGuard_i32_(").count(), 1, "one drop call per guard on the single exit path:\n{}", f);
    // the guard's drop function releases the lock once, only when it still holds it
    let d = function_body(&ir, "__mtn_drop_MutexGuard_i32_");
    assert_eq!(d.matches("@__mtn_lock_mutex_unlock(").count(), 1, "{}", d);
    assert!(d.contains("icmp ne ptr"), "the release is guarded by a held check:\n{}", d);
    // an explicit `drop(g)` releases early and the scope end does not release again (the flag/null marks it)
    let ir = emit_ir("fn probe(m: borrow Mutex<i32>) -> i32 {\n    let g = m.lock();\n    let v = *g;\n    drop(g);\n    return v;\n}\nfn main() { let m = Mutex::new(1); println(probe(borrow m)); }\n");
    let f = function_body(&ir, "mtn_probe");
    // the explicit drop moves the guard out (the variable is set to null first), so the scope-end drop of `g` sees
    // a null lock and releases nothing: the lock is released exactly once
    let first = f.find("@__mtn_drop_MutexGuard_i32_(ptr %dropped)").expect("explicit drop call");
    let nulled = f.find("store ptr null, ptr %g").expect("moved-out marker");
    assert!(nulled < first, "the guard variable is nulled before the explicit drop:\n{}", f);
    assert_eq!(f.matches("@__mtn_drop_MutexGuard_i32_(").count(), 2, "{}", f);
    run("fn main() { let m = Mutex::new(1); { let g = m.lock(); let v = *g; drop(g); println(v); } match m.tryLock() { Some(x) => println(*x), None => println(\"stuck\") } }\n", "1\n1\n");
}

#[test]
fn ir_rwlock_guards_drop_once() {
    let ir = emit_ir("fn probe(l: borrow RwLock<i32>) -> i32 {\n    let r = l.read();\n    return *r;\n}\nfn main() { let l = RwLock::new(1); println(probe(borrow l)); }\n");
    let f = function_body(&ir, "mtn_probe");
    let drops = f.lines().filter(|l| l.contains("call void @__mtn_drop_") && l.contains("Guard")).count();
    assert_eq!(drops, 1, "{}", f);
}

#[test]
fn ir_dyn_vtable_has_the_drop_slot_that_calls_the_user_drop() {
    let src = "trait Shape { fn area(borrow self) -> i32; }\nstruct Sq { s: i32 }\nimpl Shape for Sq { fn area(borrow self) -> i32 { return self.s * self.s; } }\nimpl Drop for Sq { fn drop(borrow mut self) { println(\"dropped\"); } }\nfn main() { let b: Box<dyn Shape> = Box::new(Sq { s: 3 }); println(b.area()); }\n";
    let ir = emit_ir(src);
    // slot 0 of the vtable is the drop glue, slots 1 and 2 are size and alignment, then the methods
    let vt = ir.lines().find(|l| l.starts_with("@__mtn_vtable_Sq_Shape")).expect("vtable");
    assert!(vt.contains("{ ptr @__mtn_drop_Sq, i64 8, i64 4, ptr @\"mtn_Sq::Shape::area\" }"), "{}", vt);
    // the drop glue calls the user's `Drop::drop` exactly once, behind the live flag
    let g = function_body(&ir, "__mtn_drop_Sq");
    assert_eq!(g.matches("@\"mtn_Sq::Drop::drop\"(").count(), 1, "{}", g);
    assert!(g.contains("store i8 0, ptr %livep"), "the live flag is cleared before the user drop runs:\n{}", g);
    // and the program prints it exactly once
    run("trait Shape { fn area(borrow self) -> i32; }\nstruct Sq { s: i32 }\nimpl Shape for Sq { fn area(borrow self) -> i32 { return self.s * self.s; } }\nimpl Drop for Sq { fn drop(borrow mut self) { println(\"dropped\"); } }\nfn main() { let b: Box<dyn Shape> = Box::new(Sq { s: 3 }); println(b.area()); }\n", "9\ndropped\n");
}

// ---------------------------------------------------------------- load: many tasks blocked on channels at MTN_WORKERS=1

/// `n` consumer tasks, each blocked in `recv()` on its own channel; main then sends one value to each (in reverse
/// order of creation) and sums the answers. A blocked POOL worker is compensated by an extra worker thread; at most
/// MAX_EXTRA (2048) extras exist, beyond that a blocked worker simply sleeps (register 71: the cap is on threads,
/// not on tasks, so programs with more blocked tasks than the cap still finish as long as the senders do not
/// themselves wait for those tasks).
fn blocked_consumers(n: u32) -> String {
    format!(
        "fn consumer(rx: Receiver<i64>) -> i64 {{
    match rx.recv() {{
        Some(v) => v,
        None => 0,
    }}
}}
fn run(i: i64, n: i64) -> i64 {{
    if i > n {{ return 0; }}
    let (tx, rx) = channel::<i64>(1);
    let h = spawn move {{ consumer(rx) }};
    let rest = run(i + 1, n);
    tx.send(i);
    h.join() + rest
}}
fn main() {{ println(run(1, {})); }}
",
        n
    )
}

#[test]
fn load_1000_tasks_blocked_on_channels_with_one_worker() {
    // 1000 blocked tasks stay below the compensation cap: 1000 extra workers run them
    let exe = build(&write_src(&blocked_consumers(1000))).unwrap_or_else(|e| panic!("{}", e));
    let t = Instant::now();
    let (o, c) = run_exe_limit(&exe, "1", false, 120);
    assert_eq!((o.as_str(), c), ("500500\n", 0));
    eprintln!("1000 blocked channel tasks at MTN_WORKERS=1: {:?}", t.elapsed());
}

#[test]
fn load_3000_tasks_blocked_on_channels_with_one_worker_beyond_the_cap() {
    // 3000 > MAX_EXTRA: the workers beyond the cap just sleep, the buffered sends still go through and every task finishes
    let exe = build(&write_src(&blocked_consumers(3000))).unwrap_or_else(|e| panic!("{}", e));
    let t = Instant::now();
    let (o, c) = run_exe_limit(&exe, "1", false, 120);
    assert_eq!((o.as_str(), c), ("4501500\n", 0));
    eprintln!("3000 blocked channel tasks at MTN_WORKERS=1: {:?}", t.elapsed());
}
