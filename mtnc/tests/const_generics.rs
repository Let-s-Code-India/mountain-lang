//! Phase 11b-2, M4: const generics (Doc 8 §8) and the repeat expression `[x; N]`.
use std::process::Command;

fn run_src(src: &str) -> (String, i32) {
    let dir = std::env::temp_dir().join(format!("mtn_constg_{}_{}", std::process::id(), src.len()));
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

fn rejected(src: &str, needle: &str) {
    let (o, c) = run_src(src);
    assert!(c != 0 && o.contains(needle) && !o.contains("panicked"), "expected `{}`, got ({}): {}", needle, c, o);
}

fn ir(src: &str) -> String {
    mtnc::driver::emit_ir(src, &mtnc::codegen::Options { release: false }).unwrap_or_else(|e| panic!("{:?}", e))
}

#[test]
fn repeat_expression_example() {
    assert_eq!(example("array_repeat.mtn"), ("7\n0\n1.5\n3\n28\n".to_string(), 0));
}

#[test]
fn const_generic_functions_example() {
    assert_eq!(example("const_generic_fns.mtn"), ("0\n0\n10\n1.5\n18\n".to_string(), 0));
}

#[test]
fn const_generic_container_example() {
    assert_eq!(example("const_generic_container.mtn"), ("4\n2\n2\n2.5\n6\n9\n".to_string(), 0));
}

#[test]
fn const_generic_matmul_example() {
    assert_eq!(example("const_generic_matmul.mtn"), ("58\n64\n139\n154\n".to_string(), 0));
}

#[test]
fn const_generic_assoc_example() {
    assert_eq!(example("const_generic_assoc.mtn"), ("4\n7\n2\n0\n6\n1\n".to_string(), 0));
}

#[test]
fn identity_over_const_n_is_one_instance_per_value() {
    let src = "fn identity<const N: usize>() -> [i32; N] { return [0; N]; }\nfn main() { let a: [i32; 3] = identity(); let b = identity::<3>(); let c = identity::<5>(); println(a[0] + b[1] + c[4]); }\n";
    let t = ir(src);
    assert_eq!(t.matches("define [3 x i32] @\"mtn_identity$3\"").count(), 1, "{}", t);
    assert!(t.contains("mtn_identity$5"), "{}", t);
    let (o, c) = run_src(src);
    assert_eq!((o.as_str(), c), ("0\n", 0));
}

#[test]
fn zero_length_repeat_is_valid() {
    let (o, c) = run_src("fn main() { let a: [i32; 0] = [1; 0]; println(7); }\n");
    assert_eq!((o.as_str(), c), ("7\n", 0));
}

#[test]
fn repeat_length_may_be_const_arithmetic() {
    let (o, c) = run_src("const W: usize = 2;\nfn main() { let a: [u8; W * 3 + 1] = [5; W * 3 + 1]; println(a[6]); }\n");
    assert_eq!((o.as_str(), c), ("5\n", 0));
}

// ---- rejection tests (Doc 25 §2.2 item 2)

#[test]
fn repeat_with_non_constant_length_is_rejected() {
    rejected("fn main() { let n = 3; let a = [0; n]; }\n", "must be a compile-time constant");
}

#[test]
fn repeat_with_non_copy_element_is_rejected() {
    rejected("fn main() { let a: [String; 2] = [\"a\".to_string(); 2]; }\n", "must be `copy`");
}

#[test]
fn repeat_with_non_copy_element_after_instantiation_is_rejected() {
    rejected("fn fill<T, const N: usize>(x: T) -> [T; N] { return [x; N]; }\nfn main() { let s = fill::<String, 2>(\"a\".to_string()); }\n", "must be `copy`");
}

#[test]
fn repeat_length_must_match_the_annotated_length() {
    rejected("fn main() { let a: [i32; 3] = [0; 4]; }\n", "expected `[i32; 3]`, found `[i32; 4]`");
}

#[test]
fn negative_repeat_length_is_rejected() {
    rejected("fn main() { let a = [1; -1]; }\n", "out of range");
}

const MATRIX: &str = "struct Matrix<T, const R: usize, const C: usize> { data: [T; R * C] }\nfn matmul<const R: usize, const K: usize, const C: usize>(a: Matrix<f64, R, K>, b: Matrix<f64, K, C>) -> Matrix<f64, R, C> { return Matrix { data: [0.0; R * C] }; }\n";

#[test]
fn matmul_with_mismatched_inner_dimension_is_rejected() {
    // Phase 5 exit criterion (Doc 25 §2.3, Doc 8 §9): 3 != 4.
    rejected(&format!("{}fn main() {{ let a: Matrix<f64, 2, 3> = Matrix {{ data: [0.0; 6] }}; let b: Matrix<f64, 4, 5> = Matrix {{ data: [0.0; 20] }}; let c = matmul(a, b); }}\n", MATRIX), "const generic parameter `K`");
}

#[test]
fn matmul_with_matching_dimensions_has_the_inferred_result_shape() {
    let (o, c) = run_src(&format!("{}fn main() {{ let a: Matrix<f64, 2, 3> = Matrix {{ data: [1.0; 6] }}; let b: Matrix<f64, 3, 4> = Matrix {{ data: [1.0; 12] }}; let c: Matrix<f64, 2, 4> = matmul(a, b); println(c.data[7]); }}\n", MATRIX));
    assert_eq!((o.as_str(), c), ("0\n", 0));
}

#[test]
fn matmul_result_of_the_wrong_declared_shape_is_rejected() {
    rejected(&format!("{}fn main() {{ let a: Matrix<f64, 2, 3> = Matrix {{ data: [1.0; 6] }}; let b: Matrix<f64, 3, 4> = Matrix {{ data: [1.0; 12] }}; let c: Matrix<f64, 2, 5> = matmul(a, b); }}\n", MATRIX), "expected");
}

#[test]
fn const_parameter_that_nothing_fixes_is_rejected() {
    rejected("fn identity<const N: usize>() -> [i32; N] { return [0; N]; }\nfn main() { let z = identity(); }\n", "cannot infer const generic parameter `N`");
}

#[test]
fn struct_field_with_wrong_length_for_its_instance_is_rejected() {
    rejected("struct Grid<const R: usize, const C: usize> { cells: [i32; R * C] }\nfn main() { let g: Grid<2, 3> = Grid { cells: [1, 2] }; }\n", "has 2 element(s) but the type requires 6");
}

#[test]
fn const_parameter_on_a_generic_enum() {
    let src = "enum Slot<T, const N: usize> { Full([T; N]), Empty }\nfn main() {\n    let s: Slot<i32, 3> = Slot::Full([4, 5, 6]);\n    match s {\n        Slot::Full(a) => println(a[2]),\n        Slot::Empty => println(0),\n    }\n}\n";
    let (o, c) = run_src(src);
    assert_eq!((o.as_str(), c), ("6\n", 0));
}
