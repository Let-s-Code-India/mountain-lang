//! Phase 11b-2, M5: generic traits (`trait Trait<T>`), `From<T>` as an ordinary generic trait,
//! and the built-in `Into<U>` with `.into()` (Doc 11 §2, Doc 7 §4). Every trait used here is
//! declared by the test itself, except `From`/`Into`, which are the language's own.
use std::process::Command;

fn run_src(src: &str) -> (String, i32) {
    let dir = std::env::temp_dir().join(format!("mtn_gtrait_{}_{}", std::process::id(), src.len()));
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

const CONVERT: &str = "trait Convert<T> { fn convert(borrow self) -> T; }\nimpl Convert<f64> for i32 { fn convert(borrow self) -> f64 { return 1.5; } }\nimpl Convert<bool> for i32 { fn convert(borrow self) -> bool { return true; } }\n";
const UNITS: &str = "struct Meters { v: i32 }\nstruct Feet { v: i32 }\nstruct Other { v: i32 }\nimpl From<Feet> for Meters { fn from(f: Feet) -> Meters { return Meters { v: f.v / 3 }; } }\n";

#[test]
fn generic_traits_example() {
    assert_eq!(example("generic_traits.mtn"), ("1.5\ntrue\n21\n2.5\n2\n9\n1\n8\n".to_string(), 0));
}

#[test]
fn from_and_into_example() {
    assert_eq!(example("from_into.mtn"), ("3\n4\n10\n7\n4\n5\n1\n4\n2\n".to_string(), 0));
}

#[test]
fn each_impl_of_a_generic_trait_is_its_own_function() {
    let t = ir(&format!("{}fn main() {{ let x: i32 = 3; let f: f64 = x.convert(); let b: bool = x.convert(); println(f); println(b); }}\n", CONVERT));
    assert!(t.contains("i32::Convert<f64>::convert") && t.contains("i32::Convert<bool>::convert"), "{}", t);
}

#[test]
fn nested_generic_arguments_close_with_double_angle() {
    let (o, c) = run_src("struct P<A, B> { a: A, b: B }\nstruct H<T> { v: T }\nfn main() { let h: H<P<i32, i32>> = H { v: P { a: 1, b: 2 } }; let o: Option<Option<i32>> = Some(None); println(h.v.b); }\n");
    assert_eq!((o.as_str(), c), ("2\n", 0));
}

// ---- rejection tests

#[test]
fn into_without_a_matching_from_is_rejected() {
    rejected(&format!("{}fn main() {{ let m: Meters = Other {{ v: 1 }}.into(); }}\n", UNITS), "no `impl From<Other> for Meters`");
}

#[test]
fn into_without_a_known_target_is_rejected() {
    rejected(&format!("{}fn main() {{ let m = Feet {{ v: 1 }}.into(); }}\n", UNITS), "cannot infer the target type of `.into()`");
}

#[test]
fn into_bound_not_satisfied_is_rejected() {
    rejected(&format!("{}fn toMeters<T: Into<Meters>>(x: T) -> Meters {{ return x.into(); }}\nfn main() {{ let m = toMeters(Other {{ v: 1 }}); }}\n", UNITS), "does not satisfy bound `Into<Meters>`");
}

#[test]
fn ambiguous_generic_trait_call_is_rejected() {
    rejected(&format!("{}fn main() {{ let x: i32 = 3; let y = x.convert(); }}\n", CONVERT), "ambiguous call to `convert`");
}

#[test]
fn no_impl_matches_the_arguments_is_rejected() {
    let src = "trait Mapper<A> { fn apply(borrow self, x: A) -> A; }\nstruct D { k: i32 }\nimpl Mapper<i32> for D { fn apply(borrow self, x: i32) -> i32 { return x; } }\nfn main() { let d = D { k: 1 }; let r = d.apply(true); }\n";
    rejected(src, "no implementation of `Mapper`");
}

#[test]
fn parameterised_bound_must_match_the_impl_argument() {
    let src = "trait Source<T> { fn get(borrow self) -> T; }\nstruct B1 { v: i32 }\nimpl Source<f64> for B1 { fn get(borrow self) -> f64 { return 1.5; } }\nfn pull<S: Source<i32>>(s: borrow S) -> i32 { return s.get(); }\nfn main() { let b = B1 { v: 8 }; println(pull(borrow b)); }\n";
    rejected(src, "does not satisfy bound `Source<i32>`");
}

#[test]
fn duplicate_impl_of_the_same_generic_trait_arguments_is_rejected() {
    let src = "trait Convert<T> { fn convert(borrow self) -> T; }\nimpl Convert<f64> for i32 { fn convert(borrow self) -> f64 { return 1.5; } }\nimpl Convert<f64> for i32 { fn convert(borrow self) -> f64 { return 2.5; } }\nfn main() { let x: i32 = 3; let f: f64 = x.convert(); }\n";
    rejected(src, "conflicting implementations of `Convert<f64>` for `i32`");
}
