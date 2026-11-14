//! Phase 11c C7: the whole `examples/memory/` set under valgrind, the arena benchmark, IR / exit-code / rejection tests.
use mtnc::codegen::Options;
use mtnc::driver;
use std::process::Command;

/// Unique per call: tests run in parallel threads of one process.
static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn tmp(src: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mtn_c7_{}_{}", std::process::id(), COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("p.mtn");
    std::fs::write(&f, src).unwrap();
    f
}

fn have_valgrind() -> bool {
    Command::new("valgrind").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Builds `file`, runs it; when valgrind exists also asserts: no leak, no invalid access.
fn run_file(file: &std::path::Path) -> (String, i32) {
    let exe = file.with_extension("bin");
    let b = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(file).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let out = Command::new(&exe).output().unwrap();
    if have_valgrind() {
        let v = Command::new("valgrind")
            .args(["--leak-check=full", "--errors-for-leak-kinds=definite", "--error-exitcode=99"])
            .arg(&exe)
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&v.stderr).to_string();
        assert_ne!(v.status.code(), Some(99), "valgrind found a leak or memory error:\n{}", err);
        assert!(!err.contains("Invalid read") && !err.contains("Invalid write") && !err.contains("Invalid free"), "valgrind: {}", err);
    }
    (String::from_utf8_lossy(&out.stdout).to_string(), out.status.code().unwrap_or(-1))
}

fn run_no_valgrind(src: &str) -> (String, i32) {
    let f = tmp(src);
    let exe = f.with_extension("bin");
    let b = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let out = Command::new(&exe).output().unwrap();
    (String::from_utf8_lossy(&out.stdout).to_string(), out.status.code().unwrap_or(-1))
}

fn run(src: &str) -> (String, i32) {
    run_file(&tmp(src))
}

#[allow(dead_code)]
fn ok(src: &str, want: &str) {
    let (o, c) = run(src);
    assert_eq!((o.as_str(), c), (want, 0));
}

fn rejected(src: &str) -> String {
    let f = tmp(src);
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("check").arg(&f).output().unwrap();
    assert!(!out.status.success(), "should have been rejected:\n{}", src);
    String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr)
}

fn ir(src: &str) -> String {
    driver::emit_ir(src, &Options { release: false }).unwrap_or_else(|e| panic!("codegen failed: {:?}", e))
}

fn example(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").join("memory").join(name)
}



// ------------------------------------------------------------------ examples/memory: every program, leak-free

fn memory_examples() -> Vec<std::path::PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").join("memory");
    let mut v: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "mtn").unwrap_or(false)).collect();
    v.sort();
    v
}

#[test]
fn the_memory_example_set_is_complete() {
    let names: Vec<String> = memory_examples().iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
    for want in ["nested_owned.mtn", "drop_paths.mtn", "box_match_list.mtn", "rc_weak.mtn", "arc_shared.mtn", "dyn_shapes.mtn", "dyn_values.mtn", "arena_alloc.mtn", "early_exits.mtn"] {
        assert!(names.iter().any(|n| n == want), "missing example {}", want);
    }
}

#[test]
fn every_memory_example_runs_clean_under_valgrind() {
    // run_file asserts: build ok, no leak (definite), no invalid access / free (when valgrind is installed)
    for p in memory_examples() {
        let (out, code) = run_file(&p);
        assert_eq!(code, 0, "{} exited with {}: {}", p.display(), code, out);
        assert!(!out.is_empty(), "{} printed nothing", p.display());
    }
}

#[test]
fn nested_owned_example_output() {
    let (o, c) = run_file(&example("nested_owned.mtn"));
    assert_eq!((o.as_str(), c), ("Summit\nAda\n1 Peak Rd\nBo\nHill\nc\n", 0));
}

#[test]
fn early_exits_example_output() {
    let (o, c) = run_file(&example("early_exits.mtn"));
    assert_eq!((o.as_str(), c), ("5\n10\ntok\ntok\n1\n-7\n-3\n", 0));
}

// ------------------------------------------------------------------ exit codes

#[test]
fn growable_index_out_of_bounds_panics_with_exit_101() {
    let (_, c) = run_no_valgrind("fn main() { let a = [1, 2, 3]; let i = 5; println(a[i]); }");
    assert_eq!(c, 101);
}

