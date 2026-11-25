//! Phase 13 M4: `select` over `rx.recv()` arms (Document 12 §6.1, §11.13) and the `join` fix (register 52).
//! Every program is built in its own unique directory; every run has a watchdog, so a hang FAILS the test.
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mtn_p13m4_{}_{}_{}", std::process::id(), tag, COUNTER.fetch_add(1, Ordering::SeqCst)));
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

/// Runs the program with a watchdog (`limit` seconds): a hang kills the child and fails the test.
fn run_exe_limit(exe: &std::path::Path, workers: &str, valgrind: bool, limit: u64) -> (String, i32) {
    let mut cmd = if valgrind {
        let mut c = Command::new("valgrind");
        c.args(["--leak-check=full", "--errors-for-leak-kinds=definite", "--error-exitcode=99", "-q"]).arg(exe);
        c
    } else {
        Command::new(exe)
    };
    let mut child = cmd.env("MTN_WORKERS", workers).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
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
            let _ = t2.join();
            return (out, st.code().unwrap_or(-1));
        }
        if start.elapsed() > Duration::from_secs(limit) {
            let _ = child.kill();
            let _ = child.wait();
            let out = t1.join().unwrap_or_default();
            panic!("HANG: the program did not finish within {}s at MTN_WORKERS={} (partial output: {:?})", limit, workers, out);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn run_exe(exe: &std::path::Path, workers: &str, valgrind: bool) -> (String, i32) {
    run_exe_limit(exe, workers, valgrind, if valgrind { 600 } else { 120 })
}

fn rejected(src: &str, needle: &str) {
    match build(&write_src(src)) {
        Ok(_) => panic!("should have been rejected (`{}`):\n{}", needle, src),
        Err(m) => assert!(m.contains(needle), "expected `{}` in:\n{}", needle, m),
    }
    // `mtnc check` agrees with `mtnc build`
    let f = write_src(src);
    let chk = Command::new(env!("CARGO_BIN_EXE_mtnc")).arg("check").arg(&f).output().unwrap();
    let all = String::from_utf8_lossy(&chk.stdout).to_string() + &String::from_utf8_lossy(&chk.stderr);
    assert!(!chk.status.success() && all.contains(needle), "check should reject with `{}`:\n{}", needle, all);
}

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

fn golden(name: &str) {
    let src = root().join("examples/concurrency").join(format!("{}.mtn", name));
    let want = std::fs::read_to_string(root().join("tests/golden13").join(format!("{}.out", name))).unwrap();
    let exe = build(&src).unwrap_or_else(|e| panic!("build of {} failed: {}", name, e));
    for w in ["1", "2", "4"] {
        let (o, c) = run_exe(&exe, w, false);
        assert_eq!((o.as_str(), c), (want.as_str(), 0), "{} at MTN_WORKERS={}", name, w);
    }
    if have_valgrind() {
        for w in ["1", "4"] {
            let (o, c) = run_exe(&exe, w, true);
            assert_eq!((o.as_str(), c), (want.as_str(), 0), "{} under valgrind at MTN_WORKERS={}", name, w);
        }
    }
}

// ---------------------------------------------------------------- register 52: join never runs unrelated work

#[test]
fn join_inversion_inv2_does_not_hang() {
    // `a` joins `d` (still queued, so it runs `d` itself) while `c`, queued earlier, blocks in `recv` until `a`
    // sends. Before the fix `join` ran `c` on `a`'s own stack and deadlocked.
    let src = r#"fn main() {
    let (tx, rx) = channel::<i64>(1);
    let a = spawn move {
        let d = spawn move { let o: i64 = 1; o };
        let c = spawn move { match rx.recv() { Some(v) => v, None => -1 } };
        let x = d.join();
        tx.send(5);
        c.join() + x
    };
    println(a.join());
}
"#;
    run_workers(src, "6\n");
}

#[test]
fn join_of_a_running_task_waits_and_deep_recursive_joins_work() {
    let src = r#"fn fib(n: i64) -> i64 {
    if n < 2 { return n; }
    let h = spawn move { fib(n - 1) };
    let b = fib(n - 2);
    h.join() + b
}
fn deep(n: i64) -> i64 {
    if n == 0 { return 0; }
    let h = spawn move { deep(n - 1) };
    h.join() + 1
}
fn main() {
    println(fib(18));
    println(deep(300));
}
"#;
    run_workers(src, "2584\n300\n");
}

// ---------------------------------------------------------------- semantics

