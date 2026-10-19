//! Searching a full clipboard history: 1000 entries, kept by the library as
//! copies would keep them and sent as the Extension sends them, then queried
//! in the Daemon, from a query arriving to its Results encoded for the Front
//! end. After criterion's timings it checks the p99 against the Daemon's
//! budget of 2 ms from query received to Results sent, and prints the
//! Extension's own resident memory with that history, in the bencher format
//! with `bytes` as its unit, so CI compares it with the baseline.

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use kotodama_clipboard::copied::Copied;
use kotodama_clipboard::history::{History, Limits};
use kotodama_clipboard::items;
use kotodama_protocol::{
    Channel, FromExtension, FromFrontEnd, Hello, Item, Manifest, Table, ToExtension, ToFrontEnd,
    WireFormat,
};
use kotodamad::daemon::{Daemon, Desktop, Event, ExtensionId, FrontEndId, Launcher, Output};
use kotodamad::discovery::Found;
use kotodamad::history::{History as Picks, Store};
use kotodamad::usage::Entry;

const ENTRIES: usize = 1000;
/// The Daemon's budget from query received to Results sent.
const BUDGET: Duration = Duration::from_millis(2);
const TYPED: &str = "cargo test";

/// Words common in what is copied: commands, code, paths and URLs. One word
/// in four is one of these; the rest are made-up words, so entries differ
/// from one another as real ones do.
const COMMON: [&str; 32] = [
    "cargo",
    "test",
    "git",
    "push",
    "origin",
    "main",
    "https://",
    "github.com",
    "kotodama",
    "clipboard",
    "let",
    "fn",
    "struct",
    "impl",
    "the",
    "and",
    "error:",
    "warning:",
    "export",
    "PATH=",
    "/home/user",
    "docker",
    "run",
    "--rm",
    "SELECT",
    "FROM",
    "WHERE",
    "id",
    "=",
    "{",
    "}",
    "\n",
];
const SYLLABLES: [&str; 24] = [
    "fi", "re", "fox", "ter", "mi", "nal", "code", "ka", "to", "da", "ma", "set", "tings", "vol",
    "ume", "blue", "tooth", "net", "work", "wall", "pa", "per", "dis", "play",
];

struct Nothing;

impl Launcher for Nothing {
    fn start(&mut self, _: ExtensionId, _: u64, _: &Found) {}
    fn stop(&mut self, _: ExtensionId, _: u64) {}
}

impl Desktop for Nothing {
    fn output(&mut self, _: Output) {}
    fn launch(&mut self, _: Vec<String>) {}
    fn notify(&mut self, _: String, _: String) {}
}

impl Store for Nothing {
    fn save(&mut self, _: &Picks) {}
    fn log(&mut self, _: Entry) {}
}

/// A full history's Items, made the same on every run: commands, URLs,
/// code and prose, from a few words to a few hundred.
fn items(dir: &Path) -> Vec<Item> {
    let mut seed: u32 = 0x2545_f491;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as usize
    };
    let mut history = History::open(dir);
    for n in 0..ENTRIES {
        let words = 2 + next() % 60;
        let mut text: String = (0..words)
            .map(|_| {
                if next() % 4 == 0 {
                    COMMON[next() % COMMON.len()].to_owned()
                } else {
                    (0..1 + next() % 3)
                        .map(|_| SYLLABLES[next() % SYLLABLES.len()])
                        .collect()
                }
            })
            .collect::<Vec<String>>()
            .join(" ");
        text.push_str(&format!(" {n}"));
        history
            .record(
                Copied::read("text/plain;charset=utf-8", text.into_bytes()).unwrap(),
                1_792_368_000 + n as u64,
                &Limits::default(),
            )
            .unwrap();
    }
    std::iter::once(items::history_item())
        .chain(
            history
                .entries()
                .map(|entry| items::entry_item(&history, entry)),
        )
        .collect()
}

struct Setup {
    daemon: Daemon<Nothing, Nothing, Nothing>,
    results: Receiver<ToFrontEnd>,
    buf: Vec<u8>,
}