#[test]
fn fixed_array_index_out_of_bounds_panics_with_exit_101() {
    let (_, c) = run_no_valgrind("fn main() { let a: [i32; 3] = [1, 2, 3]; let mut i = 2; i = i + 3; println(a[i]); }");
    assert_eq!(c, 101);
}

#[test]
fn explicit_panic_has_exit_code_101_and_still_runs_no_leaky_path_before() {
    let (o, c) = run_no_valgrind("fn main() { let s = String::from(\"x\"); println(s); panic(\"boom\"); }");
    assert_eq!((o.as_str(), c), ("x\n", 101));
}

#[test]
fn question_mark_with_a_from_impl_still_works_with_owned_values() {
    let src = "enum IoErr { Missing }\nenum AppErr { Io }\n\
        impl From<IoErr> for AppErr { fn from(e: IoErr) -> AppErr { return AppErr::Io; } }\n\
        fn rd(n: i32) -> Result<String, IoErr> { if n < 0 { return Err(IoErr::Missing); } return Ok(String::from(\"ok\")); }\n\
        fn app(n: i32) -> Result<i32, AppErr> { let held = String::from(\"h\"); let s = rd(n)?; println(s); return Ok(1); }\n\
        fn main() { match app(1) { Ok(v) => println(v), Err(e) => println(0) } match app(-1) { Ok(v) => println(v), Err(e) => println(-1) } }";
    let (o, c) = run(src);
    assert_eq!((o.as_str(), c), ("ok\n1\n-1\n", 0));
}

#[test]
fn loop_move_conditional_move_partial_move_and_early_exit_in_one_program() {
    let src = "struct P { a: String, b: String }\nfn take(s: String) -> i32 { return 1; }\n\
        fn f(n: i32) -> i32 { let p = P { a: String::from(\"a\"), b: String::from(\"b\") }; if n == 0 { return take(p.a); } let x = p.b; return take(x) + 10; }\n\
        fn main() { let mut i = 0; let mut t = 0; while i < 4 { let s = String::from(\"s\"); if i % 2 == 0 { t = t + take(s); } if i == 3 { break; } i = i + 1; } println(t); println(f(0)); println(f(1)); }";
    let (o, c) = run(src);
    assert_eq!((o.as_str(), c), ("2\n1\n11\n", 0));
}

// ------------------------------------------------------------------ IR unit tests

#[test]
fn generic_function_instances_are_deduplicated() {
    let src = "fn id<T>(x: T) -> T { return x; }\nfn main() { let a = id(1); let b = id(2); let c = id(2.5); println(a + b); println(c); }";
    let checked = driver::check_source(src).unwrap_or_else(|e| panic!("{:?}", e));
    let mut names: Vec<String> = checked.program.items.iter().filter_map(|it| match &it.kind {
        mtnc::ast::ItemKind::Fn(f) if f.name.starts_with("id") => Some(f.name.clone()),
        _ => None,
    }).collect();
    names.sort();
    assert_eq!(names, vec!["id$f64".to_string(), "id$i32".to_string()]);
    let text = ir(src);
    assert_eq!(text.matches("define i32 @\"mtn_id$i32\"").count(), 1, "{}", text);
}

#[test]
fn self_tail_call_is_a_loop_not_a_call() {
    let text = ir("fn count(n: i64, acc: i64) -> i64 { if n == 0 { return acc; } return count(n - 1, acc + n); }\nfn main() { println(count(10, 0)); }");
    let body = text.split("define i64 @mtn_count").nth(1).expect("count in IR");
    let body = body.split("\n}\n").next().unwrap();
    assert!(!body.contains("@mtn_count("), "the tail call must not be a call:\n{}", body);
    assert!(body.contains("br label"), "{}", body);
}

#[test]
fn drop_glue_is_emitted_once_per_type_even_with_many_uses() {
    let text = ir("struct S { a: String, b: String }\nfn main() { let x = S { a: String::from(\"1\"), b: String::from(\"2\") }; let y = S { a: String::from(\"3\"), b: String::from(\"4\") }; let z = S { a: String::from(\"5\"), b: String::from(\"6\") }; println(x.a); }");
    assert_eq!(text.matches("define void @__mtn_drop_S(").count(), 1, "{}", text);
    assert_eq!(text.matches("define void @__mtn_drop_String(").count(), 1, "{}", text);
}

