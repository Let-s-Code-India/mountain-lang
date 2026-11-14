//! Generics tests, per Document 25 §2.3's Phase 5 exit criteria: the
//! `Matrix<f64,2,3> * Matrix<f64,4,5>` dimension-mismatch case (Document
//! 8 §9) must be rejected at compile time via a real const-generic
//! check, and monomorphized generic-function calls must resolve
//! statically (never through the `dyn` machinery Phase 4 built). Tests
//! both directions: valid generic instantiation that should
//! monomorphize correctly, and invalid usage (unsatisfied bounds,
//! mismatched const-generic dimensions, wrong argument counts) that
//! must be rejected.

use mtnc::lexer;
use mtnc::parser::parse_program;
use mtnc::types::TypeChecker;

fn check(src: &str) -> Vec<String> {
    let (tokens, lex_errs) = lexer::tokenize(src);
    assert!(lex_errs.is_empty(), "lex errors: {:?}", lex_errs);
    let (program, parse_errs) = parse_program(tokens);
    assert!(parse_errs.is_empty(), "parse errors: {:?}", parse_errs);
    let mut tc = TypeChecker::new();
    tc.check_program(&program);
    tc.errors.iter().map(|e| e.to_string()).collect()
}

fn assert_ok(src: &str) {
    let errs = check(src);
    assert!(errs.is_empty(), "expected no type errors, got: {:#?}", errs);
}

fn assert_rejected(src: &str) {
    let errs = check(src);
    assert!(!errs.is_empty(), "expected type errors, got none");
}

// ============================================================
// Document 8 §8/§9 — const generics, the core exit-criteria case
// ============================================================

const MATRIX_DECL: &str = r#"
struct Matrix<T, const ROWS: usize, const COLS: usize> {
    data: [T],
}
"#;

#[test]
fn matrix_multiply_mismatched_dimensions_rejected() {
    // Document 8 §9's exact traced case: Matrix<f64,2,3> * Matrix<f64,4,5>
    // -- column count 3 of the left operand does not equal row count 4
    // of the right operand.
    let src = format!("{}\nfn main() {{\n    let a: Matrix<f64,2,3> = Matrix {{ data: [] }};\n    let b: Matrix<f64,4,5> = Matrix {{ data: [] }};\n    let c = a * b;\n}}\n", MATRIX_DECL);
    let errs = check(&src);
    assert!(!errs.is_empty(), "expected a dimension-mismatch error");
    assert!(
        errs.iter().any(|e| e.contains("dimension mismatch")),
        "expected a dimension-mismatch-specific message, got: {:#?}", errs
    );
}

#[test]
fn matrix_multiply_matching_dimensions_accepted_with_correct_result_shape() {
    // Document 8 §9's positive companion, not explicitly in the spec's
    // own trace but the necessary other half: 2x3 * 3x5 is valid and
    // produces a 2x5 result -- confirmed here by annotating the result
    // with the expected shape and asserting zero errors.
    let src = format!(
        "{}\nfn main() {{\n    let a: Matrix<f64,2,3> = Matrix {{ data: [] }};\n    let b: Matrix<f64,3,5> = Matrix {{ data: [] }};\n    let c: Matrix<f64,2,5> = a * b;\n}}\n",
        MATRIX_DECL
    );
    assert_ok(&src);
}

#[test]
fn matrix_multiply_wrong_result_shape_annotation_rejected() {
    // Same valid 2x3 * 3x5 multiplication, but annotated with the WRONG
    // result shape -- confirms the computed result type is actually
    // `Matrix<f64,2,5>`, not just "no error was raised for any reason".
    let src = format!(
        "{}\nfn main() {{\n    let a: Matrix<f64,2,3> = Matrix {{ data: [] }};\n    let b: Matrix<f64,3,5> = Matrix {{ data: [] }};\n    let c: Matrix<f64,9,9> = a * b;\n}}\n",
        MATRIX_DECL
    );
    assert_rejected(&src);
}

#[test]
fn const_generic_wrong_argument_count_rejected() {
    let src = format!("{}\nfn main() {{\n    let a: Matrix<f64,2> = Matrix {{ data: [] }};\n}}\n", MATRIX_DECL);
    let errs = check(&src);
    assert!(!errs.is_empty(), "expected a generic-argument-count error");
    assert!(errs.iter().any(|e| e.contains("generic argument")));
}

#[test]
fn const_generic_wrong_argument_kind_rejected() {
    // A type where a const value was declared, and vice versa.
    let src = format!("{}\nfn main() {{\n    let a: Matrix<f64,i32,3> = Matrix {{ data: [] }};\n}}\n", MATRIX_DECL);
    assert_rejected(&src);
}