#[test]
fn first_ready_arm_in_source_order_and_all_closed_runs_the_first_arm_with_none() {
    let src = r#"fn main() {
    let (atx, arx) = channel::<i64>(4);
    let (btx, brx) = channel::<i64>(4);
    btx.send(7);
    atx.send(1);
    let r = select {
        case a = <-arx: match a { Some(v) => v, None => -1 },
        case b = <-brx: match b { Some(v) => v * 100, None => -2 },
    };
    println(r);
    let r2 = select {
        case a = <-arx: match a { Some(v) => v, None => -1 },
        case b = <-brx: match b { Some(v) => v * 100, None => -2 },
    };
    println(r2);
    drop(atx);
    drop(btx);
    let r3 = select {
        case a = <-arx: match a { Some(v) => v, None => -1 },
        case b = <-brx: match b { Some(v) => v * 100, None => -2 },
    };
    println(r3);
}
"#;
    run_workers(src, "1\n700\n-1\n");
}

#[test]
fn a_closed_channel_gives_its_pending_values_first_then_none() {
    let src = r#"fn main() {
    let (tx, rx) = channel::<i64>(4);
    let (_ktx, krx) = channel::<i64>(4);
    tx.send(1);
    tx.send(2);
    tx.send(3);
    drop(tx);
    let mut n: i64 = 0;
    while n < 4 {
        let v = select {
            case a = <-rx: match a { Some(x) => x, None => -1 },
            case b = <-krx: match b { Some(x) => x + 1000, None => -2 },
        };
        println(v);
        n = n + 1;
    }
}
"#;
    run_workers(src, "1\n2\n3\n-1\n");
}

#[test]
fn the_winner_is_never_lost_or_duplicated_and_losing_arms_consume_nothing() {
    // both ready: arm one wins; the other channel still holds its value, and the receivers are still usable
    // afterwards (an arm borrows its receiver, it does not move it).
    let src = r#"fn main() {
    let (atx, arx) = channel::<i64>(4);
    let (btx, brx) = channel::<i64>(4);
    atx.send(10);
    atx.send(11);
    btx.send(20);
    let r = select {
        case a = <-arx: match a { Some(v) => v, None => -1 },
        case b = <-brx: match b { Some(v) => v, None => -2 },
    };
    println(r);
    println(match arx.tryRecv() { Some(v) => v, None => -1 });
    println(match arx.tryRecv() { Some(v) => v, None => -1 });
    println(match brx.tryRecv() { Some(v) => v, None => -1 });
    println(match brx.tryRecv() { Some(v) => v, None => -1 });
    println(match arx.tryRecv() { Some(v) => v, None => -1 });
    brx.recv();
}
"#;
    // brx.recv() would block forever; replace with a final send/recv pair that returns
    let src = src.replace("    brx.recv();\n", "    btx.send(30);\n    println(match brx.recv() { Some(v) => v, None => -1 });\n");
    run_workers(&src, "10\n11\n-1\n20\n-1\n-1\n30\n");
}

#[test]
fn a_select_arm_borrows_its_receiver_a_loop_may_select_again() {
    let src = r#"fn main() {
    let (atx, arx) = channel::<i64>(8);
    let (btx, brx) = channel::<i64>(8);
    let mut i: i64 = 0;
    while i < 6 {
        if i % 2 == 0 { atx.send(i); } else { btx.send(i); }
        i = i + 1;
    }
    let mut sum: i64 = 0;
    let mut n: i64 = 0;
    while n < 6 {
        sum = sum + select {
            case a = <-arx: match a { Some(v) => v, None => 0 },
            case b = <-brx: match b { Some(v) => v, None => 0 },
        };
        n = n + 1;
    }
    println(sum);
}
"#;
    run_workers(src, "15\n");
}

#[test]
fn three_arms_and_receivers_in_arc_and_struct_fields() {
    let src = r#"struct Ports { a: Receiver<i64>, b: Receiver<i64>, c: Receiver<i64> }
fn main() {
    let (atx, arx) = channel::<i64>(2);
    let (btx, brx) = channel::<i64>(2);
    let (ctx, crx) = channel::<i64>(2);
    let p = Ports { a: arx, b: brx, c: crx };
    ctx.send(3);
    let r = select {
        case x = <-p.a: match x { Some(v) => v, None => -1 },
        case y = <-p.b: match y { Some(v) => v + 10, None => -1 },
        case z = <-p.c: match z { Some(v) => v + 100, None => -1 },
    };
    println(r);
    btx.send(4);
    atx.send(5);
    let r2 = select {
        case x = <-p.a: match x { Some(v) => v, None => -1 },
        case y = <-p.b: match y { Some(v) => v + 10, None => -1 },
        case z = <-p.c: match z { Some(v) => v + 100, None => -1 },
    };
    println(r2);
    let r3 = select {
        case x = <-p.a: match x { Some(v) => v, None => -1 },
        case _ = <-p.b: 77,
        case z = <-p.c: match z { Some(v) => v + 100, None => -1 },
    };
    println(r3);
}
"#;
    run_workers(src, "103\n5\n77\n");
}

