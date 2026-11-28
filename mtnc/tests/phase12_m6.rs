//! Phase 12 M6 — final integration tests (audit, load, IR, matrix gaps).
#![allow(dead_code)]
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn mtnc() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mtnc"))
}

fn scratch(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("mtn_m6_{}_{}_{}", tag, std::process::id(), n));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Compile + run; returns (exit code, stdout, stderr).
fn run_src_env(src: &str, envs: &[(&str, &str)]) -> (i32, String, String) {
    let d = scratch("run");
    let f = d.join("main.mtn");
    std::fs::write(&f, src).unwrap();
    let mut c = Command::new(mtnc());
    c.arg("run").arg(&f);
    for (k, v) in envs {
        c.env(k, v);
    }
    let o = c.output().unwrap();
    let _ = std::fs::remove_dir_all(&d);
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn run_src(src: &str) -> (i32, String, String) {
    run_src_env(src, &[])
}

fn emit_ir(src: &str) -> String {
    let d = scratch("ir");
    let f = d.join("main.mtn");
    std::fs::write(&f, src).unwrap();
    let o = Command::new(mtnc()).arg("build").arg(&f).arg("--emit-ir").arg("-o").arg(d.join("out.bin")).output().unwrap();
    assert!(o.status.success(), "build failed: {}", String::from_utf8_lossy(&o.stderr));
    let mut ir = String::new();
    for e in std::fs::read_dir(&d).unwrap() {
        let p = e.unwrap().path();
        if p.extension().map(|x| x == "ll").unwrap_or(false) {
            ir.push_str(&std::fs::read_to_string(&p).unwrap());
        }
    }
    if ir.is_empty() {
        ir = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    }
    let _ = std::fs::remove_dir_all(&d);
    ir
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
            out.push(p);
        }
    }
}

// ---- (a) audit ---------------------------------------------------------

#[test]
fn a1_generic_fn_value_runs() {
    let (c, o, e) = run_src("fn identity<T>(x: T) -> T { return x; }\nfn apply(f: fn(i32) -> i32, v: i32) -> i32 { return f(v); }\nfn main() {\n    let f = identity::<i32>;\n    println(f(5));\n    println(apply(identity::<i32>, 7));\n}\n");
    assert_eq!(c, 0, "{}", e);
    assert_eq!(o, "5\n7\n");
}

#[test]
fn a2_generic_fn_value_two_types_and_missing_args() {
    let (c, o, e) = run_src("fn identity<T>(x: T) -> T { return x; }\nfn main() {\n    let a = identity::<i32>;\n    let b = identity::<String>;\n    println(a(1));\n    println(b(String::from(\"x\")));\n}\n");
    assert_eq!(c, 0, "{}", e);
    assert_eq!(o, "1\nx\n");
    let (c, _, e) = run_src("fn pair<A, B>(a: A, b: B) -> A { return a; }\nfn main() {\n    let f = pair::<i32>;\n}\n");
    assert_ne!(c, 0);
    assert!(e.contains("all 2 type argument"), "{}", e);
}

#[test]
fn a3_no_phase_12_in_user_facing_errors() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    let mut bad = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        for (i, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            // only string literals count; the version banner in main.rs is not an error
            if line.contains("Phase 12") && line.contains('"') && !line.contains("mtnc 1.0.0-dev") {
                bad.push(format!("{}:{}: {}", f.display(), i + 1, t));
            }
        }
    }
    assert!(bad.is_empty(), "\"Phase 12\" must not appear in a user-facing error string:\n{}", bad.join("\n"));
}

#[test]
fn e1_version_banner() {
    let o = Command::new(mtnc()).arg("--version").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "mtnc 1.0.0-dev (Phase 13 — atomics, locks, channels, select)");
}

// ---- (b) load test: 10,000 tasks SIMULTANEOUSLY SUSPENDED (Doc 25 §2.3, Phase 12 exit criterion)

#[test]
fn b1_load_10000_simultaneously_suspended_async_tasks_on_four_workers() {
    // Every task's FIRST await is a long sleep, so all 10,000 are alive (suspended) at the same
    // moment; they then yield a couple of times and sleep briefly before finishing.
    let src = "async fn work(i: i32) -> i32 {
    await sleepMs(2500);
    await yieldNow();
    await yieldNow();
    await sleepMs(5);
    return i;
}
async fn spawner(n: i32) {
    let mut i = 0;
    while i < n { let h = spawn work(i); drop(h); i = i + 1; }
}
async fn main() { await spawner(10000); }";
    let t0 = std::time::Instant::now();
    let (code, _out, err) = run_src_env(src, &[("MTN_WORKERS", "4"), ("MTN_RT_STATS", "1")]);
    let secs = t0.elapsed().as_secs_f64();
    assert_eq!(code, 0, "stderr: {}", err);
    let lines: Vec<&str> = err.lines().filter(|l| l.starts_with("rt: ")).collect();
    assert_eq!(lines.len(), 1, "exactly one stats line expected: {:?}", err);
    let l = lines[0];
    let field = |name: &str| -> u64 {
        l.split_whitespace()
            .find_map(|w| w.strip_prefix(&format!("{}=", name)))
            .unwrap_or_else(|| panic!("no {} in {}", name, l))
            .parse()
            .unwrap()
    };
    assert_eq!(field("workers"), 4, "{}", l);
    let spawned = field("spawned");
    assert!(spawned >= 10000, "{}", l);
    assert_eq!(field("completed"), spawned, "{}", l);
    assert!(field("peak_live") >= 10000, "{}", l);
    // lower bound only: the first sleep alone takes ~2.5 s
    assert!(secs >= 1.0, "finished suspiciously fast ({} s): {}", secs, l);
    eprintln!("LOAD: {} wall={:.2}s", l, secs);
}

