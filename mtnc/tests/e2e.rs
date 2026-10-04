//! Phase 10 end-to-end tests (Document 25 §2.3, Phase 10): real `.mtn`
//! programs are compiled to native executables with the real `mtnc` binary,
//! run, and their stdout / exit code checked; invalid programs must fail to
//! build and leave no binary behind; emitted IR is inspected for the
//! constructs Document 17 §6 and Document 5 §2.1 require; and Document 11
//! §3/§7's `try`/`catch` ≡ hand-written `?`-chain claim is checked by
//! comparing the two LLVM IR outputs.

use mtnc::codegen::Options;
use mtnc::driver;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn mtnc() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mtnc"))
}

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").join(format!("{}.mtn", name))
}

fn scratch() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("mtnc_e2e_{}_{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Run {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run_file(path: &Path, extra: &[&str]) -> Run {
    let out = mtnc().arg("run").arg(path).args(extra).output().expect("failed to start mtnc");
    Run {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        code: out.status.code().unwrap_or(-1),
    }
}

fn run_src(src: &str, extra: &[&str]) -> Run {
    let dir = scratch();
    let f = dir.join("prog.mtn");
    std::fs::write(&f, src).unwrap();
    run_file(&f, extra)
}

/// Builds `src`, expecting failure; returns the compiler's stderr and asserts
/// that neither the executable nor an object file was left behind.
fn build_must_fail(src: &str) -> String {
    let dir = scratch();
    let f = dir.join("bad.mtn");
    let exe = dir.join("bad_out");
    std::fs::write(&f, src).unwrap();
    let out = mtnc().arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(!out.status.success(), "build unexpectedly succeeded for:\n{}", src);
    assert!(!exe.exists(), "a binary was produced although checking failed");
    assert!(!exe.with_extension("o").exists(), "an object file was left behind");
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn ir(src: &str, release: bool) -> String {
    driver::emit_ir(src, &Options { release }).unwrap_or_else(|e| panic!("codegen failed: {:?}", e))
}

// ---------------------------------------------------------------- milestone

#[test]
fn hello_world_compiles_and_prints() {
    let r = run_file(&example("hello"), &[]);
    assert_eq!(r.stdout, "Hello, Mountain!\n");
    assert_eq!(r.code, 0);
}

#[test]
fn recursion_fib_and_factorial() {
    let r = run_file(&example("recursion"), &[]);
    assert_eq!(r.stdout, "55\n3628800\n6765\n");
    assert_eq!(r.code, 0);
}

#[test]
fn loops_labels_break_value_for_while_do() {
    let r = run_file(&example("loops"), &[]);
    assert_eq!(r.stdout, "20\n0\n1\n2\n15\n0\n1\n2\n10\n11\n12\n4\n");
}

#[test]
fn structs_enums_match_tuples() {
    let r = run_file(&example("shapes"), &[]);
    assert_eq!(r.stdout, "17\n75\n42\n0\n3\n100\n200\n-1\n300\n2\n1\n15\n");
}

#[test]
fn option_result_question_mark_coalesce_ensure() {
    let r = run_file(&example("results"), &[]);
    assert_eq!(r.stdout, "26\n-1\n-2\n8\n99\n12\n-1\n");
}

#[test]
fn try_catch_throw_end_to_end() {
    let r = run_file(&example("trycatch"), &[]);
    assert_eq!(r.stdout, "31\n-1\n-500\n7\n7\n-999\n-4\n");
}

#[test]
fn numeric_types_casts_bitwise_chars() {
    let r = run_file(&example("numeric"), &[]);
    assert_eq!(
        r.stdout,
        "12.5\n44\n-2\n-3\n3\n15\n4\n1024\n255\n1024\ntrue\nA\n\u{e9}\n7\n18446744073709551615\n-128\n\u{ff}\n"
    );
}

#[test]
fn mixed_features_debug_and_release_agree() {
    let expect = "25\n5\n-5\n0\n5\n-1\n14\n-1\n1\n100\n10\n1\n97\n255\ntrue\n12\n7\n-10\n";
    assert_eq!(run_file(&example("mixed"), &[]).stdout, expect);
    assert_eq!(run_file(&example("mixed"), &["--release"]).stdout, expect);
}

#[test]
fn diverging_control_flow_tuples_strings_and_stderr() {
    let r = run_file(&example("control"), &[]);
    assert_eq!(r.stdout, "3\n3\n4\n100\n6\n6\n200\nnonpositive\n-1\n0\n5\n25\nh\u{e9}llo\na1xtrue\n");
    assert_eq!(r.stderr, "to stderr\n");
    assert_eq!(r.code, 0);
}

#[test]
fn block_like_statement_ends_the_statement() {
    // `while .. { }` followed by `-1` must be a statement plus the tail `-1`.
    let r = run_src("fn f() -> i32 { let mut i = 0; while i < 3 { i += 1; } -1 }\nfn main() { println(f()); }\n", &[]);
    assert_eq!(r.stdout, "-1\n");
}

// ------------------------------------------------------------- panic / exit

#[test]
fn panic_exits_101_with_message_on_stderr() {
    let r = run_file(&example("panics"), &[]);
    assert_eq!(r.code, 101);
    assert_eq!(r.stdout, "before\n");
    assert!(r.stderr.contains("panicked: something impossible happened"), "stderr: {}", r.stderr);
}

#[test]
fn debug_overflow_panics_with_101() {
    let r = run_file(&example("overflow"), &[]);
    assert_eq!(r.code, 101);
    assert_eq!(r.stdout, "255\n");
    assert!(r.stderr.contains("attempt to add with overflow"), "stderr: {}", r.stderr);
}

#[test]
fn release_overflow_wraps() {
    let r = run_file(&example("overflow"), &["--release"]);
    assert_eq!(r.code, 0);
    assert_eq!(r.stdout, "255\n0\nunreachable\n");
}

#[test]
fn division_by_zero_at_runtime_panics() {
    let src = "fn div(a: i32, b: i32) -> i32 { a / b }\nfn main() { println(div(10, 2)); println(div(1, 0)); }\n";
    for extra in [&[][..], &["--release"][..]] {
        let r = run_src(src, extra);
        assert_eq!(r.code, 101, "{:?}", extra);
        assert_eq!(r.stdout, "5\n");
        assert!(r.stderr.contains("attempt to divide by zero"));
    }
}

#[test]
fn modulo_by_zero_and_signed_min_over_minus_one_panic() {
    let r = run_src("fn m(a: i32, b: i32) -> i32 { a % b }\nfn main() { println(m(1, 0)); }\n", &[]);
    assert_eq!(r.code, 101);
    let r = run_src("fn d(a: i32, b: i32) -> i32 { a / b }\nfn main() { let lo: i32 = -2147483648; println(d(lo, -1)); }\n", &[]);
    assert_eq!(r.code, 101);
    assert!(r.stderr.contains("divide with overflow"));
}

#[test]
fn constant_zero_divisor_is_a_compile_error() {
    let err = build_must_fail("fn main() { let x = 5; println(x / 0); }\n");
    assert!(err.contains("constant zero"), "stderr: {}", err);
}

#[test]
fn assert_is_debug_only_and_ensure_is_always_present() {
    let a = "fn main() { assert(1 > 2); println(\"after\"); }\n";
    assert_eq!(run_src(a, &[]).code, 101);
    let r = run_src(a, &["--release"]);
    assert_eq!((r.code, r.stdout.as_str()), (0, "after\n"));

    let e = "enum Bad { No }\nfn check(n: i32) -> Result<(), Bad> { ensure(n > 0, Bad::No)?; Ok(()) }\nfn main() -> Result<(), Bad> { check(1)?; println(\"ok\"); check(0)?; println(\"unreachable\"); Ok(()) }\n";
    for extra in [&[][..], &["--release"][..]] {
        let r = run_src(e, extra);
        assert_eq!(r.stdout, "ok\n", "{:?}", extra);
        assert_eq!(r.code, 1, "{:?}", extra);
    }
}

#[test]
fn shifts_check_amount_in_debug_and_mask_in_release() {
    let src = "fn sh(x: u32, n: u32) -> u32 { x << n }\nfn main() { println(sh(1, 4)); println(sh(1, 40)); }\n";
    let r = run_src(src, &[]);
    assert_eq!((r.code, r.stdout.as_str()), (101, "16\n"));
    let r = run_src(src, &["--release"]);
    assert_eq!((r.code, r.stdout.as_str()), (0, "16\n256\n"));
}

#[test]
fn run_forwards_the_program_exit_code() {
    let r = run_src("fn main() { panic(\"x\"); }\n", &[]);
    assert_eq!(r.code, 101);
}

// ----------------------------------------------------------- rejection tests

#[test]
fn type_error_fails_build_and_leaves_no_binary() {
    let err = build_must_fail("fn main() { let x: i32 = 5; let y: f64 = x; println(y); }\n");
    assert!(err.contains("expected `f64`, found `i32`"), "stderr: {}", err);
}

#[test]
fn borrow_error_fails_build_and_leaves_no_binary() {
    let src = "struct User { age: i32 }\nfn consume(u: User) {}\nfn main() { let a = User { age: 1 }; consume(a); consume(a); }\n";
    let err = build_must_fail(src);
    assert!(err.contains("borrow error"), "stderr: {}", err);
}

#[test]
fn non_exhaustive_match_fails_build_and_leaves_no_binary() {
    let src = "enum C { R, G, B }\nfn f(c: C) -> i32 { match c { C::R => 1, C::G => 2 } }\nfn main() { println(f(C::B)); }\n";
    let err = build_must_fail(src);
    assert!(err.to_lowercase().contains("exhaustive") || err.contains("not covered"), "stderr: {}", err);
}

#[test]
fn check_subcommand_runs_the_whole_front_end() {
    let dir = scratch();
    let f = dir.join("bad.mtn");
    std::fs::write(&f, "fn main() { let x: i32 = true; }\n").unwrap();
    let out = mtnc().arg("check").arg(&f).output().unwrap();
    assert!(!out.status.success());
    let ok = mtnc().arg("check").arg(example("shapes")).output().unwrap();
    assert!(ok.status.success());
}

#[test]
fn unsupported_features_give_clear_codegen_errors_not_crashes() {
    let cases = [
        ("fn main() { let f = |x: i32| x + 1; println(f(1)); }\n", "closures"),
        ("fn id<T>(x: T) -> T { x }\nfn main() { println(id(1)); }\n", "generic"),
        ("fn main() { let b = Box::new(5); }\n", "Phase"),
    ];
    for (src, needle) in cases {
        let err = build_must_fail(src);
        assert!(err.contains(needle) || err.contains("not yet supported"), "stderr for {:?}: {}", src, err);
        assert!(!err.contains("panicked"), "compiler crashed: {}", err);
    }
}

// ------------------------------------------------------------------ IR tests

#[test]
fn enum_and_int_matches_lower_to_llvm_switch() {
    let src = "enum C { R, G, B }\nfn f(c: C) -> i32 { match c { C::R => 1, C::G => 2, C::B => 3 } }\nfn g(n: i32) -> i32 { match n { 0 => 10, 1 => 20, 2 => 30, _ => 0 } }\nfn main() { println(f(C::G) + g(1)); }\n";
    let text = ir(src, false);
    assert_eq!(text.matches("switch i32").count(), 2, "expected two switch instructions:\n{}", text);
}

#[test]
fn guarded_match_falls_back_to_a_chain_of_branches() {
    let src = "fn f(n: i32) -> i32 { match n { k if k < 0 => -1, _ => 1 } }\nfn main() { println(f(3)); }\n";
    assert!(!ir(src, false).contains("switch i32"));
}

#[test]
fn debug_uses_overflow_intrinsics_release_wraps() {
    let src = "fn f(a: i32, b: i32) -> i32 { a + b }\nfn main() { println(f(1, 2)); }\n";
    let debug = ir(src, false);
    assert!(debug.contains("llvm.sadd.with.overflow.i32"), "{}", debug);
    let release = ir(src, true);
    assert!(!release.contains("with.overflow"), "{}", release);
    assert!(release.contains("add i32"));
}

#[test]
fn unsigned_uses_the_unsigned_overflow_intrinsic() {
    let src = "fn f(a: u8, b: u8) -> u8 { a + b }\nfn main() { println(f(1, 2)); }\n";
    assert!(ir(src, false).contains("llvm.uadd.with.overflow.i8"));
}

#[test]
fn ir_is_target_agnostic_no_hard_coded_cpu() {
    let text = ir("fn main() { println(1); }\n", false);
    // The module header (`target triple`/`datalayout`) is supplied by the
    // driver from the TargetMachine; the generated code itself must not name a CPU.
    let body: Vec<&str> = text.lines().filter(|l| !l.starts_with("target ")).collect();
    assert!(!body.join("\n").contains("x86"), "IR must not name a CPU architecture:\n{}", text);
}

#[test]
fn structs_are_stack_allocated_no_heap_calls() {
    let text = ir("struct P { x: i32, y: i32 }\nfn main() { let p = P { x: 1, y: 2 }; println(p.x + p.y); }\n", false);
    assert!(!text.contains("malloc"));
}

// ------------------------------------- Document 11 §3 / §7: try/catch == `?`

const TRY_VERSION: &str = r#"
enum ConfigError { Missing }
fn read_file(k: i32) -> Result<i32, ConfigError> { if k == 0 { return Err(ConfigError::Missing); } Ok(k) }
fn parse_config(v: i32) -> Result<i32, ConfigError> { Ok(v + 1) }
fn apply_config(v: i32) { println(v); }
fn log_error(c: i32) { println(c); }
fn apply_default_config() { println(0); }
fn load() {
    try {
        let contents = read_file(1)?;
        let parsed = parse_config(contents)?;
        apply_config(parsed);
    } catch (e) {
        log_error(1);
        apply_default_config();
    }
}
fn main() { load(); }
"#;

// The same program with the wrapper spelled out by hand (Document 11 §3's
// desugaring). NORMALIZATION: the synthesized wrapper of the N-th `try` is
// named `__try_block_N` (0-based, source order) and placed immediately before
// the item containing the `try`; the hand-written version uses that name/place.
const HAND_VERSION: &str = r#"
enum ConfigError { Missing }
fn read_file(k: i32) -> Result<i32, ConfigError> { if k == 0 { return Err(ConfigError::Missing); } Ok(k) }
fn parse_config(v: i32) -> Result<i32, ConfigError> { Ok(v + 1) }
fn apply_config(v: i32) { println(v); }
fn log_error(c: i32) { println(c); }
fn apply_default_config() { println(0); }
fn __try_block_0() -> Result<(), ConfigError> {
    let contents = read_file(1)?;
    let parsed = parse_config(contents)?;
    apply_config(parsed);
    return Ok(());
}
fn load() {
    match __try_block_0() {
        Ok(_) => {},
        Err(e) => {
            log_error(1);
            apply_default_config();
        }
    }
}
fn main() { load(); }
"#;

#[test]
fn try_catch_ir_is_identical_to_the_hand_written_question_mark_chain() {
    for release in [false, true] {
        let a = ir(TRY_VERSION, release);
        let b = ir(HAND_VERSION, release);
        assert_eq!(a, b, "LLVM IR differs between try/catch and the hand-written wrapper+match (release={})", release);
    }
}

#[test]
fn try_catch_and_hand_written_chain_behave_identically_at_runtime() {
    assert_eq!(run_src(TRY_VERSION, &[]).stdout, run_src(HAND_VERSION, &[]).stdout);
    assert_eq!(run_src(TRY_VERSION, &[]).stdout, "2\n");
}

#[test]
fn try_block_reading_outer_locals_gets_them_as_wrapper_parameters() {
    let src = "enum E { Bad }\nfn chk(n: i32) -> Result<i32, E> { if n > 5 { return Err(E::Bad); } Ok(n) }\nfn main() { let limit = 3; let r = try { chk(limit + 1)? } catch (e) { -1 }; println(r); let r2 = try { chk(limit + 9)? } catch (e) { -1 }; println(r2); }\n";
    let r = run_src(src, &[]);
    assert_eq!(r.stdout, "4\n-1\n");
}

#[test]
fn try_block_assigning_an_outer_local_is_a_clear_unsupported_error() {
    let src = "enum E { Bad }\nfn chk(n: i32) -> Result<i32, E> { Ok(n) }\nfn main() { let mut total = 0; try { total = chk(1)?; } catch (e) { println(0); } println(total); }\n";
    let err = build_must_fail(src);
    assert!(err.contains("not yet supported by codegen"), "stderr: {}", err);
}

#[test]
fn nested_try_blocks_are_numbered_and_work() {
    let src = "enum E { Bad }\nfn chk(n: i32) -> Result<i32, E> { if n > 5 { return Err(E::Bad); } Ok(n) }\nfn main() { let v = try { let a = try { chk(9)? } catch (e) { 2 }; chk(a)? } catch (e) { -1 }; println(v); }\n";
    assert_eq!(run_src(src, &[]).stdout, "2\n");
}
