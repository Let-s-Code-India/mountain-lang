//! Phase 13 M5: the lock-free Treiber stack (tagged pointer) and Michael-Scott queue, written in Mountain
//! (`examples/concurrency/lockfree_stack.mtn`, `lockfree_queue.mtn`), their ABA tests and stress tests.
//! Every program is built in its own unique directory and every run has a watchdog (a hang fails the test).
//! There are no timing assertions; times are printed to stderr only.
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mtn_p13m5_{}_{}_{}", std::process::id(), tag, COUNTER.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn have_valgrind() -> bool {
    Command::new("valgrind").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

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

fn run_exe_limit(exe: &std::path::Path, valgrind: bool, limit: u64) -> (String, i32) {
    let mut cmd = if valgrind {
        let mut c = Command::new("valgrind");
        c.args(["--leak-check=full", "--errors-for-leak-kinds=definite", "--error-exitcode=99", "-q"]).arg(exe);
        c
    } else {
        Command::new(exe)
    };
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let t1 = std::thread::spawn(move || {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut so, &mut s).ok();
        s
    });
    let t2 = std::thread::spawn(move || {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut se, &mut s).ok();
        s
    });
    let start = Instant::now();
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            let out = t1.join().unwrap();
            let err = t2.join().unwrap_or_default();
            if !err.is_empty() {
                eprintln!("stderr of the program: {}", err);
            }
            return (out, st.code().unwrap_or(-1));
        }
        if start.elapsed() > Duration::from_secs(limit) {
            let _ = child.kill();
            let _ = child.wait();
            let out = t1.join().unwrap_or_default();
            panic!("HANG: the program did not finish within {}s (partial output: {:?})", limit, out);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn run_exe(exe: &std::path::Path) -> (String, i32) {
    run_exe_limit(exe, false, 300)
}

fn example(name: &str) -> String {
    std::fs::read_to_string(root().join("examples/concurrency").join(format!("{}.mtn", name))).unwrap()
}

/// everything of an example before its `fn main`
fn library(name: &str) -> String {
    let s = example(name);
    let i = s.find("\nfn main()").expect("example has a main");
    s[..i + 1].to_string()
}

fn golden(name: &str) {
    let want = std::fs::read_to_string(root().join("tests/golden13").join(format!("{}.out", name))).unwrap();
    let exe = build(&root().join("examples/concurrency").join(format!("{}.mtn", name))).unwrap_or_else(|e| panic!("build of {} failed: {}", name, e));
    for _ in 0..3 {
        let (o, c) = run_exe(&exe);
        assert_eq!((o.as_str(), c), (want.as_str(), 0), "{}", name);
    }
    if have_valgrind() {
        let (o, c) = run_exe_limit(&exe, true, 900);
        assert_eq!((o.as_str(), c), (want.as_str(), 0), "{} under valgrind", name);
    }
}

fn rejected(src: &str, needle: &str) {
    match build(&write_src(src)) {
        Ok(_) => panic!("should have been rejected (`{}`):\n{}", needle, src),
        Err(m) => assert!(m.contains(needle), "expected `{}` in:\n{}", needle, m),
    }
}

// ---------------------------------------------------------------- the examples

#[test]
fn example_lockfree_stack() { golden("lockfree_stack") }

#[test]
fn example_lockfree_queue() { golden("lockfree_queue") }

// ---------------------------------------------------------------- ABA

#[test]
fn aba_pop_push_pop_same_address_does_not_corrupt_the_tagged_stack() {
    // list A(1) -> B(20) -> C(300); a stale pop saw (A, next B); another thread pops A, pops B and pushes A back
    // (same address). The stale CAS must fail and the list must be A -> C: sum 301, two nodes => 3012.
    let exe = build(&root().join("examples/concurrency/lockfree_stack.mtn")).unwrap();
    let (o, c) = run_exe(&exe);
    assert_eq!(c, 0);
    let lines: Vec<&str> = o.lines().collect();
    assert_eq!(lines[5], "false", "the stale compareAndSwap must fail");
    assert_eq!(lines[6], "3012", "the list must still be A -> C");
}

#[test]
fn aba_the_same_sequence_corrupts_the_list_without_the_tag() {
    // the negative control: with the tag switched off (TAG_STEP = 0) the very same sequence corrupts the list.
    // Only the sequence is run (not the example's threaded part: on an untagged stack that part may corrupt the
    // list for real and then loop forever, which is exactly what the tag prevents).
    let lib = library("lockfree_stack");
    assert!(lib.contains("const TAG_STEP: usize = 1;"));
    let untagged = lib.replace("const TAG_STEP: usize = 1;", "const TAG_STEP: usize = 0;");
    let exe = build(&write_src(&format!("{}fn main() {{ println(aba_sequence()); }}\n", untagged))).unwrap();
    let (o, c) = run_exe(&exe);
    assert_eq!(c, 0);
    let lines: Vec<&str> = o.lines().collect();
    assert_eq!(lines[0], "true", "without the tag the stale CAS wrongly succeeds");
    // top is now B (already popped) -> C: A is lost, B is back: sum 320, two nodes
    assert_eq!(lines[1], "3202", "and the list is corrupted: A lost, popped node B back on the list");
    // the same program with the tag: the CAS fails and the list is intact
    let tagged = lib.replace("const TAG_STEP: usize = 1;", "const TAG_STEP: usize = 1;");
    let exe = build(&write_src(&format!("{}fn main() {{ println(aba_sequence()); }}\n", tagged))).unwrap();
    let (o, _) = run_exe(&exe);
    assert_eq!(o, "false\n3012\n");
}

#[test]
fn the_tag_changes_on_every_successful_head_update_and_wraps_in_16_bits() {
    // pack/unpack round trip, the tag wraps at 65536, the pointer bits stay intact
    let lib = library("lockfree_stack");
    let src = format!(
        "{}fn main() {{\n    let p: usize = 140737488355320;\n    let w = pack(p, 65535 + TAG_STEP);\n    println(ptr_of(w) == p);\n    println(tag_of(w));\n    println(tag_of(pack(p, 41 + TAG_STEP)));\n    println(ptr_of(pack(p, 7)) == p);\n}}\n",
        lib
    );
    let exe = build(&write_src(&src)).unwrap();
    let (o, c) = run_exe(&exe);
    assert_eq!((o.as_str(), c), ("true\n0\n42\ntrue\n", 0));
}

// ---------------------------------------------------------------- stress (generated programs)

const ACC: &str = r#"struct Acc {
    pc: Atomic<i64>, ps: Atomic<i64>, pq: Atomic<i64>,
    oc: Atomic<i64>, os: Atomic<i64>, oq: Atomic<i64>,
    bad: Atomic<i64>
}
fn new_acc() -> Arc<Acc> {
    Arc::new(Acc {
        pc: Atomic::new(0), ps: Atomic::new(0), pq: Atomic::new(0),
        oc: Atomic::new(0), os: Atomic::new(0), oq: Atomic::new(0),
        bad: Atomic::new(0)
    })
}
fn sq(v: i64) -> i64 { (v % 1000003) * (v % 1000003) }
"#;