// ---------------------------------------------------------------- no lost wakeups

/// The producer is an OS thread; the consumer is a task blocked in `select` (or about to block). Round `i`:
/// the consumer acknowledges the previous value before the producer sends the next one, to ONE channel only
/// (alternating), so the wake-up must come from the channel the consumer registered on second.
fn lost_wakeup_src(asyn: bool) -> String {
    let (a, aw, sel) = if asyn { ("async ", "await ", "await select") } else { ("", "", "select") };
    format!(
        r#"fn feed(a: Sender<i64>, b: Sender<i64>, ack: Receiver<i64>, rounds: i64) -> i64 {{
    let mut i: i64 = 0;
    while i < rounds {{
        match ack.recv() {{ Some(_) => {{}} None => {{ return -1; }} }}
        if i % 2 == 0 {{ b.send(i); }} else {{ a.send(i); }}
        i = i + 1;
    }}
    // both at once
    match ack.recv() {{ Some(_) => {{}} None => {{ return -1; }} }}
    a.send(1000000);
    b.send(2000000);
    // keep both channels open until the consumer has taken both values (a closed first arm would win)
    match ack.recv() {{ Some(_) => {{}} None => {{ return -1; }} }}
    rounds
}}
{a}fn eat(arx: Receiver<i64>, brx: Receiver<i64>, ack: Sender<i64>, rounds: i64) -> i64 {{
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < rounds {{
        {aw}ack.send(0);
        sum = sum + {sel} {{
            case x = <-arx: match x {{ Some(v) => v, None => -100000000 }},
            case y = <-brx: match y {{ Some(v) => v, None => -100000000 }},
        }};
        i = i + 1;
    }}
    {aw}ack.send(0);
    let mut k: i64 = 0;
    while k < 2 {{
        sum = sum + {sel} {{
            case x = <-arx: match x {{ Some(v) => v, None => -100000000 }},
            case y = <-brx: match y {{ Some(v) => v, None => -100000000 }},
        }};
        k = k + 1;
    }}
    {aw}ack.send(0);
    sum
}}
fn main() {{
    let (atx, arx) = channel::<i64>(1);
    let (btx, brx) = channel::<i64>(1);
    let (ktx, krx) = channel::<i64>(1);
    let t = thread::spawnOS(move || feed(atx, btx, krx, 3000));
    let c = spawn move {{ eat(arx, brx, ktx, 3000) }};
    println(c.join());
    println(t.join());
}}
"#,
        a = a,
        aw = aw,
        sel = sel
    )
}

#[test]
fn no_lost_wakeup_message_arrives_on_the_second_channel_only_sync() {
    let sum: i64 = (0..3000).sum::<i64>() + 3000000;
    run_workers(&lost_wakeup_src(false), &format!("{}\n3000\n", sum));
}

#[test]
fn no_lost_wakeup_message_arrives_on_the_second_channel_only_async() {
    let sum: i64 = (0..3000).sum::<i64>() + 3000000;
    let src = lost_wakeup_src(true).replace("spawn move { eat(arx, brx, ktx, 3000) }", "spawn eat(arx, brx, ktx, 3000)");
    assert!(src.contains("spawn eat("));
    run_workers(&src, &format!("{}\n3000\n", sum));
}

// ---------------------------------------------------------------- liveness (a), (b), (c)

