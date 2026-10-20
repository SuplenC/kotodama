//! The Daemon as the benches drive it: the corpus Indexed off the keystroke
//! path, as the real Daemon has it, and a Front end connected. `query.rs` and
//! `instructions.rs` share it, so both measure the same keystroke.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use kotodama_protocol::{
    FromExtension, FromFrontEnd, Item, Manifest, Table, ToFrontEnd, View, ViewContent, WireFormat,
};
use kotodamad::clock::{DAY_SECS, When};
use kotodamad::daemon::{Daemon, Desktop, Event, ExtensionId, FrontEndId, Launcher, Output};
use kotodamad::discovery::Found;
use kotodamad::history::{History, Store};
use kotodamad::usage::Entry;

use crate::corpus;

pub struct Nothing;

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
pub const APPS: ExtensionId = ExtensionId(0);
pub const CLIPBOARD: ExtensionId = ExtensionId(1);
/// The menu, and a `kotodama toggle` that only passes commands on.
pub const MENU: FrontEndId = FrontEndId(0);
pub const COMMAND: FrontEndId = FrontEndId(1);

pub struct Setup {
    pub daemon: Daemon<Nothing, Nothing, Nothing>,
    results: Receiver<ToFrontEnd>,
    /// What the indexing thread sends, as the real Daemon has it.
    pub indexed: Receiver<Event>,
    /// Batches sent and not indexed yet.
    pub pending: usize,
    buf: Vec<u8>,
    /// Where the Items' payloads are kept, as in `$XDG_CACHE_HOME`.
    _payloads: tempfile::TempDir,
}

impl Setup {
    pub fn new() -> Self {
        Self::with(History::default(), corpus::items())
    }

    /// With 2000 Items picked, the most the History keeps, at every time of
    /// day over a month.
    #[allow(dead_code)] // query.rs alone
    pub fn picked() -> Self {
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

    pub fn with(history: History, items: Vec<Item>) -> Self {
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
        let payloads = tempfile::tempdir().unwrap();
        daemon.keep_items_in(payloads.path());
        let (events, indexed) = mpsc::channel();
        daemon.index_in_background(events);
        let mut setup = Setup {
            daemon,
            results: mpsc::sync_channel(1).1,
            indexed,
            pending: 0,
            buf: Vec::new(),
            _payloads: payloads,
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
    pub fn send(&mut self, extension: ExtensionId, items: Vec<Item>) {
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
    pub fn settle(&mut self) {
        while self.pending > 0 {
            let event = self.indexed.recv().unwrap();
            if matches!(event, Event::Indexed(_)) {
                self.pending -= 1;
            }
            self.daemon.handle(event, Instant::now());
        }
        self.results.try_iter().for_each(drop);
    }

    /// Sends one query and encodes its Results as the socket writer would.
    pub fn query(&mut self, text: &str) -> usize {
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
    #[allow(dead_code)] // query.rs alone
    pub fn push_list(&mut self) {
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
    #[allow(dead_code)] // query.rs alone
    pub fn filter(&mut self, text: &str) -> usize {
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
    #[allow(dead_code)] // query.rs alone
    pub fn toggle(&mut self) {
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
