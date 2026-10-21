//! The real `kotodamad` with 10k Indexed items from a real Extension process:
//! the p99 round trip of a query over the socket, and the Daemon's resident
//! memory once it has indexed them and answered queries.
//!
//! The memory line uses the bencher format with `bytes` as its unit, so CI
//! compares it with the baseline like a timing (see `scripts/bench-compare`).
//! The Extension is this same executable, run with `KOTODAMA_BENCH_EXTENSION`.

use std::fs;
use std::io::stdin;
use std::io::stdout;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use kotodama_extension::testing;
use kotodama_protocol::Channel;
use kotodama_protocol::FromExtension;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::Hello;
use kotodama_protocol::ToExtension;
use kotodama_protocol::ToFrontEnd;
use kotodama_protocol::WireFormat;

mod corpus;

const QUERIES: usize = 2_000;
const TYPED: &str = "firefoxterminal";

fn main() {
    if std::env::var_os("KOTODAMA_BENCH_EXTENSION").is_some() {
        return extension();
    }
    let dir = tempfile::tempdir().unwrap();
    let path = |sub: &str| dir.path().join(sub);
    fs::create_dir_all(path("runtime")).unwrap();
    let me = std::env::current_exe().unwrap();
    testing::install(
        &path("config/kotodama/extensions"),
        "bench",
        &format!("name = \"bench\"\ncommand = [{:?}]", me.display().to_string()),
    );

    let mut daemon = Command::new(env!("CARGO_BIN_EXE_kotodamad"))
        .env("XDG_RUNTIME_DIR", path("runtime"))
        .env("XDG_CONFIG_HOME", path("config"))
        .env("XDG_CONFIG_DIRS", path("etc"))
        .env("XDG_DATA_DIRS", path("share"))
        .env("XDG_CACHE_HOME", path("cache"))
        .env("KOTODAMA_BENCH_EXTENSION", "1")
        .env("KOTODAMA_LOG", "warn")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();

    let socket = path("runtime/kotodama.sock");
    let mut ui = testing::connect(&socket, Duration::from_secs(10));
    let mut query = |text: &str| -> usize {
        ui.send(&FromFrontEnd::Query { text: text.into() }).unwrap();
        loop {
            if let ToFrontEnd::Results { results, .. } = ui.recv().unwrap().unwrap() {
                return results.len();
            }
        }
    };
    // Every item has the keyword `app<n>`, so this finds the last one once all are in.
    let last = format!("'app{}", corpus::ITEMS - 1);
    while query(&last) == 0 {
        thread::sleep(Duration::from_millis(10));
    }

    let mut times: Vec<Duration> = (0..QUERIES)
        .map(|n| {
            let typed = &TYPED[..=n % TYPED.len()];
            let start = Instant::now();
            query(typed);
            start.elapsed()
        })
        .collect();
    times.sort();
    let p = |q: f64| times[((times.len() as f64 * q) as usize).min(times.len() - 1)];
    println!(
        "kotodamad, {} items, {QUERIES} queries over the socket: p50 {:?}, p99 {:?}, max {:?}",
        corpus::ITEMS,
        p(0.5),
        p(0.99),
        times[times.len() - 1]
    );

    match resident(daemon.id()) {
        Some(bytes) => {
            println!(
                "kotodamad, {} items: {:.1} MiB resident",
                corpus::ITEMS,
                bytes as f64 / f64::from(1 << 20)
            );
            println!("test daemon/10k_items/resident ... bench: {bytes} bytes (+/- 0)");
        }
        None => println!("resident memory is only measured on Linux"),
    }
    let _ = daemon.kill();
    let _ = daemon.wait();
}

/// `VmRSS` from `/proc/<pid>/status`, in bytes.
fn resident(pid: u32) -> Option<u64> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

/// The Extension side: sends the corpus, then idles until the Daemon hangs up.
fn extension() {
    let mut channel = Channel::handshake(stdin().lock(), stdout().lock(), &Hello::new(&WireFormat::ALL)).unwrap();
    let mut sent = false;
    while let Ok(Some(message)) = channel.recv::<ToExtension>() {
        if matches!(message, ToExtension::Config(_)) && !sent {
            sent = true;
            channel
                .send(&FromExtension::AddItems { items: corpus::items() })
                .unwrap();
        }
    }
}
