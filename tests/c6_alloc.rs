//! Phase 11c C6: `Allocator`, `Box<T, A>`, `@pack` / `@align(N)` (Doc 13 §3.2, §4).
use mtnc::codegen::Options;
use mtnc::driver;
use std::process::Command;

/// Unique per call: tests run in parallel threads of one process.
static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn tmp(src: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mtn_c6_{}_{}", std::process::id(), COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("p.mtn");
    std::fs::write(&f, src).unwrap();
    f
}

fn have_valgrind() -> bool {
    Command::new("valgrind").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Builds `file`, runs it; when valgrind exists also asserts: no leak, no invalid access.
fn run_file(file: &std::path::Path) -> (String, i32) {
    let exe = file.with_extension("bin");
    let b = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(file).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let out = Command::new(&exe).output().unwrap();
    if have_valgrind() {
        let v = Command::new("valgrind")
            .args(["--leak-check=full", "--errors-for-leak-kinds=definite", "--error-exitcode=99"])
            .arg(&exe)
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&v.stderr).to_string();
        assert_ne!(v.status.code(), Some(99), "valgrind found a leak or memory error:\n{}", err);
        assert!(!err.contains("Invalid read") && !err.contains("Invalid write") && !err.contains("Invalid free"), "valgrind: {}", err);
    }
    (String::from_utf8_lossy(&out.stdout).to_string(), out.status.code().unwrap_or(-1))
}

fn run_no_valgrind(src: &str) -> (String, i32) {
    let f = tmp(src);
    let exe = f.with_extension("bin");
    let b = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let out = Command::new(&exe).output().unwrap();
    (String::from_utf8_lossy(&out.stdout).to_string(), out.status.code().unwrap_or(-1))
}

fn run(src: &str) -> (String, i32) {
    run_file(&tmp(src))
}

#[allow(dead_code)]
fn ok(src: &str, want: &str) {
    let (o, c) = run(src);
    assert_eq!((o.as_str(), c), (want, 0));
}

fn rejected(src: &str) -> String {
    let f = tmp(src);
    let out = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("check").arg(&f).output().unwrap();
    assert!(!out.status.success(), "should have been rejected:\n{}", src);
    String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr)
}

fn ir(src: &str) -> String {
    driver::emit_ir(src, &Options { release: false }).unwrap_or_else(|e| panic!("codegen failed: {:?}", e))
}

fn example(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").join("memory").join(name)
}


const ARENA: &str = "struct Arena { base: *mut u8, cap: usize, offset: usize, live: usize }\n\
impl Arena {\n\
    fn new(cap: usize) -> Arena { unsafe { let p: *mut u8 = alloc(cap, 16); return Arena { base: p, cap: cap, offset: 0, live: 0 }; } }\n\
    fn release(borrow mut self) { unsafe { dealloc(self.base, self.cap, 16); } }\n\
}\n\
impl Allocator for Arena {\n\
    fn alloc(borrow mut self, size: usize, align: usize) -> *mut u8 {\n\
        unsafe {\n\
            let start = (self.offset + align - 1) / align * align;\n\
            if start + size > self.cap { let zero: usize = 0; return zero as *mut u8; }\n\
            self.offset = start + size;\n\
            self.live = self.live + 1;\n\
            return (self.base as usize + start) as *mut u8;\n\
        }\n\
    }\n\
    fn dealloc(borrow mut self, ptr: *mut u8, size: usize) { self.live = self.live - 1; }\n\
}\n";

fn with_arena(body: &str) -> String {
    format!("{}{}", ARENA, body)
}

// ------------------------------------------------------------------ Allocator / Box<T, A>

#[test]
fn memory_example_arena_alloc() {
    let (o, c) = run_file(&example("arena_alloc.mtn"));
    assert_eq!((o.as_str(), c), ("3\nin the arena\n42\n105\n7\n4\n0\n", 0));
}

#[test]
fn box_new_in_places_the_value_in_the_arena_and_reads_back() {
    let src = with_arena("struct Big { a: i64, b: i64 }\nfn main() { let mut ar = Arena::new(1024); let b: Box<Big, Arena> = Box::newIn(Big { a: 3, b: 4 }, borrow ar); println(b.a * b.b); println(ar.offset); drop(b); ar.release(); }");
    let (o, c) = run(&src);
    assert_eq!((o.as_str(), c), ("12\n16\n", 0));
}