const PRINT_ACC: &str = r#"    println(acc.pc.load(Ordering::SeqCst));
    println(acc.oc.load(Ordering::SeqCst));
    println(acc.ps.load(Ordering::SeqCst));
    println(acc.os.load(Ordering::SeqCst));
    println(acc.pq.load(Ordering::SeqCst));
    println(acc.oq.load(Ordering::SeqCst));
    println(acc.bad.load(Ordering::SeqCst));
"#;

/// Compares the seven numbers a stress program prints: pushed == popped (count, sum, sum of squares), no
/// violation, and the pushed count is `expect_count` (or just positive when it is `None`).
fn check_stress(out: &str, expect_count: Option<i64>, what: &str) {
    let v: Vec<i64> = out.lines().map(|l| l.trim().parse().unwrap_or_else(|_| panic!("{}: bad line {:?} in {:?}", what, l, out))).collect();
    assert_eq!(v.len(), 7, "{}: {:?}", what, out);
    assert_eq!(v[0], v[1], "{}: lost or duplicated elements (count pushed vs popped)", what);
    assert_eq!(v[2], v[3], "{}: checksum of the values differs", what);
    assert_eq!(v[4], v[5], "{}: sum of squares differs (a duplicate and a loss cancelled in the sum)", what);
    assert_eq!(v[6], 0, "{}: FIFO order per producer violated {} times", what, v[6]);
    match expect_count {
        Some(n) => assert_eq!(v[0], n, "{}: wrong number of elements", what),
        None => assert!(v[0] > 0, "{}: nothing was pushed", what),
    }
}

