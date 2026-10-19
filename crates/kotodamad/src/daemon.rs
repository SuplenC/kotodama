//! The Daemon's state machine, fed one [`Event`] at a time by the main
//! loop. It owns the index, the Front ends' sessions and the Extensions' state,
//! and never blocks: everything it sends goes into a channel that another
//! thread drains, so a stuck or dead Extension can't stall it.
//!
//! Starting processes, typing into windows and saving the History are behind
//! [`Launcher`], [`Desktop`] and [`Store`], so it runs the same in tests as in
//! `kotodamad`.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::{Duration, Instant, SystemTime};

use kotodama_protocol::{
    Action, ActionRef, CatalogEntry, Clipboard, FromExtension, FromFrontEnd, Item, ItemKind, Job,
    KindName, LiveResult, NamespacedKind, OfferedAction, Outcome, ResultRef, ShownResult, Table,
    ToExtension, ToFrontEnd,
};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::config::extension_table;
use crate::discovery::Found;
use crate::history::{History, IDENTITIES, Picks, ResultKey, Settings, Store, query_key};
use crate::index::{Index, Slot};
use crate::keywords::{self, Keywords};
use crate::rank::{self, Haystack, Matched, Tier, Tiers};
use crate::usage::{Entry, Used};

/// The name the Daemon's own Results go by: Quicklinks and the Action that
/// clears the History. No Extension may take it.
pub const OWN: &str = "kotodama";
/// The id of the Daemon's own Item that clears the History.
pub const CLEAR_HISTORY: &str = "clear-history";

/// An Extension, by its position in the list the Daemon was started with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionId(pub usize);

/// A Front end connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrontEndId(pub u64);

/// Starts and stops Extension processes. When the handshake is done it reports
/// [`Event::ExtensionUp`], and when the process ends, however it ends,
/// [`Event::ExtensionDown`], both with the `generation` it was given.
pub trait Launcher {
    fn start(&mut self, extension: ExtensionId, generation: u64, found: &Found);
    /// Kills the process, if that run of it is still going.
    fn stop(&mut self, extension: ExtensionId, generation: u64);
}

/// Types or pastes into the focused window, and starts programs for the user.
pub trait Desktop {
    fn output(&mut self, output: Output);
    /// Starts a program and its arguments, detached from the Daemon, as a
    /// Quicklink asks.
    fn launch(&mut self, command: Vec<String>);
}

/// What an Action asked the Daemon to put into the window the user came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    Type(String),
    Paste(Clipboard),
}

pub enum Event {
    ExtensionUp {
        extension: ExtensionId,
        generation: u64,
        /// Bounded: an Extension that leaves it full has stopped reading.
        sender: SyncSender<ToExtension>,
    },
    FromExtension {
        extension: ExtensionId,
        generation: u64,
        message: FromExtension,
    },
    ExtensionDown {
        extension: ExtensionId,
        generation: u64,
    },
    FrontEndUp {
        front_end: FrontEndId,
        /// Bounded: a Front end that leaves it full has stopped reading.
        sender: SyncSender<ToFrontEnd>,
        /// Cuts the connection off.
        disconnect: Box<dyn FnOnce() + Send>,
    },
    FromFrontEnd {
        front_end: FrontEndId,
        message: FromFrontEnd,
    },
    FrontEndDown {
        front_end: FrontEndId,
    },
    /// The config directory was reloaded.
    Config(Table),
    /// The Extension directories changed: these are all the Extensions now.
    Extensions(Vec<Found>),
}

/// How many Results a Front end gets for one query.
pub const MAX_RESULTS: usize = 50;
/// How many rows a Front end shows until it says otherwise.
const DEFAULT_ROWS: usize = 10;
/// How long to wait for Front ends to close before typing anyway.
const CLOSE_TIMEOUT: Duration = Duration::from_millis(500);
/// The first restart delay after a crash; each crash in a row doubles it.
const FIRST_RETRY: Duration = Duration::from_millis(100);
const MAX_RETRY: Duration = Duration::from_secs(30);
/// An Extension that ran this long before it stopped starts its backoff over.
const STABLE: Duration = Duration::from_secs(10);
/// How many results one Live query may contribute; the rest are dropped.
pub const MAX_LIVE_RESULTS: usize = 500;
/// How long an Extension or Front end may take to say hello before it counts as
/// hung.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

enum State {
    /// Started; waiting for the handshake until `deadline`.
    Starting {
        deadline: Instant,
    },
    Up {
        sender: SyncSender<ToExtension>,
        since: Instant,
    },
    Down {
        retry_at: Instant,
    },
    /// Uninstalled. The slot stays so ExtensionIds keep their meaning, and is
    /// reused if an Extension of the same name comes back.
    Removed,
}

struct Extension {
    found: Found,
    generation: u64,
    state: State,
    /// Crashes in a row, for the backoff.
    failures: u32,
    /// The config table last sent.
    config: Option<Table>,
    /// Asked for the Action catalog, and is allowed to.
    catalog: bool,
    /// Hung: it left its channel full, or never finished the handshake.
    hung: Cell<bool>,
    /// Asked the Launcher to stop this run; its ExtensionDown is on its way.
    stopping: bool,
}

impl Extension {
    fn new(found: Found, now: Instant) -> Self {
        Extension {
            found,
            generation: 0,
            state: State::Down { retry_at: now },
            failures: 0,
            config: None,
            catalog: false,
            hung: Cell::new(false),
            stopping: false,
        }
    }

    fn name(&self) -> &str {
        &self.found.manifest.name
    }

    fn sender(&self) -> Option<&SyncSender<ToExtension>> {
        match &self.state {
            State::Up { sender, .. } => Some(sender),
            _ => None,
        }
    }

    /// Never blocks. An Extension that has left its channel full has stopped
    /// reading, so it is marked hung, and stopped once the event is handled.
    /// A disconnected channel means it is gone, and its ExtensionDown is on its
    /// way. Returns whether it is still answering.
    fn send(&self, message: ToExtension) -> bool {
        if let Some(sender) = self.sender()
            && let Err(TrySendError::Full(_)) = sender.try_send(message)
        {
            self.hung.set(true);
        }
        !self.hung.get()
    }

    /// Stopped, or restarting after it stopped: its Results are gone.
    fn is_down(&self) -> bool {
        match self.state {
            State::Down { .. } => true,
            State::Starting { .. } => self.generation > 1,
            State::Up { .. } | State::Removed => false,
        }
    }

    fn debounce(&self) -> Option<Duration> {
        let live = self.found.manifest.live_query.as_ref()?;
        Some(Duration::from_millis(live.debounce_ms.into()))
    }

    /// The Live query to send for `query`: the query without its prefix, and
    /// the prefix, if a prefix or trigger routes it here.
    fn route(&self, query: &str) -> Option<(String, Option<String>)> {
        let live = self.found.manifest.live_query.as_ref()?;
        for prefix in &live.prefixes {
            if let Some(rest) = query.strip_prefix(prefix.as_str()) {
                return Some((rest.to_owned(), Some(prefix.clone())));
            }
        }
        self.found
            .triggers
            .iter()
            .any(|trigger| trigger.is_match(query))
            .then(|| (query.to_owned(), None))
    }
}

/// Where a Result comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Indexed(Slot),
    /// The `n`th Live query result from an Extension, in arrival order.
    Live(ExtensionId, u32),
    /// The Daemon's own `n`th Item.
    Own(u32),
}

/// Where a Result ranks before how well it matches is looked at, lowest
/// first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    #[default]
    Rest,
    /// The query's Learned choice.
    Learned,
    /// What the query names: by an Alias, or by an exact match the Learned
    /// choice wasn't picked over.
    Named,
}

#[derive(Clone, Copy)]
struct Candidate {
    source: Source,
    answer: bool,
    class: Class,
    /// How well it matches, as [`Tier::coarse`] gives it, after Frecency
    /// lifted it.
    tier: u8,
    /// Its Frecency, shared with every Result of the same Identity.
    frecency: f64,
    /// The Frecency of picks of this Result itself, rather than of its
    /// Identity: of the Results of one Identity, the one picked through most
    /// goes first (see [`Daemon::lead_with_direct_picks`]).
    direct: f64,
    /// How well it matches, to break a tie: initials beat a word start.
    fine: Tier,
    /// Named by an Alias rather than by its own title or keyword, which wins
    /// a tie.
    by_alias: bool,
    /// nucleo's score.
    score: u32,
    /// The title's length in characters: a shorter title wins a tie.
    len: u32,
}

