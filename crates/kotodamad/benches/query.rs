//! From a query arriving to its Results encoded for the Front end, with 10k
//! Indexed items: everything the Daemon does per keystroke except the socket
//! write.

use std::hint::black_box;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use criterion::{Criterion, criterion_group, criterion_main};
use kotodama_protocol::{FromExtension, FromFrontEnd, Manifest, Table, ToFrontEnd, WireFormat};
use kotodamad::clock::{DAY_SECS, When};
use kotodamad::daemon::{Daemon, Desktop, Event, ExtensionId, FrontEndId, Launcher, Output};
use kotodamad::discovery::Found;
use kotodamad::history::{History, Store};
use kotodamad::usage::Entry;

mod corpus;

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
    fn save(&mut self, _: &History) {}
    fn log(&mut self, _: Entry) {}
    fn forget_usage(&mut self, _: Option<&str>) {}
}

struct Setup {
    daemon: Daemon<Nothing, Nothing, Nothing>,
    results: Receiver<ToFrontEnd>,
    buf: Vec<u8>,
}

impl Setup {
    fn new() -> Self {
        Self::with_history(History::default())
    }

    /// With 2000 Items picked, the most the History keeps, at every time of
    /// day over a month.
    fn picked() -> Self {
        let mut history = History::default();
        let start = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 30 * DAY_SECS;
        for n in 0..2000u64 {
            let at = start + n * 30 * DAY_SECS / 2000;
            let when = When {
                at,
                time_of_day: (at % DAY_SECS) as u32,
            };
            history.touch("apps", &format!("item-{}", n * 5), when);
        }
        Self::with_history(history)
    }

    fn with_history(history: History) -> Self {
        let manifest = Manifest::from_toml("name = \"apps\"\ncommand = [\"x\"]").unwrap();
        let found = Found::new(manifest, PathBuf::from("/")).unwrap();
        let now = Instant::now();
        let mut daemon = Daemon::new(
            vec![found],
            Table::new(),
            history,
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
                message: FromExtension::AddItems {
                    items: corpus::items(),
                },
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
    let mut setup = Setup::new();
    let mut group = c.benchmark_group("query_10k_items");
    // The first keystroke matches the most Items and can't narrow.
    group.bench_function("first_keystroke", |b| {
        let mut alternate = false;
        b.iter(|| {
            alternate = !alternate;
            black_box(setup.query(if alternate { "f" } else { "t" }))
        })
    });
    // Two words pasted into a cleared box: the empty query forgets the last
    // search, so this one searches every Item.
    group.bench_function("two_words", |b| {
        b.iter(|| {
            setup.query("");
            black_box(setup.query("fire tool"))
        })
    });
    // Typing on: each keystroke narrows the last one's matches.
    group.bench_function("next_keystroke", |b| {
        let word = "firefoxterminal";
        let mut len = 1;
        b.iter(|| {
            len = len % word.len() + 1;
            black_box(setup.query(&word[..len]))
        })
    });
    // Opening the menu: the Results most used at this time of day.
    let mut setup = Setup::picked();
    group.bench_function("empty_query_2k_picked", |b| {
        b.iter(|| black_box(setup.query("")))
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