#[test]
fn vtable_is_a_constant_global_with_drop_size_align_then_methods() {
    let text = ir("trait T { fn f(borrow self) -> i32; }\nstruct A { s: String }\nimpl T for A { fn f(borrow self) -> i32 { return 1; } }\nfn main() { let b: Box<dyn T> = Box::new(A { s: String::from(\"x\") }); println(b.f()); }");
    assert!(text.contains("@__mtn_vtable_A_T = constant { ptr, i64, i64, ptr }"), "{}", text);
}

#[test]
fn box_new_in_goes_through_the_allocator_trait_not_the_default_interface() {
    let text = ir("struct Bump { n: usize }\nimpl Allocator for Bump {\n fn alloc(borrow mut self, size: usize, align: usize) -> *mut u8 { unsafe { let p: *mut u8 = alloc(size, align); return p; } }\n fn dealloc(borrow mut self, ptr: *mut u8, size: usize) { unsafe { dealloc(ptr, size, 1); } }\n}\nfn main() { let mut b = Bump { n: 0 }; let x = Box::newIn(5, borrow b); println(*x); }");
    let main = text.split("define void @mtn_main").nth(1).unwrap().split("\n}\n").next().unwrap();
    assert!(main.contains("@\"mtn_Bump::Allocator::alloc\""), "{}", main);
}

// ------------------------------------------------------------------ rejections (the full required list)

#[test]
fn rejects_use_after_move() {
    let m = rejected("fn eat(s: String) { }\nfn main() { let s = String::from(\"a\"); eat(s); println(s); }");
    assert!(m.contains("moved") || m.contains("move"), "{}", m);
}

#[test]
fn rejects_copy_attribute_on_a_struct_with_a_heap_field() {
    rejected("@copy\nstruct S { name: String }\nfn main() { }");
}

#[test]
fn rejects_a_recursive_struct_without_box() {
    let m = rejected("struct L { v: i32, next: L }\nfn main() { }");
    assert!(m.contains("Box"), "{}", m);
}

#[test]
fn rejects_an_unsafe_only_operation_outside_unsafe() {
    let m = rejected("fn main() { let p: *mut u8 = alloc(8, 8); }");
    assert!(m.contains("unsafe"), "{}", m);
}

#[test]
fn rejects_dereferencing_a_raw_pointer_outside_unsafe() {
    let m = rejected("fn main() { let mut x = 1; let p = &mut x as *mut i32; let y = *p; }");
    assert!(m.contains("unsafe"), "{}", m);
}

#[test]
fn rejects_use_after_explicit_drop() {
    rejected("fn main() { let s = String::from(\"a\"); drop(s); println(s); }");
}

#[test]
fn rejects_assignment_to_an_immutable_binding() {
    let m = rejected("fn main() { let x = 1; x = 2; println(x); }");
    assert!(m.contains("not declared `mut`") || m.contains("mut"), "{}", m);
}

#[test]
fn rejects_assignment_through_rc() {
    rejected("fn main() { let r = Rc::new(1); *r = 2; }");
}

// ------------------------------------------------------------------ arena benchmark (Doc 13 §3.2: an arena beats the default allocator)

const BENCH_COMMON: &str = "fn addr<T>(r: borrow T) -> usize { unsafe { return r as *const T as usize; } }\n";

fn bench_default() -> String {
    format!("{}fn main() {{ let mut i: i64 = 0; let mut sum: usize = 0; while i < 1000000 {{ let b = Box::new(i); sum = sum + addr(borrow *b) % 7 + 1; i = i + 1; }} println(sum > 0); }}", BENCH_COMMON)
}

fn bench_arena() -> String {
    format!("{}{}fn main() {{ let mut ar = Arena::new(16000000); let mut i: i64 = 0; let mut sum: usize = 0; while i < 1000000 {{ let b = Box::newIn(i, borrow ar); sum = sum + addr(borrow *b) % 7 + 1; i = i + 1; }} println(sum > 0); ar.release(); }}", ARENA, BENCH_COMMON)
}