const SPAWN_STACK: &str = r#"fn run_all(s: Arc<LockFreeStack<i64>>, acc: Arc<Acc>, i: i64, threads: i64, n: i64, mixed: bool) -> i64 {
    if i >= threads { return 0; }
    let cs = s.clone();
    let ca = acc.clone();
    let h = thread::spawnOS(move || { if mixed { worker_mixed(cs, ca, i, n) } else { worker_fill(cs, ca, i, n) } });
    let rest = run_all(s, acc, i + 1, threads, n, mixed);
    h.join() + rest
}
"#;

fn stack_program(threads: i64, n: i64, mixed: bool) -> String {
    let lib = library("lockfree_stack");
    let workers = r#"// every thread pushes n unique values, then pops up to n times
fn worker_fill(s: Arc<LockFreeStack<i64>>, acc: Arc<Acc>, id: i64, n: i64) -> i64 {
    let mut pc: i64 = 0; let mut ps: i64 = 0; let mut pq: i64 = 0;
    let mut oc: i64 = 0; let mut os: i64 = 0; let mut oq: i64 = 0;
    let mut k: i64 = 0;
    while k < n {
        let v = id * 1000000 + k;
        s.push(v);
        pc = pc + 1; ps = ps + v; pq = pq + sq(v);
        k = k + 1;
    }
    k = 0;
    while k < n {
        match s.pop() {
            Some(v) => { oc = oc + 1; os = os + v; oq = oq + sq(v); }
            None => { oc = oc + 0; }
        }
        k = k + 1;
    }
    acc.pc.fetchAdd(pc, Ordering::SeqCst); acc.ps.fetchAdd(ps, Ordering::SeqCst); acc.pq.fetchAdd(pq, Ordering::SeqCst);
    acc.oc.fetchAdd(oc, Ordering::SeqCst); acc.os.fetchAdd(os, Ordering::SeqCst); acc.oq.fetchAdd(oq, Ordering::SeqCst);
    0
}
// n operations, each a push or a pop chosen by a per-thread pseudo random generator
fn worker_mixed(s: Arc<LockFreeStack<i64>>, acc: Arc<Acc>, id: i64, n: i64) -> i64 {
    let mut pc: i64 = 0; let mut ps: i64 = 0; let mut pq: i64 = 0;
    let mut oc: i64 = 0; let mut os: i64 = 0; let mut oq: i64 = 0;
    let mut x: i64 = id * 7919 + 12345;
    let mut k: i64 = 0;
    let mut ops: i64 = 0;
    while ops < n {
        x = (x * 1103515245 + 12345) % 2147483648;
        if (x / 65536) % 2 == 0 {
            let v = id * 1000000 + k;
            s.push(v);
            pc = pc + 1; ps = ps + v; pq = pq + sq(v);
            k = k + 1;
        } else {
            match s.pop() {
                Some(v) => { oc = oc + 1; os = os + v; oq = oq + sq(v); }
                None => { oc = oc + 0; }
            }
        }
        ops = ops + 1;
    }
    acc.pc.fetchAdd(pc, Ordering::SeqCst); acc.ps.fetchAdd(ps, Ordering::SeqCst); acc.pq.fetchAdd(pq, Ordering::SeqCst);
    acc.oc.fetchAdd(oc, Ordering::SeqCst); acc.os.fetchAdd(os, Ordering::SeqCst); acc.oq.fetchAdd(oq, Ordering::SeqCst);
    0
}
"#;
    let main = format!(
        r#"fn main() {{
    let s: Arc<LockFreeStack<i64>> = Arc::new(LockFreeStack::new());
    let acc = new_acc();
    run_all(s.clone(), acc.clone(), 0, {threads}, {n}, {mixed});
    // what is still on the stack counts as popped
    loop {{
        match s.pop() {{
            Some(v) => {{ acc.oc.fetchAdd(1, Ordering::SeqCst); acc.os.fetchAdd(v, Ordering::SeqCst); acc.oq.fetchAdd(sq(v), Ordering::SeqCst); }}
            None => {{ break; }}
        }}
    }}
{print}}}
"#,
        threads = threads,
        n = n,
        mixed = mixed,
        print = PRINT_ACC
    );
    format!("{}{}{}{}{}", lib, ACC, workers, SPAWN_STACK, main)
}

