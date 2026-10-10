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

// ------------------------------------------------------------ Phase 11a

#[test]
fn refs_borrow_and_borrow_mut_parameters() {
    let r = run_file(&example("refs"), &[]);
    assert_eq!(r.stdout, "13\n12\n112\n3\n12\n15\n");
    assert_eq!(r.code, 0);
}

#[test]
fn arrays_slices_and_the_bounds_check_panic() {
    let r = run_file(&example("arrays"), &[]);
    assert_eq!(r.stdout, "50\n110\n11\n2636\n3\nc\n36\n41\n");
    assert_eq!(r.code, 101);
    assert!(r.stderr.contains("index out of bounds: the length is 4 but the index is 7"), "stderr: {}", r.stderr);
}

#[test]
fn slice_range_out_of_bounds_panics() {
    let src = "fn main() { let a: [i32; 3] = [1, 2, 3]; let hi: usize = 5; let s = borrow a[1..hi]; println(s[0]); }\n";
    let r = run_src(src, &[]);
    assert_eq!(r.code, 101);
    assert!(r.stderr.contains("slice range out of bounds"), "stderr: {}", r.stderr);
}

#[test]
fn negative_index_panics_instead_of_reading_memory() {
    let r = run_src("fn main() { let a: [i32; 2] = [1, 2]; let i: i32 = -1; println(a[i]); }\n", &[]);
    assert_eq!(r.code, 101);
}

#[test]
fn impl_blocks_methods_assoc_fns_traits_and_default_methods() {
    let r = run_file(&example("methods"), &[]);
    assert_eq!(r.stdout, "75\n150\n9\n2000\n1000\n5\n-1\n");
}

#[test]
fn const_static_type_and_nested_items() {
    let r = run_file(&example("items"), &[]);
    assert_eq!(r.stdout, "10\n20\nstatic string\n5\n-1\n42\n40\n10\n");
}

#[test]
fn named_default_and_variadic_arguments() {
    let r = run_file(&example("args"), &[]);
    assert_eq!(r.stdout, "10\n15\n150\n8\n24\n28\n6\n90\n");
}

#[test]
fn combined_11a_features() {
    let r = run_file(&example("combined"), &[]);
    assert_eq!(r.stdout, "54\n3\n8\n16\n5\n99\n5\n42\n84\n6\n7\n3\n");
}

#[test]
fn argument_errors_are_compile_errors() {
    let base = "fn f(a: i32, b: i32 = 1) -> i32 { a + b }\n";
    for (body, needle) in [
        ("fn main() { println(f(b: 2)); }\n", "missing the argument"),
        ("fn main() { println(f(1, c: 2)); }\n", "no parameter named"),
        ("fn main() { println(f(1, a: 2)); }\n", "given twice"),
        ("fn main() { println(f(a: 1, 2)); }\n", "positional argument cannot follow"),
        ("fn main() { println(f(1, 2, 3)); }\n", "more were given"),
    ] {
        let err = build_must_fail(&format!("{}{}", base, body));
        assert!(err.contains(needle), "expected {:?} in: {}", needle, err);
    }
}

#[test]
fn borrow_argument_must_be_written_borrow() {
    let err = build_must_fail("fn f(borrow n: i32) {}\nfn main() { let x = 1; f(x); }\n");
    assert!(err.contains("expected `&i32`") || err.contains("must be written `borrow"), "stderr: {}", err);
}

#[test]
fn assigning_to_a_static_is_rejected() {
    let err = build_must_fail("static S: i32 = 1;\nfn main() { S = 2; }\n");
    assert!(err.contains("static"), "stderr: {}", err);
}

#[test]
fn static_initializer_must_be_constant() {
    let err = build_must_fail("fn one() -> i32 { 1 }\nstatic S: i32 = one();\nfn main() { println(S); }\n");
    assert!(err.contains("constant"), "stderr: {}", err);
}

