//! Phase 13 M0: `Drop` (Document 6 §9.9), pointer methods and `sizeOf`/`alignOf` (Document 23 §17.13),
//! `dyn` extensions (`dyn T<A,B>`, `Rc/Arc/Weak<dyn T>`, `dyn T + Send + Sync`).
//! Every program is built in its own unique directory (AtomicUsize + pid), never shared between tests.
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mtn_p13m0_{}_{}_{}", std::process::id(), tag, COUNTER.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn have_valgrind() -> bool {
    Command::new("valgrind").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Builds `src` (a file) into a unique directory; Ok(exe) or Err(message of the failed build).
fn build(src: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let exe = unique_dir("exe").join("out_exe");
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(src).arg("-o").arg(&exe).output().unwrap();
    if out.status.success() && exe.exists() {
        Ok(exe)
    } else {
        Err(String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr))
    }
}

fn write_src(src: &str) -> std::path::PathBuf {
    let f = unique_dir("src").join("p.mtn");
    std::fs::write(&f, src).unwrap();
    f
}

fn run_exe(exe: &std::path::Path, workers: &str, valgrind: bool) -> (String, i32) {
    let out = if valgrind {
        Command::new("valgrind").args(["--leak-check=full", "--errors-for-leak-kinds=definite", "--error-exitcode=99", "-q"]).arg(exe).env("MTN_WORKERS", workers).output().unwrap()
    } else {
        Command::new(exe).env("MTN_WORKERS", workers).output().unwrap()
    };
    (String::from_utf8_lossy(&out.stdout).to_string(), out.status.code().unwrap_or(-1))
}

fn run_src(src: &str) -> String {
    let exe = build(&write_src(src)).unwrap_or_else(|e| panic!("build failed: {}\n{}", e, src));
    let (o, c) = run_exe(&exe, "2", false);
    assert_eq!(c, 0, "exit code\n{}", o);
    o
}

fn rejected(src: &str, needle: &str) {
    match build(&write_src(src)) {
        Ok(_) => panic!("should have been rejected (`{}`):\n{}", needle, src),
        Err(m) => assert!(m.contains(needle), "expected `{}` in:\n{}", needle, m),
    }
}

fn golden(name: &str) {
    let src = root().join("examples/memory").join(format!("{}.mtn", name));
    let want = std::fs::read_to_string(root().join("tests/golden13").join(format!("{}.out", name))).unwrap();
    let exe = build(&src).unwrap_or_else(|e| panic!("build of {} failed: {}", name, e));
    for w in ["1", "4"] {
        let (o, c) = run_exe(&exe, w, false);
        assert_eq!((o.as_str(), c), (want.as_str(), 0), "{} at MTN_WORKERS={}", name, w);
        if have_valgrind() {
            let (o, c) = run_exe(&exe, w, true);
            assert_eq!((o.as_str(), c), (want.as_str(), 0), "{} under valgrind at MTN_WORKERS={}", name, w);
        }
    }
}

// ---------------------------------------------------------------- Drop

#[test]
fn drop_order_nested_early_exit_explicit_reassign_loop_enum_box_rc() {
    golden("drop_trait");
}
#[test]
fn drop_generic_dyn_question_mark_closures() {
    golden("drop_containers");
}
#[test]
fn drop_generators_futures_arc_spawn() {
    golden("drop_tasks");
}

const LOUD: &str = "struct Loud { id: i32 }\nimpl Drop for Loud { fn drop(borrow mut self) { print(\"drop \"); println(self.id); } }\n";

