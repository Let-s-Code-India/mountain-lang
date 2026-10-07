//! Phase 11a front-end tests: the type-checker changes that code generation
//! relies on (fixed-size arrays, aliases, consts, `borrow` parameter types,
//! associated functions, default trait methods, nested items).

use mtnc::driver::check_source;

fn ok(src: &str) {
    if let Err(e) = check_source(src) {
        panic!("expected the program to check, got: {:?}\n--- source ---\n{}", e, src);
    }
}

fn err_contains(src: &str, needle: &str) {
    match check_source(src) {
        Ok(_) => panic!("expected an error containing {:?}, but the program checked:\n{}", needle, src),
        Err(e) => assert!(e.iter().any(|m| m.contains(needle)), "no error containing {:?} in {:?}", needle, e),
    }
}

#[test]
fn fixed_array_literal_must_have_the_declared_length() {
    ok("fn main() { let a: [i32; 3] = [1, 2, 3]; }\n");
    err_contains("fn main() { let a: [i32; 3] = [1, 2]; }\n", "requires 3");
}

#[test]
fn fixed_array_size_may_be_a_named_constant_or_a_constant_expression() {
    ok("const N: usize = 2;\nconst M: usize = N * 2;\nfn main() { let a: [i32; N] = [1, 2]; let b: [u8; M] = [1, 2, 3, 4]; }\n");
}

#[test]
fn fixed_arrays_of_different_lengths_are_different_types() {
    err_contains("fn f(a: [i32; 3]) {}\nfn main() { let a: [i32; 2] = [1, 2]; f(a); }\n", "expected `[i32; 3]`");
}

#[test]
fn borrowing_a_fixed_array_as_a_slice_is_allowed() {
    ok("fn f(xs: &[i32]) {}\nfn main() { let a: [i32; 2] = [1, 2]; f(borrow a); }\n");
}

#[test]
fn indexing_yields_the_element_type_and_a_range_yields_a_slice() {
    ok("fn main() { let a: [i64; 3] = [1, 2, 3]; let x: i64 = a[1]; let s: &[i64] = borrow a[0..2]; }\n");
    err_contains("fn main() { let a: [i64; 3] = [1, 2, 3]; let x: bool = a[1]; }\n", "expected `bool`");
}

#[test]
fn type_aliases_are_expanded() {
    ok("type Id = u64;\nfn f(x: Id) -> u64 { x }\nfn main() { let a: Id = 5; let b: u64 = f(a); }\n");
}

#[test]
fn constants_and_statics_are_values_with_their_declared_type() {
    ok("const A: i32 = 4;\nstatic B: i64 = 9;\nfn main() { let x: i32 = A; let y: i64 = B; }\n");
    err_contains("const A: i32 = true;\nfn main() {}\n", "expected `i32`");
}

#[test]
fn borrow_parameters_take_borrowed_arguments() {
    ok("struct S { n: i32 }\nfn f(borrow s: S) -> i32 { s.n }\nfn g(borrow mut s: S) { s.n += 1; }\nfn main() { let mut s = S { n: 1 }; f(borrow s); g(borrow mut s); }\n");
    err_contains("struct S { n: i32 }\nfn f(borrow s: S) -> i32 { s.n }\nfn main() { let s = S { n: 1 }; f(s); }\n", "expected `&S`");
}

#[test]
fn associated_functions_and_self_resolve() {
    ok("struct P { x: i32 }\nimpl P { fn new(x: i32) -> P { P { x: x } } fn twin() -> Self { Self::new(1) } fn get(borrow self) -> i32 { self.x } }\nfn main() { let p = P::twin(); let n: i32 = p.get(); }\n");
}

#[test]
fn trait_default_methods_are_callable_on_implementors() {
    ok("trait T { fn a(borrow self) -> i32; fn b(borrow self) -> i32 { self.a() } }\nstruct S {}\nimpl T for S { fn a(borrow self) -> i32 { 1 } }\nfn main() { let s = S {}; let n: i32 = s.b(); }\n");
}

#[test]
fn impls_on_primitive_types_type_check_their_bodies() {
    ok("trait Twice { fn twice(borrow self) -> i32; }\nimpl Twice for i32 { fn twice(borrow self) -> i32 { self * 2 } }\nfn main() { let v: i32 = 4; let n: i32 = v.twice(); }\n");
}

#[test]
fn nested_items_are_visible_to_the_checker() {
    ok("fn main() { fn inner(x: i32) -> i32 { x + 1 } struct L { a: i32 } let l = L { a: inner(2) }; let n: i32 = l.a; }\n");
}

#[test]
fn variadic_parameter_is_a_slice_inside_the_function() {
    ok("fn f(...v: i32) -> i32 { v[0] }\nfn main() { let n = f(1, 2); }\n");
}

#[test]
fn default_parameter_value_must_match_the_parameter_type() {
    ok("fn f(a: i32, b: i32 = 5) -> i32 { a + b }\nfn main() { let n = f(1); }\n");
    err_contains("fn f(a: i32, b: i32 = true) -> i32 { a }\nfn main() {}\n", "default value");
}

#[test]
fn tuple_and_positional_fields_still_work() {
    ok("fn main() { let t = (1, (2, 3)); let n: i32 = t.1.0; }\n");
}