// ---- (c) IR tests -------------------------------------------------------

/// Names of all functions defined in the IR (`define ... @name(`).
fn defined(ir: &str) -> Vec<String> {
    ir.lines()
        .filter(|l| l.starts_with("define "))
        .filter_map(|l| {
            let at = l.find('@')?;
            let rest = &l[at + 1..];
            let end = rest.find('(')?;
            Some(rest[..end].trim_matches('"').to_string())
        })
        .collect()
}

/// Text of one defined function (from its `define` line to the closing `}`).
fn body_of(ir: &str, name: &str) -> String {
    let pat = format!("@{}(", name);
    let pat_q = format!("@\"{}\"(", name);
    let mut out = String::new();
    let mut inside = false;
    for l in ir.lines() {
        if !inside && l.starts_with("define ") && (l.contains(&pat) || l.contains(&pat_q)) {
            inside = true;
        }
        if inside {
            out.push_str(l);
            out.push('\n');
            if l == "}" {
                break;
            }
        }
    }
    assert!(!out.is_empty(), "function {} not found", name);
    out
}

fn closure_fns(ir: &str) -> Vec<String> {
    defined(ir).into_iter().filter(|n| n.starts_with("__mtn_closure_") && !n.ends_with("_drop")).collect()
}

fn closure_drops(ir: &str) -> Vec<String> {
    defined(ir).into_iter().filter(|n| n.starts_with("__mtn_closure_") && n.ends_with("_drop")).collect()
}

#[test]
fn c1_closure_env_layout_for_a_scalar_capture() {
    let ir = emit_ir("fn mk(n: i32) -> fn(i32) -> i32 { return move |x: i32| x + n; }\nfn main() { let f = mk(2); println(f(3)); }\n");
    // env = { drop glue pointer, called flag, captured i32 }, 24 bytes, 8-aligned
    assert!(ir.contains("{ ptr, i64, i32 }"), "env layout missing");
    assert!(ir.contains("call ptr @__mtn_alloc(i64 24, i64 8)"));
    assert_eq!(closure_fns(&ir).len(), 1);
    assert_eq!(closure_drops(&ir).len(), 1);
}

#[test]
fn c2_closure_env_with_string_has_drop_glue_exactly_once() {
    let ir = emit_ir("fn main() { let name = String::from(\"hi\"); let c = move || { println(name); }; c(); }\n");
    let drops = closure_drops(&ir);
    assert_eq!(drops.len(), 1, "{:?}", drops);
    let b = body_of(&ir, &drops[0]);
    assert!(b.contains("{ ptr, i64, { ptr, i64, i64 } }"), "{}", b);
    assert_eq!(b.matches("call void @__mtn_drop_String").count(), 1, "{}", b);
    assert_eq!(b.matches("call void @__mtn_dealloc(ptr %0, i64 40, i64 8)").count(), 1, "{}", b);
}

#[test]
fn c3_every_function_is_defined_once_and_one_drop_per_env_type() {
    let ir = emit_ir("fn main() {\n  let a = String::from(\"a\");\n  let n = 5;\n  let c1 = move || { println(a); };\n  let c2 = move || { println(n); };\n  c1();\n  c2();\n}\n");
    let names = defined(&ir);
    let mut sorted = names.clone();
    sorted.sort();
    let before = sorted.len();
    sorted.dedup();
    assert_eq!(before, sorted.len(), "a function is defined twice: {:?}", names);
    assert_eq!(closure_fns(&ir).len(), 2);
    assert_eq!(closure_drops(&ir).len(), 2);
}