fn stack_stress(threads: i64, n: i64, mixed: bool) {
    let src = stack_program(threads, n, mixed);
    let exe = build(&write_src(&src)).unwrap_or_else(|e| panic!("build failed: {}", e));
    let t = Instant::now();
    let (o, c) = run_exe(&exe);
    eprintln!("stack {} threads x {} {}: {:?}", threads, n, if mixed { "mixed ops" } else { "fill/drain" }, t.elapsed());
    assert_eq!(c, 0, "{}", o);
    let expect = if mixed { None } else { Some(threads * n) };
    check_stress(&o, expect, &format!("stack {}x{} mixed={}", threads, n, mixed));
}

#[test]
fn stack_stress_8_threads_100000_push_then_pop_each() { stack_stress(8, 100_000, false) }

#[test]
fn stack_stress_8_threads_100000_mixed_operations_each() { stack_stress(8, 100_000, true) }

#[test]
fn stack_stress_more_threads_than_cores() {
    let cores = std::thread::available_parallelism().map(|n| n.get() as i64).unwrap_or(2);
    let threads = (cores * 4).max(16).min(128);
    stack_stress(threads, 800_000 / threads, true);
    stack_stress(threads, 800_000 / threads, false);
}

const SPAWN_QUEUE: &str = r#"fn run_all(q: Arc<LockFreeQueue<i64>>, taken: Arc<Atomic<i64>>, acc: Arc<Acc>, i: i64, producers: i64, consumers: i64, n: i64) -> i64 {
    if i >= producers + consumers { return 0; }
    let cq = q.clone();
    let ct = taken.clone();
    let ca = acc.clone();
    let h = if i < producers {
        thread::spawnOS(move || producer(cq, ca, i, n))
    } else {
        thread::spawnOS(move || consumer(cq, ct, ca, producers * n))
    };
    let rest = run_all(q, taken, acc, i + 1, producers, consumers, n);
    h.join() + rest
}
"#;

const QUEUE_WORKERS: &str = r#"// producer `id` enqueues id * 1,000,000 + k, k = 0, 1, 2, ...
fn producer(q: Arc<LockFreeQueue<i64>>, acc: Arc<Acc>, id: i64, n: i64) -> i64 {
    let mut ps: i64 = 0; let mut pq: i64 = 0;
    let mut k: i64 = 0;
    while k < n {
        let v = id * 1000000 + k;
        q.enqueue(v);
        ps = ps + v; pq = pq + sq(v);
        k = k + 1;
    }
    acc.pc.fetchAdd(n, Ordering::SeqCst); acc.ps.fetchAdd(ps, Ordering::SeqCst); acc.pq.fetchAdd(pq, Ordering::SeqCst);
    0
}
// a consumer checks that the values of each producer arrive in increasing order (FIFO per producer)
fn consumer(q: Arc<LockFreeQueue<i64>>, taken: Arc<Atomic<i64>>, acc: Arc<Acc>, total: i64) -> i64 {
    let mut last: [i64; 128] = [-1; 128];
    let mut oc: i64 = 0; let mut os: i64 = 0; let mut oq: i64 = 0; let mut bad: i64 = 0;
    while taken.load(Ordering::Acquire) < total {
        match q.dequeue() {
            Some(v) => {
                let id = (v / 1000000) as usize;
                let k = v % 1000000;
                if k <= last[id] { bad = bad + 1; }
                last[id] = k;
                oc = oc + 1; os = os + v; oq = oq + sq(v);
                taken.fetchAdd(1, Ordering::AcqRel);
            }
            None => { oc = oc + 0; }
        }
    }
    acc.oc.fetchAdd(oc, Ordering::SeqCst); acc.os.fetchAdd(os, Ordering::SeqCst); acc.oq.fetchAdd(oq, Ordering::SeqCst);
    acc.bad.fetchAdd(bad, Ordering::SeqCst);
    0
}
"#;

