//! Phase 13 M3: channels, `Sender<T>` / `Receiver<T>` (Document 12 §6, §11.13; Document 14 §3, §9.6).
//! Every program is built in its own unique directory (AtomicUsize + pid), never shared between tests.
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mtn_p13m3_{}_{}_{}", std::process::id(), tag, COUNTER.fetch_add(1, Ordering::SeqCst)));
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

fn rejected(src: &str, needle: &str) {
    match build(&write_src(src)) {
        Ok(_) => panic!("should have been rejected (`{}`):\n{}", needle, src),
        Err(m) => assert!(m.contains(needle), "expected `{}` in:\n{}", needle, m),
    }
}

fn golden_in(dir: &str, name: &str) {
    let src = root().join(dir).join(format!("{}.mtn", name));
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

fn golden(name: &str) {
    golden_in("examples/concurrency", name)
}


/// Runs `src` at MTN_WORKERS = 1, 2 and 4 and expects exactly `want` each time.
fn run_workers(src: &str, want: &str) {
    let exe = build(&write_src(src)).unwrap_or_else(|e| panic!("build failed: {}\n{}", e, src));
    for w in ["1", "2", "4"] {
        let (o, c) = run_exe(&exe, w, false);
        assert_eq!((o.as_str(), c), (want, 0), "MTN_WORKERS={}\n{}", w, src);
    }
}

fn run_workers_valgrind(src: &str, want: &str) {
    let exe = build(&write_src(src)).unwrap_or_else(|e| panic!("build failed: {}\n{}", e, src));
    for w in ["1", "4"] {
        let (o, c) = run_exe(&exe, w, false);
        assert_eq!((o.as_str(), c), (want, 0), "MTN_WORKERS={}\n{}", w, src);
        if have_valgrind() {
            let (o, c) = run_exe(&exe, w, true);
            assert_eq!((o.as_str(), c), (want, 0), "under valgrind, MTN_WORKERS={}\n{}", w, src);
        }
    }
}

fn check_and_build_agree(src: &str, needle: &str) {
    let f = write_src(src);
    let chk = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("check").arg(&f).output().unwrap();
    let all = String::from_utf8_lossy(&chk.stdout).to_string() + &String::from_utf8_lossy(&chk.stderr);
    assert!(!chk.status.success() && all.contains(needle), "check should reject with `{}`:\n{}", needle, all);
    rejected(src, needle);
}

// ---------------------------------------------------------------- producers x consumers

/// P producers of N messages each and C consumers over one channel (`cap` = None: unbounded). The consumers add
/// the message, its square and a count into shared atomics; the expected totals are computed here.
fn matrix_src(p: i64, c: i64, n: i64, cap: Option<i64>, os: bool) -> (String, String) {
    let spawn_p = |call: &str| if os { format!("thread::spawnOS(move || {{ {} }})", call) } else { format!("spawn move {{ {} }}", call) };
    let create = match cap {
        Some(k) => format!("channel::<i64>({})", k),
        None => "channelUnbounded::<i64>()".to_string(),
    };
    let src = format!(
        r#"fn prod(tx: Sender<i64>, id: i64, n: i64) -> i64 {{
    let mut k: i64 = 0;
    while k < n {{
        tx.send(id * 100000 + k);
        k = k + 1;
    }}
    n
}}
fn run_prod(tx: Sender<i64>, i: i64, p: i64, n: i64) -> i64 {{
    if i > p {{ return 0; }}
    let t = tx.clone();
    let h = {sp};
    let rest = run_prod(tx, i + 1, p, n);
    h.join() + rest
}}
fn cons(rx: Receiver<i64>, sum: Arc<Atomic<i64>>, sq: Arc<Atomic<i64>>, cnt: Arc<Atomic<i64>>) -> i64 {{
    let mut mine: i64 = 0;
    loop {{
        match rx.recv() {{
            Some(v) => {{
                sum.fetchAdd(v, Ordering::SeqCst);
                sq.fetchAdd(v * v, Ordering::SeqCst);
                cnt.fetchAdd(1, Ordering::SeqCst);
                mine = mine + 1;
            }}
            None => {{ break; }}
        }}
    }}
    mine
}}
fn run_cons(rx: Receiver<i64>, i: i64, c: i64, sum: Arc<Atomic<i64>>, sq: Arc<Atomic<i64>>, cnt: Arc<Atomic<i64>>) -> i64 {{
    if i > c {{ return 0; }}
    let r2 = rx.clone();
    let a = sum.clone();
    let b = sq.clone();
    let d = cnt.clone();
    let h = {sc};
    let rest = run_cons(rx, i + 1, c, sum, sq, cnt);
    h.join() + rest
}}
fn main() {{
    let (tx, rx) = {create};
    let sum: Arc<Atomic<i64>> = Arc::new(Atomic::new(0));
    let sq: Arc<Atomic<i64>> = Arc::new(Atomic::new(0));
    let cnt: Arc<Atomic<i64>> = Arc::new(Atomic::new(0));
    let s2 = sum.clone();
    let q2 = sq.clone();
    let n2 = cnt.clone();
    let cg = {cg};
    let pg = {pg};
    println(pg.join());
    println(cg.join());
    println(sum.load());
    println(sq.load());
    println(cnt.load());
}}
"#,
        sp = spawn_p("prod(t, i, n)"),
        sc = spawn_p("cons(r2, a, b, d)"),
        cg = spawn_p(&format!("run_cons(rx, 1, {}, s2, q2, n2)", c)),
        pg = spawn_p(&format!("run_prod(tx, 1, {}, {})", p, n)),
        create = create,
    );
    let (mut sum, mut sq, mut cnt) = (0i64, 0i64, 0i64);
    for id in 1..=p {
        for k in 0..n {
            let v = id * 100000 + k;
            sum += v;
            sq += v * v;
            cnt += 1;
        }
    }
    (src, format!("{}\n{}\n{}\n{}\n{}\n", p * n, p * n, sum, sq, cnt))
}

