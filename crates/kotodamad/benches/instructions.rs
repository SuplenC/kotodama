//! The instructions the Daemon runs per keystroke with 10k Indexed items, from
//! a query arriving to its Results encoded for the Front end, as `query.rs`
//! times them. gungraun counts them under Valgrind's Callgrind, so the count
//! is the same from run to run on one machine, and a few percent more shows,
//! where the wall-clock benchmarks can't tell it from noise. Another CPU can
//! count a little differently: glibc picks its string functions by CPU feature.
//!
//! Needs Valgrind and the gungraun-runner of the `gungraun` version pinned in
//! Cargo.toml (both in the dev shell): `cargo bench -p kotodamad --bench instructions`.
//! Building the Daemon and its Index is setup, and dropping them teardown:
//! neither is counted.

use std::hint::black_box;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use gungraun::{library_benchmark, library_benchmark_group, main};
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
    fn notify(&mut self, _: String, _: String) {}
}

impl Store for Nothing {
    fn save(&mut self, _: &History) {}
    fn log(&mut self, _: Entry) {}
    fn forget_usage(&mut self, _: Option<&str>) {}
}

/// The Daemon with the corpus Indexed and a Front end connected, with `typed`
/// in the search box already.
struct Search {
    daemon: Daemon<Nothing, Nothing, Nothing>,
    results: Receiver<ToFrontEnd>,
    buf: Vec<u8>,
}

impl Search {
    fn new(typed: &str) -> Self {
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
        let mut search = Search {
            daemon,
            results,
            buf: Vec::new(),
        };
        search.results.try_iter().for_each(drop);
        search.query(typed);
        search
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
        let reply = self.results.try_recv().unwrap();
        self.buf.clear();
        WireFormat::Postcard
            .encode_into(&reply, &mut self.buf)
            .unwrap();
        self.buf.len()
    }
}

/// The search with `before` typed, and the query the next keystroke makes.
fn typed_then(before: &str, query: &'static str) -> (Search, &'static str) {
    (Search::new(before), query)
}

/// Drops the Daemon outside of what is counted.
fn drop_search(search: Search) {
    drop(search);
}

#[library_benchmark(setup = typed_then, teardown = drop_search)]
// The first keystroke matches the most Items and can't narrow.
#[bench::first_keystroke("", "f")]
// Typing on: the keystroke narrows the last one's matches.
#[bench::next_keystroke("firefo", "firefox")]
// Two words pasted into a cleared box: no last search to narrow.
#[bench::two_words_pasted("", "fire tool")]
fn keystroke((mut search, query): (Search, &'static str)) -> Search {
    black_box(search.query(black_box(query)));
    search
}

library_benchmark_group!(name = query_10k_items, benchmarks = [keystroke]);

main!(library_benchmark_groups = query_10k_items);