impl Candidate {
    fn new(source: Source, tier: Tier, score: u32, len: u32) -> Self {
        Candidate {
            source,
            answer: false,
            class: Class::Rest,
            tier: tier.coarse(),
            frecency: 0.0,
            direct: 0.0,
            fine: tier,
            by_alias: false,
            score,
            len,
        }
    }

    /// Answers first, then by class, how well it matches, Frecency, and
    /// nucleo's score.
    fn rank(&self, other: &Candidate) -> std::cmp::Ordering {
        other
            .answer
            .cmp(&self.answer)
            .then(other.class.cmp(&self.class))
            .then(other.tier.cmp(&self.tier))
            .then(other.frecency.total_cmp(&self.frecency))
            .then(other.fine.cmp(&self.fine))
            .then(self.by_alias.cmp(&other.by_alias))
            .then(other.score.cmp(&self.score))
            .then(self.len.cmp(&other.len))
            .then(self.source.cmp(&other.source))
    }
}

/// One Extension's Live query for one session.
#[derive(Default)]
struct Live {
    /// The id of the query sent and not yet done or cancelled.
    sent: Option<u64>,
    /// When to send the query, once the debounce is over.
    due: Option<Instant>,
    query: String,
    prefix: Option<String>,
    results: Vec<LiveResult>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Update {
    None,
    /// Results changed under an unchanged query: the visible rows stay put.
    Late,
    /// The query changed: rank from scratch.
    Fresh,
}

/// One Front end's search.
struct Session {
    sender: SyncSender<ToFrontEnd>,
    disconnect: Option<Box<dyn FnOnce() + Send>>,
    /// Left its channel full: it has stopped reading.
    hung: Cell<bool>,
    query: String,
    /// The query as Learned choices and Aliases are kept under.
    key: String,
    /// The query as tiers are worked out from.
    folded: rank::Query,
    pattern: Pattern,
    /// What a perfect match of the query scores, which Live query scores are
    /// scaled to.
    max_score: u32,
    /// How many rows, from the first, late Results may not move: those on
    /// screen and those scrolled past.
    rows: usize,
    /// The slots that matched `narrowed_query` at index version
    /// `narrowed_version`. A query that only adds to it can only match a
    /// subset, so the next search looks at these alone.
    narrowed: Vec<Slot>,
    narrowed_query: String,
    narrowed_version: u64,
    /// Whether `narrowed_query` searched subtitles.
    narrowed_subtitles: bool,
    /// By Extension.
    live: Vec<Live>,
    /// The Results last sent, in order, and whether each is an answer.
    shown: Vec<(Source, bool)>,
    /// The Result an Alias named, and the Alias: picking it counts for the
    /// Alias alone.
    via_alias: Option<(Source, String)>,
    update: Update,
}

impl Session {
    /// Never blocks; see [`Extension::send`].
    fn send(&self, message: ToFrontEnd) {
        if let Err(TrySendError::Full(_)) = self.sender.try_send(message) {
            self.hung.set(true);
        }
    }

    fn can_narrow(&self, version: u64) -> bool {
        const SYNTAX: &[char] = &['^', '$', '!', '\'', '\\'];
        // Subtitles are searched from a few letters on, so the matches of a
        // query too short for them may lack some of this one's.
        !self.narrowed_query.is_empty()
            && self.narrowed_subtitles == self.folded.searches_subtitles()
            && self.narrowed_version == version
            && self.query.starts_with(&self.narrowed_query)
            && !self.query.contains(SYNTAX)
    }
}

/// One of the Daemon's own Items.
struct Own {
    item: Item,
    haystack: Haystack,
    does: OwnAction,
}

#[derive(Clone, Copy)]
enum OwnAction {
    /// Opens the `n`th Quicklink.
    Quicklink(usize),
    ClearHistory,
}

impl Own {
    fn new(item: Item, does: OwnAction) -> Self {
        Own {
            haystack: Haystack::new(&item),
            item,
            does,
        }
    }
}

/// The clear-history Item.
fn clear_history() -> Item {
    Item {
        id: CLEAR_HISTORY.into(),
        kind: ItemKind::Namespaced(NamespacedKind::new("kotodama.command").expect("a valid kind")),
        title: "Clear history".into(),
        subtitle: Some("Forget what was picked, for Frecency and Learned choices".into()),
        icon: None,
        keyword: None,
        identity: None,
        keywords: Vec::new(),
        actions: vec![Action {
            id: "run".into(),
            title: "Clear".into(),
            shortcut: None,
            arguments: Vec::new(),
        }],
    }
}

/// A Result the query names by a keyword.
struct Keyword {
    source: Source,
    /// Named by an Alias, which always goes first, rather than by a keyword of
    /// its own.
    alias: bool,
    /// The Alias's own Frecency, which ranks it instead of its Result's.
    frecency: Option<f64>,
}

/// The id an Alias's picks are kept under, for the Extension [`OWN`].
fn alias_id(keyword: &str) -> String {
    format!("alias:{keyword}")
}

/// What the History keeps a Result under: its Identity, which every
/// Extension's Result of that Identity shares, or else the name of the
/// Extension that owns it and its id.
fn history_key<'a>(extension: &'a str, item: &'a Item) -> (&'a str, &'a str) {
    match &item.identity {
        Some(identity) => (IDENTITIES, identity),
        None => (extension, &item.id),
    }
}

/// What a pick the History kept counts for, for one Indexed item.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Counts {
    /// Its Identity's: the Frecency it ranks by.
    Shared,
    /// Picks of the Item itself, which only order the Results of one Identity.
    Direct,
    /// An Item without an Identity's: both.
    Both,
}

/// What was typed after `keyword`, if the query starts with it. `keyword` is
/// lowercase.
fn after_keyword<'q>(query: &'q str, keyword: &str) -> Option<&'q str> {
    let query = query.trim_start();
    let (word, rest) = query.split_once(char::is_whitespace).unwrap_or((query, ""));
    word.chars()
        .flat_map(char::to_lowercase)
        .eq(keyword.chars())
        .then(|| rest.trim())
}

struct PendingOutput {
    output: Output,
    /// The Front ends that haven't answered Close yet.
    waiting: Vec<FrontEndId>,
    deadline: Instant,
}

pub struct Daemon<L, D, S> {
    extensions: Vec<Extension>,
    index: Index,
    matcher: Matcher,
    sessions: BTreeMap<FrontEndId, Session>,
    live_queries: HashMap<u64, (FrontEndId, ExtensionId)>,
    invocations: HashMap<u64, (FrontEndId, ExtensionId)>,
    next_id: u64,
    config: Table,
    /// Running Extensions' Contributed actions by Item kind, in Extension order.
    contributed: HashMap<String, Vec<(ExtensionId, Action)>>,
    /// The Action catalog last sent, if any Extension reads it.
    catalog: Option<Vec<CatalogEntry>>,
    catalog_dirty: bool,
    down: Vec<String>,
    jobs: BTreeMap<(ExtensionId, String), Job>,
    pending_output: Option<PendingOutput>,
    launcher: L,
    desktop: D,
    store: S,
    history: History,
    /// Wall-clock seconds at `started`, so the History's times follow `now`.
    epoch: u64,
    started: Instant,
    keywords: Keywords,
    /// One per Quicklink, then the clear-history Item.
    own: Vec<Own>,
    /// The picked Indexed items, by slot, as of the index and History
    /// versions in `frecent_versions`.
    frecent: Vec<(Slot, Picks, Counts)>,
    frecent_versions: Option<(u64, u64)>,
    scored: Vec<(Slot, u32, Matched)>,
    tiers: Tiers,
    highlights: Vec<u32>,
}