fn matrix(cap: Option<i64>, os: bool) {
    for p in [1, 4, 8] {
        for c in [1, 4, 8] {
            let n = 300;
            let (src, want) = matrix_src(p, c, n, cap, os);
            let exe = build(&write_src(&src)).unwrap_or_else(|e| panic!("build failed: {}\n{}", e, src));
            for w in ["1", "4"] {
                let (o, code) = run_exe(&exe, w, false);
                assert_eq!((o.as_str(), code), (want.as_str(), 0), "{} producers, {} consumers, cap {:?}, os threads {}, MTN_WORKERS={}", p, c, cap, os, w);
            }
        }
    }
}

#[test]
fn producers_consumers_1_4_8_on_a_bounded_channel_tasks() {
    matrix(Some(8), false);
}
#[test]
fn producers_consumers_1_4_8_on_an_unbounded_channel_tasks() {
    matrix(None, false);
}
#[test]
fn producers_consumers_1_4_8_on_a_bounded_channel_os_threads() {
    matrix(Some(8), true);
}
#[test]
fn producers_consumers_1_4_8_on_an_unbounded_channel_os_threads() {
    matrix(None, true);
}
#[test]
fn bounded_channel_of_capacity_one_with_8_producers_and_8_consumers() {
    let (src, want) = matrix_src(8, 8, 200, Some(1), false);
    run_workers(&src, &want);
}

// ---------------------------------------------------------------- single-worker liveness (MTN_WORKERS=1)

const LIVE_A: &str = r#"fn produce(tx: Sender<i64>, n: i64) -> i64 {
    let mut i: i64 = 1;
    while i <= n {
        tx.send(i);
        i = i + 1;
    }
    n
}
fn consume(rx: Receiver<i64>) -> i64 {
    let mut sum: i64 = 0;
    let mut cnt: i64 = 0;
    loop {
        match rx.recv() {
            Some(v) => { sum = sum + v; cnt = cnt + 1; }
            None => { break; }
        }
    }
    sum * 100000 + cnt
}
fn main() {
    let (tx, rx) = channel::<i64>(1);
    let c = spawn move { consume(rx) };
    let p = spawn move { produce(tx, 10000) };
    p.join();
    println(c.join());
}
"#;

#[test]
fn liveness_a_producer_and_consumer_task_exchange_10000_values_through_capacity_1() {
    // 10,000 values: sum 50,005,000, count 10,000
    run_workers(LIVE_A, "5000500010000\n");
    // the consumer spawned after the producer (the producer then holds the single worker first)
    let swapped = LIVE_A.replace(
        "    let c = spawn move { consume(rx) };\n    let p = spawn move { produce(tx, 10000) };\n",
        "    let p = spawn move { produce(tx, 10000) };\n    let c = spawn move { consume(rx) };\n",
    );
    assert_ne!(swapped, LIVE_A);
    run_workers(&swapped, "5000500010000\n");
}

#[test]
fn liveness_b_100_producer_tasks_and_one_consumer_through_capacity_1() {
    let src = r#"fn consume(rx: Receiver<i64>) -> i64 {
    let mut sum: i64 = 0;
    let mut cnt: i64 = 0;
    loop {
        match rx.recv() {
            Some(v) => { sum = sum + v; cnt = cnt + 1; }
            None => { break; }
        }
    }
    sum * 1000 + cnt
}
fn one(tx: Sender<i64>, id: i64) -> i64 {
    let mut k: i64 = 0;
    while k < 20 {
        tx.send(id * 100 + k);
        k = k + 1;
    }
    0
}
fn run(tx: Sender<i64>, i: i64, n: i64) -> i64 {
    if i > n { return 0; }
    let t = tx.clone();
    let h = spawn move { one(t, i) };
    let rest = run(tx, i + 1, n);
    h.join() + rest
}
fn main() {
    let (tx, rx) = channel::<i64>(1);
    let c = spawn move { consume(rx) };
    run(tx, 1, 100);
    println(c.join());
}
"#;
    // ids 1..=100, 20 values each: sum = 20 * 100 * 5050 + 100 * 190 = 10,119,000, count 2,000
    run_workers(src, "10119002000\n");
}

