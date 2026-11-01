//! Phase 11b M6 (Document 23 §17.9): `unsafe`, raw pointers, `alloc` / `dealloc`.
use std::process::Command;

/// Unique per call: tests run in parallel threads of one process, so uniqueness must never come from the source text.
static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn tmp(src: &str, tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mtn_rawptr_{}_{}_{}", std::process::id(), tag, COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("p.mtn");
    std::fs::write(&f, src).unwrap();
    f
}

fn run(src: &str) -> (String, i32) {
    let f = tmp(src, "run");
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("run").arg(&f).output().unwrap();
    (String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr), out.status.code().unwrap_or(-1))
}

/// Non-zero exit, the needle in the message, and NO binary produced by `build`.
fn rejected(src: &str, needle: &str) {
    let (o, c) = run(src);
    assert!(c != 0 && o.contains(needle), "expected `{}`, got ({}): {}", needle, c, o);
    let f = tmp(src, "build");
    let exe = f.with_file_name("out_exe");
    let _ = std::fs::remove_file(&exe);
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(!out.status.success() && !exe.exists(), "build must fail without a binary: {:?}", out);
}

fn ok(src: &str, want: &str) {
    let (o, c) = run(src);
    assert_eq!((o.as_str(), c), (want, 0), "program output");
}

#[test]
fn example_program_runs() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/raw_pointers.mtn")).unwrap();
    ok(&src, "42\n43\n8\n99\n99\n99\n0\n");
}

#[test]
fn doc3_pointer_to_local() {
    ok("fn main() { let mut value: i32 = 10; let raw_ptr = &mut value as *mut i32; unsafe { *raw_ptr = 42; } println(value); }", "42\n");
}

#[test]
fn compound_assignment_and_precedence() {
    ok("fn main() { let mut v: i32 = 5; let p = &mut v as *mut i32; unsafe { *p += 2; println(*p + 1); println(*p * 2); } }", "8\n14\n");
}

#[test]
fn doc13_alloc_write_read_dealloc() {
    ok("fn main() { unsafe { let raw = alloc(4, 4) as *mut i32; *raw = 7; println(*raw); dealloc(raw as *mut u8, 4, 4); } }", "7\n");
}

#[test]
fn alloc_over_aligned() {
    ok("fn main() { unsafe { let b = alloc(100, 64); println((b as usize) % 64); dealloc(b, 100, 64); } }", "0\n");
}

#[test]
fn usize_round_trip() {
    ok("fn main() { let mut n: i32 = 5; let p = &mut n as *mut i32; let a = p as usize; let q = a as *mut i32; unsafe { *q = 99; } println(n); }", "99\n");
}

#[test]
fn mut_to_const_is_implicit() {
    ok("fn rd(p: *const i32) -> i32 { return unsafe { *p }; } fn main() { let mut n: i32 = 3; let p = &mut n as *mut i32; println(rd(p)); let c: *const i32 = p; println(unsafe { *c }); }", "3\n3\n");
}

#[test]
fn shared_ref_to_const_pointer() {
    ok("fn main() { let n: i32 = 4; let c = &n as *const i32; println(unsafe { *c }); }", "4\n");
}

#[test]
fn pointer_to_pointer_cast_and_struct_pointee() {
    ok("struct P { x: i32, y: i32 } fn main() { let mut p = P { x: 1, y: 2 }; let q = &mut p as *mut P; unsafe { (*q).y = 9; println((*q).x + (*q).y); } let b = q as *mut u8; let _c = b as *const i32; }", "10\n");
}

#[test]
fn null_pointer_inside_unsafe() {
    ok("fn main() { let p: *mut i32 = unsafe { null }; println(p as usize); }", "0\n");
}

#[test]
fn multiplication_and_ampersand_still_parse() {
    ok("fn main() { let a = 3; let b = 4; println(a * b); println(a & b); println(a * -b); let t = a > 1 && b > 1; println(t); }", "12\n0\n-12\ntrue\n");
}