fn queue_program(producers: i64, consumers: i64, n: i64) -> String {
    let lib = library("lockfree_queue");
    let main = format!(
        r#"fn main() {{
    let q: Arc<LockFreeQueue<i64>> = Arc::new(LockFreeQueue::new());
    let taken: Arc<Atomic<i64>> = Arc::new(Atomic::new(0));
    let acc = new_acc();
    run_all(q.clone(), taken.clone(), acc.clone(), 0, {p}, {c}, {n});
    match q.dequeue() {{
        Some(v) => {{ acc.bad.fetchAdd(1000000000, Ordering::SeqCst); }}
        None => {{ acc.bad.fetchAdd(0, Ordering::SeqCst); }}
    }}
{print}}}
"#,
        p = producers,
        c = consumers,
        n = n,
        print = PRINT_ACC
    );
    // the library part defines `produce`/`consume` of the example's own demo: they are unused here
    format!("{}{}{}{}{}", lib, ACC, QUEUE_WORKERS, SPAWN_QUEUE, main)
}

fn queue_stress(producers: i64, consumers: i64, n: i64) {
    let src = queue_program(producers, consumers, n);
    let exe = build(&write_src(&src)).unwrap_or_else(|e| panic!("build failed: {}", e));
    let t = Instant::now();
    let (o, c) = run_exe(&exe);
    eprintln!("queue {} producers + {} consumers x {}: {:?}", producers, consumers, n, t.elapsed());
    assert_eq!(c, 0, "{}", o);
    check_stress(&o, Some(producers * n), &format!("queue {}p+{}c", producers, consumers));
}

#[test]
fn queue_stress_4_producers_4_consumers_100000_each_fifo_per_producer() { queue_stress(4, 4, 100_000) }

#[test]
fn queue_stress_8_threads_1_producer_7_consumers() { queue_stress(1, 7, 100_000) }

#[test]
fn queue_stress_7_producers_1_consumer() { queue_stress(7, 1, 100_000) }

#[test]
fn queue_stress_more_threads_than_cores() {
    let cores = std::thread::available_parallelism().map(|n| n.get() as i64).unwrap_or(2);
    let threads = (cores * 4).max(16).min(120);
    queue_stress(threads / 2, threads / 2, 800_000 / threads);
}