#[test]
fn liveness_c_async_producer_and_a_synchronous_consumer_thread() {
    let src = r#"async fn produce(tx: Sender<i64>, n: i64) {
    let mut i: i64 = 1;
    while i <= n {
        await tx.send(i);
        i = i + 1;
    }
}
fn main() {
    let (tx, rx) = channel::<i64>(1);
    let p = spawn produce(tx, 5000);
    let t = thread::spawnOS(move || {
        let mut s: i64 = 0;
        loop {
            match rx.recv() {
                Some(v) => { s = s + v; }
                None => { break; }
            }
        }
        s
    });
    p.join();
    println(t.join());
}
"#;
    run_workers(src, "12502500\n");
}

#[test]
fn async_consumer_and_async_producers_never_hold_a_worker() {
    // 20 async producers and one async consumer on one worker: every wait is a suspension
    let src = r#"async fn prod(tx: Sender<i64>, id: i64) -> i64 {
    let mut k: i64 = 0;
    while k < 50 {
        await tx.send(id * 1000 + k);
        k = k + 1;
    }
    0
}
async fn cons(rx: Receiver<i64>) -> i64 {
    let mut sum: i64 = 0;
    let mut cnt: i64 = 0;
    loop {
        match await rx.recv() {
            Some(v) => { sum = sum + v; cnt = cnt + 1; }
            None => { break; }
        }
    }
    sum * 10000 + cnt
}
async fn run(tx: Sender<i64>, i: i64, n: i64) -> i64 {
    if i > n { return 0; }
    let t = tx.clone();
    let h = spawn prod(t, i);
    let rest = await run(tx, i + 1, n);
    return (await h) + rest;
}
async fn main() {
    let (tx, rx) = channel::<i64>(2);
    let c = spawn cons(rx);
    await run(tx, 1, 20);
    println(await c);
}
"#;
    // ids 1..=20, k 0..49: sum = 50 * 1000 * 210 + 20 * 1225 = 10,524,500; count 1,000
    run_workers(src, "105245001000\n");
}

// ---------------------------------------------------------------- back-pressure, close, try*, order

fn backpressure_src(cap: i64) -> String {
    format!(
        r#"fn prod(tx: Sender<i64>, sent: Arc<Atomic<i64>>, n: i64) -> i64 {{
    let mut i: i64 = 1;
    while i <= n {{
        tx.send(i);
        sent.fetchAdd(1, Ordering::SeqCst);
        i = i + 1;
    }}
    n
}}
fn main() {{
    let (tx, rx) = channel::<i64>({cap});
    let sent: Arc<Atomic<i64>> = Arc::new(Atomic::new(0));
    let s2 = sent.clone();
    let p = spawn move {{ prod(tx, s2, 1000) }};
    let mut got: i64 = 0;
    let mut worst: i64 = 0;
    loop {{
        match rx.recv() {{
            Some(v) => {{
                got = got + 1;
                let ahead = sent.load() - got;
                if ahead > worst {{ worst = ahead; }}
            }}
            None => {{ break; }}
        }}
    }}
    p.join();
    println(got);
    println(worst <= {cap} + 1);
}}
"#,
        cap = cap
    )
}

#[test]
fn back_pressure_the_producer_never_gets_more_than_capacity_ahead() {
    // `sent` counts completed sends; at every receive `sent - received <= capacity + 1` (the +1 is the send
    // that has completed but whose counter increment is not visible yet)
    for cap in [1, 2, 7] {
        run_workers(&backpressure_src(cap), "1000\ntrue\n");
    }
}

#[test]
fn per_sender_order_is_kept() {
    // four producers; the single consumer checks that each producer's numbers arrive in increasing order
    let src = r#"fn prod(tx: Sender<i64>, id: i64) -> i64 {
    let mut k: i64 = 0;
    while k < 500 {
        tx.send(id * 1000000 + k);
        k = k + 1;
    }
    0
}
fn run(tx: Sender<i64>, i: i64) -> i64 {
    if i > 4 { return 0; }
    let t = tx.clone();
    let h = spawn move { prod(t, i) };
    let rest = run(tx, i + 1);
    h.join() + rest
}
fn main() {
    let (tx, rx) = channel::<i64>(3);
    let g = spawn move { run(tx, 1) };
    let mut l1: i64 = -1;
    let mut l2: i64 = -1;
    let mut l3: i64 = -1;
    let mut l4: i64 = -1;
    let mut bad: i64 = 0;
    let mut cnt: i64 = 0;
    loop {
        match rx.recv() {
            Some(v) => {
                let id = v / 1000000;
                let k = v % 1000000;
                cnt = cnt + 1;
                if id == 1 { if k <= l1 { bad = bad + 1; } l1 = k; }
                if id == 2 { if k <= l2 { bad = bad + 1; } l2 = k; }
                if id == 3 { if k <= l3 { bad = bad + 1; } l3 = k; }
                if id == 4 { if k <= l4 { bad = bad + 1; } l4 = k; }
            }
            None => { break; }
        }
    }
    g.join();
    println(cnt);
    println(bad);
}
"#;
    run_workers(src, "2000\n0\n");
}