#[test]
fn rejects_deref_outside_unsafe() {
    rejected("fn main() { let mut v: i32 = 1; let p = &mut v as *mut i32; let x = *p; println(x); }", "only allowed inside an `unsafe` block");
}

#[test]
fn rejects_write_outside_unsafe() {
    rejected("fn main() { let mut v: i32 = 1; let p = &mut v as *mut i32; *p = 2; }", "only allowed inside an `unsafe` block");
}

#[test]
fn rejects_alloc_outside_unsafe() {
    rejected("fn main() { let p = alloc(4, 4); println(1); }", "`alloc` is only allowed inside an `unsafe` block");
}

#[test]
fn rejects_dealloc_outside_unsafe() {
    rejected("fn main() { let p = unsafe { alloc(4, 4) }; dealloc(p, 4, 4); }", "`dealloc` is only allowed inside an `unsafe` block");
}

#[test]
fn rejects_deref_of_non_pointer() {
    rejected("fn main() { let v: i32 = 1; let x = unsafe { *v }; println(x); }", "cannot dereference a value of type `i32`");
}

#[test]
fn rejects_assignment_through_const_pointer() {
    rejected("fn main() { let v: i32 = 1; let p = &v as *const i32; unsafe { *p = 3; } }", "cannot assign through a `*const T` pointer");
}

#[test]
fn rejects_int_to_pointer_cast() {
    rejected("fn main() { let v: i32 = 1; let p = v as *mut i32; println(1); }", "invalid cast from `i32` to `*mut i32`");
}

#[test]
fn rejects_pointer_to_i32_cast() {
    rejected("fn main() { let mut v: i32 = 1; let p = &mut v as *mut i32; let x = p as i32; println(x); }", "invalid cast from `*mut i32` to `i32`");
}

#[test]
fn rejects_shared_ref_to_mut_pointer() {
    rejected("fn main() { let v: i32 = 1; let p = &v as *mut i32; println(1); }", "a shared `&T` cannot become `*mut T`");
}

#[test]
fn rejects_const_to_mut_implicit() {
    rejected("fn f(p: *mut i32) {} fn main() { let v: i32 = 1; let c = &v as *const i32; f(c); }", "expected `*mut i32`, found `*const i32`");
}

#[test]
fn rejects_mut_borrow_of_non_mut_binding() {
    rejected("fn main() { let v: i32 = 1; let p = &mut v as *mut i32; println(1); }", "cannot borrow `v` as mutable: it is not declared `mut`");
}

#[test]
fn rejects_null_pointer_outside_unsafe() {
    rejected("fn main() { let p: *mut i32 = null; println(1); }", "only allowed inside an `unsafe` block");
}

#[test]
fn pointer_arithmetic_not_yet_supported() {
    rejected("fn main() { let mut v: i32 = 1; let p = &mut v as *mut i32; let q = p + 1; println(1); }", "pointer arithmetic is not yet supported -- decided with the library that needs it");
}

#[test]
fn pointer_methods_and_indexing_not_yet_supported() {
    rejected("fn main() { let mut v: i32 = 1; let p = &mut v as *mut i32; let q = p.offset(1); println(1); }", "not yet supported -- decided with the library that needs");
    rejected("fn main() { let mut v: i32 = 1; let p = &mut v as *mut i32; let x = unsafe { p[0] }; println(x); }", "indexing a raw pointer");
}

#[test]
fn one_allocation_interface_in_ir() {
    let text = mtnc::driver::emit_ir("fn main() { unsafe { let b = alloc(8, 8); dealloc(b, 8, 8); } }", &mtnc::codegen::Options { release: false }).unwrap_or_else(|e| panic!("codegen failed: {:?}", e));
    // user code calls only the internal interface; the system allocator is reached only through it
    assert!(text.contains("@__mtn_alloc") && text.contains("@__mtn_dealloc"), "{}", text);
    let main_body = text.split("define").find(|d| d.contains("mtn_main")).unwrap_or("");
    assert!(!main_body.contains("call ptr @malloc") && !main_body.contains("call void @free"), "{}", main_body);
}