#[test]
fn ir_bounds_check_methods_and_defaults() {
    let text = ir(&std::fs::read_to_string(example("methods")).unwrap(), false);
    // methods and associated functions are real functions named Type::method
    assert!(text.contains("@\"mtn_Account::deposit\""), "{}", text);
    assert!(text.contains("@\"mtn_Account::new\""));
    // the trait default method is instantiated for the implementing type, not for the one that overrides it twice
    assert!(text.contains("@\"mtn_Account::Describe::describe\""));
    assert!(text.contains("@\"mtn_Plain::Describe::describe\""));
    let arr = ir("fn main() { let a: [i32; 4] = [1, 2, 3, 4]; let i: usize = 2; println(a[i]); }\n", false);
    assert!(arr.contains("__mtn_oob"), "{}", arr);
    assert!(arr.contains("[4 x i32]"));
}

#[test]
fn ir_const_is_inlined_and_static_is_one_global() {
    let text = ir("const C: i32 = 7;\nstatic S: i32 = 9;\nfn main() { println(C + C); println(S); }\n", false);
    assert!(!text.contains("mtn_static_C"));
    assert_eq!(text.matches("@mtn_static_S = ").count(), 1, "{}", text);
}

// ----------------------------------------------------------- Phase 11b-1

#[test]
fn same_named_nested_items_in_different_functions_are_scoped() {
    let r = run_file(&example("nested_scope"), &[]);
    assert_eq!(r.stdout, "21\n8\n1000\n");
}

#[test]
fn nested_impl_gets_default_trait_methods() {
    // `twice` is a default method used on impls nested inside functions (see nested_scope).
    let src = "trait T { fn a(borrow self) -> i32; fn b(borrow self) -> i32 { self.a() + 1 } }\nfn main() { struct S { n: i32 } impl T for S { fn a(borrow self) -> i32 { self.n } } let s = S { n: 4 }; println(s.b()); }\n";
    assert_eq!(run_src(src, &[]).stdout, "5\n");
}

#[test]
fn duplicate_nested_names_inside_one_function_are_still_rejected() {
    let err = build_must_fail("fn main() { fn h() -> i32 { 1 } fn h() -> i32 { 2 } println(h()); }\n");
    assert!(err.contains("more than once"), "stderr: {}", err);
}

#[test]
fn a_nested_item_may_share_its_name_with_a_top_level_item() {
    let src = "fn h() -> i32 { 100 }\nfn f() -> i32 { fn h() -> i32 { 1 } h() }\nfn main() { println(f() + h()); }\n";
    assert_eq!(run_src(src, &[]).stdout, "101\n");
}

#[test]
fn a_local_variable_shadows_a_renamed_nested_item() {
    let src = "fn a() -> i32 { fn h() -> i32 { 1 } let h = 5; h }\nfn b() -> i32 { fn h() -> i32 { 2 } h() }\nfn main() { println(a() + b()); }\n";
    assert_eq!(run_src(src, &[]).stdout, "7\n");
}

#[test]
fn static_initializers_may_use_constant_arithmetic_and_comparisons() {
    let r = run_file(&example("const_fold"), &[]);
    assert_eq!(r.stdout, "40\n255\ntrue\n6.5\n1099511627781\n21\n-6\n19\n2\nfalse\n");
}

#[test]
fn static_constant_overflow_and_calls_are_compile_errors() {
    let err = build_must_fail("static B: u8 = 200 + 56;\nfn main() { println(B); }\n");
    assert!(err.contains("overflows"), "stderr: {}", err);
    let err = build_must_fail("static D: i32 = 1 / 0;\nfn main() { println(D); }\n");
    assert!(err.contains("zero"), "stderr: {}", err);
    let err = build_must_fail("fn one() -> i32 { 1 }\nstatic S: i32 = one() + 1;\nfn main() { println(S); }\n");
    assert!(err.contains("constant"), "stderr: {}", err);
    // a `static` is not a `const`: it cannot feed another static's arithmetic
    let err = build_must_fail("static A: i32 = 1;\nstatic B: i32 = A + 1;\nfn main() { println(B); }\n");
    assert!(err.contains("constant"), "stderr: {}", err);
}

#[test]
fn question_mark_converts_errors_through_from_impls() {
    let r = run_file(&example("from_conv"), &[]);
    assert_eq!(r.stdout, "31\n1000\n993\n2200\n21\n497\n1100\n3\n-1\n2004\n");
}

