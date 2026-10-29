//! Phase 11b-2, M3: associated types (Doc 8 §5) and impls specialised on concrete
//! arguments (`impl Trait for Stack<i32>`, `impl Stack<i32> { .. }`).
use std::process::Command;

fn run_src(src: &str) -> (String, i32) {
    let dir = std::env::temp_dir().join(format!("mtn_assoc_{}_{}", std::process::id(), src.len()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("p.mtn");
    std::fs::write(&f, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("run").arg(&f).output().unwrap();
    (String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr), out.status.code().unwrap_or(-1))
}

fn example(name: &str) -> (String, i32) {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").join(name);
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("run").arg(&p).output().unwrap();
    (String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr), out.status.code().unwrap_or(-1))
}

fn rejected(src: &str) -> String {
    let (o, c) = run_src(src);
    assert_ne!(c, 0, "should be rejected: {}", o);
    assert!(!o.contains("panicked"), "{}", o);
    o
}

#[test]
fn doc8_container_example_with_specialised_impls_and_projection() {
    assert_eq!(example("assoc_container.mtn"), ("2\n10\n20\n2.5\n3\nsome\nnone\n".to_string(), 0));
}

#[test]
fn generic_impl_associated_type_inherent_specialised_impl_and_plain_type() {
    assert_eq!(example("assoc_generic_impl.mtn"), ("2.5\n9\n9\n".to_string(), 0));
}

#[test]
fn impl_missing_an_associated_type_is_rejected() {
    let o = rejected("trait C { type Item; fn f(borrow self) -> i32; }\nstruct S { v: i32 }\nimpl C for S { fn f(borrow self) -> i32 { return 1; } }\nfn main() { println(1); }\n");
    assert!(o.contains("missing associated type `Item`"), "{}", o);
}

#[test]
fn impl_defining_an_undeclared_associated_type_is_rejected() {
    let o = rejected("trait C { fn f(borrow self) -> i32; }\nstruct S { v: i32 }\nimpl C for S { type Extra = i32; fn f(borrow self) -> i32 { return 1; } }\nfn main() { println(1); }\n");
    assert!(o.contains("does not declare"), "{}", o);
}

#[test]
fn specialised_inherent_impl_does_not_apply_to_other_arguments() {
    let o = rejected("struct Stack<T> { items: [T; 2] }\nimpl Stack<i32> { fn sum(borrow self) -> i32 { return 0; } }\nfn main() { let s = Stack { items: [1.5, 2.5] }; println(s.sum()); }\n");
    assert!(o.contains("no method named `sum`"), "{}", o);
}

#[test]
fn specialised_trait_impl_does_not_satisfy_bound_for_other_arguments() {
    let o = rejected("trait C { type Item; fn f(borrow self) -> i32; }\nstruct Stack<T> { items: [T; 2] }\nimpl C for Stack<i32> { type Item = i32; fn f(borrow self) -> i32 { return 1; } }\nfn need<X>(borrow x: X) -> i32 where X: C { return x.f(); }\nfn main() { let s = Stack { items: [true, false] }; println(need(borrow s)); }\n");
    assert!(o.contains("does not satisfy bound `C`"), "{}", o);
}

#[test]
fn projection_follows_each_instantiation() {
    // `second` returns `C::Item`: i32 for Stack<i32>, f64 for Stack<f64>.
    let src = "trait Container { type Item; fn at(borrow self, i: usize) -> Self::Item; }\nstruct Stack<T> { items: [T; 2] }\nimpl Container for Stack<i32> { type Item = i32; fn at(borrow self, i: usize) -> i32 { return self.items[i]; } }\nimpl Container for Stack<f64> { type Item = f64; fn at(borrow self, i: usize) -> f64 { return self.items[i]; } }\nfn pick<C>(borrow c: C) -> C::Item where C: Container { return c.at(1); }\nfn main() { let a: i32 = pick(borrow Stack { items: [1, 2] }); let b: f64 = pick(borrow Stack { items: [1.5, 2.5] }); println(a); println(b); }\n";
    let (o, c) = run_src(src);
    assert_eq!((o.as_str(), c), ("2\n2.5\n", 0), "{}", o);
}

#[test]
fn projection_type_mismatch_is_rejected() {
    // `pick` over Stack<i32> yields i32, so binding it as f64 is a type error.
    let o = rejected("trait Container { type Item; fn at(borrow self, i: usize) -> Self::Item; }\nstruct Stack<T> { items: [T; 2] }\nimpl Container for Stack<i32> { type Item = i32; fn at(borrow self, i: usize) -> i32 { return self.items[i]; } }\nfn pick<C>(borrow c: C) -> C::Item where C: Container { return c.at(1); }\nfn main() { let s = Stack { items: [1, 2] }; let b: f64 = pick(borrow s); println(b); }\n");
    assert!(o.contains("expected `f64`"), "{}", o);
}
