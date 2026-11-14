//! Phase 11c C2: growable `[T]` and heap `String` handles (Doc 13 §1.3).
use std::process::Command;

/// Unique per call: tests run in parallel threads of one process, so uniqueness must never come from the source text.
static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn tmp(src: &str, tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mtn_c2_{}_{}_{}", std::process::id(), tag, COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("p.mtn");
    std::fs::write(&f, src).unwrap();
    f
}

fn run_args(src: &str, args: &[&str]) -> (String, i32) {
    let f = tmp(src, "run");
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("run").arg(&f).args(args).output().unwrap();
    (String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr), out.status.code().unwrap_or(-1))
}

fn run(src: &str) -> (String, i32) {
    run_args(src, &[])
}

fn ok(src: &str, want: &str) {
    let (o, c) = run(src);
    assert_eq!((o.as_str(), c), (want, 0), "program output");
}

#[test]
fn growable_literal_default_is_i32_and_indexable() {
    ok("fn main() { let v = [1, 2, 3]; println(v[0]); println(v[2]); }", "1\n3\n");
}

#[test]
fn growable_index_write_and_compound_assignment() {
    ok("fn main() { let mut w: [i32] = [10, 20, 30]; w[2] = 99; w[0] += 1; println(w[0] + w[2]); }", "110\n");
}

#[test]
fn growable_lends_a_slice_to_a_slice_parameter() {
    ok("fn sum(a: &[i32]) -> i32 { return a[0] + a[1] + a[2]; } fn main() { let w = [1, 2, 3]; println(sum(borrow w)); }", "6\n");
}

#[test]
fn growable_sub_slice() {
    ok("fn main() { let w = [5, 6, 7, 8]; let s = borrow w[1..3]; println(s[0]); println(s[1]); }", "6\n7\n");
}

#[test]
fn growable_through_a_borrow_parameter() {
    ok("fn first(borrow v: [i32]) -> i32 { return v[0]; } fn main() { let w = [42, 1]; println(first(borrow w)); }", "42\n");
}

#[test]
fn growable_inside_a_struct_and_empty_literal() {
    ok("struct U { tags: [i32] } fn main() { let u = U { tags: [7, 8] }; println(u.tags[1]); let e: [i32] = []; }", "8\n");
}

#[test]
fn growable_of_strings() {
    ok("fn main() { let names = [\"a\", \"bb\"]; println(names[1]); }", "bb\n");
}

#[test]
fn growable_of_structs() {
    ok("struct P { x: i32, y: i32 } fn main() { let ps = [P { x: 1, y: 2 }, P { x: 3, y: 4 }]; println(ps[1].y); }", "4\n");
}

#[test]
fn string_literal_is_a_string_in_a_struct_field() {
    ok("struct User { name: String, email: String } fn main() { let u = User { name: \"Alice\", email: \"a@x.io\" }; println(u.name); println(u.email); }", "Alice\na@x.io\n");
}

#[test]
fn string_literal_still_works_as_a_str_argument() {
    ok("fn shout(s: &str) { println(s); } fn main() { shout(\"hi\"); let t = \"there\"; shout(t); }", "hi\nthere\n");
}

#[test]
fn growable_index_out_of_bounds_exits_101() {
    let (o, c) = run("fn main() { let w = [1, 2, 3]; let i = 5; println(w[i]); }");
    assert_eq!(c, 101, "{}", o);
    assert!(o.contains("index out of bounds: the length is 3 but the index is 5"), "{}", o);
}

#[test]
fn growable_negative_index_exits_101() {
    let (o, c) = run("fn main() { let w = [1, 2, 3]; let i: i32 = 0 - 1; println(w[i]); }");
    assert_eq!(c, 101, "{}", o);
}

#[test]
fn empty_growable_index_exits_101() {
    let (o, c) = run("fn main() { let e: [i32] = []; println(e[0]); }");
    assert_eq!(c, 101, "{}", o);
}

#[test]
fn growable_slice_range_out_of_bounds_exits_101() {
    let (o, c) = run("fn main() { let w = [1, 2, 3]; let s = borrow w[1..9]; println(s[0]); }");
    assert_eq!(c, 101, "{}", o);
    assert!(o.contains("slice range out of bounds"), "{}", o);
}

#[test]
fn for_over_a_growable_array_is_still_phase_16() {
    let (o, c) = run("fn main() { let w = [1, 2]; for x in w { println(x); } }");
    assert!(c != 0 && o.contains("Phase 16"), "{}", o);
}

#[test]
fn growable_handle_layout_is_ptr_len_cap_in_the_ir() {
    let f = tmp("fn main() { let w = [1, 2, 3]; println(w[0]); }", "ir");
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("--emit-ir").output().unwrap();
    let ir = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    let ir = if ir.contains("define") { ir } else { std::fs::read_to_string(f.with_extension("ll")).unwrap_or(ir) };
    assert!(ir.contains("{ ptr, i64, i64 }"), "{}", ir);
    assert!(ir.contains("@__mtn_alloc"), "{}", ir);
}