const LIVE_A: &str = r#"fn prod(tx: Sender<i64>, base: i64, n: i64) -> i64 {
    let mut k: i64 = 0;
    while k < n {
        tx.send(base + k);
        k = k + 1;
    }
    n
}
fn drain(rx: Receiver<i64>) -> i64 {
    let mut s: i64 = 0;
    loop {
        match rx.recv() {
            Some(v) => { s = s + v; }
            None => { break; }
        }
    }
    s
}
fn consume(arx: Receiver<i64>, brx: Receiver<i64>) -> i64 {
    let mut sum: i64 = 0;
    loop {
        select {
            case a = <-arx: match a {
                Some(v) => { sum = sum + v; }
                None => { sum = sum + drain(brx); break; }
            },
            case b = <-brx: match b {
                Some(v) => { sum = sum + v; }
                None => { sum = sum + drain(arx); break; }
            },
        }
    }
    sum
}
fn main() {
    let (atx, arx) = channel::<i64>(1);
    let (btx, brx) = channel::<i64>(1);
    let pa = spawn move { prod(atx, 0, 1000) };
    let pb = spawn move { prod(btx, 1000000, 1000) };
    let c = spawn move { consume(arx, brx) };
    pa.join();
    pb.join();
    println(c.join());
}
"#;

#[test]
fn liveness_a_select_consumer_over_two_channels_with_two_producer_tasks() {
    run_workers(LIVE_A, "1000999000\n");
}

#[test]
fn liveness_b_select_fed_by_an_os_thread_and_a_task() {
    let src = LIVE_A.replace("let pa = spawn move { prod(atx, 0, 1000) };", "let pa = thread::spawnOS(move || prod(atx, 0, 1000));");
    assert!(src.contains("spawnOS"));
    run_workers(&src, "1000999000\n");
}

#[test]
fn liveness_c_async_await_select_with_async_producers() {
    let src = r#"async fn prod(tx: Sender<i64>, base: i64, n: i64) -> i64 {
    let mut k: i64 = 0;
    while k < n {
        await tx.send(base + k);
        k = k + 1;
    }
    n
}
async fn drain(rx: Receiver<i64>) -> i64 {
    let mut s: i64 = 0;
    loop {
        match await rx.recv() {
            Some(v) => { s = s + v; }
            None => { break; }
        }
    }
    s
}
async fn consume(arx: Receiver<i64>, brx: Receiver<i64>) -> i64 {
    let mut sum: i64 = 0;
    loop {
        await select {
            case a = <-arx: match a {
                Some(v) => { sum = sum + v; }
                None => { sum = sum + await drain(brx); break; }
            },
            case b = <-brx: match b {
                Some(v) => { sum = sum + v; }
                None => { sum = sum + await drain(arx); break; }
            },
        }
    }
    sum
}
async fn main() {
    let (atx, arx) = channel::<i64>(1);
    let (btx, brx) = channel::<i64>(1);
    let pa = spawn prod(atx, 0, 1000);
    let pb = spawn prod(btx, 1000000, 1000);
    let c = spawn consume(arx, brx);
    await pa;
    await pb;
    println(await c);
}
"#;
    run_workers(src, "1000999000\n");
}

#[test]
fn async_select_holds_no_worker_many_selecting_tasks_with_one_worker() {
    // 50 tasks all suspended in `await select` on their own pair, one worker: they must not each hold it.
    let src = r#"async fn waiter(arx: Receiver<i64>, brx: Receiver<i64>) -> i64 {
    await select {
        case a = <-arx: match a { Some(v) => v, None => -1 },
        case b = <-brx: match b { Some(v) => v + 1000, None => -2 },
    }
}
async fn one(i: i64) -> i64 {
    let (atx, arx) = channel::<i64>(1);
    let (btx, brx) = channel::<i64>(1);
    let w = spawn waiter(arx, brx);
    await btx.send(i);
    let r = await w;
    drop(atx);
    r
}
async fn many(i: i64, n: i64) -> i64 {
    if i >= n { return 0; }
    let h = spawn one(i);
    let rest = await many(i + 1, n);
    rest + await h
}
async fn main() {
    println(await many(0, 50));
}
"#;
    let want: i64 = (0..50).map(|i| i + 1000).sum();
    run_workers(src, &format!("{}\n", want));
}

// ---------------------------------------------------------------- stress