#[test]
fn box_in_an_arena_frees_through_its_own_allocator() {
    // live goes 0 -> 2 -> 0: dropping a `Box<T, Arena>` calls `Arena::dealloc`, not the default `free`
    let src = with_arena("fn main() { let mut ar = Arena::new(1024); { let a = Box::newIn(1, borrow ar); let b = Box::newIn(String::from(\"s\"), borrow ar); println(ar.live); } println(ar.live); ar.release(); }");
    let (o, c) = run(&src);
    assert_eq!((o.as_str(), c), ("2\n0\n", 0));
}

#[test]
fn two_arenas_each_box_returns_to_the_arena_it_came_from() {
    let src = with_arena("fn main() { let mut x = Arena::new(256); let mut y = Arena::new(256); { let a = Box::newIn(1, borrow x); let b = Box::newIn(2, borrow x); let c = Box::newIn(3, borrow y); println(x.live * 10 + y.live); drop(a); println(x.live * 10 + y.live); } println(x.live * 10 + y.live); x.release(); y.release(); }");
    let (o, c) = run(&src);
    assert_eq!((o.as_str(), c), ("21\n11\n0\n", 0));
}

#[test]
fn box_dyn_in_an_arena_drops_the_object_then_frees_through_the_arena() {
    let src = with_arena("trait T { fn v(borrow self) -> i32; }\nstruct A { s: String }\nstruct B { n: i32 }\n\
        impl T for A { fn v(borrow self) -> i32 { return 1; } }\nimpl T for B { fn v(borrow self) -> i32 { return self.n; } }\n\
        fn main() { let mut ar = Arena::new(1024); { let a: Box<dyn T, Arena> = Box::newIn(A { s: String::from(\"x\") }, borrow ar); let b: Box<dyn T, Arena> = Box::newIn(B { n: 9 }, borrow ar); println(a.v() + b.v()); println(ar.live); } println(ar.live); ar.release(); }");
    let (o, c) = run(&src);
    assert_eq!((o.as_str(), c), ("10\n2\n0\n", 0));
}

#[test]
fn moving_a_box_in_an_arena_does_not_free_twice() {
    let src = with_arena("fn take(b: Box<i32, Arena>) -> i32 { return *b; }\nfn main() { let mut ar = Arena::new(256); let b = Box::newIn(5, borrow ar); println(take(b)); println(ar.live); ar.release(); }");
    let (o, c) = run(&src);
    assert_eq!((o.as_str(), c), ("5\n0\n", 0));
}

#[test]
fn arena_that_is_out_of_memory_aborts_with_the_panic_exit_code() {
    let src = with_arena("fn main() { let mut ar = Arena::new(4); let a = Box::newIn(1, borrow ar); let b = Box::newIn(2, borrow ar); println(1); }");
    let (_, c) = run_no_valgrind(&src);
    assert_eq!(c, 101);
}

#[test]
fn box_new_in_goes_through_the_allocator_trait_in_the_ir() {
    let text = ir(&with_arena("fn main() { let mut ar = Arena::new(64); let b = Box::newIn(1, borrow ar); ar.release(); }"));
    assert!(text.contains("call ptr @\"mtn_Arena::Allocator::alloc\""), "{}", text);
    assert!(text.contains("call void @\"mtn_Arena::Allocator::dealloc\""), "{}", text);
}

#[test]
fn box_in_an_arena_does_not_call_the_default_free_path() {
    let text = ir(&with_arena("fn main() { let mut ar = Arena::new(64); let b = Box::newIn(1, borrow ar); }"));
    let glue = text.split("define void @__mtn_drop_Box_i32__Arena_").nth(1).expect("glue for Box<i32, Arena>");
    let glue = glue.split("\n}\n").next().unwrap();
    assert!(glue.contains("@\"mtn_Arena::Allocator::dealloc\""), "{}", glue);
    assert!(!glue.contains("@__mtn_dealloc"), "{}", glue);
}

#[test]
fn plain_box_still_uses_the_default_interface() {
    let text = ir("fn main() { let b = Box::new(1); }");
    let glue = text.split("define void @__mtn_drop_Box_i32").nth(1).expect("glue for Box<i32>");
    let glue = glue.split("\n}\n").next().unwrap();
    assert!(glue.contains("@__mtn_dealloc"), "{}", glue);
}