#[test]
fn close_by_dropping_every_sender_drains_then_none() {
    let src = r#"fn main() {
    let (tx, rx) = channelUnbounded::<i64>();
    let t2 = tx.clone();
    tx.send(1);
    t2.send(2);
    drop(tx);
    t2.send(3);
    drop(t2);
    let a = rx.recv();
    let b = rx.recv();
    let c = rx.recv();
    let d = rx.recv();
    let e = rx.tryRecv();
    println(match a { Some(v) => v, None => -1 });
    println(match b { Some(v) => v, None => -1 });
    println(match c { Some(v) => v, None => -1 });
    println(match d { Some(v) => v, None => -1 });
    println(match e { Some(v) => v, None => -1 });
}
"#;
    run_workers(src, "1\n2\n3\n-1\n-1\n");
}

#[test]
fn close_by_dropping_every_receiver_gives_the_same_value_back() {
    let src = r#"fn main() {
    let (tx, rx) = channel::<String>(2);
    let r2 = rx.clone();
    drop(rx);
    let first = tx.send(String::from("kept"));
    println(first.isOk());
    drop(r2);
    let second = tx.send(String::from("same value"));
    println(second.isErr());
    match second {
        Ok(_) => println("delivered"),
        Err(s) => println(s),
    }
    let third = tx.trySend(String::from("again"));
    match third {
        Ok(_) => println("delivered"),
        Err(s) => println(s),
    }
}
"#;
    run_workers_valgrind(src, "true\ntrue\nsame value\nagain\n");
}

#[test]
fn a_blocked_sender_is_woken_when_the_last_receiver_is_dropped() {
    let src = r#"fn fill(tx: Sender<i64>) -> i64 {
    let mut i: i64 = 0;
    loop {
        let r = tx.send(i);
        if r.isErr() { break; }
        i = i + 1;
    }
    i
}
fn main() {
    let (tx, rx) = channel::<i64>(2);
    let p = spawn move { fill(tx) };
    let mut got: i64 = 0;
    while got < 5 {
        match rx.recv() { Some(_) => { got = got + 1; } None => { break; } }
    }
    drop(rx);
    let n = p.join();
    println(n >= 5);
}
"#;
    run_workers(src, "true\n");
}

#[test]
fn a_blocked_receiver_is_woken_when_the_last_sender_is_dropped() {
    let src = r#"fn wait(rx: Receiver<i64>) -> i64 {
    match rx.recv() { Some(v) => v, None => -7 }
}
fn main() {
    let (tx, rx) = channel::<i64>(2);
    let c0 = spawn move { wait(rx) };
    let mut spin: i64 = 0;
    while spin < 100000 { spin = spin + 1; }
    drop(tx);
    println(c0.join());
}
"#;
    run_workers(src, "-7\n");
}

#[test]
fn try_send_on_full_and_try_recv_on_empty() {
    let src = r#"fn main() {
    let (tx, rx) = channel::<i64>(2);
    let e = rx.tryRecv();
    println(match e { Some(v) => v, None => -1 });
    println(tx.trySend(1).isOk());
    println(tx.trySend(2).isOk());
    let f = tx.trySend(3);
    println(match f { Err(v) => v, Ok(_) => -2 });
    println(match rx.recv() { Some(v) => v, None => -1 });
    println(tx.trySend(3).isOk());
    println(match rx.tryRecv() { Some(v) => v, None => -1 });
    println(match rx.tryRecv() { Some(v) => v, None => -1 });
    println(match rx.tryRecv() { Some(v) => v, None => -1 });
}
"#;
    run_workers(src, "-1\ntrue\ntrue\n3\n1\ntrue\n2\n3\n-1\n");
}

#[test]
fn the_default_capacity_is_1024() {
    let src = r#"fn main() {
    let (tx, rx) = channel::<i64>();
    let mut i: i64 = 0;
    while i < 1024 {
        if tx.trySend(i).isErr() { println("full too early"); }
        i = i + 1;
    }
    println(tx.trySend(0).isErr());
    println(match rx.recv() { Some(v) => v, None => -1 });
    println(tx.trySend(5).isOk());
}
"#;
    run_workers(src, "true\n0\ntrue\n");
}