const ARENA: &str = "struct Arena { base: *mut u8, cap: usize, offset: usize, live: usize }\n\
impl Arena {\n\
    fn new(cap: usize) -> Arena { unsafe { let p: *mut u8 = alloc(cap, 16); return Arena { base: p, cap: cap, offset: 0, live: 0 }; } }\n\
    fn release(borrow mut self) { unsafe { dealloc(self.base, self.cap, 16); } }\n\
}\n\
impl Allocator for Arena {\n\
    fn alloc(borrow mut self, size: usize, align: usize) -> *mut u8 {\n\
        unsafe {\n\
            let start = (self.offset + align - 1) / align * align;\n\
            if start + size > self.cap { let zero: usize = 0; return zero as *mut u8; }\n\
            self.offset = start + size;\n\
            return (self.base as usize + start) as *mut u8;\n\
        }\n\
    }\n\
    fn dealloc(borrow mut self, ptr: *mut u8, size: usize) { }\n\
}\n";

fn build_release(src: &str) -> std::path::PathBuf {
    let f = tmp(src);
    let exe = f.with_extension("bin");
    let b = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("--release").arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    exe
}

fn median_ms(exe: &std::path::Path, runs: usize) -> f64 {
    let mut t: Vec<f64> = (0..runs)
        .map(|_| {
            let s = std::time::Instant::now();
            let out = Command::new(exe).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
            s.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    t.sort_by(|a, b| a.partial_cmp(b).unwrap());
    t[t.len() / 2]
}

#[test]
fn arena_allocator_beats_the_default_allocator_on_a_million_small_allocations() {
    let d = build_release(&bench_default());
    let a = build_release(&bench_arena());
    let _ = median_ms(&d, 1); // warm-up
    let _ = median_ms(&a, 1);
    // interleave the measurements so that machine noise hits both sides alike
    let (mut ds, mut as_) = (Vec::new(), Vec::new());
    for _ in 0..3 {
        ds.push(median_ms(&d, 5));
        as_.push(median_ms(&a, 5));
    }
    ds.sort_by(|x, y| x.partial_cmp(y).unwrap());
    as_.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let (dm, am) = (ds[1], as_[1]);
    println!("arena benchmark (1,000,000 allocations, release build, median of 15 runs): default allocator {:.3} ms, arena {:.3} ms", dm, am);
    assert!(am < dm, "the arena ({:.3} ms) must be faster than the default allocator ({:.3} ms)", am, dm);
}

// ------------------------------------------------------------------ audit: no "Phase 11" excuse is left

#[test]
fn an_i128_inside_an_enum_payload_works() {
    let src = "enum V { Big(i128), Small(i32), Nothing }\nfn get(v: V) -> i64 { match v { V::Big(x) => return x as i64, V::Small(s) => return s as i64, V::Nothing => return -1 } }\nfn main() { println(get(V::Big(123456789012))); println(get(V::Small(5))); println(get(V::Nothing)); let o: Option<i128> = Some(77); match o { Some(x) => println(x as i64), None => println(0) } }";
    let (o, c) = run(src);
    assert_eq!((o.as_str(), c), ("123456789012\n5\n-1\n77\n", 0));
}

#[test]
fn an_uninstantiated_generic_struct_and_function_compile_fine() {
    let (o, c) = run("struct W<T> { v: T }\nfn id<T>(x: T) -> T { return x; }\nfn main() { println(1); }");
    assert_eq!((o.as_str(), c), ("1\n", 0));
}

#[test]
fn named_arguments_to_a_tuple_struct_are_a_clear_error_without_a_phase() {
    let m = rejected_build("struct P(i32, i32);\nfn main() { let p = P(a: 1, b: 2); }");
    assert!(!m.contains("Phase 11"), "{}", m);
}

#[test]
fn unknown_pointer_associated_function_is_a_clear_error_without_a_phase() {
    let m = rejected_build("fn main() { let r = Rc::frobnicate(1); }");
    assert!(!m.contains("Phase 11"), "{}", m);
}

#[test]
fn no_user_visible_message_blames_phase_11() {
    // every program above that failed did so with a message free of the retired "Phase 11" excuse
    for src in [
        "fn main() { let x = null; }",
        "fn main() { let a = [1, 2]; match a { [x, y] => println(x), _ => println(0) } }",
        "fn f<T>(x: T) -> i32 { return 1; }\nfn main() { let g = f::<i32>; }",
    ] {
        let m = rejected_build(src);
        assert!(!m.contains("Phase 11"), "{}\n{}", src, m);
    }
}

fn rejected_build(src: &str) -> String {
    let f = tmp(src);
    let exe = f.with_extension("bin");
    let b = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(!b.status.success(), "should have been rejected:\n{}", src);
    String::from_utf8_lossy(&b.stdout).to_string() + &String::from_utf8_lossy(&b.stderr)
}
