//! From a query arriving to its Results encoded for the Front end, with 10k
//! Indexed items: everything the Daemon does per keystroke except the socket
//! write. Also: titles 200 characters long, a keystroke right behind a 1,000
//! Item batch, and the first keystroke after the menu opens on a cold CPU
//! cache. Then the same for a list View of 10k Items the Daemon filters.

use std::hint::black_box;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use kotodama_protocol::{
    FromExtension, FromFrontEnd, Item, Manifest, Table, ToFrontEnd, View, ViewContent, WireFormat,
};
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

/// The Extensions every Setup runs: `apps` holds the corpus, `clipboard`
/// whatever a bench sends it.
const APPS: ExtensionId = ExtensionId(0);
const CLIPBOARD: ExtensionId = ExtensionId(1);
/// The menu, and a `kotodama toggle` that only passes commands on.
const MENU: FrontEndId = FrontEndId(0);
const COMMAND: FrontEndId = FrontEndId(1);

struct Setup {
    daemon: Daemon<Nothing, Nothing, Nothing>,
    results: Receiver<ToFrontEnd>,
    /// What the indexing thread sends, as the real Daemon has it.
    indexed: Receiver<Event>,
    /// Batches sent and not indexed yet.
    pending: usize,
    buf: Vec<u8>,
}

impl Setup {
    fn new() -> Self {
        Self::with(History::default(), corpus::items())
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
        Self::with(history, corpus::items())
    }

    fn with(history: History, items: Vec<Item>) -> Self {
        let found = ["apps", "clipboard"].map(|name| {
            let manifest =
                Manifest::from_toml(&format!("name = \"{name}\"\ncommand = [\"x\"]")).unwrap();
            Found::new(manifest, PathBuf::from("/")).unwrap()
        });
        let now = Instant::now();
        let mut daemon = Daemon::new(
            found.into(),
            Table::new(),
            history,
            Nothing,
            Nothing,
            Nothing,
            now,
        );
        for extension in [APPS, CLIPBOARD] {
            let (sender, _) = mpsc::sync_channel(64);
            daemon.handle(
                Event::ExtensionUp {
                    extension,
                    generation: 1,
                    sender,
                },
                now,
            );
        }
        let (events, indexed) = mpsc::channel();
        daemon.index_in_background(events);
        let mut setup = Setup {
            daemon,
            results: mpsc::sync_channel(1).1,
            indexed,
            pending: 0,
            buf: Vec::new(),
        };
        setup.send(APPS, items);
        setup.settle();
        let (sender, results) = mpsc::sync_channel(64);
        setup.daemon.handle(
            Event::FrontEndUp {
                front_end: MENU,
                sender,
                disconnect: Box::new(|| {}),
            },
            now,
        );
        let (sender, _) = mpsc::sync_channel(64);
        setup.daemon.handle(
            Event::FrontEndUp {
                front_end: COMMAND,
                sender,
                disconnect: Box::new(|| {}),
            },
            now,
        );
        results.try_iter().for_each(drop);
        setup.results = results;
        setup
    }

    /// Sends a batch of Items, which the indexing thread takes.
    fn send(&mut self, extension: ExtensionId, items: Vec<Item>) {
        self.daemon.handle(
            Event::FromExtension {
                extension,
                generation: 1,
                message: FromExtension::AddItems { items },
            },
            Instant::now(),
        );
        self.pending += 1;
    }

    /// Waits for each batch sent to be indexed, and puts it in.
    fn settle(&mut self) {
        while self.pending > 0 {
            let indexed = self.indexed.recv().unwrap();
            self.daemon.handle(indexed, Instant::now());
            self.pending -= 1;
        }
        self.results.try_iter().for_each(drop);
    }

    /// Sends one query and encodes its Results as the socket writer would.
    fn query(&mut self, text: &str) -> usize {
        self.daemon.handle(
            Event::FromFrontEnd {
                front_end: MENU,
                message: FromFrontEnd::Query { text: text.into() },
            },
            Instant::now(),
        );
        let results = self
            .results
            .try_iter()
            .filter_map(|message| match message {
                ToFrontEnd::Results { results } => Some(results),
                _ => None,
            })
            .last()
            .unwrap();
        self.buf.clear();
        WireFormat::Postcard
            .encode_into(&results, &mut self.buf)
            .unwrap();
        self.buf.len()
    }

    /// Pushes the corpus as a list View, as `apps/all`.
    fn push_list(&mut self) {
        self.daemon.handle(
            Event::FromExtension {
                extension: APPS,
                generation: 1,
                message: FromExtension::PushView(View {
                    id: "all".into(),
                    title: "All".into(),
                    content: ViewContent::List {
                        items: corpus::items(),
                        filters_itself: false,
                    },
                }),
            },
            Instant::now(),
        );
        self.results.try_iter().for_each(drop);
    }

    /// Types `text` into the list View and encodes the Daemon's matches.
    fn filter(&mut self, text: &str) -> usize {
        self.daemon.handle(
            Event::FromFrontEnd {
                front_end: MENU,
                message: FromFrontEnd::FilterView {
                    extension: "apps".into(),
                    view: "all".into(),
                    text: text.into(),
                },
            },
            Instant::now(),
        );
        let reply = self.results.try_recv().unwrap();
        self.buf.clear();
        WireFormat::Postcard
            .encode_into(&reply, &mut self.buf)
            .unwrap();
        self.buf.len()
    }