#[test]
fn type_without_allocator_impl_is_rejected_by_new_in() {
    let m = rejected("struct NotAnAlloc { x: i32 }\nfn main() { let a = NotAnAlloc { x: 1 }; let b = Box::newIn(1, borrow a); }");
    assert!(m.contains("does not implement `Allocator`"), "{}", m);
}

#[test]
fn new_in_needs_a_borrowed_allocator() {
    let m = rejected(&with_arena("fn main() { let ar = Arena::new(8); let b = Box::newIn(1, ar); }"));
    assert!(m.contains("borrow <allocator>"), "{}", m);
}

#[test]
fn box_in_different_allocator_types_do_not_mix() {
    let src = with_arena("struct Other { x: i32 }\nimpl Allocator for Other { fn alloc(borrow mut self, size: usize, align: usize) -> *mut u8 { unsafe { let p: *mut u8 = alloc(size, align); return p; } } fn dealloc(borrow mut self, ptr: *mut u8, size: usize) { unsafe { dealloc(ptr, size, 1); } } }\n\
        fn main() { let mut ar = Arena::new(64); let o = Other { x: 0 }; let b: Box<i32, Other> = Box::newIn(1, borrow ar); }");
    rejected(&src);
}

#[test]
fn allocator_impl_with_a_wrong_signature_is_rejected() {
    let m = rejected("struct A { x: i32 }\nimpl Allocator for A { fn alloc(borrow mut self, size: usize) -> *mut u8 { unsafe { let p: *mut u8 = alloc(size, 8); return p; } } fn dealloc(borrow mut self, ptr: *mut u8, size: usize) { } }\nfn main() { }");
    assert!(!m.is_empty());
}

#[test]
fn allocator_impl_missing_a_method_is_rejected() {
    rejected("struct A { x: i32 }\nimpl Allocator for A { fn dealloc(borrow mut self, ptr: *mut u8, size: usize) { } }\nfn main() { }");
}

// ------------------------------------------------------------------ @pack / @align(N)

const LAYOUT: &str = "fn addr<T>(r: borrow T) -> usize { unsafe { return r as *const T as usize; } }\n";

#[test]
fn pack_removes_all_padding() {
    let src = format!("{}@pack\nstruct P {{ a: u8, b: u32 }}\nstruct N {{ a: u8, b: u32 }}\nfn main() {{\n\
        let p: [P; 2] = [P {{ a: 1, b: 2 }}, P {{ a: 3, b: 4 }}];\n\
        let n: [N; 2] = [N {{ a: 1, b: 2 }}, N {{ a: 3, b: 4 }}];\n\
        println(addr(borrow p[1]) - addr(borrow p[0]));\n\
        println(addr(borrow p[0].b) - addr(borrow p[0]));\n\
        println(addr(borrow n[1]) - addr(borrow n[0]));\n\
        println(addr(borrow n[0].b) - addr(borrow n[0]));\n\
        println(p[1].b + p[0].b);\n\
        println(p[1].a + p[0].a);\n}}", LAYOUT);
    let (o, c) = run(&src);
    assert_eq!((o.as_str(), c), ("5\n1\n8\n4\n6\n4\n", 0));
}

#[test]
fn packed_struct_fields_can_be_written_and_read_unaligned() {
    let src = "@pack\nstruct H { version: u8, length: u16, id: u32 }\nfn main() { let mut h = H { version: 1, length: 2, id: 3 }; h.length = 515; h.id = h.id + 1000; h.id += 1; println(h.length); println(h.id); println(h.version); }";
    let (o, c) = run(src);
    assert_eq!((o.as_str(), c), ("515\n1004\n1\n", 0));
}

#[test]
fn packed_network_header_has_the_exact_wire_size() {
    // Document 13 §4.2's NetworkPacketHeader: 1 + 1 + 2 = 4 bytes
    let src = format!("{}@pack\nstruct NetworkPacketHeader {{ version: u8, flags: u8, length: u16 }}\nfn main() {{ let h: [NetworkPacketHeader; 2] = [NetworkPacketHeader {{ version: 1, flags: 2, length: 3 }}, NetworkPacketHeader {{ version: 4, flags: 5, length: 6 }}]; println(addr(borrow h[1]) - addr(borrow h[0])); println(addr(borrow h[0].length) - addr(borrow h[0])); }}", LAYOUT);
    let (o, c) = run(&src);
    assert_eq!((o.as_str(), c), ("4\n2\n", 0));
}