#[test]
fn drop_zero_valued_struct_is_still_dropped() {
    assert_eq!(run_src(&format!("{}fn main() {{ let g = Loud {{ id: 0 }}; println(\"x\"); }}", LOUD)), "x\ndrop 0\n");
}
#[test]
fn drop_moved_value_dropped_once_by_the_new_owner() {
    assert_eq!(run_src(&format!("{}fn eat(l: Loud) {{ println(\"eat\"); }}\nfn main() {{ let a = Loud {{ id: 1 }}; eat(a); println(\"after\"); }}", LOUD)), "eat\ndrop 1\nafter\n");
}
#[test]
fn drop_explicit_call_is_rejected() {
    rejected(&format!("{}fn main() {{ let a = Loud {{ id: 1 }}; a.drop(); }}", LOUD), "explicit destructor calls are not allowed");
    rejected(&format!("{}fn main() {{ let mut a = Loud {{ id: 1 }}; Drop::drop(borrow mut a); }}", LOUD), "explicit destructor calls are not allowed");
}
#[test]
fn drop_user_trait_named_drop_is_rejected() {
    rejected("trait Drop { fn x(borrow self); }\nfn main() {}", "reserved");
}
#[test]
fn drop_on_foreign_type_is_rejected() {
    rejected("impl Drop for i32 { fn drop(borrow mut self) {} }\nfn main() {}", "declared in the current package");
}
#[test]
fn drop_specialised_or_bounded_impl_is_rejected() {
    rejected("struct W<T> { v: T }\nimpl Drop for W<i32> { fn drop(borrow mut self) {} }\nfn main() {}", "specialised");
    rejected("trait Cl { fn c(borrow self); }\nstruct W<T> { v: T }\nimpl<T: Cl> Drop for W<T> { fn drop(borrow mut self) {} }\nfn main() {}", "extra bounds");
}
#[test]
fn drop_copy_type_is_rejected() {
    rejected("@copy struct P { x: i32 }\nimpl Drop for P { fn drop(borrow mut self) {} }\nfn main() {}", "@copy");
}
#[test]
fn drop_wrong_receiver_or_duplicate_is_rejected() {
    rejected("struct P { x: i32 }\nimpl Drop for P { fn drop(self) {} }\nfn main() {}", "borrow mut self");
    rejected("struct P { x: i32 }\nimpl Drop for P { fn drop(borrow mut self) {} }\nimpl Drop for P { fn drop(borrow mut self) {} }\nfn main() {}", "");
}
#[test]
fn drop_moving_a_field_out_of_a_drop_type_is_rejected() {
    rejected("struct S { name: String }\nimpl Drop for S { fn drop(borrow mut self) {} }\nfn main() { let s = S { name: String::from(\"a\") }; let n = s.name; println(n); }", "cannot be taken apart");
}
#[test]
fn drop_use_after_move_is_rejected() {
    rejected(&format!("{}fn eat(l: Loud) {{}}\nfn main() {{ let a = Loud {{ id: 1 }}; eat(a); eat(a); }}", LOUD), "");
}

// ---------------------------------------------------------------- pointers

#[test]
fn pointer_methods_sizeof_alignof_copy_dropinplace() {
    golden("pointer_methods");
}
#[test]
fn pointer_misuse_is_rejected() {
    for (f, needle) in [
        ("q1", "only allowed inside an `unsafe` block"),
        ("q2", "only allowed inside an `unsafe` block"),
        ("q3", "cannot be written through"),
        ("q4", "only allowed inside an `unsafe` block"),
        ("q5", "no method named `frobnicate` on a raw pointer"),
        ("q6", "needs its type argument"),
    ] {
        let m = build(&root().join("tests/golden13/rej").join(format!("{}.mtn", f))).err().unwrap_or_else(|| panic!("{} accepted", f));
        assert!(m.contains(needle), "{}: {}", f, m);
    }
}

// ---------------------------------------------------------------- dyn

