//! Phase 11b: later-use inference for generic type parameters (Doc 5 rules 3/4/6).
use std::process::Command;

fn run(src: &str) -> (String, i32) {
    let dir = std::env::temp_dir().join(format!("mtn_infer_{}_{}", std::process::id(), src.len()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("p.mtn");
    std::fs::write(&f, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("run").arg(&f).output().unwrap();
    (String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr), out.status.code().unwrap_or(-1))
}

const HOLDER: &str = r####"

struct Holder<T> { slot: Option<T> }

impl<T> Holder<T> {
    fn new() -> Holder<T> { return Holder { slot: None }; }
    fn put(borrow mut self, v: T) { self.slot = Some(v); }
    fn has(borrow self) -> bool {
        match self.slot {
            Some(_) => true,
            None => false,
        }
    }
}

"####;

fn rejected(body: &str, needle: &str) {
    let (o, c) = run(&format!("{}{}", HOLDER, body));
    assert!(c != 0 && o.contains(needle), "expected `{}`, got ({}): {}", needle, c, o);
}

#[test]
fn example_program_runs() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/infer_later_use.mtn")).unwrap();
    let (o, c) = run(&src);
    assert_eq!((o.as_str(), c), ("false\ntrue\nfalse\nfalse\ntrue\n", 0));
}

#[test]
fn fixed_by_later_method_argument() {
    let (o, c) = run(&format!("{}fn main() {{ let mut h = Holder::new(); h.put(2.5); println(h.has()); }}\n", HOLDER));
    assert_eq!((o.as_str(), c), ("true\n", 0));
}

#[test]
fn never_fixed_is_rejected() {
    rejected("fn main() { let h = Holder::new(); }\n", "cannot infer type parameter `T`");
}

#[test]
fn conflicting_later_uses_rejected() {
    rejected("fn main() { let mut h = Holder::new(); h.put(1); h.put(2.5); }\n", "expected `i32`, found `f64`");
}

#[test]
fn generic_fn_result_only_param_never_fixed_rejected() {
    rejected("fn mk<T>() -> Holder<T> { return Holder::new(); }\nfn main() { let h = mk(); }\n", "cannot infer type parameter `T` of generic function `mk`");
}

const P: &str = "trait P { fn p(borrow self) -> i32; }\nimpl P for i32 { fn p(borrow self) -> i32 { return 1; } }\nfn mk<T>() -> Holder<T> where T: P { return Holder::new(); }\n";

#[test]
fn deferred_bound_satisfied_accepted() {
    let (o, c) = run(&format!("{}{}fn main() {{ let mut h = mk(); h.put(7); println(h.has()); }}\n", HOLDER, P));
    assert_eq!((o.as_str(), c), ("true\n", 0));
}

#[test]
fn deferred_bound_unsatisfied_rejected() {
    rejected(&format!("{}fn main() {{ let mut h = mk(); h.put(2.5); }}\n", P), "does not satisfy bound `P`");
}

#[test]
fn generic_method_with_own_params_in_generic_impl_is_clear_error() {
    let src = "struct Box2<T> { v: T }\nimpl<T> Box2<T> {\n    fn pairWith<U>(self, u: U) -> U { return u; }\n}\nfn main() { let b = Box2 { v: 1 }; println(b.pairWith(2.5)); }\n";
    let (o, c) = run(src);
    assert!(c != 0 && o.contains("not yet supported by codegen — Phase 11"), "{}", o);
}