impl<L: Launcher, D: Desktop, S: Store> Daemon<L, D, S> {
    /// Starts every Extension.
    pub fn new(
        found: Vec<Found>,
        config: Table,
        history: History,
        launcher: L,
        desktop: D,
        store: S,
        now: Instant,
    ) -> Self {
        let epoch = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let mut daemon = Daemon {
            index: Index::new(found.len()),
            extensions: found
                .into_iter()
                .map(|found| Extension::new(found, now))
                .collect(),
            matcher: Matcher::new(Config::DEFAULT),
            sessions: BTreeMap::new(),
            live_queries: HashMap::new(),
            invocations: HashMap::new(),
            next_id: 1,
            config,
            contributed: HashMap::new(),
            catalog: None,
            catalog_dirty: false,
            down: Vec::new(),
            jobs: BTreeMap::new(),
            pending_output: None,
            launcher,
            desktop,
            store,
            history,
            epoch,
            started: now,
            keywords: Keywords::default(),
            own: Vec::new(),
            frecent: Vec::new(),
            frecent_versions: None,
            scored: Vec::new(),
            tiers: Tiers::default(),
            highlights: Vec::new(),
        };
        daemon.load_keywords();
        daemon.load_settings();
        for id in 0..daemon.extensions.len() {
            daemon.start(ExtensionId(id), now);
        }
        daemon
    }