#[test]
fn dyn_generic_trait_objects() {
    golden("dyn_generic_trait");
}
#[test]
fn dyn_send_box_crosses_a_thread() {
    golden("dyn_send_box");
}
#[test]
fn dyn_rc_clone_weak_counts_drop_and_overaligned_value() {
    golden("dyn_rc");
}
#[test]
fn dyn_arc_send_sync_across_threads_and_weak_arc() {
    golden("dyn_arc_send_sync");
}
#[test]
fn dyn_auto_trait_and_coercion_rules_are_rejected() {
    for (f, needle) in [
        ("r1", "cannot be sent between threads"),
        ("r2", "is not `Send`"),
        ("r3", "does not promise `Send`"),
        ("r5", "listed twice"),
        ("r6", "can only add the auto traits"),
        ("r7", "does not implement `Source<bool>`"),
        ("s1", "cannot be sent between threads"),
        ("s2", "`+ Sync`"),
        ("s3", "through an `Rc`/`Arc`"),
        ("s4", "expected `Arc<dyn Shape>`"),
    ] {
        let m = build(&root().join("tests/golden13/rej").join(format!("{}.mtn", f))).err().unwrap_or_else(|| panic!("{} accepted", f));
        assert!(m.contains(needle), "{}: {}", f, m);
    }
}
#[test]
fn dyn_dropping_auto_bits_is_allowed() {
    let src = "trait Shape { fn area(borrow self) -> i32; }\nstruct Sq { s: i32 }\nimpl Shape for Sq { fn area(borrow self) -> i32 { return self.s; } }\nfn main() { let b: Box<dyn Shape + Send> = Box::new(Sq { s: 3 }); let c: Box<dyn Shape> = b; println(c.area()); let r: Rc<dyn Shape + Send> = Rc::new(Sq { s: 4 }); let q: Rc<dyn Shape> = r; println(q.area()); }";
    assert_eq!(run_src(src), "3\n4\n");
}
#[test]
fn dyn_rc_cannot_be_dereferenced_into_a_value() {
    rejected("trait Shape { fn area(borrow self) -> i32; }\nstruct Sq { s: i32 }\nimpl Shape for Sq { fn area(borrow self) -> i32 { return self.s; } }\nfn t(x: borrow dyn Shape) -> i32 { return x.area(); }\nfn main() { let r: Rc<dyn Shape> = Rc::new(Sq { s: 1 }); println(t(borrow *r)); }", "unsized trait object");
}

// ---------------------------------------------------------------- Drop: layout, and `check` parity

#[test]
fn drop_layout_sizeof_alignof_include_the_hidden_byte_pack_and_align_work() {
    golden("drop_layout");
}

/// Runs `mtnc check` on a file: (exit code, combined output).
fn check_file(src: &std::path::Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("check").arg(src).output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr))
}

#[test]
fn check_reports_field_move_out_of_a_drop_type() {
    for f in ["w3", "w6"] {
        let (code, m) = check_file(&root().join("tests/golden13/rej").join(format!("{}.mtn", f)));
        assert_ne!(code, 0, "{}: {}", f, m);
        assert!(m.contains("cannot be taken apart") && m.contains("Document 6 §9.9, rule 5"), "{}: {}", f, m);
    }
}
#[test]
fn check_reports_pattern_move_out_of_a_drop_type() {
    let (code, m) = check_file(&root().join("tests/golden13/rej/w5.mtn"));
    assert_ne!(code, 0, "{}", m);
    assert!(m.contains("with a pattern") && m.contains("cannot be taken apart"), "{}", m);
}
#[test]
fn check_and_build_agree_on_drop_field_rules() {
    // borrowing a field, reading a Copy field, matching through `borrow`, and `_` patterns are fine
    let ok = root().join("tests/golden13/drop_field_ok.mtn");
    let (code, m) = check_file(&ok);
    assert_eq!(code, 0, "{}", m);
    let exe = build(&ok).unwrap();
    assert_eq!(run_exe(&exe, "1", false).0, "1\nshow\nref\na\ne\nd\n");
    for f in ["w3", "w5", "w6"] {
        let p = root().join("tests/golden13/rej").join(format!("{}.mtn", f));
        assert!(build(&p).is_err(), "{} must not build", f);
    }
}
