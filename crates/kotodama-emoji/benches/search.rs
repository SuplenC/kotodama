//! How long a keystroke takes to be answered with the emoji indexed: the real
//! Daemon, run in this process, with the real emoji Extension as its child,
//! and a Front end on its socket. Each query is timed from the Front end
//! sending it to the Front end receiving its Results. The p99 must stay under
//! 8 ms, one frame at 120 Hz. Also reports the emoji Extension's resident
//! memory.

use std::fs;
use std::time::Duration;
use std::time::Instant;

#[expect(dead_code, reason = "the benchmark only starts the menu and searches")]
#[path = "../tests/support/menu.rs"]
mod menu;

const QUERIES: usize = 2_000;
/// One frame at 120 Hz.
const BUDGET: Duration = Duration::from_millis(8);
/// Typed one letter at a time, as a user would.
const TYPED: [&str; 3] = ["thumbs up", "arrow right", "smiling face"];

fn main() {
    let mut menu = menu::Menu::start("");
    let typed: Vec<&str> = TYPED
        .iter()
        .flat_map(|word| (1..=word.len()).map(move |len| &word[..len]))
        .collect();
    let mut times: Vec<Duration> = (0..QUERIES)
        .map(|n| {
            let query = typed[n % typed.len()];
            let start = Instant::now();
            menu.search(query);
            start.elapsed()
        })
        .collect();
    times.sort();
    let p = |q: f64| times[((times.len() as f64 * q) as usize).min(times.len() - 1)];
    println!(
        "emoji, {QUERIES} keystrokes over the socket: p50 {:?}, p99 {:?}, max {:?}",
        p(0.5),
        p(0.99),
        times[times.len() - 1]
    );
    println!(
        "test emoji/keystroke/p99 ... bench: {} ns/iter (+/- 0)",
        p(0.99).as_nanos()
    );
    match emoji_resident() {
        Some(bytes) => {
            println!("emoji Extension: {:.1} MiB resident", bytes as f64 / f64::from(1 << 20));
            println!("test emoji/resident ... bench: {bytes} bytes (+/- 0)");
        }
        None => println!("resident memory is only measured on Linux"),
    }
    assert!(
        p(0.99) < BUDGET,
        "a keystroke took {:?} at p99 to be answered, over the {BUDGET:?} budget",
        p(0.99)
    );
}

/// The emoji Extension's `VmRSS`, in bytes: the child of this process, which
/// the Daemon started, named after its binary.
fn emoji_resident() -> Option<u64> {
    let tasks = fs::read_dir("/proc/self/task").ok()?;
    let pid = tasks
        .flatten()
        .filter_map(|task| fs::read_to_string(task.path().join("children")).ok())
        .flat_map(|children| children.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
        .find(|pid| {
            fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|comm| comm.trim() == "kotodama-emoji")
        })?;
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}