fn queue_mixed_program(threads: i64, n: i64) -> String {
    let lib = library("lockfree_queue");
    let workers = r#"// n operations per thread, each an enqueue or a dequeue chosen by a per-thread pseudo random generator; every
// thread is producer and consumer at once, and checks FIFO per producer on what it dequeues itself
fn worker(q: Arc<LockFreeQueue<i64>>, acc: Arc<Acc>, id: i64, n: i64) -> i64 {
    let mut last: [i64; 128] = [-1; 128];
    let mut pc: i64 = 0; let mut ps: i64 = 0; let mut pq: i64 = 0;
    let mut oc: i64 = 0; let mut os: i64 = 0; let mut oq: i64 = 0; let mut bad: i64 = 0;
    let mut x: i64 = id * 7919 + 12345;
    let mut k: i64 = 0;
    let mut ops: i64 = 0;
    while ops < n {
        x = (x * 1103515245 + 12345) % 2147483648;
        if (x / 65536) % 2 == 0 {
            let v = id * 1000000 + k;
            q.enqueue(v);
            pc = pc + 1; ps = ps + v; pq = pq + sq(v);
            k = k + 1;
        } else {
            match q.dequeue() {
                Some(v) => {
                    let p = (v / 1000000) as usize;
                    let j = v % 1000000;
                    if j <= last[p] { bad = bad + 1; }
                    last[p] = j;
                    oc = oc + 1; os = os + v; oq = oq + sq(v);
                }
                None => { oc = oc + 0; }
            }
        }
        ops = ops + 1;
    }
    acc.pc.fetchAdd(pc, Ordering::SeqCst); acc.ps.fetchAdd(ps, Ordering::SeqCst); acc.pq.fetchAdd(pq, Ordering::SeqCst);
    acc.oc.fetchAdd(oc, Ordering::SeqCst); acc.os.fetchAdd(os, Ordering::SeqCst); acc.oq.fetchAdd(oq, Ordering::SeqCst);
    acc.bad.fetchAdd(bad, Ordering::SeqCst);
    0
}
fn run_all(q: Arc<LockFreeQueue<i64>>, acc: Arc<Acc>, i: i64, threads: i64, n: i64) -> i64 {
    if i >= threads { return 0; }
    let cq = q.clone();
    let ca = acc.clone();
    let h = thread::spawnOS(move || worker(cq, ca, i, n));
    let rest = run_all(q, acc, i + 1, threads, n);
    h.join() + rest
}
"#;
    let main = format!(
        r#"fn main() {{
    let q: Arc<LockFreeQueue<i64>> = Arc::new(LockFreeQueue::new());
    let acc = new_acc();
    run_all(q.clone(), acc.clone(), 0, {threads}, {n});
    // what is left in the queue counts as dequeued (in order, per producer, too)
    let mut last: [i64; 128] = [-1; 128];
    loop {{
        match q.dequeue() {{
            Some(v) => {{
                let p = (v / 1000000) as usize;
                let j = v % 1000000;
                if j <= last[p] {{ acc.bad.fetchAdd(1, Ordering::SeqCst); }}
                last[p] = j;
                acc.oc.fetchAdd(1, Ordering::SeqCst); acc.os.fetchAdd(v, Ordering::SeqCst); acc.oq.fetchAdd(sq(v), Ordering::SeqCst);
            }}
            None => {{ break; }}
        }}
    }}
{print}}}
"#,
        threads = threads,
        n = n,
        print = PRINT_ACC
    );
    format!("{}{}{}{}", lib, ACC, workers, main)
}

fn queue_mixed(threads: i64, n: i64) {
    let src = queue_mixed_program(threads, n);
    let exe = build(&write_src(&src)).unwrap_or_else(|e| panic!("build failed: {}", e));
    let t = Instant::now();
    let (o, c) = run_exe(&exe);
    eprintln!("queue {} threads x {} mixed ops: {:?}", threads, n, t.elapsed());
    assert_eq!(c, 0, "{}", o);
    check_stress(&o, None, &format!("queue mixed {}x{}", threads, n));
}

#[test]
fn queue_stress_8_threads_100000_mixed_operations_each() { queue_mixed(8, 100_000) }

#[test]
fn queue_stress_mixed_more_threads_than_cores() {
    let cores = std::thread::available_parallelism().map(|n| n.get() as i64).unwrap_or(2);
    let threads = (cores * 4).max(16).min(120);
    queue_mixed(threads, 800_000 / threads);
}

// ---------------------------------------------------------------- Drop payloads and reclamation

const LOUD: &str = r#"struct Loud { id: i64, hits: Arc<Atomic<i64>> }
impl Drop for Loud { fn drop(borrow mut self) { self.hits.fetchAdd(1, Ordering::SeqCst); } }
"#;