#[test]
fn unbounded_never_reports_full() {
    let src = r#"fn main() {
    let (tx, rx) = channelUnbounded::<i64>();
    let mut i: i64 = 0;
    let mut full: i64 = 0;
    while i < 50000 {
        if tx.trySend(i).isErr() { full = full + 1; }
        i = i + 1;
    }
    drop(tx);
    let mut sum: i64 = 0;
    loop { match rx.recv() { Some(v) => { sum = sum + v; } None => { break; } } }
    println(full);
    println(sum);
}
"#;
    run_workers(src, "0\n1249975000\n");
}

// ---------------------------------------------------------------- payloads

#[test]
fn payloads_string_struct_dyn_arc_and_nested_channels() {
    let src = r#"trait Shape { fn area(borrow self) -> i32; }
struct Sq { side: i32 }
impl Shape for Sq { fn area(borrow self) -> i32 { return self.side * self.side; } }
struct Pt { x: i64, y: i64, name: String }
fn main() {
    let (stx, srx) = channel::<String>(4);
    let (ptx, prx) = channel::<Pt>(4);
    let (dtx, drx) = channel::<Box<dyn Shape + Send>>(4);
    let (atx, arx) = channel::<Arc<i64>>(4);
    let shared: Arc<i64> = Arc::new(77);
    let h = thread::spawnOS(move || {
        stx.send(String::from("hello"));
        ptx.send(Pt { x: 3, y: 4, name: String::from("p") });
        let b: Box<dyn Shape + Send> = Box::new(Sq { side: 6 });
        dtx.send(b);
        0
    });
    h.join();
    atx.send(shared.clone());
    match srx.recv() { Some(s) => println(s), None => println("none") }
    match prx.recv() { Some(p) => { println(p.x + p.y); println(p.name); } None => println("none") }
    match drx.recv() { Some(b) => println(b.area()), None => println("none") }
    match arx.recv() { Some(a) => println(*a), None => println("none") }
    println(Arc::strongCount(borrow shared));
}
"#;
    // the clone sent through the channel is dropped at the end of the receiving arm: one owner is left
    run_workers_valgrind(src, "hello\n7\np\n36\n77\n1\n");
}

#[test]
fn a_drop_payload_is_dropped_exactly_once_on_every_path() {
    // received (1, dropped at the end of its arm), returned by a failed send (4), left in the queue (2, 3)
    let src = r#"struct Loud { id: i64 }
impl Drop for Loud { fn drop(borrow mut self) { print("drop "); println(self.id); } }
fn main() {
    let (tx, rx) = channel::<Loud>(4);
    tx.send(Loud { id: 1 });
    tx.send(Loud { id: 2 });
    tx.send(Loud { id: 3 });
    match rx.recv() { Some(l) => { print("got "); println(l.id); } None => {} }
    drop(rx);
    let r = tx.send(Loud { id: 4 });
    match r { Ok(_) => println("sent"), Err(l) => { print("returned "); println(l.id); } }
    drop(tx);
    println("end");
}
"#;
    run_workers_valgrind(src, "got 1\ndrop 1\nreturned 4\ndrop 4\ndrop 2\ndrop 3\nend\n");
}

#[test]
fn a_drop_payload_left_in_an_unbounded_channel_and_a_try_send_that_fails() {
    let src = r#"struct Loud { id: i64 }
impl Drop for Loud { fn drop(borrow mut self) { print("drop "); println(self.id); } }
fn main() {
    let (tx, rx) = channel::<Loud>(1);
    println(tx.trySend(Loud { id: 1 }).isOk());
    let r = tx.trySend(Loud { id: 2 });
    match r { Ok(_) => println("queued"), Err(l) => { print("full, returned "); println(l.id); } }
    println("before");
    let (utx, urx) = channelUnbounded::<Loud>();
    utx.send(Loud { id: 10 });
    utx.send(Loud { id: 11 });
    println("end");
}
"#;
    // at the end of main the locals go out in reverse order: urx, utx (drops 10, 11), then rx, tx (drops 1)
    run_workers_valgrind(src, "true\nfull, returned 2\ndrop 2\nbefore\nend\ndrop 10\ndrop 11\ndrop 1\n");
}

#[test]
fn a_drop_payload_through_tasks_is_dropped_once() {
    let src = r#"struct Loud { id: i64 }
impl Drop for Loud { fn drop(borrow mut self) { println("drop"); } }
fn prod(tx: Sender<Loud>) -> i64 {
    let mut i: i64 = 0;
    while i < 3 { tx.send(Loud { id: i }); i = i + 1; }
    0
}
fn main() {
    let (tx, rx) = channel::<Loud>(1);
    let p = spawn move { prod(tx) };
    let mut n: i64 = 0;
    loop { match rx.recv() { Some(l) => { n = n + l.id + 1; } None => { break; } } }
    p.join();
    println(n);
}
"#;
    run_workers_valgrind(src, "drop\ndrop\ndrop\n6\n");
}