    /// `kotodama toggle`: the menu opens on the empty query.
    fn toggle(&mut self) {
        self.daemon.handle(
            Event::FromFrontEnd {
                front_end: COMMAND,
                message: FromFrontEnd::Toggle,
            },
            Instant::now(),
        );
        self.results.try_iter().for_each(drop);
    }
}

/// Typing on: each keystroke narrows the last one's matches.
fn next_keystroke(setup: &mut Setup, word: &str) -> impl FnMut() -> usize {
    let mut len = 0;
    move || {
        len = len % word.len() + 1;
        setup.query(&word[..len])
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
    // search, so this one searches every Item. (Named `two_words` while the
    // Daemon still narrowed it from the last iteration's.)
    group.bench_function("two_words_pasted", |b| {
        b.iter(|| {
            setup.query("");
            black_box(setup.query("fire tool"))
        })
    });
    group.bench_function("next_keystroke", |b| {
        let mut keystroke = next_keystroke(&mut setup, "firefoxterminal");
        b.iter(|| black_box(keystroke()))
    });
    // A keystroke right behind a batch of 1,000 Items, as when the apps are
    // scanned again or the clipboard sends its history: from the batch
    // arriving to the keystroke's Results.
    group.bench_function("keystroke_behind_1k_batch", |b| {
        let batches = [1, 2].map(|seed| corpus::long_titles(1000, 60, seed));
        let mut n = 0;
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                setup.settle();
                n += 1;
                let batch = batches[n % 2].clone();
                let start = Instant::now();
                setup.send(CLIPBOARD, batch);
                black_box(setup.query(if n.is_multiple_of(2) { "f" } else { "t" }));
                total += start.elapsed();
            }
            total
        });
        setup.settle();
    });
    group.finish();

    // How long a batch takes to show: from an Extension sending it to its
    // new version being ready on the indexing thread. A batch of new clips,
    // and the apps scanned again.
    let mut group = c.benchmark_group("staleness");
    let clips = corpus::long_titles(1000, 60, 1);
    let apps = corpus::items();
    for (name, extension, batch) in [
        ("1k_clips", CLIPBOARD, &clips),
        ("10k_apps_rescanned", APPS, &apps),
    ] {
        group.bench_function(name, |b| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let batch = batch.clone();
                    let start = Instant::now();
                    setup.send(extension, batch);
                    let indexed = setup.indexed.recv().unwrap();
                    total += start.elapsed();
                    setup.daemon.handle(indexed, Instant::now());
                    setup.pending -= 1;
                }
                total
            })
        });
    }
    group.finish();

    let mut group = c.benchmark_group("query_10k_items");
    // Opening the menu: the Results most used at this time of day.
    let mut setup = Setup::picked();
    group.bench_function("empty_query_2k_picked", |b| {
        b.iter(|| black_box(setup.query("")))
    });
    group.finish();

    // The first keystroke after the menu opens, with the CPU cache taken by
    // other programs since the last time: only the keystroke is timed. Each
    // iteration also writes 32 MiB, so it runs fewer.
    let mut group = c.benchmark_group("open_10k_items");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(2));
    group.bench_function("first_keystroke_cold_cache", |b| {
        let mut evict = vec![0u8; 32 << 20];
        let mut n = 0u8;
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                setup.query("");
                n = n.wrapping_add(1);
                for page in evict.chunks_mut(64) {
                    page[0] = n;
                }
                black_box(&evict);
                setup.toggle();
                let start = Instant::now();
                black_box(setup.query(if n.is_multiple_of(2) { "f" } else { "t" }));
                total += start.elapsed();
            }
            total
        })
    });
    group.finish();

    // Clipboard entries searched by their whole text: the time per keystroke
    // should not grow with the titles' length.
    let mut setup = Setup::with(History::default(), corpus::long_titles(1000, 200, 7));
    let mut group = c.benchmark_group("query_1k_long_titles");
    group.bench_function("first_keystroke", |b| {
        let mut alternate = false;
        b.iter(|| {
            alternate = !alternate;
            black_box(setup.query(if alternate { "f" } else { "t" }))
        })
    });
    group.bench_function("next_keystroke", |b| {
        let mut keystroke = next_keystroke(&mut setup, "firefoxterminal");
        b.iter(|| black_box(keystroke()))
    });
    group.finish();
}

/// Typing into a list View of 10k Items, matched as the search matches, to
/// stay within `query_10k_items`'s budget.
fn filter(c: &mut Criterion) {
    let mut setup = Setup::new();
    setup.push_list();
    let mut group = c.benchmark_group("filter_10k_items");
    group.bench_function("first_keystroke", |b| {
        let mut alternate = false;
        b.iter(|| {
            alternate = !alternate;
            black_box(setup.filter(if alternate { "f" } else { "t" }))
        })
    });
    group.bench_function("next_keystroke", |b| {
        let word = "firefoxterminal";
        let mut len = 1;
        b.iter(|| {
            len = len % word.len() + 1;
            black_box(setup.filter(&word[..len]))
        })
    });
    group.finish();
}

criterion_group!(benches, bench, filter);
criterion_main!(benches);
