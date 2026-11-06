//! Phase 11c C5: `dyn Trait` — fat pointers and vtables (Doc 7, Doc 13 §1).
use mtnc::codegen::Options;
use mtnc::driver;
use std::process::Command;

/// Unique per call: tests run in parallel threads of one process.
static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn tmp(src: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mtn_c5_{}_{}", std::process::id(), COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
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

// ------------------------------------------------------------------ examples

#[test]
fn memory_example_dyn_shapes() {
    let (o, c) = run_file(&example("dyn_shapes.mtn"));
    assert_eq!(c, 0);
    assert_eq!(o, "4\n6\n7\n73\n9\n2\nholder\n3\n251\n");
}

// ------------------------------------------------------------------ behaviour

fn prog(main_body: &str) -> String {
    format!("{}\nfn main() {{ {} }}", PRELUDE, main_body)
}

const PRELUDE: &str = r#"trait Shape { fn area(borrow self) -> i32; fn grow(borrow mut self, by: i32); }
struct Sq { s: i32 }
struct Re { w: i32, h: i32 }
struct Nm { name: String, s: i32 }
impl Shape for Sq { fn area(borrow self) -> i32 { return self.s * self.s; } fn grow(borrow mut self, by: i32) { self.s = self.s + by; } }
impl Shape for Re { fn area(borrow self) -> i32 { return self.w * self.h; } fn grow(borrow mut self, by: i32) { self.w = self.w + by; } }
impl Shape for Nm { fn area(borrow self) -> i32 { return self.s; } fn grow(borrow mut self, by: i32) { self.s = self.s + by; } }
fn total(s: borrow dyn Shape) -> i32 { return s.area(); }
"#;

#[test]
fn three_concrete_types_dispatch_through_the_vtable() {
    let (o, c) = run(&prog("let a: Box<dyn Shape> = Box::new(Sq { s: 3 }); let b: Box<dyn Shape> = Box::new(Re { w: 2, h: 5 }); let c: Box<dyn Shape> = Box::new(Nm { name: String::from(\"n\"), s: 7 }); println(a.area()); println(b.area()); println(c.area());"));
    assert_eq!((o.as_str(), c), ("9\n10\n7\n", 0));
}

#[test]
fn borrow_dyn_parameter_accepts_a_box_and_a_plain_value() {
    let (o, _) = run(&prog("let a: Box<dyn Shape> = Box::new(Re { w: 2, h: 2 }); let q = Sq { s: 3 }; println(total(borrow a)); println(total(borrow q));"));
    assert_eq!(o, "4\n9\n");
}

#[test]
fn borrow_mut_method_through_dyn_mutates_the_concrete_value() {
    let (o, _) = run(&prog("let mut a: Box<dyn Shape> = Box::new(Sq { s: 1 }); a.grow(2); println(a.area());"));
    assert_eq!(o, "9\n");
}

#[test]
fn dropping_a_boxed_dyn_runs_the_concrete_glue() {
    // `Nm` owns a String: a leak would fail the valgrind check inside `run`
    let (_, c) = run(&prog("let a: Box<dyn Shape> = Box::new(Nm { name: String::from(\"owned\"), s: 1 }); println(a.area());"));
    assert_eq!(c, 0);
}

#[test]
fn growable_of_boxed_dyn_frees_every_element() {
    let (o, _) = run(&prog("let v: [Box<dyn Shape>] = [Box::new(Sq { s: 1 }), Box::new(Nm { name: String::from(\"x\"), s: 4 })]; println(v[1].area());"));
    assert_eq!(o, "4\n");
}

#[test]
fn boxed_dyn_returned_from_a_function_and_held_in_a_struct() {
    let src = format!("{}\nstruct H {{ s: Box<dyn Shape> }}\nfn mk(k: i32) -> Box<dyn Shape> {{ if k == 0 {{ return Box::new(Sq {{ s: 2 }}); }} return Box::new(Nm {{ name: String::from(\"m\"), s: 5 }}); }}\nfn main() {{ let h = H {{ s: mk(1) }}; println(h.s.area()); println(mk(0).area()); }}", PRELUDE);
    let (o, c) = run(&src);
    assert_eq!((o.as_str(), c), ("5\n4\n", 0));
}

#[test]
fn option_of_boxed_dyn_moves_and_drops() {
    let (o, _) = run(&prog("let o: Option<Box<dyn Shape>> = Some(Box::new(Nm { name: String::from(\"o\"), s: 8 })); match o { Some(s) => println(s.area()), None => println(0) }"));
    assert_eq!(o, "8\n");
}

#[test]
fn explicit_drop_of_a_boxed_dyn() {
    let (o, _) = run(&prog("let a: Box<dyn Shape> = Box::new(Nm { name: String::from(\"d\"), s: 2 }); drop(a); println(1);"));
    assert_eq!(o, "1\n");
}

// ------------------------------------------------------------------ rejections

#[test]
fn a_type_that_does_not_implement_the_trait_cannot_be_boxed_as_dyn() {
    let e = rejected(&format!("{}\nstruct Other {{ x: i32 }}\nfn main() {{ let a: Box<dyn Shape> = Box::new(Other {{ x: 1 }}); }}", PRELUDE));
    assert!(e.contains("expected `Box<dyn Shape>`"), "{}", e);
}

#[test]
fn by_value_self_method_cannot_be_called_through_dyn() {
    let f = tmp("trait T { fn eat(self) -> i32; } struct A { x: i32 } impl T for A { fn eat(self) -> i32 { return self.x; } } fn main() { let b: Box<dyn T> = Box::new(A { x: 1 }); println(b.eat()); }");
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("-o").arg(f.with_extension("bin")).output().unwrap();
    assert!(!out.status.success());
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("cannot be called through `dyn T`"), "{}", e);
}