/// `chans` channels with `p` producer tasks each, 100,000 messages in all (the remainder of the division goes
/// to the first producers), one select consumer; unique ids, checked by count, sum and a sum of squares.
fn stress_src(chans: i64, p: i64) -> (String, String) {
    let total = 100_000i64;
    let producers = chans * p;
    let (n, rem) = (total / producers, total % producers);
    let mut arms = String::new();
    for i in 0..chans {
        let others: Vec<i64> = (0..chans).filter(|x| *x != i).collect();
        let mut drains = String::new();
        for o in &others {
            drains.push_str(&format!("drain(r{o}, sum.clone(), sq.clone(), cnt.clone()); ", o = o));
        }
        arms.push_str(&format!(
            "            case m{i} = <-r{i}: match m{i} {{\n                Some(v) => {{ sum.fetchAdd(v, Ordering::SeqCst); sq.fetchAdd((v % 1000003) * (v % 1000003), Ordering::SeqCst); cnt.fetchAdd(1, Ordering::SeqCst); }}\n                None => {{ {drains}break; }}\n            }},\n",
            i = i,
            drains = drains
        ));
    }
    let params: String = (0..chans).map(|i| format!("r{}: Receiver<i64>", i)).collect::<Vec<_>>().join(", ");
    let mut mainb = String::new();
    for i in 0..chans {
        mainb.push_str(&format!("    let (t{i}, r{i}) = channel::<i64>(16);\n", i = i));
    }
    mainb.push_str("    let sum: Arc<Atomic<i64>> = Arc::new(Atomic::new(0));\n    let sq: Arc<Atomic<i64>> = Arc::new(Atomic::new(0));\n    let cnt: Arc<Atomic<i64>> = Arc::new(Atomic::new(0));\n");
    let args: String = (0..chans).map(|i| format!("r{}", i)).collect::<Vec<_>>().join(", ");
    mainb.push_str(&format!("    let cs = sum.clone();\n    let cq = sq.clone();\n    let cn = cnt.clone();\n    let c = spawn move {{ consume({args}, cs, cq, cn) }};\n", args = args));
    for i in 0..chans {
        mainb.push_str(&format!("    let s{i} = spawn move {{ spawn_all(t{i}, {i}, 0, {p}, {g0}, {n}, {rem}) }};\n", i = i, p = p, g0 = i * p, n = n, rem = rem));
    }
    for i in 0..chans {
        mainb.push_str(&format!("    s{}.join();\n", i));
    }
    mainb.push_str("    c.join();\n    println(cnt.load());\n    println(sum.load());\n    println(sq.load());\n");
    let src = format!(
        r#"fn prod(tx: Sender<i64>, base: i64, n: i64) -> i64 {{
    let mut k: i64 = 0;
    while k < n {{
        tx.send(base + k);
        k = k + 1;
    }}
    n
}}
fn spawn_all(tx: Sender<i64>, chan: i64, i: i64, p: i64, g0: i64, n: i64, rem: i64) -> i64 {{
    if i >= p {{ return 0; }}
    let t = tx.clone();
    let g = g0 + i;
    let cnt = if g < rem {{ n + 1 }} else {{ n }};
    let base = chan * 10000000 + i * 100000;
    let h = spawn move {{ prod(t, base, cnt) }};
    let rest = spawn_all(tx, chan, i + 1, p, g0, n, rem);
    h.join() + rest
}}
fn drain(rx: Receiver<i64>, sum: Arc<Atomic<i64>>, sq: Arc<Atomic<i64>>, cnt: Arc<Atomic<i64>>) -> i64 {{
    loop {{
        match rx.recv() {{
            Some(v) => {{ sum.fetchAdd(v, Ordering::SeqCst); sq.fetchAdd((v % 1000003) * (v % 1000003), Ordering::SeqCst); cnt.fetchAdd(1, Ordering::SeqCst); }}
            None => {{ break; }}
        }}
    }}
    0
}}
fn consume({params}, sum: Arc<Atomic<i64>>, sq: Arc<Atomic<i64>>, cnt: Arc<Atomic<i64>>) -> i64 {{
    loop {{
        select {{
{arms}        }}
    }}
    0
}}
fn main() {{
{mainb}}}
"#,
        params = params,
        arms = arms,
        mainb = mainb
    );
    let (mut cnt, mut sum, mut sq) = (0i64, 0i64, 0i64);
    for c in 0..chans {
        for i in 0..p {
            let g = c * p + i;
            let count = if g < rem { n + 1 } else { n };
            for k in 0..count {
                let v = c * 10_000_000 + i * 100_000 + k;
                cnt += 1;
                sum += v;
                sq += (v % 1000003) * (v % 1000003);
            }
        }
    }
    assert_eq!(cnt, total);
    (src, format!("{}\n{}\n{}\n", cnt, sum, sq))
}

fn stress(chans: i64, p: i64) {
    let (src, want) = stress_src(chans, p);
    let exe = build(&write_src(&src)).unwrap_or_else(|e| panic!("build failed: {}\n{}", e, src));
    for w in ["1", "2", "4"] {
        let (o, c) = run_exe(&exe, w, false);
        assert_eq!((o.as_str(), c), (want.as_str(), 0), "{} channels x {} producers, MTN_WORKERS={}", chans, p, w);
    }
}

