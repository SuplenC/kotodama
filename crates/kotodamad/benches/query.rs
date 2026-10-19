//! From a query arriving to its Results encoded for the Front end, with 10k
//! Indexed items: everything the Daemon does per keystroke except the socket
//! write.

use std::hint::black_box;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use criterion::{Criterion, criterion_group, criterion_main};
use kotodama_protocol::{FromExtension, FromFrontEnd, Manifest, Table, ToFrontEnd, WireFormat};
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
}

impl Store for Nothing {
    fn save(&mut self, _: &History) {}
    fn log(&mut self, _: Entry) {}
}

struct Setup {
    daemon: Daemon<Nothing, Nothing, Nothing>,
    results: Receiver<ToFrontEnd>,
    buf: Vec<u8>,
}

impl Setup {
    fn new() -> Self {
        let manifest = Manifest::from_toml("name = \"apps\"\ncommand = [\"x\"]").unwrap();
        let found = Found::new(manifest, PathBuf::from("/")).unwrap();
        let now = Instant::now();
        let mut daemon = Daemon::new(
            vec![found],
            Table::new(),
            History::default(),
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
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