#[test]
fn c4_generator_and_async_frames_use_the_internal_allocator_only() {
    let ir = emit_ir("fn gen(n: i32) -> Generator<i32> { let mut i = 0; while i < n { yield i; i = i + 1; } }\nasync fn two(x: i32) -> i32 { await yieldNow(); return x + 1; }\nasync fn main() {\n  let mut g = gen(3);\n  match g.next() { Some(v) => println(v), None => println(0) }\n  println(await two(1));\n}\n");
    for f in ["mtn_gen", "mtn_two"] {
        let b = body_of(&ir, f);
        assert!(b.contains("@llvm.coro.id"), "{}", f);
        assert!(b.contains("@llvm.coro.size.i64"), "{}", f);
        assert_eq!(b.matches("call ptr @__mtn_alloc(i64 %").count(), 1, "frame allocated once: {}", f);
        assert!(b.contains("@llvm.coro.free"), "{}", f);
        assert_eq!(b.matches("call void @__mtn_dealloc(").count(), 1, "frame freed once: {}", f);
        assert!(!b.contains("@malloc") && !b.contains("@free("), "raw malloc/free in {}", f);
    }
    // the result slot (promise) of `async fn two -> i32` is an i32
    assert!(body_of(&ir, "mtn_two").contains("%promise = alloca i32"));
    // the only direct malloc call in the whole module is inside the allocator itself
    for name in defined(&ir) {
        if name == "__mtn_alloc" {
            continue;
        }
        assert!(!body_of(&ir, &name).contains("call ptr @malloc"), "{} calls malloc directly", name);
    }
}

#[test]
fn c5_async_future_with_string_result_has_a_string_promise_and_one_drop_glue() {
    let ir = emit_ir("async fn greet() -> String { await yieldNow(); return String::from(\"x\"); }\nasync fn main() { let s = await greet(); println(s); }\n");
    assert!(body_of(&ir, "mtn_greet").contains("%promise = alloca { ptr, i64, i64 }"));
    let n = defined(&ir).iter().filter(|n| n.starts_with("__mtn_drop_Future_")).count();
    let mut names: Vec<String> = defined(&ir).into_iter().filter(|n| n.starts_with("__mtn_drop_Future_")).collect();
    names.sort();
    names.dedup();
    assert_eq!(n, names.len(), "duplicate future drop glue");
}

#[test]
fn c6_mono_dedupes_generic_async_instances() {
    let same = emit_ir("async fn id<T>(x: T) -> T { await yieldNow(); return x; }\nasync fn main() {\n  let a = await id::<i32>(1);\n  let b = await id::<i32>(2);\n  println(a + b);\n}\n");
    let n_same = defined(&same).iter().filter(|n| n.starts_with("mtn_id")).count();
    assert_eq!(n_same, 1, "{:?}", defined(&same));
    let two = emit_ir("async fn id<T>(x: T) -> T { await yieldNow(); return x; }\nasync fn main() {\n  let a = await id::<i32>(1);\n  let b = await id::<i64>(2);\n  println(a);\n  println(b);\n}\n");
    let n_two = defined(&two).iter().filter(|n| n.starts_with("mtn_id")).count();
    assert_eq!(n_two, 2, "{:?}", defined(&two));
}

#[test]
fn c7_mono_dedupes_closures_inside_generic_functions() {
    let one = emit_ir("fn run<T>(x: T) -> i32 { let k = 1; let c = move || k; return c(); }\nfn main() { println(run::<i32>(1) + run::<i32>(2)); }\n");
    assert_eq!(closure_fns(&one).len(), 1, "{:?}", defined(&one));
    assert_eq!(closure_drops(&one).len(), 1);
    let two = emit_ir("fn run<T>(x: T) -> i32 { let k = 1; let c = move || k; return c(); }\nfn main() { println(run::<i32>(1) + run::<i64>(2)); }\n");
    assert_eq!(closure_fns(&two).len(), 2, "{:?}", defined(&two));
    assert_eq!(closure_drops(&two).len(), 2);
}

#[test]
fn c8_generic_fn_value_instance_is_shared_with_calls() {
    let ir = emit_ir("fn identity<T>(x: T) -> T { return x; }\nfn main() {\n  let f = identity::<i32>;\n  println(f(1) + identity::<i32>(2));\n}\n");
    let n = defined(&ir).iter().filter(|n| n.starts_with("mtn_identity")).count();
    assert_eq!(n, 1, "{:?}", defined(&ir));
}

#[test]
fn a4_actor_message_send_rejection_names_the_handler() {
    let (c, _, e) = run_src("actor A { on ok(n: i32) { } on bad(r: Rc<i32>) { } }\nfn main() { let a = A::spawn(); }\n");
    assert_ne!(c, 0);
    assert!(e.contains("the message `bad(..)` of actor `A` must be `Send`"), "{}", e);
    assert!(e.contains("`Rc<i32>`"), "{}", e);
}

#[test]
fn a5_a_bare_generic_name_is_still_not_a_value_and_says_how_to_fix_it() {
    let (c, _, e) = run_src("fn identity<T>(x: T) -> T { return x; }\nfn apply(f: fn(i32) -> i32, v: i32) -> i32 { return f(v); }\nfn main() { println(apply(identity, 3)); }\n");
    assert_ne!(c, 0);
    assert!(e.contains("generic function `identity` cannot be used as a value"), "{}", e);
    assert!(!e.contains("Phase 12"), "{}", e);
}