#[test]
fn stack_drops_every_payload_exactly_once_values_left_on_the_stack_included() {
    let lib = library("lockfree_stack");
    // 3 pushed and popped, 2 left on the stack: dropping the stack drops the 2; the 3 are dropped by the caller
    let src = format!(
        r#"{lib}{loud}fn main() {{
    let hits: Arc<Atomic<i64>> = Arc::new(Atomic::new(0));
    {{
        let s: LockFreeStack<Loud> = LockFreeStack::new();
        s.push(Loud {{ id: 1, hits: hits.clone() }});
        s.push(Loud {{ id: 2, hits: hits.clone() }});
        s.push(Loud {{ id: 3, hits: hits.clone() }});
        match s.pop() {{ Some(l) => {{ println(l.id); }} None => {{ println(-1); }} }}
        println(hits.load(Ordering::SeqCst));
        match s.pop() {{ Some(l) => {{ println(l.id); }} None => {{ println(-1); }} }}
        match s.pop() {{ Some(l) => {{ println(l.id); }} None => {{ println(-1); }} }}
        println(hits.load(Ordering::SeqCst));
        s.push(Loud {{ id: 4, hits: hits.clone() }});
        s.push(Loud {{ id: 5, hits: hits.clone() }});
    }}
    println(hits.load(Ordering::SeqCst));
}}
"#,
        lib = lib,
        loud = LOUD
    );
    let exe = build(&write_src(&src)).unwrap_or_else(|e| panic!("build failed: {}", e));
    let (o, c) = run_exe(&exe);
    assert_eq!((o.as_str(), c), ("3\n1\n2\n1\n3\n5\n", 0));
    if have_valgrind() {
        let (o, c) = run_exe_limit(&exe, true, 600);
        assert_eq!((o.as_str(), c), ("3\n1\n2\n1\n3\n5\n", 0), "under valgrind");
    }
}

#[test]
fn queue_drops_every_payload_exactly_once_values_left_in_the_queue_included() {
    let lib = library("lockfree_queue");
    let src = format!(
        r#"{lib}{loud}fn main() {{
    let hits: Arc<Atomic<i64>> = Arc::new(Atomic::new(0));
    {{
        let q: LockFreeQueue<Loud> = LockFreeQueue::new();
        q.enqueue(Loud {{ id: 1, hits: hits.clone() }});
        q.enqueue(Loud {{ id: 2, hits: hits.clone() }});
        q.enqueue(Loud {{ id: 3, hits: hits.clone() }});
        match q.dequeue() {{ Some(l) => {{ println(l.id); }} None => {{ println(-1); }} }}
        println(hits.load(Ordering::SeqCst));
        match q.dequeue() {{ Some(l) => {{ println(l.id); }} None => {{ println(-1); }} }}
        q.enqueue(Loud {{ id: 4, hits: hits.clone() }});
        println(hits.load(Ordering::SeqCst));
    }}
    println(hits.load(Ordering::SeqCst));
}}
"#,
        lib = lib,
        loud = LOUD
    );
    let exe = build(&write_src(&src)).unwrap_or_else(|e| panic!("build failed: {}", e));
    let (o, c) = run_exe(&exe);
    // 1 dropped by the caller after `println`, 2 likewise, then the two left (3 and 4) by the queue's Drop
    assert_eq!((o.as_str(), c), ("1\n1\n2\n2\n4\n", 0));
    if have_valgrind() {
        let (o, c) = run_exe_limit(&exe, true, 600);
        assert_eq!((o.as_str(), c), ("1\n1\n2\n2\n4\n", 0), "under valgrind");
    }
}

// ---------------------------------------------------------------- Send / Sync

#[test]
fn rejects_a_raw_pointer_crossing_spawn_and_spawnos() {
    rejected("fn main() { let mut x: i32 = 1; let p = &mut x as *mut i32; let h = spawn move { unsafe { *p } }; println(h.join()); }", "a raw pointer is not `Send`");
    rejected("fn main() { let mut x: i32 = 1; let p = &mut x as *mut i32; let h = thread::spawnOS(move || { unsafe { *p } }); println(h.join()); }", "a raw pointer is not `Send`");
}

#[test]
fn rejects_a_struct_with_a_plain_raw_pointer_inside_an_arc_crossing_threads() {
    rejected(
        "struct Bad { p: *mut i64 }\nfn main() { let mut x: i64 = 1; let b = Arc::new(Bad { p: &mut x as *mut i64 }); let c = b.clone(); let h = spawn move { unsafe { *c.p } }; println(h.join()); }",
        "cannot be sent between threads",
    );
}