impl Setup {
    fn new(items: Vec<Item>) -> Self {
        let manifest = Manifest::from_toml("name = \"clipboard\"\ncommand = [\"x\"]").unwrap();
        let found = Found::new(manifest, PathBuf::from("/")).unwrap();
        let now = Instant::now();
        let mut daemon = Daemon::new(
            vec![found],
            Table::new(),
            Picks::default(),
            Nothing,
            Nothing,
            Nothing,
            now,
        );
        let (sender, _extension) = mpsc::sync_channel(64);
        daemon.handle(
            Event::ExtensionUp {
                extension: ExtensionId(0),
                generation: 1,
                sender,
            },
            now,
        );
        daemon.handle(
            Event::FromExtension {
                extension: ExtensionId(0),
                generation: 1,
                message: FromExtension::AddItems { items },
            },
            now,
        );
        let (sender, results) = mpsc::sync_channel(64);
        daemon.handle(
            Event::FrontEndUp {
                front_end: FrontEndId(0),
                sender,
                disconnect: Box::new(|| {}),
            },
            now,
        );
        results.try_iter().for_each(drop);
        Setup {
            daemon,
            results,
            buf: Vec::new(),
        }
    }

    /// Sends one query and encodes its Results as the socket writer would.
    fn query(&mut self, text: &str) -> usize {
        self.daemon.handle(
            Event::FromFrontEnd {
                front_end: FrontEndId(0),
                message: FromFrontEnd::Query { text: text.into() },
            },
            Instant::now(),
        );
        let results = self.results.try_recv().unwrap();
        self.buf.clear();
        WireFormat::Postcard
            .encode_into(&results, &mut self.buf)
            .unwrap();
        self.buf.len()
    }
}

fn bench(c: &mut Criterion) {
    let state = tempfile::tempdir().unwrap();
    let mut setup = Setup::new(items(&state.path().join("kotodama/clipboard")));
    let mut group = c.benchmark_group("clipboard_1k_entries");
    group.bench_function("first_keystroke", |b| {
        let mut alternate = false;
        b.iter(|| {
            alternate = !alternate;
            black_box(setup.query(if alternate { "c" } else { "t" }))
        })
    });
    group.bench_function("next_keystroke", |b| {
        let mut len = 1;
        b.iter(|| {
            len = len % TYPED.len() + 1;
            black_box(setup.query(&TYPED[..len]))
        })
    });
    group.finish();

    // Every keystroke of typing, then clearing, many times over.
    let mut times: Vec<Duration> = (0..2000)
        .map(|n| {
            let typed = &TYPED[..n % TYPED.len() + 1];
            let start = Instant::now();
            black_box(setup.query(typed));
            start.elapsed()
        })
        .collect();
    times.sort();
    let p99 = times[times.len() * 99 / 100];
    println!(
        "clipboard, {ENTRIES} entries: p50 {:?}, p99 {p99:?}, max {:?}",
        times[times.len() / 2],
        times[times.len() - 1]
    );
    assert!(
        p99 < BUDGET,
        "searching {ENTRIES} clipboard entries takes {p99:?} at p99, over the budget of {BUDGET:?}"
    );
    resident(state.path());
}

/// Starts the Extension on the history in `state` and prints its resident
/// memory once it has sent its Items.
fn resident(state: &Path) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kotodama-clipboard"))
        .env("XDG_STATE_HOME", state)
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let (stdout, stdin) = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let mut channel = Channel::handshake(stdout, stdin, &Hello::new(&WireFormat::ALL)).unwrap();
    channel.send(&ToExtension::Config(Table::new())).unwrap();
    match channel.recv::<FromExtension>().unwrap() {
        Some(FromExtension::AddItems { items }) => assert_eq!(items.len(), ENTRIES + 1),
        other => panic!("expected add_items, got {other:?}"),
    }
    match vm_rss(child.id()) {
        Some(bytes) => {
            println!(
                "kotodama-clipboard, {ENTRIES} entries: {:.1} MiB resident",
                bytes as f64 / (1 << 20) as f64
            );
            println!("test clipboard/1k_entries/resident ... bench: {bytes} bytes (+/- 0)");
        }
        None => println!("resident memory is only measured on Linux"),
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// `VmRSS` from `/proc/<pid>/status`, in bytes.
fn vm_rss(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

criterion_group!(benches, bench);
criterion_main!(benches);