    /// The merged config.
    pub fn config(&self) -> &Table {
        &self.config
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Seconds since the Unix epoch at `now`.
    fn wall(&self, now: Instant) -> u64 {
        self.epoch + now.saturating_duration_since(self.started).as_secs()
    }

    pub fn handle(&mut self, event: Event, now: Instant) {
        match event {
            Event::ExtensionUp {
                extension,
                generation,
                sender,
            } => self.extension_up(extension, generation, sender, now),
            Event::FromExtension {
                extension,
                generation,
                message,
            } => {
                if self.extensions[extension.0].generation == generation
                    && self.extensions[extension.0].sender().is_some()
                {
                    self.extension_message(extension, message, now);
                }
            }
            Event::ExtensionDown {
                extension,
                generation,
            } => {
                if self.extensions[extension.0].generation == generation {
                    self.extension_down(extension, now);
                }
            }
            Event::FrontEndUp {
                front_end,
                sender,
                disconnect,
            } => self.front_end_up(front_end, sender, disconnect),
            Event::FromFrontEnd { front_end, message } => {
                self.front_end_message(front_end, message, now)
            }
            Event::FrontEndDown { front_end } => self.front_end_down(front_end),
            Event::Config(config) => self.set_config(config),
            Event::Extensions(found) => self.set_extensions(found, now),
        }
        self.flush(now);
    }

    /// Does whatever was waiting for `now`: restarts, debounced Live queries,
    /// and typing that has waited long enough for Front ends to close.
    pub fn tick(&mut self, now: Instant) {
        for id in 0..self.extensions.len() {
            if let State::Down { retry_at } = self.extensions[id].state
                && retry_at <= now
            {
                self.start(ExtensionId(id), now);
            }
            if let State::Starting { deadline } = self.extensions[id].state
                && deadline <= now
            {
                log::warn!(
                    "{} didn't finish its handshake in {HANDSHAKE_TIMEOUT:?}",
                    self.extensions[id].name()
                );
                self.extensions[id].hung.set(true);
            }
        }
        if self
            .pending_output
            .as_ref()
            .is_some_and(|pending| pending.deadline <= now)
        {
            self.deliver_output();
        }
        self.flush(now);
    }

    /// When [`tick`](Self::tick) next has something to do.
    pub fn next_deadline(&self) -> Option<Instant> {
        let restarts = self.extensions.iter().filter_map(|ext| match ext.state {
            State::Down { retry_at } => Some(retry_at),
            State::Starting { deadline } if !ext.stopping => Some(deadline),
            _ => None,
        });
        let live = self
            .sessions
            .values()
            .flat_map(|session| session.live.iter().filter_map(|live| live.due));
        let output = self.pending_output.as_ref().map(|pending| pending.deadline);
        restarts.chain(live).chain(output).min()
    }

    fn start(&mut self, id: ExtensionId, now: Instant) {
        let ext = &mut self.extensions[id.0];
        ext.generation += 1;
        ext.state = State::Starting {
            deadline: now + HANDSHAKE_TIMEOUT,
        };
        ext.catalog = false;
        ext.config = None;
        ext.hung.set(false);
        ext.stopping = false;
        self.launcher.start(id, ext.generation, &ext.found);
    }

    fn extension_up(
        &mut self,
        id: ExtensionId,
        generation: u64,
        sender: SyncSender<ToExtension>,
        now: Instant,
    ) {
        let ext = &mut self.extensions[id.0];
        if ext.generation != generation
            || ext.stopping
            || !matches!(ext.state, State::Starting { .. })
        {
            return;
        }
        let table = extension_table(&self.config, ext.name());
        ext.state = State::Up { sender, since: now };
        ext.send(ToExtension::Config(table.clone()));
        ext.config = Some(table);

        // Whatever is typed already reaches it, as if typed just now.
        if let Some(debounce) = ext.debounce() {
            for session in self.sessions.values_mut() {
                if session.query.is_empty() {
                    continue;
                }
                if let Some((query, prefix)) = ext.route(&session.query) {
                    session.live[id.0] = Live {
                        query,
                        prefix,
                        due: Some(now + debounce),
                        ..Live::default()
                    };
                }
            }
        }
        self.extensions_changed();
    }

    fn extension_down(&mut self, id: ExtensionId, now: Instant) {
        let ext = &mut self.extensions[id.0];
        if let State::Up { since, .. } = ext.state
            && now.duration_since(since) >= STABLE
        {
            ext.failures = 0;
        }
        let delay = FIRST_RETRY
            .saturating_mul(1 << ext.failures.min(16))
            .min(MAX_RETRY);
        ext.failures = ext.failures.saturating_add(1);
        ext.state = State::Down {
            retry_at: now + delay,
        };
        log::warn!("extension {} stopped; restarting in {delay:?}", ext.name());
        self.forget_run(id);
    }

    /// Drops everything one run of an Extension left behind: its Items, Live
    /// results and jobs, and the Actions it was running, which fail.
    fn forget_run(&mut self, id: ExtensionId) {
        let name = self.extensions[id.0].name().to_owned();
        if self.index.clear(id) {
            self.mark_late();
        }
        self.live_queries.retain(|_, (_, ext)| *ext != id);
        self.jobs.retain(|(ext, _), _| *ext != id);
        for session in self.sessions.values_mut() {
            let live = &mut session.live[id.0];
            if !live.results.is_empty() {
                session.update = session.update.max(Update::Late);
            }
            *live = Live::default();
        }
        let sessions = &self.sessions;
        self.invocations.retain(|_, (front_end, ext)| {
            if *ext == id
                && let Some(session) = sessions.get(front_end)
            {
                session.send(ToFrontEnd::ActionFailed {
                    message: format!("{name} stopped before the Action finished"),
                });
            }
            *ext != id
        });
        self.extensions_changed();
    }

    /// The set of running Extensions changed.
    fn extensions_changed(&mut self) {
        self.contributed.clear();
        for (id, ext) in self.extensions.iter().enumerate() {
            if ext.sender().is_none() {
                continue;
            }
            for contributed in &ext.found.manifest.contributed_actions {
                self.contributed
                    .entry(contributed.kind.as_str().to_owned())
                    .or_default()
                    .push((ExtensionId(id), contributed.action.clone()));
            }
        }
        self.catalog_dirty = true;
        self.mark_late();
    }

    fn extension_message(&mut self, id: ExtensionId, message: FromExtension, now: Instant) {
        let name = self.extensions[id.0].name().to_owned();
        match message {
            FromExtension::AddItems { items } => {
                for item in items {
                    if item.kind.extension().is_some_and(|owner| owner != name) {
                        log::warn!(
                            "{name}: Item `{}` has another Extension's kind `{}`",
                            item.id,
                            item.kind.name()
                        );
                        continue;
                    }
                    self.index.add(id, item);
                }
                self.items_changed();
            }
            FromExtension::RemoveItems { ids } => {
                for item in ids {
                    self.index.remove(id, &item);
                }
                self.items_changed();
            }
            FromExtension::LiveResults {
                live_query,
                results,
                done,
            } => self.live_results(id, live_query, results, done),
            FromExtension::ActionDone {
                invocation,
                outcome,
            } => self.action_done(id, invocation, outcome, now),
            FromExtension::PushView(view) => self.broadcast(|| ToFrontEnd::PushView {
                extension: name.clone(),
                view: view.clone(),
            }),
            FromExtension::UpdateView(view) => self.broadcast(|| ToFrontEnd::UpdateView {
                extension: name.clone(),
                view: view.clone(),
            }),
            FromExtension::PopView { view } => self.broadcast(|| ToFrontEnd::PopView {
                extension: name.clone(),
                view: view.clone(),
            }),
            FromExtension::JobProgress(job) => {
                self.jobs.insert((id, job.id.clone()), job.clone());
                self.broadcast(|| ToFrontEnd::JobProgress {
                    extension: name.clone(),
                    job: job.clone(),
                });
            }
            FromExtension::RequestActionCatalog => {
                let ext = &mut self.extensions[id.0];
                if !ext.found.manifest.action_catalog {
                    log::warn!("{name} asked for the Action catalog, but its Manifest doesn't");
                    return;
                }
                ext.catalog = true;
                let catalog = self.compute_catalog();
                self.extensions[id.0].send(ToExtension::ActionCatalog {
                    entries: catalog.clone(),
                });
                self.catalog = Some(catalog);
            }
        }
    }

    fn items_changed(&mut self) {
        self.catalog_dirty = true;
        self.mark_late();
    }

    fn mark_late(&mut self) {
        for session in self.sessions.values_mut() {
            if !session.query.is_empty() {
                session.update = session.update.max(Update::Late);
            }
        }
    }

    fn live_results(
        &mut self,
        id: ExtensionId,
        live_query: u64,
        results: Vec<LiveResult>,
        done: bool,
    ) {
        let Some(&(front_end, ext)) = self.live_queries.get(&live_query) else {
            return;
        };
        if ext != id {
            return;
        }
        let answers = self.extensions[id.0].debounce() == Some(Duration::ZERO);
        let Some(session) = self.sessions.get_mut(&front_end) else {
            return;
        };
        let live = &mut session.live[id.0];
        let room = MAX_LIVE_RESULTS.saturating_sub(live.results.len());
        if results.len() > room {
            log::warn!(
                "{}: dropping Live results past {MAX_LIVE_RESULTS} for one query",
                self.extensions[id.0].name()
            );
        }
        live.results
            .extend(results.into_iter().take(room).map(|result| LiveResult {
                answer: result.answer && answers,
                ..result
            }));
        if done {
            live.sent = None;
            self.live_queries.remove(&live_query);
        }
        session.update = session.update.max(Update::Late);
    }

    fn action_done(&mut self, id: ExtensionId, invocation: u64, outcome: Outcome, now: Instant) {
        if self.invocations.get(&invocation).map(|&(_, ext)| ext) != Some(id) {
            return;
        }
        let Some((front_end, _)) = self.invocations.remove(&invocation) else {
            return;
        };
        let session = self.sessions.get(&front_end);
        match outcome {
            Outcome::Close => {
                if let Some(session) = session {
                    session.send(ToFrontEnd::Close);
                }
            }
            Outcome::KeepOpen => {}
            Outcome::Failed { message } => {
                if let Some(session) = session {
                    session.send(ToFrontEnd::ActionFailed { message });
                }
            }
            Outcome::Type { text } => self.output(Output::Type(text), now),
            Outcome::Paste(clipboard) => self.output(Output::Paste(clipboard), now),
        }
    }

    /// Closes every Front end, then types or pastes once they have all closed,
    /// so the text lands in the window the user came from.
    fn output(&mut self, output: Output, now: Instant) {
        if self.pending_output.is_some() {
            self.deliver_output();
        }
        self.broadcast(|| ToFrontEnd::Close);
        self.pending_output = Some(PendingOutput {
            output,
            waiting: self.sessions.keys().copied().collect(),
            deadline: now + CLOSE_TIMEOUT,
        });
        self.closed(None);
    }

    /// A Front end answered Close, or went away.
    fn closed(&mut self, front_end: Option<FrontEndId>) {
        let Some(pending) = &mut self.pending_output else {
            return;
        };
        pending.waiting.retain(|&id| Some(id) != front_end);
        if pending.waiting.is_empty() {
            self.deliver_output();
        }
    }

    fn deliver_output(&mut self) {
        if let Some(pending) = self.pending_output.take() {
            self.desktop.output(pending.output);
        }
    }

    fn broadcast(&self, message: impl Fn() -> ToFrontEnd) {
        for session in self.sessions.values() {
            session.send(message());
        }
    }

    /// Sends to every Front end but the one that asked.
    fn broadcast_from(&self, from: FrontEndId, message: impl Fn() -> ToFrontEnd) {
        for (_, session) in self.sessions.iter().filter(|&(&id, _)| id != from) {
            session.send(message());
        }
    }

    fn front_end_up(
        &mut self,
        id: FrontEndId,
        sender: SyncSender<ToFrontEnd>,
        disconnect: Box<dyn FnOnce() + Send>,
    ) {
        let session = Session {
            sender,
            disconnect: Some(disconnect),
            hung: Cell::new(false),
            query: String::new(),
            key: String::new(),
            folded: rank::Query::default(),
            pattern: Pattern::default(),
            max_score: 0,
            rows: DEFAULT_ROWS,
            narrowed: Vec::new(),
            narrowed_query: String::new(),
            narrowed_version: 0,
            narrowed_subtitles: false,
            live: self.extensions.iter().map(|_| Live::default()).collect(),
            shown: Vec::new(),
            via_alias: None,
            update: Update::None,
        };
        session.send(ToFrontEnd::Status {
            down: self.down.clone(),
        });
        for ((ext, _), job) in &self.jobs {
            session.send(ToFrontEnd::JobProgress {
                extension: self.extensions[ext.0].name().to_owned(),
                job: job.clone(),
            });
        }
        self.sessions.insert(id, session);
    }

    fn front_end_down(&mut self, id: FrontEndId) {
        if let Some(session) = self.sessions.remove(&id) {
            for (ext, live) in session.live.iter().enumerate() {
                if let Some(query) = live.sent {
                    self.live_queries.remove(&query);
                    self.extensions[ext].send(ToExtension::CancelLiveQuery { id: query });
                }
            }
        }
        self.closed(Some(id));
    }

    fn front_end_message(&mut self, id: FrontEndId, message: FromFrontEnd, now: Instant) {
        match message {
            FromFrontEnd::Query { text } => self.query(id, text, now),
            FromFrontEnd::Viewport { top, rows } => {
                if let Some(session) = self.sessions.get_mut(&id) {
                    // Rows scrolled past are protected as much as those on screen.
                    session.rows = top as usize + rows as usize;
                }
            }
            FromFrontEnd::RunAction {
                result,
                action,
                arguments,
            } => {
                let run = if result.extension == OWN {
                    self.run_own(id, &result, &action, arguments, now)
                } else {
                    self.run_action(id, &result, &action, arguments, now)
                };
                if let Err(message) = run
                    && let Some(session) = self.sessions.get(&id)
                {
                    session.send(ToFrontEnd::ActionFailed { message });
                }
            }
            FromFrontEnd::SubmitForm {
                extension,
                view,
                values,
            } => {
                if let Some(ext) = self.extension_named(&extension) {
                    self.extensions[ext.0].send(ToExtension::SubmitForm { view, values });
                }
            }
            FromFrontEnd::ViewPopped { extension, view } => {
                if let Some(ext) = self.extension_named(&extension) {
                    self.extensions[ext.0].send(ToExtension::ViewPopped { view });
                }
            }
            FromFrontEnd::Closed => self.closed(Some(id)),
            FromFrontEnd::Open { query, view } => self.broadcast_from(id, || ToFrontEnd::Open {
                query: query.clone(),
                view: view.clone(),
            }),
            FromFrontEnd::Toggle => self.broadcast_from(id, || ToFrontEnd::Toggle),
        }
    }

    fn extension_named(&self, name: &str) -> Option<ExtensionId> {
        self.extensions
            .iter()
            .position(|ext| ext.name() == name && !matches!(ext.state, State::Removed))
            .map(ExtensionId)
    }

    /// Brings the running Extensions in line with what is installed: new ones
    /// start, removed ones stop for good, and one whose Manifest changed
    /// restarts at once with the new Manifest.
    fn set_extensions(&mut self, found: Vec<Found>, now: Instant) {
        for id in 0..self.extensions.len() {
            let ext = &self.extensions[id];
            if !matches!(ext.state, State::Removed)
                && !found.iter().any(|f| f.manifest.name == ext.name())
            {
                log::info!("extension {} was removed", ext.name());
                self.retire(ExtensionId(id));
            }
        }
        for found in found {
            match self
                .extensions
                .iter()
                .position(|ext| ext.name() == found.manifest.name)
            {
                Some(id) => {
                    let ext = &mut self.extensions[id];
                    let removed = matches!(ext.state, State::Removed);
                    if !removed
                        && ext.found.manifest == found.manifest
                        && ext.found.dir == found.dir
                    {
                        continue;
                    }
                    log::info!("extension {} changed; restarting it", found.manifest.name);
                    ext.found = found;
                    ext.failures = 0;
                    if !removed {
                        self.retire(ExtensionId(id));
                    }
                    self.start(ExtensionId(id), now);
                }
                None => {
                    log::info!("extension {} was installed", found.manifest.name);
                    let id = ExtensionId(self.extensions.len());
                    self.extensions.push(Extension::new(found, now));
                    self.index.add_extension();
                    for session in self.sessions.values_mut() {
                        session.live.push(Live::default());
                    }
                    self.start(id, now);
                }
            }
        }
    }

    /// Ends the current run of an Extension, leaving it removed until started
    /// again: stops its process and forgets what it left behind. Moving on to the next generation means nothing
    /// from the old run, its ExtensionDown included, is heard any more.
    fn retire(&mut self, id: ExtensionId) {
        let ext = &mut self.extensions[id.0];
        if matches!(ext.state, State::Starting { .. } | State::Up { .. }) {
            self.launcher.stop(id, ext.generation);
        }
        ext.generation += 1;
        ext.state = State::Removed;
        self.forget_run(id);
    }

    fn query(&mut self, id: FrontEndId, text: String, now: Instant) {
        if !self.sessions.contains_key(&id) {
            return;
        }
        if self.history.settings().usage_log && !text.is_empty() {
            self.store.log(Entry {
                at: self.wall(now),
                used: Used::Query {
                    query: text.clone(),
                },
            });
        }
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        // Folded like the haystacks, so neither case nor accents matter.
        session.pattern = Pattern::parse(
            &rank::fold_text(&text),
            CaseMatching::Ignore,
            Normalization::Never,
        );
        session.folded = rank::Query::new(&text);
        session.max_score = session
            .pattern
            .atoms
            .iter()
            .filter_map(|atom| {
                let needle = atom.needle_text();
                self.matcher.fuzzy_match(needle, needle)
            })
            .map(u32::from)
            .sum();
        session.key = query_key(&text);
        session.query = text;
        session.update = Update::Fresh;
        for (ext_id, ext) in self.extensions.iter().enumerate() {
            let live = &mut session.live[ext_id];
            if let Some(query) = live.sent.take() {
                self.live_queries.remove(&query);
                ext.send(ToExtension::CancelLiveQuery { id: query });
            }
            *live = Live::default();
            if ext.sender().is_none() || session.query.is_empty() {
                continue;
            }
            if let (Some(debounce), Some((query, prefix))) =
                (ext.debounce(), ext.route(&session.query))
            {
                live.query = query;
                live.prefix = prefix;
                live.due = Some(now + debounce);
            }
        }
    }

    fn run_action(
        &mut self,
        front_end: FrontEndId,
        result: &ResultRef,
        action: &ActionRef,
        arguments: BTreeMap<String, String>,
        now: Instant,
    ) -> Result<(), String> {
        let gone = || "that Result is gone".to_owned();
        let owner = self.extension_named(&result.extension).ok_or_else(gone)?;
        let session = self.sessions.get(&front_end).ok_or_else(gone)?;
        let item = match self.index.find(owner, &result.id) {
            Some(slot) => self.index.entry(slot).map(|entry| &entry.item),
            None => session.live[owner.0]
                .results
                .iter()
                .rev()
                .find(|live| live.item.id == result.id)
                .map(|live| &live.item),
        }
        .ok_or_else(gone)?;

        let target = if action.extension == result.extension
            && item.actions.iter().any(|own| own.id == action.id)
        {
            Some(owner)
        } else {
            self.extension_named(&action.extension).filter(|&ext| {
                ext != owner
                    && self.contributed.get(item.kind.name()).is_some_and(|list| {
                        list.iter()
                            .any(|(by, offered)| *by == ext && offered.id == action.id)
                    })
            })
        }
        .ok_or_else(|| format!("`{}` has no Action `{}`", item.title, action.id))?;

        let ext = &self.extensions[target.0];
        if ext.sender().is_none() {
            return Err(format!("{} isn't running", ext.name()));
        }
        let invocation = self.next_id;
        self.next_id += 1;
        let answering = ext.send(ToExtension::RunAction {
            invocation,
            action: action.id.clone(),
            item: item.to_ref(),
            arguments,
        });
        // An Extension that stopped reading is stopped once this event is
        // handled, so the Action fails now rather than when its ExtensionDown
        // arrives.
        if !answering {
            return Err(format!("{} stopped answering", ext.name()));
        }
        self.invocations.insert(invocation, (front_end, target));
        self.picked(front_end, result, action, now);
        Ok(())
    }

    /// Runs an Action on one of the Daemon's own Results, then closes the menu.
    fn run_own(
        &mut self,
        front_end: FrontEndId,
        result: &ResultRef,
        action: &ActionRef,
        arguments: BTreeMap<String, String>,
        now: Instant,
    ) -> Result<(), String> {
        let gone = || "that Result is gone".to_owned();
        let session = self.sessions.get(&front_end).ok_or_else(gone)?;
        let n = self
            .own
            .iter()
            .position(|own| own.item.id == result.id)
            .ok_or_else(gone)?;
        let item = &self.own[n].item;
        if action.extension != OWN || !item.actions.iter().any(|own| own.id == action.id) {
            return Err(format!("`{}` has no Action `{}`", item.title, action.id));
        }
        match self.own[n].does {
            OwnAction::Quicklink(quicklink) => {
                let quicklink = &self.keywords.quicklinks[quicklink];
                let query = match arguments.get("query") {
                    Some(query) => query.as_str(),
                    None => after_keyword(&session.query, &quicklink.keyword).unwrap_or(""),
                };
                self.desktop.launch(quicklink.command(query));
                session.send(ToFrontEnd::Close);
                self.picked(front_end, result, action, now);
            }
            OwnAction::ClearHistory => {
                session.send(ToFrontEnd::Close);
                self.history.clear();
                self.store.save(&self.history);
            }
        }
        Ok(())
    }

    /// Records that the user picked a Result in a Front end's session, by
    /// running `action`. A Result an Alias named counts for the Alias alone.
    fn picked(
        &mut self,
        front_end: FrontEndId,
        result: &ResultRef,
        action: &ActionRef,
        now: Instant,
    ) {
        let at = self.wall(now);
        let Some(session) = self.sessions.get(&front_end) else {
            return;
        };
        let source = self.source_of(session, &result.extension, &result.id);
        if self.history.settings().usage_log {
            self.store.log(Entry {
                at,
                used: Used::Pick {
                    query: session.query.clone(),
                    extension: result.extension.clone(),
                    id: result.id.clone(),
                    action: action.id.clone(),
                    row: session
                        .shown
                        .iter()
                        .position(|(shown, _)| Some(*shown) == source),
                },
            });
        }
        if let Some((_, alias)) = session
            .via_alias
            .as_ref()
            .filter(|(named, _)| Some(*named) == source)
        {
            self.history.touch(OWN, &alias_id(alias), at);
        } else {
            let key = source
                .and_then(|source| self.item(session, source))
                .map(|(extension, item)| history_key(extension, item))
                .unwrap_or((&result.extension, &result.id));
            // What ranked above it, nearest first: what it was picked over.
            // The same thing from another Extension wasn't.
            let above: Vec<ResultKey> = session
                .shown
                .iter()
                .position(|(shown, _)| Some(*shown) == source)
                .map(|at| {
                    session.shown[..at]
                        .iter()
                        .rev()
                        .filter_map(|&(shown, _)| self.item(session, shown))
                        .map(|(extension, item)| history_key(extension, item))
                        .filter(|&above| above != key)
                        .map(|(extension, id)| (extension.to_owned(), id.to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            let (extension, id) = (key.0.to_owned(), key.1.to_owned());
            let query = session.key.clone();
            self.history.pick(&extension, &id, &query, &above, at);
            if extension == IDENTITIES {
                // The Result itself too, so it goes ahead of the same thing
                // from other Extensions.
                self.history.touch(&result.extension, &result.id, at);
            }
        }
        self.store.save(&self.history);
    }

    fn set_config(&mut self, config: Table) {
        self.config = config;
        self.load_keywords();
        self.load_settings();
        for ext in &mut self.extensions {
            if ext.sender().is_none() {
                continue;
            }
            let table = extension_table(&self.config, ext.name());
            if ext.config.as_ref() != Some(&table) {
                ext.send(ToExtension::Config(table.clone()));
                ext.config = Some(table);
            }
        }
    }

    /// Reads the History's settings from the config. Open sessions are ranked
    /// again if they changed.
    fn load_settings(&mut self) {
        let settings = Settings::from_config(&self.config);
        if settings != *self.history.settings() {
            self.history.set_settings(settings);
            self.mark_late();
        }
    }

    /// Reads the Aliases and Quicklinks from the config, and rebuilds the
    /// Daemon's own Items from them.
    fn load_keywords(&mut self) {
        let keywords = keywords::from_config(&self.config);
        if keywords == self.keywords && !self.own.is_empty() {
            return;
        }
        self.own = keywords
            .quicklinks
            .iter()
            .enumerate()
            .map(|(n, quicklink)| Own::new(quicklink.item(), OwnAction::Quicklink(n)))
            .chain([Own::new(clear_history(), OwnAction::ClearHistory)])
            .collect();
        self.keywords = keywords;
        // The own Items' positions changed, so rows showing them can't be
        // kept where they are.
        for session in self.sessions.values_mut() {
            session
                .shown
                .retain(|(source, _)| !matches!(source, Source::Own(_)));
        }
        self.mark_late();
    }

    /// Sends what the last event changed: due Live queries, Results, the Action
    /// catalog and the status.
    fn flush(&mut self, now: Instant) {
        self.send_due_live_queries(now);

        let ids: Vec<FrontEndId> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.update != Update::None)
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            self.refresh(id, now);
        }

        if self.catalog_dirty {
            self.catalog_dirty = false;
            if self.extensions.iter().any(|ext| ext.catalog) {
                let catalog = self.compute_catalog();
                if self.catalog.as_ref() != Some(&catalog) {
                    for ext in self.extensions.iter().filter(|ext| ext.catalog) {
                        ext.send(ToExtension::ActionCatalog {
                            entries: catalog.clone(),
                        });
                    }
                    self.catalog = Some(catalog);
                }
            } else {
                self.catalog = None;
            }
        }

        let hung: Vec<FrontEndId> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.hung.get())
            .map(|(&id, _)| id)
            .collect();
        for id in hung {
            log::warn!("front end {} stopped reading; disconnecting it", id.0);
            if let Some(disconnect) = self
                .sessions
                .get_mut(&id)
                .and_then(|session| session.disconnect.take())
            {
                disconnect();
            }
            self.front_end_down(id);
        }

        for (id, ext) in self.extensions.iter_mut().enumerate() {
            if ext.hung.get() && !ext.stopping {
                log::warn!("{} stopped answering; stopping it", ext.name());
                ext.stopping = true;
                self.launcher.stop(ExtensionId(id), ext.generation);
            }
        }

        let down: Vec<String> = self
            .extensions
            .iter()
            .filter(|ext| ext.is_down())
            .map(|ext| ext.name().to_owned())
            .collect();
        if down != self.down {
            self.down = down;
            self.broadcast(|| ToFrontEnd::Status {
                down: self.down.clone(),
            });
        }
    }

    fn send_due_live_queries(&mut self, now: Instant) {
        for (&front_end, session) in &mut self.sessions {
            for (ext_id, live) in session.live.iter_mut().enumerate() {
                if !live.due.is_some_and(|due| due <= now) {
                    continue;
                }
                live.due = None;
                let id = self.next_id;
                self.next_id += 1;
                live.sent = Some(id);
                self.live_queries
                    .insert(id, (front_end, ExtensionId(ext_id)));
                self.extensions[ext_id].send(ToExtension::LiveQuery {
                    id,
                    query: live.query.clone(),
                    prefix: live.prefix.clone(),
                });
            }
        }
    }

    /// Ranks one session's Results and sends them. Results that come late
    /// never move the rows on screen (ADR 0003): those stay where they are,
    /// and everything new goes below them, except an answer, which takes the
    /// top row.
    fn refresh(&mut self, id: FrontEndId, now: Instant) {
        let Some(mut session) = self.sessions.remove(&id) else {
            return;
        };
        let fresh = session.update == Update::Fresh;
        session.update = Update::None;

        let mut candidates = Vec::new();
        if !session.query.is_empty() {
            self.scored.clear();
            let within = session
                .can_narrow(self.index.version())
                .then_some(session.narrowed.as_slice());
            self.index.search(
                &session.pattern,
                &mut self.matcher,
                session.folded.filter(),
                within,
                &mut self.scored,
            );
            session.narrowed.clear();
            session
                .narrowed
                .extend(self.scored.iter().map(|&(slot, ..)| slot));
            session.narrowed_query.clone_from(&session.query);
            session.narrowed_version = self.index.version();
            session.narrowed_subtitles = session.folded.searches_subtitles();
            for &(slot, score, matched) in &self.scored {
                if let Some(entry) = self.index.entry(slot)
                    && let Some(tier) =
                        self.tiers
                            .item(&entry.haystack, &entry.item, &session.folded, matched)
                {
                    let len = entry.haystack.title.len() as u32;
                    candidates.push(Candidate::new(Source::Indexed(slot), tier, score, len));
                }
            }
        }
        // Up to here the candidates are in slot order.
        let indexed = candidates.len();
        for (ext, live) in session.live.iter().enumerate() {
            if live.results.is_empty() {
                continue;
            }
            // Matched against what the Extension was asked, without its
            // routing prefix. Its own idea of what matches stands, at worst as
            // letters scattered.
            let asked = rank::Query::new(&live.query);
            for (n, result) in live.results.iter().enumerate() {
                let haystack = Haystack::new(&result.item);
                let tier = self
                    .tiers
                    .item(&haystack, &result.item, &asked, Matched::ALL)
                    .unwrap_or(Tier::Scattered);
                let score = (result.score.get() * session.max_score as f32).round() as u32;
                let len = result.item.title.chars().count() as u32;
                let mut candidate =
                    Candidate::new(Source::Live(ExtensionId(ext), n as u32), tier, score, len);
                candidate.answer = result.answer;
                candidates.push(candidate);
            }
        }
        if !session.query.is_empty() {
            for (n, own) in self.own.iter().enumerate() {
                let filter = session.folded.filter();
                if let Some((score, matched)) =
                    own.haystack
                        .score(&session.pattern, &mut self.matcher, filter)
                    && let Some(tier) =
                        self.tiers
                            .item(&own.haystack, &own.item, &session.folded, matched)
                {
                    let len = own.haystack.title.len() as u32;
                    candidates.push(Candidate::new(Source::Own(n as u32), tier, score, len));
                }
            }
        }
        let primary = self.learn(&mut session, &mut candidates, indexed, now);

        let mut answers: Vec<Candidate> = Vec::new();
        candidates.retain(|candidate| {
            if candidate.answer {
                answers.push(*candidate);
            }
            !candidate.answer
        });
        answers.sort_unstable_by(Candidate::rank);

        let mut kept: Vec<Candidate> = Vec::new();
        if !fresh {
            let visible = session.shown.iter().take(session.rows);
            for (source, _) in visible.filter(|(_, answer)| !answer) {
                if let Some(at) = candidates.iter().position(|c| c.source == *source) {
                    kept.push(candidates.swap_remove(at));
                }
            }
        }

        let room = MAX_RESULTS.saturating_sub(answers.len() + kept.len());
        if candidates.len() > room && room > 0 {
            candidates.select_nth_unstable_by(room - 1, Candidate::rank);
        }
        candidates.truncate(room);
        candidates.sort_unstable_by(Candidate::rank);
        self.lead_with_direct_picks(&session, &mut candidates);

        let ranked: Vec<Candidate> = answers.into_iter().chain(kept).chain(candidates).collect();
        session.shown = ranked
            .iter()
            .map(|candidate| (candidate.source, candidate.answer))
            .collect();

        let results = ranked
            .iter()
            .filter_map(|candidate| {
                let primary = primary
                    .as_ref()
                    .filter(|(source, _)| *source == candidate.source)
                    .map(|(_, action)| action.as_str());
                self.row(&session, candidate.source, primary)
            })
            .collect();
        session.send(ToFrontEnd::Results { results });
        self.sessions.insert(id, session);
    }

    /// Applies what the Daemon learned from the user, and the user's
    /// keywords, to the candidates. Frecency breaks ties within a tier and,
    /// from [`Settings::lift_threshold`], lifts a Result a tier. What the query names by an
    /// Alias goes first, with exact matches the Learned choice wasn't picked
    /// over, then the Learned choice. The candidates before
    /// `indexed` must be the Indexed items, in slot order. Returns the Action
    /// an Alias makes primary, with the Result it is for.
    fn learn(
        &mut self,
        session: &mut Session,
        candidates: &mut Vec<Candidate>,
        indexed: usize,
        now: Instant,
    ) -> Option<(Source, String)> {
        session.via_alias = None;
        if session.key.is_empty() {
            return None;
        }
        let at = self.wall(now);
        if !self.history.is_empty() {
            let Settings {
                half_life_secs,
                lift_threshold,
                ..
            } = *self.history.settings();
            self.cache_frecent();
            for &(slot, picks, counts) in &self.frecent {
                if let Ok(n) =
                    candidates[..indexed].binary_search_by_key(&Source::Indexed(slot), |c| c.source)
                {
                    let frecency = picks.frecency(at, half_life_secs);
                    if counts != Counts::Direct {
                        candidates[n].frecency = frecency;
                    }
                    if counts != Counts::Shared {
                        candidates[n].direct = frecency;
                    }
                }
            }
            for candidate in &mut candidates[indexed..] {
                if let Some((extension, item)) = self.item(session, candidate.source) {
                    (candidate.frecency, candidate.direct) = self.frecency(extension, item, at);
                }
            }
            for candidate in candidates.iter_mut() {
                if candidate.frecency >= lift_threshold {
                    candidate.tier = rank::lift(candidate.tier);
                }
            }
        }

        // What the query names: an Alias, typed alone or before an Argument,
        // goes first. A Quicklink's keyword or an Item's own keyword counts as
        // an exact match, which a Learned choice can beat.
        let first = session.key.split(' ').next().unwrap_or_default();
        let mut named: Vec<Keyword> = Vec::new();
        let mut primary = None;
        let alias = self
            .keywords
            .aliases
            .get_key_value(session.key.as_str())
            .or_else(|| self.keywords.aliases.get_key_value(first));
        if let Some((keyword, alias)) = alias
            && let Some(source) = self.source_of(session, &alias.extension, &alias.item)
        {
            named.push(Keyword {
                source,
                alias: true,
                frecency: Some(self.history.frecency(OWN, &alias_id(keyword), at)),
            });
            primary = alias.action.clone().map(|action| (source, action));
            session.via_alias = Some((source, keyword.clone()));
        }
        if let Some(n) = self
            .keywords
            .quicklinks
            .iter()
            .position(|quicklink| quicklink.keyword == first)
        {
            named.push(Keyword {
                source: Source::Own(n as u32),
                alias: false,
                frecency: None,
            });
        }
        for &slot in self.index.keyed(first) {
            named.push(Keyword {
                source: Source::Indexed(slot),
                alias: false,
                frecency: None,
            });
        }
        for Keyword {
            source,
            alias,
            frecency,
        } in named
        {
            let at_n = match candidates.iter().position(|c| c.source == source) {
                Some(n) => n,
                None => {
                    let Some((extension, item)) = self.item(session, source) else {
                        continue;
                    };
                    let mut candidate = Candidate::new(
                        source,
                        Tier::Exact,
                        session.max_score,
                        item.title.chars().count() as u32,
                    );
                    (candidate.frecency, candidate.direct) = self.frecency(extension, item, at);
                    candidates.push(candidate);
                    candidates.len() - 1
                }
            };
            let candidate = &mut candidates[at_n];
            if alias {
                candidate.class = Class::Named;
                candidate.by_alias = true;
            }
            candidate.tier = Tier::Exact.coarse();
            candidate.fine = Tier::Exact;
            if let Some(frecency) = frecency {
                candidate.frecency = frecency;
            }
        }

        // The strongest Learned choice that applies and is among the
        // candidates: every Result of it, when several Extensions offer it.
        let choices = self.history.learned(&session.key, at);
        let mut passed_over = None;
        for choice in &choices {
            for source in self.sources_of(session, choice.extension, choice.id) {
                if let Some(candidate) = candidates.iter_mut().find(|c| c.source == source) {
                    if candidate.class == Class::Rest {
                        // One that matches exactly, or is used enough to rank
                        // as if it did, goes with the exact matches.
                        candidate.class = if candidate.tier == Tier::Exact.coarse() {
                            Class::Named
                        } else {
                            Class::Learned
                        };
                    }
                    passed_over = Some(choice.passed_over);
                }
            }
            if passed_over.is_some() {
                break;
            }
        }
        // Exact matches go first, unless the Learned choice was picked over
        // them. With a Learned choice, only a true exact match does: one that
        // Frecency lifted to the tier doesn't.
        for candidate in candidates.iter_mut() {
            if candidate.class != Class::Rest || candidate.tier != Tier::Exact.coarse() {
                continue;
            }
            let named = match passed_over {
                None => true,
                Some(passed_over) => {
                    candidate.fine == Tier::Exact
                        && !self
                            .item(session, candidate.source)
                            .is_some_and(|(extension, item)| {
                                let (extension, id) = history_key(extension, item);
                                passed_over.iter().any(|(e, i)| e == extension && i == id)
                            })
                }
            };
            if named {
                candidate.class = Class::Named;
            }
        }
        primary
    }

    /// Rebuilds the list of picked Indexed items if the index or the History
    /// changed since it was built.
    fn cache_frecent(&mut self) {
        let versions = (self.index.version(), self.history.version());
        if self.frecent_versions == Some(versions) {
            return;
        }
        let mut frecent = std::mem::take(&mut self.frecent);
        frecent.clear();
        for (extension, id, picks) in self.history.picked() {
            if extension == IDENTITIES {
                let slots = self.index.identified(id);
                frecent.extend(slots.iter().map(|&slot| (slot, picks, Counts::Shared)));
            } else if let Some(ext) = self.extension_named(extension)
                && let Some(slot) = self.index.find(ext, id)
            {
                let identified = self
                    .index
                    .entry(slot)
                    .is_some_and(|entry| entry.item.identity.is_some());
                let counts = if identified {
                    Counts::Direct
                } else {
                    Counts::Both
                };
                frecent.push((slot, picks, counts));
            }
        }
        frecent.sort_unstable_by_key(|&(slot, ..)| slot);
        self.frecent = frecent;
        self.frecent_versions = Some(versions);
    }

    /// Where the Result named by an Extension's name and an Item id comes from
    /// in this session, if it is there.
    fn source_of(&self, session: &Session, extension: &str, id: &str) -> Option<Source> {
        if extension == OWN {
            return self
                .own
                .iter()
                .position(|own| own.item.id == id)
                .map(|n| Source::Own(n as u32));
        }
        let ext = self.extension_named(extension)?;
        if let Some(slot) = self.index.find(ext, id) {
            return Some(Source::Indexed(slot));
        }
        session.live[ext.0]
            .results
            .iter()
            .position(|live| live.item.id == id)
            .map(|n| Source::Live(ext, n as u32))
    }

    /// Where the Results the History keeps under `extension` and `id` come
    /// from in this session: every one of the Identity's, for [`IDENTITIES`],
    /// or else the one [`source_of`](Self::source_of) finds.
    fn sources_of(&self, session: &Session, extension: &str, id: &str) -> Vec<Source> {
        if extension != IDENTITIES {
            return self.source_of(session, extension, id).into_iter().collect();
        }
        let indexed = self
            .index
            .identified(id)
            .iter()
            .copied()
            .map(Source::Indexed);
        let live = session.live.iter().enumerate().flat_map(|(ext, live)| {
            live.results
                .iter()
                .enumerate()
                .filter(|(_, result)| result.item.identity.as_deref() == Some(id))
                .map(move |(n, _)| Source::Live(ExtensionId(ext), n as u32))
        });
        indexed.chain(live).collect()
    }

    /// Reorders the Results of each Identity among the places they ranked
    /// in, so the one the user picked through most takes the best of them:
    /// the thing ranks by its best Result, and leads with the user's way to
    /// it. Everything else stays put.
    fn lead_with_direct_picks(&self, session: &Session, ranked: &mut [Candidate]) {
        if !ranked.iter().any(|candidate| candidate.direct > 0.0) {
            return;
        }
        let identity = |candidate: &Candidate| {
            self.item(session, candidate.source)
                .and_then(|(_, item)| item.identity.as_deref())
        };
        let mut done: Vec<&str> = Vec::new();
        for first in 0..ranked.len() {
            let Some(id) = identity(&ranked[first]) else {
                continue;
            };
            if done.contains(&id) {
                continue;
            }
            done.push(id);
            let places: Vec<usize> = (first..ranked.len())
                .filter(|&n| identity(&ranked[n]) == Some(id))
                .collect();
            if places.len() < 2 {
                continue;
            }
            let mut group: Vec<Candidate> = places.iter().map(|&n| ranked[n]).collect();
            // Stable, so equal direct picks keep their ranked order.
            group.sort_by(|a, b| b.direct.total_cmp(&a.direct));
            for (&n, candidate) in places.iter().zip(group) {
                ranked[n] = candidate;
            }
        }
    }

    /// A Result's Frecency, shared with every Result of its Identity, and
    /// that of picks of the Result itself.
    fn frecency(&self, extension: &str, item: &Item, at: u64) -> (f64, f64) {
        let own = self.history.frecency(extension, &item.id, at);
        match &item.identity {
            Some(identity) => (self.history.frecency(IDENTITIES, identity, at), own),
            None => (own, own),
        }
    }

    /// A Result's Item and the name of the Extension that owns it.
    fn item<'a>(&'a self, session: &'a Session, source: Source) -> Option<(&'a str, &'a Item)> {
        match source {
            Source::Indexed(slot) => {
                let entry = self.index.entry(slot)?;
                Some((self.extensions[entry.extension.0].name(), &entry.item))
            }
            Source::Live(ext, n) => Some((
                self.extensions[ext.0].name(),
                &session.live[ext.0].results.get(n as usize)?.item,
            )),
            Source::Own(n) => Some((OWN, &self.own.get(n as usize)?.item)),
        }
    }

    /// One Result as the Front end gets it. `primary`, one of the Item's own
    /// Actions, is moved to the front.
    fn row(
        &mut self,
        session: &Session,
        source: Source,
        primary: Option<&str>,
    ) -> Option<ShownResult> {
        let live_title;
        let (owner, item, haystack): (Option<ExtensionId>, &Item, Utf32Str) = match source {
            Source::Indexed(slot) => {
                let entry = self.index.entry(slot)?;
                (
                    Some(entry.extension),
                    &entry.item,
                    entry.haystack.title.slice(..),
                )
            }
            Source::Live(ext, n) => {
                let item = &session.live[ext.0].results.get(n as usize)?.item;
                live_title = rank::folded(&item.title);
                (Some(ext), item, live_title.slice(..))
            }
            Source::Own(n) => {
                let own = self.own.get(n as usize)?;
                (None, &own.item, own.haystack.title.slice(..))
            }
        };
        self.highlights.clear();
        if session
            .pattern
            .indices(haystack, &mut self.matcher, &mut self.highlights)
            .is_some()
        {
            self.highlights.sort_unstable();
            self.highlights.dedup();
        } else {
            self.highlights.clear();
        }
        let mut item = item.clone();
        if let Source::Own(n) = source
            && let Some(OwnAction::Quicklink(quicklink)) =
                self.own.get(n as usize).map(|own| own.does)
        {
            let quicklink = &self.keywords.quicklinks[quicklink];
            if quicklink.takes_query()
                && let Some(query) = after_keyword(&session.query, &quicklink.keyword)
                && !query.is_empty()
            {
                item.title = format!("{} for “{query}”", item.title);
            }
        }
        if let Some(at) = primary.and_then(|id| item.actions.iter().position(|a| a.id == id)) {
            item.actions[..=at].rotate_right(1);
        }
        let (extension, contributed) = match owner {
            Some(owner) => (
                self.extensions[owner.0].name().to_owned(),
                self.contributed_for(owner, &item),
            ),
            None => (OWN.to_owned(), Vec::new()),
        };
        Some(ShownResult {
            extension,
            item,
            contributed,
            highlights: self.highlights.clone(),
        })
    }

    /// Other running Extensions' Actions for `item`'s kind, in Extension order.
    /// When a shortcut clashes, the owner's binding wins, then the earlier
    /// Extension's.
    fn contributed_for(&self, owner: ExtensionId, item: &Item) -> Vec<OfferedAction> {
        let Some(offered) = self.contributed.get(item.kind.name()) else {
            return Vec::new();
        };
        let mut taken: Vec<&str> = item
            .actions
            .iter()
            .filter_map(|action| action.shortcut.as_deref())
            .collect();
        offered
            .iter()
            .filter(|(ext, _)| *ext != owner)
            .map(|(ext, offered)| {
                let mut action = offered.clone();
                if let Some(shortcut) = offered.shortcut.as_deref() {
                    if taken.contains(&shortcut) {
                        action.shortcut = None;
                    } else {
                        taken.push(shortcut);
                    }
                }
                OfferedAction {
                    extension: self.extensions[ext.0].name().to_owned(),
                    action,
                }
            })
            .collect()
    }

    /// Every Action on offer from running Extensions: their Contributed
    /// actions, then the Actions on their Indexed items, one entry per Item
    /// kind and Action id. Never the Items themselves.
    fn compute_catalog(&self) -> Vec<CatalogEntry> {
        let mut entries = Vec::new();
        for (id, ext) in self.extensions.iter().enumerate() {
            if ext.sender().is_none() {
                continue;
            }
            for contributed in &ext.found.manifest.contributed_actions {
                entries.push(CatalogEntry {
                    extension: ext.name().to_owned(),
                    kind: contributed.kind.clone(),
                    action: contributed.action.clone(),
                });
            }
            let mut own: BTreeMap<(&str, &str), &Action> = BTreeMap::new();
            for item in self.index.items(ExtensionId(id)) {
                for action in &item.actions {
                    own.entry((item.kind.name(), &action.id)).or_insert(action);
                }
            }
            for ((kind, _), action) in own {
                if let Ok(kind) = KindName::new(kind) {
                    entries.push(CatalogEntry {
                        extension: ext.name().to_owned(),
                        kind,
                        action: action.clone(),
                    });
                }
            }
        }
        entries
    }
}
