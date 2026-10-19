//! The Daemon's state machine, fed one [`Event`] at a time by the main
//! loop. It owns the index, the Front ends' sessions and the Extensions' state,
//! and never blocks: everything it sends goes into a channel that another
//! thread drains, so a stuck or dead Extension can't stall it.
//!
//! Starting processes and typing into windows are behind [`Launcher`] and
//! [`Desktop`], so it runs the same in tests as in `kotodamad`.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::{Duration, Instant};

use kotodama_protocol::{
    Action, ActionRef, CatalogEntry, Clipboard, FromExtension, FromFrontEnd, Item, Job, KindName,
    LiveResult, OfferedAction, Outcome, ResultRef, ShownResult, Table, ToExtension, ToFrontEnd,
};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str, Utf32String};

use crate::config::extension_table;
use crate::discovery::Found;
use crate::index::{Index, Slot};

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

/// Types or pastes into the focused window.
pub trait Desktop {
    fn output(&mut self, output: Output);
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
    /// way.
    fn send(&self, message: ToExtension) {
        if let Some(sender) = self.sender()
            && let Err(TrySendError::Full(_)) = sender.try_send(message)
        {
            self.hung.set(true);
        }
    }

    /// Stopped, or restarting after it stopped: its Results are gone.
    fn is_down(&self) -> bool {
        match self.state {
            State::Down { .. } => true,
            State::Starting { .. } => self.generation > 1,
            State::Up { .. } => false,
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
}

#[derive(Clone, Copy)]
struct Candidate {
    source: Source,
    score: u32,
    answer: bool,
    /// The title's length in characters: a shorter title wins a tie.
    len: u32,
}

impl Candidate {
    fn rank(&self, other: &Candidate) -> std::cmp::Ordering {
        other
            .answer
            .cmp(&self.answer)
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
    /// By Extension.
    live: Vec<Live>,
    /// The Results last sent, in order, and whether each is an answer.
    shown: Vec<(Source, bool)>,
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
        !self.narrowed_query.is_empty()
            && self.narrowed_version == version
            && self.query.starts_with(&self.narrowed_query)
            && !self.query.contains(SYNTAX)
    }
}

struct PendingOutput {
    output: Output,
    /// The Front ends that haven't answered Close yet.
    waiting: Vec<FrontEndId>,
    deadline: Instant,
}

pub struct Daemon<L, D> {
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
    scored: Vec<(Slot, u32)>,
    highlights: Vec<u32>,
}

impl<L: Launcher, D: Desktop> Daemon<L, D> {
    /// Starts every Extension.
    pub fn new(found: Vec<Found>, config: Table, launcher: L, desktop: D, now: Instant) -> Self {
        let mut daemon = Daemon {
            index: Index::new(found.len()),
            extensions: found
                .into_iter()
                .map(|found| Extension {
                    found,
                    generation: 0,
                    state: State::Down { retry_at: now },
                    failures: 0,
                    config: None,
                    catalog: false,
                    hung: Cell::new(false),
                    stopping: false,
                })
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
            scored: Vec::new(),
            highlights: Vec::new(),
        };
        for id in 0..daemon.extensions.len() {
            daemon.start(ExtensionId(id), now);
        }
        daemon
    }

    /// The merged config.
    pub fn config(&self) -> &Table {
        &self.config
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
        let name = ext.name().to_owned();

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
            pattern: Pattern::default(),
            max_score: 0,
            rows: DEFAULT_ROWS,
            narrowed: Vec::new(),
            narrowed_query: String::new(),
            narrowed_version: 0,
            live: self.extensions.iter().map(|_| Live::default()).collect(),
            shown: Vec::new(),
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
                if let Err(message) = self.run_action(id, &result, &action, arguments)
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
        }
    }

    fn extension_named(&self, name: &str) -> Option<ExtensionId> {
        self.extensions
            .iter()
            .position(|ext| ext.name() == name)
            .map(ExtensionId)
    }

    fn query(&mut self, id: FrontEndId, text: String, now: Instant) {
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        session.pattern = Pattern::parse(&text, CaseMatching::Smart, Normalization::Smart);
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
        let sender = ext
            .sender()
            .ok_or_else(|| format!("{} isn't running", ext.name()))?;
        let invocation = self.next_id;
        self.next_id += 1;
        let _ = sender.send(ToExtension::RunAction {
            invocation,
            action: action.id.clone(),
            item: item.to_ref(),
            arguments,
        });
        self.invocations.insert(invocation, (front_end, target));
        Ok(())
    }

    fn set_config(&mut self, config: Table) {
        self.config = config;
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
            self.refresh(id);
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
    fn refresh(&mut self, id: FrontEndId) {
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
                within,
                &mut self.scored,
            );
            session.narrowed.clear();
            session
                .narrowed
                .extend(self.scored.iter().map(|&(slot, _)| slot));
            session.narrowed_query.clone_from(&session.query);
            session.narrowed_version = self.index.version();
            candidates.extend(self.scored.iter().map(|&(slot, score)| {
                Candidate {
                    source: Source::Indexed(slot),
                    score,
                    answer: false,
                    len: self
                        .index
                        .entry(slot)
                        .map_or(0, |entry| entry.title.len() as u32),
                }
            }));
        }
        for (ext, live) in session.live.iter().enumerate() {
            for (n, result) in live.results.iter().enumerate() {
                candidates.push(Candidate {
                    source: Source::Live(ExtensionId(ext), n as u32),
                    score: (result.score.get() * session.max_score as f32).round() as u32,
                    answer: result.answer,
                    len: result.item.title.chars().count() as u32,
                });
            }
        }

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

        let ranked: Vec<Candidate> = answers.into_iter().chain(kept).chain(candidates).collect();
        session.shown = ranked
            .iter()
            .map(|candidate| (candidate.source, candidate.answer))
            .collect();

        let results = ranked
            .iter()
            .filter_map(|candidate| self.row(&session, candidate.source))
            .collect();
        session.send(ToFrontEnd::Results { results });
        self.sessions.insert(id, session);
    }

    fn row(&mut self, session: &Session, source: Source) -> Option<ShownResult> {
        let (owner, item, title) = match source {
            Source::Indexed(slot) => {
                let entry = self.index.entry(slot)?;
                (entry.extension, &entry.item, None)
            }
            Source::Live(ext, n) => {
                let item = &session.live[ext.0].results.get(n as usize)?.item;
                (ext, item, Some(Utf32String::from(item.title.as_str())))
            }
        };
        self.highlights.clear();
        let haystack: Utf32Str = match (&title, source) {
            (Some(title), _) => title.slice(..),
            (None, Source::Indexed(slot)) => self.index.entry(slot)?.title.slice(..),
            (None, Source::Live(..)) => unreachable!(),
        };
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
        Some(ShownResult {
            extension: self.extensions[owner.0].name().to_owned(),
            item: item.clone(),
            contributed: self.contributed_for(owner, item),
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