#[test]
fn arc_of_the_stack_and_of_the_queue_cross_spawn_and_spawnos() {
    let lib_s = library("lockfree_stack");
    let src = format!(
        r#"{lib_s}fn main() {{
    let s: Arc<LockFreeStack<i64>> = Arc::new(LockFreeStack::new());
    let a = s.clone();
    let b = s.clone();
    let h = spawn move {{ a.push(5); 1 }};
    let t = thread::spawnOS(move || {{ b.push(6); 2 }});
    println(h.join() + t.join());
    let mut sum: i64 = 0;
    loop {{ match s.pop() {{ Some(v) => {{ sum = sum + v; }} None => {{ break; }} }} }}
    println(sum);
}}
"#
    );
    let exe = build(&write_src(&src)).unwrap_or_else(|e| panic!("build failed: {}", e));
    assert_eq!(run_exe(&exe), ("3\n11\n".to_string(), 0));
    let lib_q = library("lockfree_queue");
    let src = format!(
        r#"{lib_q}fn main() {{
    let q: Arc<LockFreeQueue<i64>> = Arc::new(LockFreeQueue::new());
    let a = q.clone();
    let b = q.clone();
    let h = spawn move {{ a.enqueue(5); 1 }};
    let t = thread::spawnOS(move || {{ b.enqueue(6); 2 }});
    println(h.join() + t.join());
    let mut sum: i64 = 0;
    loop {{ match q.dequeue() {{ Some(v) => {{ sum = sum + v; }} None => {{ break; }} }} }}
    println(sum);
}}
"#
    );
    let exe = build(&write_src(&src)).unwrap_or_else(|e| panic!("build failed: {}", e));
    assert_eq!(run_exe(&exe), ("3\n11\n".to_string(), 0));
}

#[test]
fn a_stack_or_queue_of_a_non_send_payload_does_not_cross_threads() {
    // the payload type decides: Rc is not Send, so neither is a stack or a queue of Rc (this is why the stack has
    // an `Atomic<*mut Node<T>>` marker field)
    let lib_s = library("lockfree_stack");
    rejected(
        &format!("{lib_s}fn main() {{ let s: Arc<LockFreeStack<Rc<i64>>> = Arc::new(LockFreeStack::new()); let t = s.clone(); let h = spawn move {{ t.pop().isOk() }}; println(1); }}\n"),
        "cannot be sent between threads",
    );
    let lib_q = library("lockfree_queue");
    rejected(
        &format!("{lib_q}fn main() {{ let q: Arc<LockFreeQueue<Rc<i64>>> = Arc::new(LockFreeQueue::new()); let t = q.clone(); let h = spawn move {{ t.dequeue().isOk() }}; println(1); }}\n"),
        "cannot be sent between threads",
    );
}

// ---------------------------------------------------------------- whole-program ThreadSanitizer (manual, not in CI)

/// `cargo test --test phase13_m5 -- --ignored`: the stress programs (smaller, TSAN is slow) and both examples
/// are compiled with ThreadSanitizer instrumentation by `scripts/tsan_program.sh`; no warning may appear.
/// Ignored by default: it needs clang-18 and GCC's libtsan, and it is not part of CI.
#[test]
#[ignore]
fn whole_program_threadsanitizer_on_the_stress_programs() {
    let dir = unique_dir("tsan");
    let mut files = vec![root().join("examples/concurrency/lockfree_stack.mtn"), root().join("examples/concurrency/lockfree_queue.mtn")];
    let progs = [
        ("stack_fill", stack_program(8, 20_000, false)),
        ("stack_mixed", stack_program(8, 20_000, true)),
        ("queue_pc", queue_program(4, 4, 20_000)),
        ("queue_mixed", queue_mixed_program(8, 20_000)),
    ];
    for (name, src) in progs.iter() {
        let f = dir.join(format!("{}.mtn", name));
        std::fs::write(&f, src).unwrap();
        files.push(f);
    }
    let out = Command::new("bash").arg(root().join("scripts/tsan_program.sh")).arg(env!("CARGO_BIN_EXE_mtnc")).args(&files).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    eprintln!("{}", text);
    assert!(out.status.success(), "ThreadSanitizer findings:\n{}", text);
    assert_eq!(text.matches("tsan clean:").count(), files.len());
}