#[test]
fn channel_of_channels() {
    let src = r#"fn serve(reqs: Receiver<Sender<i64>>) -> i64 {
    let mut served: i64 = 0;
    loop {
        match reqs.recv() {
            Some(reply) => { reply.send(served * 11); served = served + 1; }
            None => { break; }
        }
    }
    served
}
fn main() {
    let (rtx, rrx) = channel::<Sender<i64>>(2);
    let h = spawn move { serve(rrx) };
    let mut i: i64 = 0;
    let mut total: i64 = 0;
    while i < 4 {
        let (tx, rx) = channel::<i64>(1);
        rtx.send(tx);
        match rx.recv() { Some(v) => { total = total + v; } None => { println("no reply"); } }
        i = i + 1;
    }
    drop(rtx);
    println(total);
    println(h.join());
}
"#;
    run_workers_valgrind(src, "66\n4\n");
}

// ---------------------------------------------------------------- tasks, threads, actors, closures

#[test]
fn sender_cloned_into_8_tasks() {
    let src = r#"fn one(tx: Sender<i64>, id: i64) -> i64 {
    tx.send(id);
    tx.send(id * 100);
    0
}
fn main() {
    let (tx, rx) = channel::<i64>(2);
    let t1 = tx.clone(); let h1 = spawn move { one(t1, 1) };
    let t2 = tx.clone(); let h2 = spawn move { one(t2, 2) };
    let t3 = tx.clone(); let h3 = spawn move { one(t3, 3) };
    let t4 = tx.clone(); let h4 = spawn move { one(t4, 4) };
    let t5 = tx.clone(); let h5 = spawn move { one(t5, 5) };
    let t6 = tx.clone(); let h6 = spawn move { one(t6, 6) };
    let t7 = tx.clone(); let h7 = spawn move { one(t7, 7) };
    let t8 = tx.clone(); let h8 = spawn move { one(t8, 8) };
    drop(tx);
    let mut sum: i64 = 0;
    let mut cnt: i64 = 0;
    loop { match rx.recv() { Some(v) => { sum = sum + v; cnt = cnt + 1; } None => { break; } } }
    h1.join(); h2.join(); h3.join(); h4.join(); h5.join(); h6.join(); h7.join(); h8.join();
    println(sum);
    println(cnt);
}
"#;
    // (1 + .. + 8) * 101 = 3636
    run_workers(src, "3636\n16\n");
}

#[test]
fn an_actor_sends_into_a_channel_and_an_actor_holds_one_in_its_state() {
    let src = r#"actor Worker { on work(n: i64, out: Sender<i64>) { out.send(n * n); } }
actor Hub {
    state pair: (Sender<i64>, Receiver<i64>) = channel::<i64>(8);
    on put(n: i64) { self.pair.0.send(n); }
    on take() { match self.pair.1.tryRecv() { Some(v) => println(v), None => println("empty") } }
}
fn main() {
    let (tx, rx) = channel::<i64>(8);
    let w = Worker::spawn();
    let mut i: i64 = 1;
    while i <= 5 { w.send(work(i, tx.clone())); i = i + 1; }
    drop(tx);
    let mut s: i64 = 0;
    loop { match rx.recv() { Some(v) => { s = s + v; } None => { break; } } }
    println(s);
    let h = Hub::spawn();
    h.send(put(7));
    h.send(take());
    h.send(take());
}
"#;
    run_workers_valgrind(src, "55\n7\nempty\n");
}

#[test]
fn receiver_moved_into_a_closure_in_a_loop_and_sender_dropped_in_a_match_arm() {
    let src = r#"fn main() {
    let (tx, rx) = channel::<i64>(4);
    let mut i: i64 = 0;
    let mut tot: i64 = 0;
    while i < 3 {
        let r = rx.clone();
        let t = tx.clone();
        let h = spawn move { t.send(i * 10); match r.recv() { Some(v) => v, None => -1 } };
        tot = tot + h.join();
        i = i + 1;
    }
    println(tot);
    let (tx2, rx2) = channel::<i64>(2);
    let g = spawn move { let mut n: i64 = 0; loop { match rx2.recv() { Some(v) => { n = n + v; } None => { break; } } } n };
    let mut k: i64 = 0;
    while k < 10 {
        match k { 5 => { tx2.send(100); } _ => { tx2.send(k); } }
        k = k + 1;
    }
    match k { 10 => { drop(tx2); } _ => { } }
    println(g.join());
}
"#;
    run_workers(src, "30\n140\n");
}

#[test]
fn select_like_polling_with_try_recv() {
    let src = r#"fn main() {
    let (atx, arx) = channel::<i64>(4);
    let (btx, brx) = channel::<i64>(4);
    let h = spawn move { let mut i: i64 = 0; while i < 3 { atx.send(i); btx.send(i * 100); i = i + 1; } 0 };
    let mut from_a: i64 = 0;
    let mut from_b: i64 = 0;
    while from_a + from_b < 6 {
        match arx.tryRecv() { Some(v) => { from_a = from_a + 1; } None => { } }
        match brx.tryRecv() { Some(v) => { from_b = from_b + 1; } None => { } }
    }
    h.join();
    println(from_a);
    println(from_b);
}
"#;
    run_workers(src, "3\n3\n");
}