#[test]
fn question_mark_without_a_from_impl_is_a_compile_error() {
    let src = "enum A { X }\nenum B { Y }\nfn f() -> Result<i32, A> { Err(A::X) }\nfn g() -> Result<i32, B> { let v = f()?; Ok(v) }\nfn main() { }\n";
    let err = build_must_fail(src);
    assert!(err.contains("From"), "stderr: {}", err);
}

#[test]
fn ir_question_mark_calls_the_from_function() {
    let text = ir(&std::fs::read_to_string(example("from_conv")).unwrap(), false);
    assert!(text.contains("call %") || text.contains("call {"), "{}", text);
    assert!(text.contains("@\"mtn_AppError::From<IoError>::from\""), "{}", text);
    assert!(text.contains("@\"mtn_AppError::From<ParseError>::from\""), "{}", text);
}

#[test]
fn tail_calls_run_a_million_deep_in_a_debug_build() {
    let r = run_file(&example("tailcall"), &[]);
    assert_eq!(r.stdout, "500000500000\n7\nfalse\n1000000\n2000000\n21\n3628800\n");
    assert_eq!(r.code, 0);
    let r = run_file(&example("tailcall"), &["--release"]);
    assert_eq!(r.stdout, "500000500000\n7\nfalse\n1000000\n2000000\n21\n3628800\n");
}

#[test]
fn tail_recursive_function_has_no_self_call_left_in_the_ir() {
    let text = ir(&std::fs::read_to_string(example("tailcall")).unwrap(), false);
    let body_of = |name: &str| -> String {
        let start = text.find(&format!("@mtn_{}(", name)).unwrap_or_else(|| panic!("no function {}", name));
        let start = text[..start].rfind("define").unwrap();
        let end = start + text[start..].find("\n}\n").unwrap();
        text[start..end].to_string()
    };
    for f in ["sum_to", "count_down", "parity", "gcd"] {
        let b = body_of(f);
        // the only mention of `@mtn_<f>(` in its own body is the `define` line itself
        assert_eq!(b.matches(&format!("@mtn_{}(", f)).count(), 1, "{} still calls itself:\n{}", f, b);
        assert!(b.contains("tail_loop"), "{} has no tail loop", f);
    }
    // non-tail recursion is an ordinary call
    assert!(body_of("factorial").matches("call i64 @mtn_factorial(").count() >= 1);
}

#[test]
fn non_tail_recursion_still_works_deeply_enough() {
    let src = "fn depth(n: u64) -> u64 { if n == 0 { 0 } else { 1 + depth(n - 1) } }\nfn main() { println(depth(10000)); }\n";
    assert_eq!(run_src(src, &[]).stdout, "10000\n");
}

#[test]
fn try_blocks_may_assign_and_mutably_borrow_outer_locals() {
    let r = run_file(&example("trymut"), &[]);
    assert_eq!(r.stdout, "-6\n3\n3\n20\n2\n");
}

#[test]
fn break_leaving_a_try_block_is_still_a_clear_error() {
    let src = "enum E { Bad }\nfn chk(n: i32) -> Result<i32, E> { Ok(n) }\nfn main() { let mut i = 0; while i < 3 { try { chk(i)?; break; } catch (e) { } i += 1; } }\n";
    let err = build_must_fail(src);
    assert!(err.contains("not yet supported by codegen"), "stderr: {}", err);
}

#[test]
fn return_inside_try_is_a_compile_error() {
    let src = "enum E { Bad }\nfn chk(n: i32) -> Result<i32, E> { Ok(n) }\nfn f() -> i32 { try { chk(1)?; return 5; } catch (e) { } 0 }\nfn main() { println(f()); }\n";
    let err = build_must_fail(src);
    assert!(err.contains("return"), "stderr: {}", err);
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
fn nested_try_blocks_are_numbered_and_work() {
    let src = "enum E { Bad }\nfn chk(n: i32) -> Result<i32, E> { if n > 5 { return Err(E::Bad); } Ok(n) }\nfn main() { let v = try { let a = try { chk(9)? } catch (e) { 2 }; chk(a)? } catch (e) { -1 }; println(v); }\n";
    assert_eq!(run_src(src, &[]).stdout, "2\n");
}