// ============================================================
// Document 8 §1–§4 — generic structs, type-parameter fields
// ============================================================

#[test]
fn generic_struct_field_types_checked_via_annotation() {
    assert_ok(r#"
struct Pair<A, B> {
    first: A,
    second: B,
}
fn main() {
    let p: Pair<i32, String> = Pair { first: 1, second: "x" };
}
"#);
}

#[test]
fn generic_struct_field_wrong_type_rejected() {
    assert_rejected(r#"
struct Pair<A, B> {
    first: A,
    second: B,
}
fn main() {
    let p: Pair<i32, String> = Pair { first: "not an int", second: "x" };
}
"#);
}

#[test]
fn generic_struct_field_access_resolves_substituted_type() {
    assert_ok(r#"
struct Pair<A, B> {
    first: A,
    second: B,
}
fn main() {
    let p: Pair<i32, String> = Pair { first: 1, second: "x" };
    let n: i32 = p.first;
}
"#);
}

// ============================================================
// Document 8 §2/§3 — where-clause bounds, monomorphized calls
// ============================================================

#[test]
fn generic_function_call_with_satisfied_bound_accepted() {
    assert_ok(r#"
trait Comparable {
    fn compareTo(borrow self, other: borrow Self) -> i32;
}
impl Comparable for i32 {
    fn compareTo(borrow self, other: borrow Self) -> i32 {
        return 0;
    }
}
fn largest<T>(list: [T]) -> T where T: Comparable {
    return list[0];
}
fn main() {
    let nums: [i32] = [3, 7, 1];
    let biggest: i32 = largest(nums);
}
"#);
}

#[test]
fn generic_function_call_with_unsatisfied_bound_rejected() {
    // i32 here deliberately has NO `impl Comparable for i32` -- the
    // bound is declared but never satisfied.
    let errs = check(r#"
trait Comparable {
    fn compareTo(borrow self, other: borrow Self) -> i32;
}
fn largest<T>(list: [T]) -> T where T: Comparable {
    return list[0];
}
fn main() {
    let nums: [i32] = [3, 7, 1];
    let biggest = largest(nums);
}
"#);
    assert!(!errs.is_empty(), "expected an unsatisfied-bound error");
    assert!(
        errs.iter().any(|e| e.contains("does not satisfy bound") && e.contains("Comparable")),
        "got: {:#?}", errs
    );
}

#[test]
fn generic_function_multiple_bounds_via_plus_all_required() {
    // Document 8 §3: `T: Serializable + Comparable + Clone` -- a type
    // satisfying only ONE of several required bounds must still be
    // rejected.
    let errs = check(r#"
trait Serializable {
    fn serialize(borrow self) -> String;
}
trait Comparable {
    fn compareTo(borrow self, other: borrow Self) -> i32;
}
struct Widget {
    id: i32,
}
impl Serializable for Widget {
    fn serialize(borrow self) -> String {
        return "widget";
    }
}
fn processItem<T>(item: T) where T: Serializable, T: Comparable {
}
fn main() {
    let w = Widget { id: 1 };
    processItem(w);
}
"#);
    assert!(!errs.is_empty(), "expected an error for the unsatisfied Comparable bound");
    assert!(errs.iter().any(|e| e.contains("Comparable")));
    // Serializable IS satisfied -- must not also be (incorrectly) flagged.
    assert!(!errs.iter().any(|e| e.contains("Serializable") && e.contains("does not satisfy")));
}

#[test]
fn generic_call_resolves_statically_not_via_dyn() {
    // Exit criterion: "monomorphized output must be verified to contain
    // zero dyn-style dispatch for generic-only code". The concrete,
    // checkable evidence: `satisfies_bound` looks up the registry key
    // via `ty_lookup_name`, which has NO match arm for `Ty::DynTrait` at
    // all (only `Ty::Named`/`Ty::Generic`/primitives) -- so if the
    // generic-call substitution in `Expr::Call` had (incorrectly) bound
    // `T` to `Ty::DynTrait("Comparable")` instead of the concrete
    // `Ty::Named("Widget")`, bound-checking would universally fail
    // (`ty_lookup_name` returns `None` for it) regardless of any real
    // `impl` existing. This test passing at all is direct evidence the
    // substitution used the concrete type, not a `dyn` handle.
    assert_ok(r#"
trait Comparable {
    fn describe(borrow self) -> String;
}
struct Widget {
    id: i32,
}
impl Comparable for Widget {
    fn describe(borrow self) -> String {
        return "described";
    }
}
fn useGeneric<T>(item: T) -> bool where T: Comparable {
    return true;
}
fn main() {
    let w = Widget { id: 1 };
    let ok: bool = useGeneric(w);
}
"#);
}
