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

// ---------------------------------------------------------------- examples

fn example(name: &str) -> (String, i32) {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").join(name);
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("run").arg(&p).output().unwrap();
    (String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr), out.status.code().unwrap_or(-1))
}

#[test]
fn example_generic_fn() {
    assert_eq!(example("generic_fn.mtn"), ("4\n1.5\n8\n9\n".to_string(), 0));
}

#[test]
fn example_generic_bounds_inline_where_and_chain() {
    assert_eq!(example("generic_bounds.mtn"), ("9\n2.5\n8\n4\n".to_string(), 0));
}

#[test]
fn example_generic_struct_pair_and_wrapper() {
    assert_eq!(example("generic_struct_pair.mtn"), ("1\n4.5\n7\n42\n3\n".to_string(), 0));
}

#[test]
fn example_generic_nested_bounded_impl_stack_and_trait_impl() {
    assert_eq!(example("generic_nested_impls.mtn"), ("7\n5\n3\n3\n77\n".to_string(), 0));
}

#[test]
fn example_generic_enum_with_data() {
    assert_eq!(example("generic_enum.mtn"), ("5\n0\n2.5\n1.5\n".to_string(), 0));
}

#[test]
fn generic_struct_instances_are_deduplicated() {
    let src = "struct W<T> { v: T }\nfn main() { let a = W { v: 1 }; let b = W { v: 2 }; let c = W { v: 2.5 }; println(a.v + b.v); println(c.v); }\n";
    let checked = mtnc::driver::check_source(src).unwrap_or_else(|e| panic!("{:?}", e));
    let mut names: Vec<String> = checked.program.items.iter().filter_map(|it| match &it.kind {
        mtnc::ast::ItemKind::Struct(s) => Some(s.name.clone()),
        _ => None,
    }).collect();
    names.sort();
    assert_eq!(names, vec!["W$f64".to_string(), "W$i32".to_string()]);
}

// -------------------------------------------------------------- rejections

fn rejected(src: &str) -> String {
    let (o, c) = run(src);
    assert_ne!(c, 0, "should be rejected: {}", o);
    assert!(!o.contains("panicked"), "{}", o);
    o
}

#[test]
fn missing_one_of_several_bounds_is_rejected() {
    let o = rejected("trait A { fn a(borrow self) -> i32; }\ntrait B { fn b(borrow self) -> i32; }\nimpl A for i32 { fn a(borrow self) -> i32 { return 1; } }\nfn f<T>(x: T) -> i32 where T: A + B { return 0; }\nfn main() { println(f(3)); }\n");
    assert!(o.contains("bound `B`"), "{}", o);
}

#[test]
fn method_on_unbounded_type_parameter_is_rejected() {
    let o = rejected("trait R { fn r(borrow self) -> i32; }\nfn bad<T>(a: T) -> i32 { return a.r(); }\nfn main() { println(1); }\n");
    assert!(o.contains("no method named `r`"), "{}", o);
}

#[test]
fn ambiguous_generic_enum_variant_needs_annotation() {
    let o = rejected("enum M<T> { J(T), N }\nfn main() { let x = M::N; println(1); }\n");
    assert!(o.contains("cannot infer"), "{}", o);
}

#[test]
fn bounded_impl_method_is_unavailable_when_bound_unsatisfied() {
    let o = rejected("trait S { fn s(borrow self) -> i32; }\nimpl S for i32 { fn s(borrow self) -> i32 { return 1; } }\nstruct P<A> { a: A }\nimpl<A> P<A> where A: S { fn t(borrow self) -> i32 { return self.a.s(); } }\nfn main() { let p = P { a: 2.5 }; println(p.t()); }\n");
    assert!(o.contains("no method named `t`") || o.contains("`t`"), "{}", o);
}

#[test]
fn generic_struct_literal_cannot_infer_unused_parameter() {
    let o = rejected("struct Tagged<T, U> { v: T }\nfn main() { let x = Tagged { v: 1 }; println(1); }\n");
    assert!(o.contains("cannot infer"), "{}", o);
}

#[test]
fn example_turbofish_on_type_signatures_and_generic_over_generic() {
    assert_eq!(example("generic_turbofish_sigs.mtn"), ("5\n8\n1\n1\n".to_string(), 0));
}