#[test]
fn align_raises_size_and_alignment_to_the_requested_boundary() {
    let src = format!("{}@align(64)\nstruct CacheLinePadded {{ value: i64 }}\nfn main() {{ let p: [CacheLinePadded; 2] = [CacheLinePadded {{ value: 1 }}, CacheLinePadded {{ value: 2 }}]; println(addr(borrow p[1]) - addr(borrow p[0])); println(addr(borrow p[0]) % 64); println(addr(borrow p[1]) % 64); println(p[0].value + p[1].value); }}", LAYOUT);
    let (o, c) = run(&src);
    assert_eq!((o.as_str(), c), ("64\n0\n0\n3\n", 0));
}

#[test]
fn align_also_holds_for_a_heap_box() {
    let src = format!("{}@align(128)\nstruct Padded {{ value: i32 }}\nfn main() {{ let b = Box::new(Padded {{ value: 7 }}); println(addr(borrow *b) % 128); println(b.value); }}", LAYOUT);
    let (o, c) = run(&src);
    assert_eq!((o.as_str(), c), ("0\n7\n", 0));
}

#[test]
fn align_can_be_smaller_than_the_natural_alignment_without_changing_the_layout() {
    let src = format!("{}@align(1)\nstruct S {{ a: u32 }}\nfn main() {{ let s: [S; 2] = [S {{ a: 1 }}, S {{ a: 2 }}]; println(addr(borrow s[1]) - addr(borrow s[0])); }}", LAYOUT);
    let (o, c) = run(&src);
    assert_eq!((o.as_str(), c), ("4\n", 0));
}

#[test]
fn pack_and_align_appear_in_the_llvm_struct_types() {
    let text = ir("@pack\nstruct P { a: u8, b: u32 }\n@align(64)\nstruct C { v: i64 }\nfn main() { let p = P { a: 1, b: 2 }; let c = C { v: 3 }; println(p.b); println(c.v); }");
    assert!(text.contains("<{ i8, i32 }>"), "{}", text);
    assert!(text.contains("[0 x <64 x i8>]"), "{}", text);
}

#[test]
fn align_that_is_not_a_power_of_two_is_rejected() {
    let m = rejected("@align(3)\nstruct S { a: u8 }\nfn main() { }");
    assert!(m.contains("power of two"), "{}", m);
}

#[test]
fn align_without_an_argument_is_rejected() {
    rejected("@align\nstruct S { a: u8 }\nfn main() { }");
}

#[test]
fn pack_with_an_argument_is_rejected() {
    rejected("@pack(2)\nstruct S { a: u8 }\nfn main() { }");
}

#[test]
fn pack_and_align_together_are_rejected() {
    let m = rejected("@pack\n@align(8)\nstruct S { a: u8 }\nfn main() { }");
    assert!(m.contains("cannot be combined"), "{}", m);
}

#[test]
fn pack_on_an_enum_is_rejected() {
    let m = rejected("@pack\nenum E { A, B }\nfn main() { }");
    assert!(m.contains("structs only"), "{}", m);
}

#[test]
fn allocator_impl_with_a_by_value_self_is_rejected() {
    let m = rejected("struct A { x: i32 }\nimpl Allocator for A { fn alloc(self, size: usize, align: usize) -> *mut u8 { unsafe { let p: *mut u8 = alloc(size, align); return p; } } fn dealloc(borrow mut self, ptr: *mut u8, size: usize) { } }\nfn main() { }");
    assert!(m.contains("borrow mut self"), "{}", m);
}

#[test]
fn trait_impl_method_with_the_wrong_parameter_count_is_rejected() {
    let m = rejected("trait T { fn f(borrow self, a: i32) -> i32; }\nstruct S { x: i32 }\nimpl T for S { fn f(borrow self) -> i32 { return 1; } }\nfn main() { }");
    assert!(m.contains("parameter"), "{}", m);
}
