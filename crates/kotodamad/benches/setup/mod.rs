//! The Daemon as the benches drive it: the corpus Indexed off the keystroke
//! path, as the real Daemon has it, and a Front end connected. `query.rs` and
//! `instructions.rs` share it, so both measure the same keystroke.
//!
//! Time stands still in a Setup: every event happens at the moment it was
//! made, on a wall clock that reads [`START`] in UTC, or a second later once
//! [`Setup::plan`] waited for the hot Items to be planned. So a run counts
//! the same instructions as the last: the History ranks the same, the time
//! zone is never read, and no timer (planning the hot Items) falls due
//! inside a measured query.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use kotodama_protocol::{
    FromExtension, FromFrontEnd, Item, Manifest, PROTOCOL_VERSION, Table, ToFrontEnd, View,
    ViewContent, WireFormat,
};
use kotodamad::clock::{Clock, DAY_SECS, When};
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

/// The wall clock when a Setup is made, in seconds since the Unix epoch:
/// 2026-10-20 at 8:30 UTC.
pub const START: u64 = 1_792_485_000;

/// The Extensions every Setup runs: `apps` holds the corpus, `clipboard`
/// whatever a bench sends it.
pub const APPS: ExtensionId = ExtensionId(0);
#[allow(dead_code)] // query.rs alone
pub const CLIPBOARD: ExtensionId = ExtensionId(1);
/// The emoji Extension, which `Setup::with_emoji` runs too.
const EMOJI: ExtensionId = ExtensionId(2);
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
    /// When every event happens.
    pub now: Instant,
    /// Where the Items' payloads are kept, as in `$XDG_CACHE_HOME`.
    _payloads: tempfile::TempDir,
}

impl Setup {
    pub fn new() -> Self {
        Self::with(History::default(), corpus::items())
    }

    /// With 2000 Items picked, the most the History keeps, at every time of
    /// day over the month before [`START`], and the hot Items planned.
    pub fn picked() -> Self {
        let mut history = History::default();
        let start = START - 30 * DAY_SECS;
        for n in 0..2000u64 {
            let at = start + n * 30 * DAY_SECS / 2000;
            let when = When {
                at,
                time_of_day: (at % DAY_SECS) as u32,
            };
            history.touch("apps", &format!("item-{}", n * 5), when);
        }
        let mut setup = Self::with(history, corpus::items());
        setup.plan();
        setup
    }

    /// Moves the clock on to when the hot Items are planned next, a second
    /// after the last change, and hands the Daemon what the indexing thread
    /// planned: the hot Items, and the Empty query ranked ahead.
    pub fn plan(&mut self) {
        self.now = self.daemon.next_wake().expect("the hot Items are planned");
        self.daemon.tick(self.now);
        loop {
            let event = self
                .indexed
                .recv_timeout(Duration::from_secs(10))
                .expect("the indexing thread plans the hot Items");
            let planned = matches!(event, Event::Kept(_));
            self.daemon.handle(event, self.now);
            if planned {
                break;
            }
        }
        self.results.try_iter().for_each(drop);
    }

    pub fn with(history: History, items: Vec<Item>) -> Self {
        Self::running(&["apps", "clipboard"], Table::new(), history, items)
    }

    /// The corpus, and the emoji Extension's Items beside it, as the Daemon
    /// with `config` holds them.
    #[allow(dead_code)] // instructions.rs alone
    pub fn with_emoji(config: Table) -> Self {
        let mut setup = Self::running(
            &["apps", "clipboard", "emoji"],
            config,
            History::default(),
            corpus::items(),
        );
        setup.send(EMOJI, kotodama_emoji::items());
        setup.settle();
        setup
    }

    /// The Extensions `names` up, with `items` sent to the first.
    fn running(names: &[&str], config: Table, history: History, items: Vec<Item>) -> Self {
        let found: Vec<Found> = names
            .iter()
            .map(|name| {
                let toml = format!("name = \"{name}\"\ncommand = [\"x\"]");
                Found::new(Manifest::from_toml(&toml).unwrap(), PathBuf::from("/")).unwrap()
            })
            .collect();
        let now = Instant::now();
        let mut daemon = Daemon::new(found, config, history, Nothing, Nothing, Nothing, now);
        daemon.set_clock(Clock::starting_at(START));
        for extension in (0..names.len()).map(ExtensionId) {
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
            now,
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
            self.now,
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
            self.daemon.handle(event, self.now);
        }
        self.results.try_iter().for_each(drop);
    }

    /// Sends one query and encodes its Results as the socket writer would for a
    /// Front end on this protocol version.
    pub fn query(&mut self, text: &str) -> usize {
        self.daemon.handle(
            Event::FromFrontEnd {
                front_end: MENU,
                message: FromFrontEnd::Query { text: text.into() },
            },
            self.now,
        );
        let results = self
            .results
            .try_iter()
            .filter_map(|message| match message {
                ToFrontEnd::Results { results, .. } => Some(results),
                _ => None,
            })
            .last()
            .unwrap();
        self.buf.clear();
        WireFormat::Postcard
            .encode_for_into(PROTOCOL_VERSION, &results, &mut self.buf)
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
            self.now,
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
            self.now,
        );
        let reply = self.results.try_recv().unwrap();
        self.buf.clear();
        WireFormat::Postcard
            .encode_for_into(PROTOCOL_VERSION, &reply, &mut self.buf)
            .unwrap();
        self.buf.len()
    }

    /// `kotodama toggle`: the menu opens on the Empty query.
    #[allow(dead_code)] // query.rs alone
    pub fn toggle(&mut self) {
        self.daemon.handle(
            Event::FromFrontEnd {
                front_end: COMMAND,
                message: FromFrontEnd::Toggle,
            },
            self.now,
        );
        self.results.try_iter().for_each(drop);
    }
}