#[test]
fn stress_2_channels_1_producer_each() { stress(2, 1) }
#[test]
fn stress_2_channels_4_producers_each() { stress(2, 4) }
#[test]
fn stress_2_channels_8_producers_each() { stress(2, 8) }
#[test]
fn stress_3_channels_1_producer_each() { stress(3, 1) }
#[test]
fn stress_3_channels_4_producers_each() { stress(3, 4) }
#[test]
fn stress_3_channels_8_producers_each() { stress(3, 8) }

// ---------------------------------------------------------------- Drop payloads

#[test]
fn a_drop_payload_through_select_is_dropped_exactly_once() {
    let src = r#"struct Loud { id: i64 }
impl Drop for Loud { fn drop(borrow mut self) { print("drop "); println(self.id); } }
fn main() {
    let (atx, arx) = channel::<Loud>(4);
    let (btx, brx) = channel::<Loud>(4);
    atx.send(Loud { id: 1 });
    btx.send(Loud { id: 2 });
    atx.send(Loud { id: 3 });
    select {
        case a = <-arx: match a { Some(l) => { print("a got "); println(l.id); } None => {} },
        case b = <-brx: match b { Some(l) => { print("b got "); println(l.id); } None => {} },
    }
    select {
        case _ = <-arx: println("a ignored"),
        case b = <-brx: match b { Some(l) => { print("b got "); println(l.id); } None => {} },
    }
    println("end");
}
"#;
    // 1 is dropped at the end of its arm; `_` drops 3 at once; 2 is dropped after `got`; nothing is dropped twice
    run_workers_valgrind(src, "a got 1\ndrop 1\na ignored\ndrop 3\nend\ndrop 2\n");
}

// ---------------------------------------------------------------- rejections

#[test]
fn rejects_an_arm_that_is_not_a_receiver() {
    rejected("fn main() { let x: i64 = 1; select { case v = <-x: v, } }", "Receiver");
    rejected("fn main() { let (tx, rx) = channel::<i64>(1); select { case v = <-tx: 1, } }", "Receiver");
}

#[test]
fn rejects_select_with_zero_arms() {
    rejected("fn main() { select { } }", "at least one");
}

#[test]
fn rejects_a_wrong_arm_binding_type() {
    // `v` is an `Option<i64>`, not an `i64`
    rejected("fn main() { let (tx, rx) = channel::<i64>(1); tx.send(1); let r: i64 = select { case v = <-rx: v, }; println(r); }", "Option");
    rejected("fn main() { let (tx, rx) = channel::<i64>(1); tx.send(1); select { case v = <-rx: match v { Some(s) => { let t: String = s; println(t); } None => {} }, } }", "String");
}

#[test]
fn rejects_missing_await_in_an_async_fn() {
    rejected(
        "async fn f(rx: Receiver<i64>) -> i64 { select { case v = <-rx: match v { Some(x) => x, None => 0 }, } }\nfn main() {}",
        "await select",
    );
}

#[test]
fn rejects_a_receiver_used_after_it_moved_into_a_task() {
    rejected(
        "fn main() { let (tx, rx) = channel::<i64>(1); let h = spawn move { match rx.recv() { Some(v) => v, None => 0 } }; select { case v = <-rx: 1, } tx.send(1); h.join(); }",
        "moved",
    );
}

#[test]
fn rejects_arms_with_different_types() {
    rejected(
        "fn main() { let (tx, rx) = channel::<i64>(1); let (ty, ry) = channel::<i64>(1); tx.send(1); ty.send(1); let r = select { case a = <-rx: 1, case b = <-ry: \"x\", }; }",
        "incompatible",
    );
}

#[test]
fn rejects_send_arms_default_arm_and_non_simple_patterns() {
    rejected("fn main() { let (tx, rx) = channel::<i64>(1); select { case tx <- 1: 1, } }", "send arms");
    rejected("fn main() { let (tx, rx) = channel::<i64>(1); select { case v = <-rx: 1, default: 2, } }", "default");
    rejected("fn main() { let (tx, rx) = channel::<i64>(1); tx.send(1); select { case Some(v) = <-rx: v, } }", "binds the whole");
}

// ---------------------------------------------------------------- examples

#[test]
fn example_select_two_sources() { golden("select_two_sources") }

#[test]
fn example_select_async() { golden("select_async") }