#[test]
fn use_after_move_of_a_boxed_dyn_is_rejected() {
    let e = rejected(&prog("let a: Box<dyn Shape> = Box::new(Sq { s: 1 }); let b = a; println(a.area());"));
    assert!(e.contains("use of moved value"), "{}", e);
}

// ------------------------------------------------------------------ IR

#[test]
fn vtable_layout_is_drop_size_align_then_methods_in_trait_order() {
    let text = ir(&prog("let a: Box<dyn Shape> = Box::new(Nm { name: String::from(\"n\"), s: 1 }); println(a.area());"));
    let line = text.lines().find(|l| l.starts_with("@__mtn_vtable_Nm_Shape")).expect("vtable global");
    assert!(line.contains("constant { ptr, i64, i64, ptr, ptr }"), "{}", line);
    assert!(line.contains("@__mtn_drop_Nm"), "drop glue first: {}", line);
    let area = line.find("@mtn_Nm__Shape__area").or_else(|| line.find("area")).expect("area slot");
    let grow = line.find("grow").expect("grow slot");
    assert!(area < grow, "trait order: {}", line);
}

#[test]
fn a_type_without_heap_has_a_null_drop_slot() {
    let text = ir(&prog("let a: Box<dyn Shape> = Box::new(Sq { s: 1 }); println(a.area());"));
    let line = text.lines().find(|l| l.starts_with("@__mtn_vtable_Sq_Shape")).expect("vtable global");
    assert!(line.contains("{ ptr null, i64 4, i64 4,"), "{}", line);
}

#[test]
fn the_call_goes_through_an_indirect_call_and_the_box_glue_through_the_vtable() {
    let text = ir(&prog("let a: Box<dyn Shape> = Box::new(Sq { s: 2 }); println(a.area());"));
    assert!(text.contains("call i32 %method("), "{}", text);
    assert!(text.contains("define void @__mtn_drop_Box_dyn_Shape_"), "{}", text);
    assert!(text.contains("call void %dropfn("), "{}", text);
}

#[test]
fn one_vtable_per_type_and_trait_pair() {
    let text = ir(&prog("let a: Box<dyn Shape> = Box::new(Sq { s: 1 }); let b: Box<dyn Shape> = Box::new(Sq { s: 2 }); println(a.area() + b.area());"));
    assert_eq!(text.matches("@__mtn_vtable_Sq_Shape = ").count(), 1, "{}", text);
}
