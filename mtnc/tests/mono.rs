//! Phase 11b-2: monomorphization tests (Doc 17 §5, Doc 8 §1-2).
use std::process::Command;

fn run(src: &str) -> (String, i32) {
    let dir = std::env::temp_dir().join(format!("mtn_mono_{}_{}", std::process::id(), src.len()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("p.mtn");
    std::fs::write(&f, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("run").arg(&f).output().unwrap();
    (String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr), out.status.code().unwrap_or(-1))
}

fn ir(src: &str) -> String {
    mtnc::driver::emit_ir(src, &mtnc::codegen::Options { release: false }).unwrap_or_else(|e| panic!("{:?}", e))
}

const PICK: &str = "fn pick<T>(a: T, b: T, first: bool) -> T { if first { return a; } return b; }\n";

#[test]
fn generic_fn_inferred_over_two_types() {
    let (o, c) = run(&format!("{}fn main() {{ println(pick(3, 4, false)); println(pick(1.5, 2.5, true)); }}\n", PICK));
    assert_eq!((o.as_str(), c), ("4\n1.5\n", 0));
}

#[test]
fn explicit_turbofish_selects_the_type() {
    let (o, c) = run(&format!("{}fn main() {{ let x = pick::<i64>(7, 8, false); println(x); }}\n", PICK));
    assert_eq!((o.as_str(), c), ("8\n", 0));
}

#[test]
fn generic_calling_generic() {
    let src = format!("{}fn twice<T>(x: T, y: T) -> T {{ return pick(x, y, true); }}\nfn main() {{ println(twice(9, 10)); println(twice(1.5, 2.5)); }}\n", PICK);
    let (o, c) = run(&src);
    assert_eq!((o.as_str(), c), ("9\n1.5\n", 0));
}

#[test]
fn same_instantiation_is_emitted_once_distinct_types_twice() {
    let one = ir(&format!("{}fn main() {{ println(pick(1, 2, true)); println(pick(3, 4, false)); }}\n", PICK));
    assert_eq!(one.matches("define i32 @\"mtn_pick$i32\"").count(), 1, "{}", one);
    let two = ir(&format!("{}fn main() {{ println(pick(1, 2, true)); println(pick(1.5, 2.5, true)); }}\n", PICK));
    assert!(two.contains("mtn_pick$i32") && two.contains("mtn_pick$f64"), "{}", two);
}

#[test]
fn generic_only_code_has_no_dynamic_dispatch() {
    let text = ir(&format!("{}fn main() {{ println(pick(1, 2, true)); }}\n", PICK));
    assert!(!text.contains("vtable") && !text.contains("call void %"), "{}", text);
}

#[test]
fn infinite_instantiation_is_a_compile_error() {
    let src = "fn f<T>(x: T, n: i32) -> i32 { if n > 0 { return f([x], n - 1); } return 0; }\nfn main() { println(f(1, 3)); }\n";
    let (o, c) = run(src);
    assert_ne!(c, 0, "{}", o);
}

#[test]
fn ambiguous_type_parameter_is_rejected() {
    let (o, c) = run("fn mk<T>() -> i32 { 1 }\nfn main() { println(mk()); }\n");
    assert_ne!(c, 0);
    assert!(o.contains("cannot infer"), "{}", o);
}