#[test]
fn pipeline_with_a_worker_pool_of_os_threads() {
    let src = r#"fn worker(jobs: Receiver<i64>, out: Sender<i64>) -> i64 {
    let mut done: i64 = 0;
    loop { match jobs.recv() { Some(v) => { out.send(v + 1); done = done + 1; } None => { break; } } }
    done
}
fn main() {
    let (jtx, jrx) = channel::<i64>(4);
    let (rtx, rrx) = channel::<i64>(4);
    let j1 = jrx.clone(); let o1 = rtx.clone();
    let j2 = jrx.clone(); let o2 = rtx.clone();
    let w1 = thread::spawnOS(move || worker(j1, o1));
    let w2 = thread::spawnOS(move || worker(j2, o2));
    drop(jrx);
    drop(rtx);
    let feeder = thread::spawnOS(move || { let mut i: i64 = 0; while i < 200 { jtx.send(i); i = i + 1; } 0 });
    let mut sum: i64 = 0;
    loop { match rrx.recv() { Some(v) => { sum = sum + v; } None => { break; } } }
    feeder.join();
    println(w1.join() + w2.join());
    println(sum);
}
"#;
    // numbers 0..199 plus one each: 19,900 + 200
    run_workers(src, "200\n20100\n");
}

#[test]
fn one_million_messages_exact_count_and_sum() {
    let src = r#"fn produce(tx: Sender<i64>, n: i64) -> i64 {
    let mut i: i64 = 1;
    while i <= n { tx.send(i); i = i + 1; }
    n
}
fn main() {
    let (tx, rx) = channel::<i64>();
    let p = spawn move { produce(tx, 1000000) };
    let mut sum: i64 = 0;
    let mut cnt: i64 = 0;
    loop { match rx.recv() { Some(v) => { sum = sum + v; cnt = cnt + 1; } None => { break; } } }
    p.join();
    println(cnt);
    println(sum);
}
"#;
    let exe = build(&write_src(src)).unwrap_or_else(|e| panic!("build failed: {}", e));
    for w in ["1", "4"] {
        let t = std::time::Instant::now();
        let (o, c) = run_exe(&exe, w, false);
        eprintln!("1,000,000 messages (bounded 1024) at MTN_WORKERS={}: {:?}", w, t.elapsed());
        assert_eq!((o.as_str(), c), ("1000000\n500000500000\n", 0));
    }
    // unbounded, producer and consumer on OS threads
    let u = src.replace("channel::<i64>()", "channelUnbounded::<i64>()");
    let exe = build(&write_src(&u)).unwrap_or_else(|e| panic!("build failed: {}", e));
    let t = std::time::Instant::now();
    let (o, c) = run_exe(&exe, "4", false);
    eprintln!("1,000,000 messages (unbounded) at MTN_WORKERS=4: {:?}", t.elapsed());
    assert_eq!((o.as_str(), c), ("1000000\n500000500000\n", 0));
}

// ---------------------------------------------------------------- examples

#[test]
fn example_channel_pipeline() {
    golden("channel_pipeline");
}
#[test]
fn example_channel_close() {
    golden("channel_close");
}
#[test]
fn example_channel_async_bridge() {
    golden("channel_async_bridge");
}

// ---------------------------------------------------------------- rejections

