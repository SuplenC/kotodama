//! How long simple arithmetic takes to be answered: the real Daemon, run in
//! this process, with the real calculator Extension as its child, and a
//! Front end on its socket. Each query is timed from the Front end sending it
//! to the Front end receiving the Results that lead with the answer, which
//! bounds the time from the keystroke reaching the Daemon to the answer
//! being sent. The p99 must stay under 8 ms, one frame at 120 Hz.

use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use kotodama_extension::testing;
use kotodama_protocol::{FromFrontEnd, ToFrontEnd};
use kotodamad::paths::Paths;

const QUERIES: usize = 1_000;
/// One frame at 120 Hz.
const BUDGET: Duration = Duration::from_millis(8);

fn main() {
    let dir = tempfile::tempdir().unwrap();
    let path = |sub: &str| dir.path().join(sub);
    fs::create_dir_all(path("config")).unwrap();
    // The Bundled extension's own Manifest, run from where cargo built it.
    testing::install(
        &path("extensions"),
        "calculator",
        &testing::bundled(
            include_str!("../manifest.toml"),
            env!("CARGO_BIN_EXE_kotodama-calculator"),
        ),
    );

    let paths = Paths {
        socket: path("kotodama.sock"),
        config_dirs: vec![path("config")],
        extension_dirs: vec![path("extensions")],
        history: path("state/history.json"),
        usage_log: path("state/usage.jsonl"),
        cache: path("cache"),
    };
    let socket = paths.socket.clone();
    thread::spawn(move || kotodamad::run(&paths));

    // A read that times out ends a wait for an answer that isn't coming.
    let mut ui = testing::connect(&socket, Duration::from_secs(1));
    // Sends `query` and waits for the answer to lead the Results; false if
    // it doesn't within a second.
    let mut answered = |query: &str| -> bool {
        ui.send(&FromFrontEnd::Query { text: query.into() })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            let Ok(Some(message)) = ui.recv::<ToFrontEnd>() else {
                return false;
            };
            if let ToFrontEnd::Results { results } = message
                && results
                    .first()
                    .is_some_and(|row| row.extension == "calculator")
            {
                return true;
            }
        }
        false
    };
    // The calculator is up once it answers.
    let started = Instant::now();
    while !answered("1+1") {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the calculator never answered"
        );
    }

    let mut times: Vec<Duration> = (0..QUERIES)
        .map(|n| {
            let query = format!("{n}+{}", n % 7 + 1);
            let start = Instant::now();
            assert!(answered(&query), "no answer for {query}");
            start.elapsed()
        })
        .collect();
    times.sort();
    let p = |q: f64| times[((times.len() as f64 * q) as usize).min(times.len() - 1)];
    println!(
        "calculator, {QUERIES} answers over the socket: p50 {:?}, p99 {:?}, max {:?}",
        p(0.5),
        p(0.99),
        times[times.len() - 1]
    );
    println!(
        "test calculator/answer/p99 ... bench: {} ns/iter (+/- 0)",
        p(0.99).as_nanos()
    );
    match calculator_resident() {
        Some(bytes) => {
            println!(
                "calculator, after {QUERIES} answers: {:.1} MiB resident",
                bytes as f64 / (1 << 20) as f64
            );
            println!("test calculator/resident ... bench: {bytes} bytes (+/- 0)");
        }
        None => println!("resident memory is only measured on Linux"),
    }
    assert!(
        p(0.99) < BUDGET,
        "simple arithmetic took {:?} at p99 to be answered, over the {BUDGET:?} budget",
        p(0.99)
    );
}

/// The calculator's `VmRSS`, in bytes: the child of this process, which the
/// Daemon started, named after its binary (cut to the kernel's 15 bytes).
fn calculator_resident() -> Option<u64> {
    let tasks = fs::read_dir("/proc/self/task").ok()?;
    let pid = tasks
        .flatten()
        .filter_map(|task| fs::read_to_string(task.path().join("children")).ok())
        .flat_map(|children| {
            children
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .find(|pid| {
            fs::read_to_string(format!("/proc/{pid}/comm"))
                .is_ok_and(|comm| "kotodama-calculator".starts_with(comm.trim()))
        })?;
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}