#[test]
fn non_send_payload_is_rejected_when_a_handle_crosses_spawn() {
    rejected(
        "fn main() { let (tx, rx) = channel::<Rc<i32>>(); let h = spawn move { let _t = tx; 1 }; println(h.join()); }",
        "`Sender<Rc<i32>>` is `Send` only when the message type `Rc<i32>` is `Send`",
    );
}
#[test]
fn non_send_payload_is_rejected_when_a_handle_crosses_spawn_os() {
    rejected(
        "fn main() { let (tx, rx) = channel::<Rc<i32>>(); let h = thread::spawnOS(move || { let _t = rx; 1 }); println(h.join()); }",
        "`Receiver<Rc<i32>>` is `Send` only when the message type `Rc<i32>` is `Send`",
    );
}
#[test]
fn non_send_payload_is_rejected_in_an_actor_message() {
    rejected("actor A { on go(t: Sender<Rc<i32>>) { } }\nfn main() { }", "is `Send` only when the message type `Rc<i32>` is `Send`");
}
#[test]
fn box_dyn_without_send_is_rejected_and_with_send_is_fine() {
    rejected(
        "trait Shape { fn area(borrow self) -> i32; }\nfn main() { let (tx, rx) = channel::<Box<dyn Shape>>(); let h = spawn move { let _t = tx; 1 }; println(h.join()); }",
        "write `dyn Shape + Send`",
    );
    let src = "trait Shape { fn area(borrow self) -> i32; }\nstruct Sq { s: i32 }\nimpl Shape for Sq { fn area(borrow self) -> i32 { return self.s; } }\nfn main() { let (tx, rx) = channel::<Box<dyn Shape + Send>>(); let h = spawn move { let b: Box<dyn Shape + Send> = Box::new(Sq { s: 5 }); tx.send(b); 1 }; h.join(); match rx.recv() { Some(b) => println(b.area()), None => println(0) } }";
    run_workers(src, "5\n");
}
#[test]
fn use_after_send_of_a_moved_value_is_rejected() {
    check_and_build_agree("fn main() { let (tx, rx) = channel::<Box<i32>>(); let b = Box::new(5); tx.send(b); println(*b); }", "use of moved value `b`");
}
#[test]
fn use_of_a_moved_handle_is_rejected() {
    check_and_build_agree("fn main() { let (tx, rx) = channel::<i64>(); let t2 = tx; tx.send(1); }", "use of moved value `tx`");
}
#[test]
fn a_handle_moved_into_spawn_cannot_be_used_afterwards() {
    check_and_build_agree("fn main() { let (tx, rx) = channel::<i64>(); let h = spawn move { tx.send(1); 0 }; tx.send(2); h.join(); }", "use of moved value `tx`");
}
#[test]
fn recv_without_await_inside_an_async_fn_is_rejected() {
    rejected(
        "async fn f(rx: Receiver<i64>) -> i64 { match rx.recv() { Some(v) => v, None => 0 } }\nfn main() { }",
        "`recv` blocks the worker: inside an `async fn` write `await rx.recv()`",
    );
}
#[test]
fn send_without_await_inside_an_async_fn_is_rejected() {
    rejected(
        "async fn f(tx: Sender<i64>) { tx.send(1); }\nfn main() { }",
        "`send` blocks the worker: inside an `async fn` write `await tx.send(v)`",
    );
}
#[test]
fn trying_forms_are_fine_inside_an_async_fn() {
    let src = "async fn f(tx: Sender<i64>, rx: Receiver<i64>) -> i64 { tx.trySend(1); match rx.tryRecv() { Some(v) => v, None => 0 } }\nasync fn main() { let (tx, rx) = channel::<i64>(2); println(await f(tx, rx)); }";
    run_workers(src, "1\n");
}
#[test]
fn wrong_element_type_is_rejected() {
    rejected("fn main() { let (tx, rx) = channel::<i64>(); tx.send(String::from(\"x\")); }", "expected `i64`, found `String`");
}
#[test]
fn the_message_type_is_required() {
    rejected("fn main() { let c = channel(); }", "`channel` needs its message type: write `channel::<T>()`");
}
#[test]
fn unknown_channel_methods_and_bad_argument_counts_are_rejected() {
    rejected("fn main() { let (tx, rx) = channel::<i64>(); tx.recv(); }", "no method named `recv` on `Sender<i64>`");
    rejected("fn main() { let (tx, rx) = channel::<i64>(); rx.send(1); }", "no method named `send` on `Receiver<i64>`");
    rejected("fn main() { let (tx, rx) = channel::<i64>(); tx.send(); }", "`send` takes exactly one argument");
    rejected("fn main() { let (tx, rx) = channelUnbounded::<i64>(5); }", "`channelUnbounded::<T>()` takes no arguments");
}
#[test]
fn capacity_zero_panics_with_exit_code_101() {
    let exe = build(&write_src("fn main() { println(\"start\"); let (tx, rx) = channel::<i64>(0); println(\"unreachable\"); }")).unwrap();
    let out = Command::new(&exe).output().unwrap();
    assert_eq!(out.status.code(), Some(101));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "start\n");
    assert!(String::from_utf8_lossy(&out.stderr).contains("channel capacity must be at least 1"), "{}", String::from_utf8_lossy(&out.stderr));
}
#[test]
fn a_capacity_held_in_a_variable_is_checked_too() {
    let exe = build(&write_src("fn main() { let n: usize = 0; let (tx, rx) = channel::<i64>(n); }")).unwrap();
    let out = Command::new(&exe).output().unwrap();
    assert_eq!(out.status.code(), Some(101));
    assert!(String::from_utf8_lossy(&out.stderr).contains("channel capacity must be at least 1"));
}
#[test]
fn a_signed_capacity_is_rejected_at_compile_time() {
    rejected("fn main() { let n: i64 = 3; let (tx, rx) = channel::<i64>(n); }", "expected `usize`, found `i64`");
}
#[test]
fn a_destructured_handle_pair_can_be_captured_by_a_closure() {
    // a tuple `let` binds names the closure analysis must see (a pre-existing gap found by this milestone)
    run_workers("fn main() { let (a, b) = (7, 1); let h = spawn move { a + b }; println(h.join()); }", "8\n");
}
