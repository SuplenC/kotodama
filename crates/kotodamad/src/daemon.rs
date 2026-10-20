//! The Daemon's state machine, fed one [`Event`] at a time by the main
//! loop. It owns the index, the Front ends' sessions and the Extensions' state,
//! and never blocks: everything it sends goes into a channel that another
//! thread drains, so a stuck or dead Extension can't stall it.
//!
//! Starting processes, typing into windows and saving the History are behind
//! [`Launcher`], [`Desktop`] and [`Store`], so it runs the same in tests as in
//! `kotodamad`.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc::{Sender, SyncSender, TrySendError};
use std::time::{Duration, Instant};

use kotodama_protocol::{
    Action, ActionRef, CatalogEntry, Clipboard, FromExtension, FromFrontEnd, Item, ItemKind, Job,
    JobState, KindName, LiveResult, NamespacedKind, OfferedAction, Outcome, ResultRef, ShownResult,
    Table, ToExtension, ToFrontEnd, View, ViewContent,
};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

use crate::arguments;
use crate::clock::{Clock, When};
use crate::config::extension_table;
use crate::discovery::Found;
use crate::empty_query::EmptyQuery;
use crate::history::{
    Change, Habit, History, IDENTITIES, Picks, ResultKey, Settings, Store, query_key,
};
use crate::index::{Index, ItemRef, Slot};
use crate::indexer::{Indexed, Indexer, Kept, Plan};
use crate::keywords::{self, Keywords};
use crate::kind_order::{KindOrder, KindPlaces};
use crate::open_view::{Opening, ViewName};
use crate::rank::{self, Haystack, Matched, Text, Tier, Tiers};
use crate::segment::Edit;
use crate::usage::{Entry, Used};
use crate::views::{Open, Views};

/// The name the Daemon's own Results go by: Quicklinks and the Action that
/// clears the History. No Extension may take it.
pub const OWN: &str = "kotodama";
/// The id of the Daemon's own Item that clears the History.
pub const CLEAR_HISTORY: &str = "clear-history";
/// The id of the Action the Daemon offers on every Result the History keeps
/// anything for, which forgets it.
pub const FORGET: &str = "forget";
/// The id of the Action the Daemon offers on an Extension's Results while the
/// History keeps anything for that Extension, which clears it.
pub const CLEAR_EXTENSION_HISTORY: &str = "clear-extension-history";
/// The id of the Daemon's own Item that undoes the latest change to the
/// History, while there is one to undo.
pub const UNDO: &str = "undo";
/// How long after a change to the History it can be undone.
pub const UNDO_FOR: Duration = Duration::from_secs(60);

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

/// Types or pastes into the focused window, copies, starts programs for the
/// user, and shows desktop notifications.
pub trait Desktop {
    fn output(&mut self, output: Output);
    /// Starts a program and its arguments, detached from the Daemon, as a
    /// Quicklink asks.
    fn launch(&mut self, command: Vec<String>);
    /// Shows a desktop notification, as when a job finishes.
    fn notify(&mut self, summary: String, body: String);
}

/// What an Action asked the Daemon to put into the window the user came from,
/// or on the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    Type(String),
    Paste(Clipboard),
    Copy(Clipboard),
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
    /// A new version of an Extension's Items, indexed off the keystroke path
    /// (see [`Daemon::index_in_background`]).
    Indexed(Indexed),
    /// Items to keep resident, read on the indexing thread: the hot ones,
    /// planned there, and those a keyword or an Alias names.
    Kept(Kept),
}

/// How many Results a Front end gets for one query.
pub const MAX_RESULTS: usize = 50;
/// How many rows a Front end shows until it says otherwise.
const DEFAULT_ROWS: usize = 10;
/// How long after a keystroke Late Results may still rerank the rows on
/// screen, unless the selection moves first. Most Live queries answer
/// within it, and the eye has barely found the top row by then.
pub const RERANK_FOR: Duration = Duration::from_millis(150);
/// How long to wait for Front ends to close before typing anyway.
const CLOSE_TIMEOUT: Duration = Duration::from_millis(500);
/// How long `kotodama open --view` waits for its View before opening the
/// menus on the search instead (ADR 0014). It covers starting the Extension,
/// as when the keypress also started the Daemon.
const OPEN_VIEW_TIMEOUT: Duration = Duration::from_secs(1);
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

/// How many Items are hot at most: kept resident, and read when the menu
/// opens (ADR 0017).
pub const HOT_CAPACITY: usize = 256;
/// How often the hot Items are worked out again, ahead of the time of day.
const HOT_EVERY: Duration = Duration::from_secs(5 * 60);
/// How soon after the Index or the History changes they are, once it
/// settles.
const HOT_SETTLE: Duration = Duration::from_secs(1);

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
    /// The Manifest's `live_query.fallback`, read once per Manifest: every
    /// refresh asks it.
    fallback: bool,
}

/// Whether `found`'s Manifest makes it a fallback, such as web search.
fn is_fallback(found: &Found) -> bool {
    found
        .manifest
        .live_query
        .as_ref()
        .is_some_and(|live| live.fallback)
}

impl Extension {
    fn new(found: Found, now: Instant) -> Self {
        Extension {
            fallback: is_fallback(&found),
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

    /// A fallback, such as web search: also asked what real matches leave
    /// rows empty for, and its results, but for an answer, rank last.
    fn fallback(&self) -> bool {
        self.fallback
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
    standing: Standing,
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
    /// Where its Item kind ranks on an equal match, lowest first (see
    /// [`KindOrder`]).
    kind: u8,
    /// nucleo's score.
    score: u32,
    /// The title's length in characters: a shorter title wins a tie.
    len: u32,
}

/// Where a Candidate stands before anything else is compared: answers
/// first, fallbacks last.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Standing {
    /// A Live query's answer, such as the calculator's result.
    Answer,
    Match,
    /// A fallback Extension's result, below every real match, near misses
    /// included (ADR 0016).
    Fallback,
}

// Ranking sorts thousands of Candidates a keystroke: they stay small.
const _: () = assert!(size_of::<Candidate>() <= 48);

impl Candidate {
    fn new(source: Source, tier: Tier, kind: u8, score: u32, len: u32) -> Self {
        Candidate {
            source,
            standing: Standing::Match,
            class: Class::Rest,
            tier: tier.coarse(),
            frecency: 0.0,
            direct: 0.0,
            fine: tier,
            by_alias: false,
            kind,
            score,
            len,
        }
    }

    /// Answers first and fallbacks last, then by class, how well it
    /// matches, Frecency, its Item kind, and nucleo's score.
    fn rank(&self, other: &Candidate) -> std::cmp::Ordering {
        self.standing
            .cmp(&other.standing)
            .then(other.class.cmp(&self.class))
            .then(other.tier.cmp(&self.tier))
            .then(other.frecency.total_cmp(&self.frecency))
            .then(other.fine.cmp(&self.fine))
            .then(self.by_alias.cmp(&other.by_alias))
            .then(self.kind.cmp(&other.kind))
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
    /// Whether the query is, or has been, asked of the Extension.
    asked: bool,
    query: String,
    prefix: Option<String>,
    results: Vec<LiveResult>,
}

impl Live {
    /// A Live query of `query`, routed by `prefix`, to send at `due`.
    fn asking(query: String, prefix: Option<String>, due: Instant) -> Self {
        Live {
            query,
            prefix,
            due: Some(due),
            asked: true,
            ..Live::default()
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Update {
    None,
    /// Results changed under an unchanged query: ranked from scratch while
    /// [`Session::reranks`], and otherwise below the rows on screen.
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
    /// How many rows, from the first, Late Results may not move once the
    /// rerank is over: those on screen and those scrolled past.
    rows: usize,
    /// When the query last changed.
    typed: Instant,
    /// Whether the user moved the selection since the query last changed.
    selection_moved: bool,
    narrowing: Narrowing,
    /// By Extension.
    live: Vec<Live>,
    /// The Results last sent, in order, and whether each is an answer.
    shown: Vec<(Source, bool)>,
    /// The version of the Results last sent.
    version: u32,
    /// The Results a late update of this query moved up a row or added, each
    /// with the last version that did. Until a frame shows that version, the
    /// row the user runs held something else, so a run confirmed in an
    /// earlier one is refused.
    risen: Vec<(Source, u32)>,
    /// The Result an Alias named, and the Alias: picking it counts for the
    /// Alias alone.
    via_alias: Option<(Source, String)>,
    /// Whether it has sent a query. A Front end that only passes on commands
    /// never does, and gets no Results.
    searched: bool,
    update: Update,
    /// What is typed into each open list View the Daemon filters, by its
    /// Extension and id; never empty.
    filters: Vec<(ExtensionId, String, String)>,
}

impl Session {
    /// Never blocks; see [`Extension::send`].
    fn send(&self, message: ToFrontEnd) {
        if let Err(TrySendError::Full(_)) = self.sender.try_send(message) {
            self.hung.set(true);
        }
    }

    /// Whether Late Results that arrive at `now` rank with the rest, the rows
    /// on screen included: until the selection moves or [`RERANK_FOR`] after
    /// the keystroke.
    fn reranks(&self, now: Instant) -> bool {
        !self.selection_moved && now < self.typed + RERANK_FOR
    }

    /// Sends `results`, the rows of `shown`, as the next version. A late
    /// update notes each Result it adds or moves up a row as risen in that
    /// version.
    fn send_results(&mut self, shown: Vec<(Source, bool)>, results: Vec<ShownResult>, fresh: bool) {
        self.version = self.version.wrapping_add(1);
        if fresh {
            self.risen.clear();
        } else {
            for (row, &(source, _)) in shown.iter().enumerate() {
                let was = self.shown.iter().position(|&(old, _)| old == source);
                if was.is_some_and(|was| was <= row) {
                    continue;
                }
                match self.risen.iter_mut().find(|(risen, _)| *risen == source) {
                    Some((_, version)) => *version = self.version,
                    None => self.risen.push((source, self.version)),
                }
            }
        }
        self.shown = shown;
        self.send(ToFrontEnd::Results {
            version: self.version,
            results,
        });
    }

    /// The filter typed into a list View, if any.
    fn filter(&self, extension: ExtensionId, view: &str) -> Option<&str> {
        self.filters
            .iter()
            .find(|(ext, id, _)| *ext == extension && id == view)
            .map(|(_, _, text)| text.as_str())
    }

    fn set_filter(&mut self, extension: ExtensionId, view: &str, text: String) {
        self.filters
            .retain(|(ext, id, _)| !(*ext == extension && id == view));
        if !text.is_empty() {
            self.filters.push((extension, view.to_owned(), text));
        }
    }
}

/// What a Session's last search of the Index found. A query that only adds
/// to that one can only match a subset, so the next search looks at those
/// alone.
#[derive(Default)]
struct Narrowing {
    /// The slots that matched `query` at index version `version`, in slot
    /// order.
    matched: Vec<Slot>,
    query: String,
    version: u64,
    /// Whether `query` searched subtitles.
    subtitles: bool,
    /// Whether `query` was one letter, which matches only at a word start, so
    /// `matched` lacks the Items a longer query matches inside a word.
    by_letter: bool,
    /// The slots that matched `query` or nearly did, in slot order, if near
    /// misses were looked for. A query that only adds to it, with as many
    /// typos for each word, can only nearly match these.
    nearly: Vec<Slot>,
    /// `query` as tiers are worked out from, if `nearly` is for it.
    nearly_for: Option<rank::Query>,
}

impl Narrowing {
    /// Whether `query`, folded as `folded`, need only search `matched` at
    /// index version `version`.
    fn narrows(&self, query: &str, folded: &rank::Query, version: u64) -> bool {
        const SYNTAX: &[char] = &['^', '$', '!', '\'', '\\'];
        // Subtitles are searched from a few letters on, so the matches of a
        // query too short for them may lack some of this one's.
        !self.query.is_empty()
            && self.subtitles == folded.searches_subtitles()
            && !self.by_letter
            && self.version == version
            && query.starts_with(&self.query)
            && !query.contains(SYNTAX)
    }

    /// Remembers what `query`, folded as `folded`, matched at index version
    /// `version`.
    fn found(
        &mut self,
        query: &str,
        folded: &rank::Query,
        version: u64,
        matched: impl Iterator<Item = Slot>,
    ) {
        self.matched.clear();
        self.matched.extend(matched);
        self.query.clear();
        self.query.push_str(query);
        self.version = version;
        self.subtitles = folded.searches_subtitles();
        self.by_letter = folded.filter().by_letter();
    }

    /// Forgets the last search, as the Empty query does: the next one starts
    /// from every Item.
    fn forget(&mut self) {
        self.matched.clear();
        self.query.clear();
        self.nearly.clear();
        self.nearly_for = None;
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
    /// Undoes the latest change to the History.
    Undo,
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
        hidden: false,
        actions: vec![Action {
            id: "run".into(),
            title: "Clear".into(),
            shortcut: None,
            arguments: Vec::new(),
        }],
    }
}

/// The Item that undoes the latest change to the History, which `title` names.
fn undo_item(title: &str) -> Item {
    Item {
        id: UNDO.into(),
        kind: ItemKind::Namespaced(NamespacedKind::new("kotodama.command").expect("a valid kind")),
        title: title.into(),
        subtitle: Some("Put back what the History forgot".into()),
        icon: None,
        keyword: None,
        identity: None,
        keywords: Vec::new(),
        hidden: false,
        actions: vec![action_titled("run", "Undo".into())],
    }
}

/// An Action that takes no Arguments and has no shortcut.
fn action_titled(id: &str, title: String) -> Action {
    Action {
        id: id.to_owned(),
        title,
        shortcut: None,
        arguments: Vec::new(),
    }
}

/// A change to the History that can be undone until `until`. Changes are
/// undone by id, so one can be undone whatever changed after it.
struct Undoable {
    id: u64,
    /// What the Undo Result reads while it is the latest.
    title: String,
    change: Change,
    until: Instant,
}

/// A Result the Empty query may show as one of the most used.
struct MostUsed {
    source: Source,
    used: Habit,
    /// How much it was picked through itself rather than its Identity: of
    /// the Results of one Identity, the one picked through most stands for it.
    direct: f64,
}

impl MostUsed {
    fn new(source: Source) -> Self {
        MostUsed {
            source,
            used: Habit::default(),
            direct: 0.0,
        }
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

/// What running an Action does when a required Argument is missing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Missing {
    /// Asks for it in a form, and runs the Action once it is submitted.
    AskInForm,
    /// Fails, as when the form was already submitted without it.
    Fail,
}

/// An Action waiting on the form that asks for its missing Arguments.
struct ArgumentForm {
    /// The Front end the form is shown in.
    front_end: FrontEndId,
    result: ResultRef,
    action: ActionRef,
    /// The Arguments already given or typed.
    arguments: BTreeMap<String, String>,
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
    /// Indexes Extensions' Items off the keystroke path, if started.
    indexer: Option<Indexer>,
    /// By Extension, the last change made before its Items were last
    /// cleared: a version holding no later one is stale.
    stale: Vec<u64>,
    hot_capacity: usize,
    /// When the hot Items are next worked out.
    hot_due: Instant,
    /// The versions of the Index and the History they were worked out for.
    hot_versions: (u64, u64),
    /// A menu is opening: read what its first keystroke will read.
    warm: bool,
    matcher: Matcher,
    sessions: BTreeMap<FrontEndId, Session>,
    live_queries: HashMap<u64, (FrontEndId, ExtensionId)>,
    invocations: HashMap<u64, (FrontEndId, ExtensionId)>,
    /// By the id of the form's View.
    argument_forms: HashMap<String, ArgumentForm>,
    next_id: u64,
    config: Table,
    /// Running Extensions' Contributed actions by Item kind, in Extension order.
    contributed: HashMap<String, Vec<(ExtensionId, Action)>>,
    /// The Action catalog last sent, if any Extension reads it.
    catalog: Option<Vec<CatalogEntry>>,
    catalog_dirty: bool,
    down: Vec<String>,
    jobs: BTreeMap<(ExtensionId, String), Job>,
    /// The Views Extensions have open, as the menus stack them.
    views: Views,
    /// Hidden Items, by Extension and id: kept out of the index, so out of
    /// the search, and reached only by `kotodama open --view`.
    hidden: HashMap<ExtensionId, HashMap<String, Item>>,
    pending_output: Option<PendingOutput>,
    openings: Vec<Opening>,
    /// What other Front ends asked the menus while an Opening was awaited,
    /// passed on once none is, so the search never shows before its View.
    held: Vec<(FrontEndId, ToFrontEnd)>,
    launcher: L,
    desktop: D,
    store: S,
    history: History,
    /// The wall clock the History's times come from.
    clock: Clock,
    started: Instant,
    keywords: Keywords,
    empty_query: EmptyQuery,
    kinds: KindOrder,
    /// `kinds` for each Segment's kinds, as of the last search.
    kind_places: KindPlaces,
    /// One per Quicklink, then the clear-history Item, then the Undo Item
    /// while there is something to undo.
    own: Vec<Own>,
    /// The changes to the History that can still be undone, oldest first.
    undos: Vec<Undoable>,
    /// The picked Indexed items, by slot, as of the index and History
    /// versions in `frecent_versions`.
    frecent: Vec<(Slot, Picks, Counts)>,
    frecent_versions: Option<(u64, u64)>,
    /// The slots the last search matched.
    scored: Vec<Slot>,
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
        let mut daemon = Daemon {
            index: Index::new(found.len()),
            indexer: None,
            stale: vec![0; found.len()],
            hot_capacity: HOT_CAPACITY,
            hot_due: now,
            hot_versions: (0, 0),
            warm: false,
            extensions: found
                .into_iter()
                .map(|found| Extension::new(found, now))
                .collect(),
            matcher: Matcher::new(Config::DEFAULT),
            sessions: BTreeMap::new(),
            live_queries: HashMap::new(),
            invocations: HashMap::new(),
            argument_forms: HashMap::new(),
            next_id: 1,
            config,
            contributed: HashMap::new(),
            catalog: None,
            catalog_dirty: false,
            down: Vec::new(),
            jobs: BTreeMap::new(),
            views: Views::default(),
            hidden: HashMap::new(),
            pending_output: None,
            openings: Vec::new(),
            held: Vec::new(),
            launcher,
            desktop,
            store,
            history,
            clock: Clock::system(),
            started: now,
            keywords: Keywords::default(),
            empty_query: EmptyQuery::default(),
            kinds: KindOrder::default(),
            kind_places: KindPlaces::default(),
            own: Vec::new(),
            undos: Vec::new(),
            frecent: Vec::new(),
            frecent_versions: None,
            scored: Vec::new(),
            tiers: Tiers::default(),
            highlights: Vec::new(),
        };
        daemon.load_keywords();
        daemon.load_settings();
        daemon.empty_query = EmptyQuery::from_config(&daemon.config);
        daemon.kinds = KindOrder::from_config(&daemon.config);
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

    /// Indexes Extensions' Items on a thread of its own from now on, which
    /// sends each new version to `events` as an [`Event::Indexed`]. Until
    /// then, and in tests, a change to them is made as its event is handled.
    pub fn index_in_background(&mut self, events: Sender<Event>) {
        self.indexer = Some(Indexer::start(events, self.index.payload_dir()));
        self.repin();
    }

    /// Keeps what a search doesn't read of Indexed items in files in `dir`
    /// rather than in memory. Call it before any Item arrives.
    pub fn keep_items_in(&mut self, dir: &Path) {
        self.index.keep_payloads_in(dir.into());
    }

    /// How many Items may be hot at most; none turns the hot Items off.
    pub fn set_hot_capacity(&mut self, capacity: usize) {
        self.hot_capacity = capacity;
        self.hot_due = self.started;
    }

    /// The hot Items, by Extension name and id, each resident.
    pub fn hot(&self) -> Vec<(String, String)> {
        self.resident(self.index.hot())
    }

    /// The Items a keyword or an Alias names, by Extension name and id, that
    /// are resident.
    pub fn pinned(&self) -> Vec<(String, String)> {
        self.resident(self.index.pinned())
    }

    /// Those of `slots` that are resident, by Extension name and id.
    fn resident(&self, slots: &[Slot]) -> Vec<(String, String)> {
        slots
            .iter()
            .filter(|&&slot| self.index.is_resident(slot))
            .filter_map(|&slot| {
                let extension = self.extensions[slot.extension().0].name();
                Some((extension.to_owned(), self.index.id(slot)?.to_owned()))
            })
            .collect()
    }

    /// Works out the hot Items: those picked most near this time of day,
    /// from a little before it to a little after, up to the hot capacity,
    /// and reads their payloads, so the menu shows them without waiting. Both
    /// happen on the indexing thread, when there is one.
    fn plan_hot(&mut self, now: Instant) {
        self.hot_due = now + HOT_EVERY;
        self.hot_versions = (self.index.version(), self.history.version());
        let mut picked = Vec::new();
        if self.hot_capacity > 0 && !self.history.is_empty() {
            self.cache_frecent();
            picked.extend(self.frecent.iter().map(|&(slot, picks, _)| (slot, picks)));
        }
        let plan = Plan {
            picked,
            when: self.when(now),
            half_life: self.history.settings().half_life_secs,
            capacity: self.hot_capacity,
        };
        match &mut self.indexer {
            // Nothing to plan, and nothing hot to let go of.
            _ if plan.picked.is_empty() && self.index.hot().is_empty() => {}
            Some(indexer) => indexer.plan(plan),
            None => self.index.set_hot(plan.hot()),
        }
    }

    /// Keeps resident every Item a keyword or an Alias names, so a keyword
    /// typed never waits for a read. The indexing thread reads them, when
    /// there is one; otherwise they are read now.
    fn repin(&mut self) {
        let aliases = self.aliased();
        if let Some(indexer) = &mut self.indexer {
            indexer.pin(aliases);
        }
        self.repin_index();
    }

    /// The Items the Aliases name, by Extension and id.
    fn aliased(&self) -> Vec<(ExtensionId, String)> {
        self.keywords
            .aliases
            .values()
            .filter_map(|alias| Some((self.extension_named(&alias.extension)?, alias.item.clone())))
            .collect()
    }

    /// Works out again which Items the Index keeps resident for a keyword or
    /// an Alias, after the Items or the Aliases changed. Reads those that
    /// aren't resident only when there is no indexing thread to.
    fn repin_index(&mut self) {
        let mut pinned = self.index.keyed_all();
        for (ext, id) in self.aliased() {
            pinned.extend(self.index.find(ext, &id));
        }
        pinned.sort_unstable();
        pinned.dedup();
        self.index.pin(pinned);
        if self.indexer.is_none() {
            self.index.read_pinned();
        }
    }

    /// Replaces the wall clock, which starts as the system's.
    pub fn set_clock(&mut self, clock: Clock) {
        self.clock = clock;
        self.mark_late();
    }

    /// The wall-clock time at `now`.
    fn when(&self, now: Instant) -> When {
        self.clock.when(now.saturating_duration_since(self.started))
    }

    /// Seconds since the Unix epoch at `now`.
    fn wall(&self, now: Instant) -> u64 {
        self.when(now).at
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
            Event::Indexed(indexed) => self.indexed(indexed, now),
            Event::Kept(Kept { hot, reads }) => {
                self.index.keep(reads);
                match hot {
                    Some(hot) => self.index.hot_read(hot),
                    None => self.index.trim(),
                }
            }
        }
        self.release_held();
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
        if self.undos.first().is_some_and(|undo| undo.until <= now) {
            self.undos.retain(|undo| undo.until > now);
            self.load_undo_item();
        }
        self.open_overdue(now);
        self.release_held();
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
        let undo = self.undos.first().map(|undo| undo.until);
        let opening = self
            .openings
            .iter()
            .filter(|opening| opening.awaited())
            .map(|opening| opening.deadline);
        restarts
            .chain(live)
            .chain(output)
            .chain(undo)
            .chain(opening)
            .min()
    }

    /// When the Daemon next has something to do: what
    /// [`next_deadline`](Self::next_deadline) says, or working out the hot
    /// Items again ahead of the time of day.
    pub fn next_wake(&self) -> Option<Instant> {
        let hot = (self.hot_capacity > 0 && !self.history.is_empty()).then_some(self.hot_due);
        self.next_deadline().into_iter().chain(hot).min()
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
                    session.live[id.0] = Live::asking(query, prefix, now + debounce);
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
    /// Shows every Front end how a job is doing. A running job is kept, so a
    /// menu opened later shows it; a finished one is announced on the desktop,
    /// then forgotten.
    fn job_progress(&mut self, id: ExtensionId, name: &str, job: Job) {
        self.broadcast(|| ToFrontEnd::JobProgress {
            extension: name.to_owned(),
            job: job.clone(),
        });
        if job.state == JobState::Running {
            self.jobs.insert((id, job.id.clone()), job);
        } else {
            self.jobs.remove(&(id, job.id.clone()));
            let body = job.message.unwrap_or_else(|| {
                match job.state {
                    JobState::Failed => "Failed",
                    _ => "Done",
                }
                .to_owned()
            });
            self.desktop.notify(job.title, body);
        }
    }

    fn forget_run(&mut self, id: ExtensionId) {
        let name = self.extensions[id.0].name().to_owned();
        if self.index.clear(id) {
            self.repin_index();
            self.mark_late();
        }
        if let Some(indexer) = &mut self.indexer {
            self.stale[id.0] = indexer.clear(id);
        }
        self.live_queries.retain(|_, (_, ext)| *ext != id);
        // Its running jobs won't finish now.
        let stopped: Vec<String> = self
            .jobs
            .keys()
            .filter(|(ext, _)| *ext == id)
            .map(|(_, job)| job.clone())
            .collect();
        for job in stopped {
            if let Some(mut job) = self.jobs.remove(&(id, job)) {
                job.state = JobState::Failed;
                job.message = Some(format!("{name} stopped"));
                self.job_progress(id, &name, job);
            }
        }
        self.hidden.remove(&id);
        // Its Views can't be answered any more.
        for session in self.sessions.values_mut() {
            session.filters.retain(|(ext, _, _)| *ext != id);
        }
        for view in self.views.pop_all(id) {
            self.broadcast(|| ToFrontEnd::PopView {
                extension: name.clone(),
                view: view.clone(),
            });
        }
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
        // A View it was asked to open won't come now.
        let invocations = &self.invocations;
        let (stopped, waiting) =
            std::mem::take(&mut self.openings)
                .into_iter()
                .partition(|opening| {
                    opening
                        .running
                        .is_some_and(|running| !invocations.contains_key(&running))
                });
        self.openings = waiting;
        for opening in stopped {
            let failure = format!("{name} stopped before its View opened");
            self.open_menus(&opening, Some(failure));
        }
        self.extensions_changed();
    }

    /// The set of running Extensions changed.
    fn extensions_changed(&mut self) {
        // The Contributed actions on the open list Views, to tell the menus
        // of those that change.
        let before: Vec<_> = self
            .views
            .iter()
            .filter(|open| matches!(open.view.content, ViewContent::List { .. }))
            .map(|open| {
                let contributed = self.view_contributed(open.extension, &open.view);
                (open.extension, open.view.id.clone(), contributed)
            })
            .collect();
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
        for (id, view_id, contributed) in before {
            let Some(open) = self.views.get(id, &view_id) else {
                continue;
            };
            if self.view_contributed(id, &open.view) != contributed {
                let view = open.view.clone();
                let name = self.extensions[id.0].name().to_owned();
                self.update_view(id, &name, view);
            }
        }
    }

    fn extension_message(&mut self, id: ExtensionId, message: FromExtension, now: Instant) {
        let name = self.extensions[id.0].name().to_owned();
        match message {
            FromExtension::AddItems { mut items } => {
                items.retain(|item| {
                    let theirs = item.kind.extension().is_some_and(|owner| owner != name);
                    if theirs {
                        log::warn!(
                            "{name}: Item `{}` has another Extension's kind `{}`",
                            item.id,
                            item.kind.name()
                        );
                    }
                    !theirs
                });
                // A hidden Item is kept out of the index: the Segment drops
                // it, and the Daemon keeps it for `kotodama open --view`.
                let hidden = self.hidden.entry(id).or_default();
                for item in &items {
                    if item.hidden {
                        hidden.insert(item.id.clone(), item.clone());
                    } else {
                        hidden.remove(&item.id);
                    }
                }
                self.edit_items(id, Edit::Add(items), now);
            }
            FromExtension::RemoveItems { ids } => {
                if let Some(hidden) = self.hidden.get_mut(&id) {
                    for item in &ids {
                        hidden.remove(item);
                    }
                }
                self.edit_items(id, Edit::Remove(ids), now);
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
            FromExtension::PushView(view) => {
                let contributed = self.view_contributed(id, &view);
                self.forget_filters(id, &view.id);
                self.views.push(id, view.clone());
                self.broadcast(|| ToFrontEnd::PushView {
                    extension: name.clone(),
                    view: view.clone(),
                    contributed: contributed.clone(),
                });
            }
            FromExtension::UpdateView(view) => self.update_view(id, &name, view),
            FromExtension::PopView { view } => {
                self.forget_filters(id, &view);
                self.views.pop(id, &view);
                self.broadcast(|| ToFrontEnd::PopView {
                    extension: name.clone(),
                    view: view.clone(),
                });
            }
            FromExtension::ReplaceView { view, with } => {
                let contributed = self.view_contributed(id, &with);
                self.forget_filters(id, &view);
                self.forget_filters(id, &with.id);
                self.views.pop(id, &view);
                self.views.push(id, with.clone());
                self.broadcast(|| ToFrontEnd::ReplaceView {
                    extension: name.clone(),
                    view: view.clone(),
                    with: with.clone(),
                    contributed: contributed.clone(),
                });
            }
            FromExtension::JobProgress(job) => self.job_progress(id, &name, job),
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

    /// Changes an Extension's Items: here and now, or on the indexing
    /// thread, whose new version comes back as an [`Event::Indexed`].
    fn edit_items(&mut self, id: ExtensionId, edit: Edit, now: Instant) {
        match &mut self.indexer {
            Some(indexer) => {
                indexer.edit(id, edit);
            }
            None => {
                self.index.edit(id, edit);
                self.repin_index();
                self.items_changed();
                self.run_openings(now);
            }
        }
    }

    /// Puts in a new version of an Extension's Items, unless they were
    /// cleared since.
    fn indexed(&mut self, indexed: Indexed, now: Instant) {
        let Indexed {
            extension,
            change,
            segment,
            reads,
        } = indexed;
        if change <= self.stale[extension.0] {
            return;
        }
        self.index.replace(extension, segment);
        self.index.keep(reads);
        self.repin_index();
        self.items_changed();
        self.run_openings(now);
    }

    /// Replaces an open View. Each menu filtering a list View is sent the
    /// new Items that match its filter along with it.
    fn update_view(&mut self, id: ExtensionId, name: &str, view: View) {
        let contributed = self.view_contributed(id, &view);
        let view_id = view.id.clone();
        let filtered = self.views.update(id, view.clone())
            && self
                .views
                .get(id, &view_id)
                .is_some_and(Open::filtered_here);
        if !filtered {
            self.forget_filters(id, &view_id);
        }
        let ids: Vec<FrontEndId> = self.sessions.keys().copied().collect();
        for front_end in ids {
            let Some(session) = self.sessions.get(&front_end) else {
                continue;
            };
            session.send(ToFrontEnd::UpdateView {
                extension: name.to_owned(),
                view: view.clone(),
                contributed: contributed.clone(),
            });
            if let Some(text) = session.filter(id, &view_id).map(str::to_owned) {
                self.send_filtered(front_end, id, &view_id, text);
            }
        }
    }

    /// No menu filters the View any more: it was pushed afresh or popped.
    fn forget_filters(&mut self, id: ExtensionId, view: &str) {
        for session in self.sessions.values_mut() {
            session.set_filter(id, view, String::new());
        }
    }

    /// A menu typed `text` into one of an Extension's list Views. The Daemon
    /// filters it and answers, unless the Extension filters it itself.
    fn filter_view(&mut self, front_end: FrontEndId, extension: &str, view: &str, text: String) {
        let Some(id) = self.extension_named(extension) else {
            return;
        };
        let Some(open) = self.views.get(id, view) else {
            return;
        };
        if !matches!(open.view.content, ViewContent::List { .. }) {
            return;
        }
        if open.filters_itself() {
            self.extensions[id.0].send(ToExtension::FilterView {
                view: view.to_owned(),
                text,
            });
            return;
        }
        let Some(session) = self.sessions.get_mut(&front_end) else {
            return;
        };
        session.set_filter(id, view, text.clone());
        if !text.is_empty() {
            self.send_filtered(front_end, id, view, text);
        }
    }

    fn send_filtered(&mut self, front_end: FrontEndId, id: ExtensionId, view: &str, text: String) {
        let matches = self
            .views
            .filter(id, view, &text, &mut self.matcher, &mut self.tiers);
        if let Some(session) = self.sessions.get(&front_end) {
            session.send(ToFrontEnd::FilteredView {
                extension: self.extensions[id.0].name().to_owned(),
                view: view.to_owned(),
                text,
                matches,
            });
        }
    }

    /// The Contributed actions on a list View's Items, by the Item kinds it
    /// has. Other Views have none.
    fn view_contributed(
        &self,
        owner: ExtensionId,
        view: &View,
    ) -> BTreeMap<String, Vec<OfferedAction>> {
        let mut by_kind = BTreeMap::new();
        let ViewContent::List { items, .. } = &view.content else {
            return by_kind;
        };
        let mut seen: HashSet<&str> = HashSet::new();
        for item in items {
            let kind = item.kind.name();
            if seen.insert(kind) {
                let offered = self.contributed_for(owner, kind, &[]);
                if !offered.is_empty() {
                    by_kind.insert(kind.to_owned(), offered);
                }
            }
        }
        by_kind
    }

    fn items_changed(&mut self) {
        self.catalog_dirty = true;
        self.mark_late();
    }

    fn mark_late(&mut self) {
        for session in self.sessions.values_mut() {
            if session.searched {
                session.update = session.update.max(Update::Late);
            }
        }
    }

    /// Ranks the Empty query again in every session showing it but `except`,
    /// for what the History learned or the time of day now: from scratch
    /// for a menu opening, and otherwise as a late update, which keeps the
    /// rows on screen where they are.
    fn refresh_empty(&mut self, update: Update, except: Option<FrontEndId>) {
        for (&id, session) in &mut self.sessions {
            if session.searched && session.query.is_empty() && Some(id) != except {
                session.update = session.update.max(update);
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
        // Nothing to show changes the rows on screen, so they aren't sent again.
        let changed = !results.is_empty();
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
        if changed {
            session.update = session.update.max(Update::Late);
        }
    }

    fn action_done(&mut self, id: ExtensionId, invocation: u64, outcome: Outcome, now: Instant) {
        if self.invocations.get(&invocation).map(|&(_, ext)| ext) != Some(id) {
            return;
        }
        let Some((front_end, _)) = self.invocations.remove(&invocation) else {
            return;
        };
        // Run for `kotodama open --view`, it answers the menus rather than
        // the command; what it types or copies goes out as for any Action.
        if let Some(n) = self
            .openings
            .iter()
            .position(|opening| opening.running == Some(invocation))
        {
            let opening = self.openings.swap_remove(n);
            match outcome {
                Outcome::KeepOpen => return self.open_menus(&opening, None),
                Outcome::Failed { message } => return self.open_menus(&opening, Some(message)),
                // Closes a menu that was open already, or opened late.
                Outcome::Close => return self.broadcast(|| ToFrontEnd::Close),
                _ => {}
            }
        }
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
            Outcome::Copy(clipboard) => self.copy(clipboard),
            Outcome::RunCatalogAction {
                extension,
                kind,
                action,
                arguments,
            } => {
                let action = ActionRef {
                    extension,
                    id: action,
                };
                let run = self.run_catalog_action(front_end, id, &kind, action, arguments, now);
                self.failed(front_end, run);
            }
        }
    }

    /// Runs an Action from the Action catalog that `proposer` proposed and the
    /// user confirmed, on the one Indexed item of its kind that
    /// offers it, as if the user had picked that Item's Action.
    fn run_catalog_action(
        &mut self,
        front_end: FrontEndId,
        proposer: ExtensionId,
        kind: &KindName,
        action: ActionRef,
        arguments: BTreeMap<String, String>,
        now: Instant,
    ) -> Result<(), String> {
        let asking = &self.extensions[proposer.0];
        if !asking.found.manifest.action_catalog {
            return Err(format!(
                "{} may not run Actions from the Action catalog",
                asking.name()
            ));
        }
        let target = self
            .extension_named(&action.extension)
            .filter(|&target| target != proposer && self.extensions[target.0].sender().is_some())
            .ok_or_else(|| format!("{} isn't running", action.extension))?;
        // Two are enough to know it isn't one.
        let mut offering: Vec<String> = self
            .index
            .offering(target, kind.as_str(), &action.id)
            .take(2)
            .filter_map(|slot| self.index.id(slot).map(str::to_owned))
            .collect();
        let id = match offering.len() {
            1 => offering.swap_remove(0),
            2 => {
                return Err(format!(
                    "several {kind} Items offer `{}`; pick one in the search",
                    action.id
                ));
            }
            _ => {
                let contributed = self.extensions[target.0]
                    .found
                    .manifest
                    .contributed_actions
                    .iter()
                    .find(|offered| offered.kind == *kind && offered.action.id == action.id);
                return Err(match contributed {
                    Some(offered) => format!(
                        "pick a {kind} in the search, then “{}” in its Action panel",
                        offered.action.title
                    ),
                    None => format!("no {kind} Item offers `{}`", action.id),
                });
            }
        };
        let result = ResultRef {
            extension: action.extension.clone(),
            id,
        };
        self.run_action(
            front_end,
            &result,
            &action,
            arguments,
            Missing::AskInForm,
            now,
        )
    }

    /// Nothing goes into a window, so there is no waiting for the menus to
    /// close. A paste still waiting goes first, so it can't take the
    /// clipboard back afterwards.
    fn copy(&mut self, clipboard: Clipboard) {
        self.deliver_output();
        self.broadcast(|| ToFrontEnd::Close);
        self.desktop.output(Output::Copy(clipboard));
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
            typed: self.started,
            selection_moved: false,
            narrowing: Narrowing::default(),
            live: self.extensions.iter().map(|_| Live::default()).collect(),
            shown: Vec::new(),
            version: 0,
            risen: Vec::new(),
            via_alias: None,
            searched: false,
            update: Update::None,
            filters: Vec::new(),
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
        // A menu that restarted shows the Views it had, bottom first, with
        // nothing typed into them: a list that filters itself lists every
        // Item again.
        for open in self.views.iter() {
            if open.filters_itself() {
                self.extensions[open.extension.0].send(ToExtension::FilterView {
                    view: open.view.id.clone(),
                    text: String::new(),
                });
            }
            session.send(ToFrontEnd::PushView {
                extension: self.extensions[open.extension.0].name().to_owned(),
                view: open.view.clone(),
                contributed: self.view_contributed(open.extension, &open.view),
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
        self.argument_forms.retain(|_, form| form.front_end != id);
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
            FromFrontEnd::SelectionMoved => {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.selection_moved = true;
                }
            }
            FromFrontEnd::RunAction {
                result,
                shown,
                action,
                arguments,
            } => {
                let daemons = action.extension == OWN;
                let run = if self.unseen(id, &result, shown) {
                    Err("that Result wasn't on screen yet".to_owned())
                } else if daemons && action.id == FORGET {
                    self.forget_result(id, &result, now)
                } else if daemons && action.id == CLEAR_EXTENSION_HISTORY {
                    self.clear_extension_history(id, &result, now)
                } else if result.extension == OWN {
                    self.run_own(id, &result, &action, arguments, now)
                } else {
                    self.run_action(id, &result, &action, arguments, Missing::AskInForm, now)
                };
                self.failed(id, run);
            }
            FromFrontEnd::SubmitForm {
                extension,
                view,
                values,
            } => {
                if extension == OWN {
                    self.submit_arguments(id, &view, values, now);
                } else if let Some(ext) = self.extension_named(&extension) {
                    self.extensions[ext.0].send(ToExtension::SubmitForm { view, values });
                }
            }
            FromFrontEnd::ViewPopped { extension, view } => {
                if extension == OWN {
                    self.take_argument_form(id, &view);
                } else if let Some(ext) = self.extension_named(&extension) {
                    self.forget_filters(ext, &view);
                    self.views.pop(ext, &view);
                    self.extensions[ext.0].send(ToExtension::ViewPopped { view });
                }
            }
            FromFrontEnd::FilterView {
                extension,
                view,
                text,
            } => self.filter_view(id, &extension, &view, text),
            FromFrontEnd::Closed => self.closed(Some(id)),
            // The menu may open on the Empty query, which may have been
            // ranked hours ago.
            FromFrontEnd::Open { query, view } => {
                self.warm = true;
                self.refresh_empty(Update::Fresh, Some(id));
                match view {
                    Some(text) => self.open_on_view(id, query, &text, now),
                    None => self.ask_menus(id, ToFrontEnd::Open { query, view: None }),
                }
            }
            FromFrontEnd::Toggle => {
                self.warm = true;
                self.refresh_empty(Update::Fresh, Some(id));
                self.ask_menus(id, ToFrontEnd::Toggle);
            }
        }
    }

    /// Whether a late update added `result` or moved it up a row in a later
    /// version of the Results than `shown`, the one on screen when the user
    /// chose it: the user hasn't seen it where it is.
    fn unseen(&self, front_end: FrontEndId, result: &ResultRef, shown: u32) -> bool {
        let Some(session) = self.sessions.get(&front_end) else {
            return false;
        };
        let Some(source) = self.source_of(session, &result.extension, &result.id) else {
            return false;
        };
        session
            .risen
            .iter()
            .any(|&(risen, version)| risen == source && version > shown)
    }

    /// Tells the Front end why an Action it asked for didn't run.
    fn failed(&self, front_end: FrontEndId, run: Result<(), String>) {
        if let Err(message) = run
            && let Some(session) = self.sessions.get(&front_end)
        {
            session.send(ToFrontEnd::ActionFailed { message });
        }
    }

    /// The user filled in the form that asked for an Action's Arguments: the
    /// form goes, and the Action runs.
    fn submit_arguments(&mut self, front_end: FrontEndId, view: &str, values: Table, now: Instant) {
        let Some(form) = self.take_argument_form(front_end, view) else {
            return;
        };
        if let Some(session) = self.sessions.get(&front_end) {
            session.send(ToFrontEnd::PopView {
                extension: OWN.to_owned(),
                view: view.to_owned(),
            });
        }
        let mut arguments = form.arguments;
        arguments.extend(arguments::submitted(values));
        let run = self.run_action(
            front_end,
            &form.result,
            &form.action,
            arguments,
            Missing::Fail,
            now,
        );
        self.failed(front_end, run);
    }

    /// Removes the argument form `view`, if `front_end` is the one showing it.
    fn take_argument_form(&mut self, front_end: FrontEndId, view: &str) -> Option<ArgumentForm> {
        if self
            .argument_forms
            .get(view)
            .is_some_and(|form| form.front_end == front_end)
        {
            self.argument_forms.remove(view)
        } else {
            None
        }
    }

    /// Passes `message` on to every menu but `from`'s, or holds it back
    /// while the menus wait on a View.
    fn ask_menus(&mut self, from: FrontEndId, message: ToFrontEnd) {
        if self.openings.iter().any(Opening::awaited) {
            self.held.push((from, message));
        } else {
            self.broadcast_from(from, || message.clone());
        }
    }

    /// Passes on what was held back, once no View is awaited.
    fn release_held(&mut self) {
        if self.held.is_empty() || self.openings.iter().any(Opening::awaited) {
            return;
        }
        for (from, message) in std::mem::take(&mut self.held) {
            self.broadcast_from(from, || message.clone());
        }
    }

    /// `kotodama open --view <text>`: the menus wait until the View is there.
    fn open_on_view(&mut self, from: FrontEndId, query: String, text: &str, now: Instant) {
        let Some(view) = ViewName::parse(text) else {
            let failure = format!("No View `{text}`; name one as <extension>/<item>");
            self.broadcast_from(from, || ToFrontEnd::Open {
                query: query.clone(),
                view: Some(text.to_owned()),
            });
            self.broadcast_from(from, || ToFrontEnd::ActionFailed {
                message: failure.clone(),
            });
            return;
        };
        self.openings.push(Opening {
            from,
            query,
            view,
            running: None,
            opened: false,
            deadline: now + OPEN_VIEW_TIMEOUT,
        });
        self.run_openings(now);
    }

    /// Runs the primary Action of each Item `kotodama open --view` waits on,
    /// once its Extension has indexed it. One that can't run opens the menus
    /// on the search, saying why. Not recorded as a pick: it wasn't picked
    /// from a search.
    fn run_openings(&mut self, now: Instant) {
        let mut n = 0;
        while n < self.openings.len() {
            if self.openings[n].running.is_some() {
                n += 1;
                continue;
            }
            match self.run_opening(n, now) {
                Ok(running) => {
                    self.openings[n].running = running;
                    n += 1;
                }
                Err(message) => {
                    let opening = self.openings.remove(n);
                    self.open_menus(&opening, Some(message));
                }
            }
        }
    }

    /// Runs the primary Action of the `n`th Opening's Item: its invocation,
    /// or `None` while the Extension starts, or started so recently that the
    /// Item may still be on its way.
    fn run_opening(&mut self, n: usize, now: Instant) -> Result<Option<u64>, String> {
        let opening = &self.openings[n];
        let view = &opening.view;
        let no_view = || format!("No View `{view}`; name one as <extension>/<item>");
        let ext_id = self.extension_named(&view.extension).ok_or_else(no_view)?;
        let ext = &self.extensions[ext_id.0];
        let State::Up { since, .. } = ext.state else {
            return Ok(None);
        };
        let Some(item) = self
            .index
            .find(ext_id, &view.item)
            .and_then(|slot| self.index.item(slot))
            .map(ItemRef::Resident)
            .or_else(|| {
                let hidden = self.hidden.get(&ext_id)?.get(&view.item)?;
                Some(ItemRef::Borrowed(hidden))
            })
        else {
            return if now.duration_since(since) < OPEN_VIEW_TIMEOUT {
                Ok(None)
            } else {
                Err(no_view())
            };
        };
        let primary = item.actions.first().ok_or_else(no_view)?;
        if let Some(absent) = arguments::missing(&primary.arguments, &BTreeMap::new()) {
            return Err(format!("`{}` needs {}", primary.title, absent.name));
        }
        let invocation = self.next_id;
        if !ext.send(ToExtension::RunAction {
            invocation,
            action: primary.id.clone(),
            item: item.to_ref(),
            arguments: BTreeMap::new(),
        }) {
            return Err(format!("{} stopped answering", ext.name()));
        }
        let from = opening.from;
        self.next_id += 1;
        self.invocations.insert(invocation, (from, ext_id));
        Ok(Some(invocation))
    }

    /// Opens the menus for `kotodama open --view`, unless they opened when it
    /// ran late, and shows `failure` if there is one.
    fn open_menus(&self, opening: &Opening, failure: Option<String>) {
        if !opening.opened {
            self.broadcast_from(opening.from, || ToFrontEnd::Open {
                query: opening.query.clone(),
                view: Some(opening.view.to_string()),
            });
        }
        if let Some(message) = failure {
            self.broadcast_from(opening.from, || ToFrontEnd::ActionFailed {
                message: message.clone(),
            });
        }
    }

    /// Opens the menus on the search for each `kotodama open --view` that has
    /// waited long enough. An Action still running is still followed: its
    /// View goes on top when it comes, and the rest of what it does counts.
    fn open_overdue(&mut self, now: Instant) {
        let mut n = 0;
        while n < self.openings.len() {
            let opening = &mut self.openings[n];
            if !opening.awaited() || opening.deadline > now {
                n += 1;
            } else if opening.running.is_some() {
                self.open_menus(&self.openings[n], None);
                self.openings[n].opened = true;
                n += 1;
            } else {
                let opening = self.openings.remove(n);
                let failure = format!("No View `{}` turned up", opening.view);
                self.open_menus(&opening, Some(failure));
            }
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
                    ext.fallback = is_fallback(&found);
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
                    self.stale.push(0);
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
        let logged = (self.history.settings().usage_log && !text.is_empty()).then(|| text.clone());
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
        session.searched = true;
        session.update = Update::Fresh;
        session.typed = now;
        session.selection_moved = false;
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
                *live = Live::asking(query, prefix, now + debounce);
            }
        }
        if let Some(query) = logged {
            self.store.log(Entry {
                at: self.wall(now),
                used: Used::Query { query },
            });
        }
    }

    fn run_action(
        &mut self,
        front_end: FrontEndId,
        result: &ResultRef,
        action: &ActionRef,
        mut arguments: BTreeMap<String, String>,
        missing: Missing,
        now: Instant,
    ) -> Result<(), String> {
        let gone = || "that Result is gone".to_owned();
        let owner = self.extension_named(&result.extension).ok_or_else(gone)?;
        let session = self.sessions.get(&front_end).ok_or_else(gone)?;
        let searched = match self.index.find(owner, &result.id) {
            Some(slot) => self.index.item(slot).map(ItemRef::Resident),
            None => session.live[owner.0]
                .results
                .iter()
                .rev()
                .find(|live| live.item.id == result.id)
                .map(|live| ItemRef::Borrowed(&live.item)),
        };
        // Or an Item of one of the owner's list Views.
        let in_view = || self.views.item(owner, &result.id).map(ItemRef::Borrowed);
        let found_searched = searched.is_some();
        let item = searched.or_else(in_view).ok_or_else(gone)?;

        let own = (action.extension == result.extension)
            .then(|| item.actions.iter().find(|own| own.id == action.id))
            .flatten();
        let (target, wanted) = match own {
            Some(own) => Some((owner, own)),
            None => self
                .extension_named(&action.extension)
                .filter(|&ext| ext != owner)
                .and_then(|ext| {
                    self.contributed
                        .get(item.kind.name())?
                        .iter()
                        .find(|(by, offered)| *by == ext && offered.id == action.id)
                        .map(|(_, offered)| (ext, offered))
                }),
        }
        .ok_or_else(|| format!("`{}` has no Action `{}`", item.title, action.id))?;

        // Arguments are typed in the search, after the Item's keyword.
        if found_searched
            && let Some(keyword) = &item.keyword
            && let Some(typed) = after_keyword(&session.query, &keyword.to_lowercase())
        {
            arguments::fill(&wanted.arguments, typed, &mut arguments);
        }
        if let Some(absent) = arguments::missing(&wanted.arguments, &arguments) {
            if missing == Missing::Fail {
                return Err(format!("`{}` needs {}", wanted.title, absent.name));
            }
            let id = format!("arguments-{}", self.next_id);
            self.next_id += 1;
            session.send(ToFrontEnd::PushView {
                extension: OWN.to_owned(),
                view: arguments::form(id.clone(), &wanted.title, &wanted.arguments, &arguments),
                contributed: BTreeMap::new(),
            });
            self.argument_forms.insert(
                id,
                ArgumentForm {
                    front_end,
                    result: result.clone(),
                    action: action.clone(),
                    arguments,
                },
            );
            return Ok(());
        }

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
                let change = self.history.clear();
                self.store.forget_usage(None);
                self.changed("Undo clearing history".into(), change, now);
            }
            OwnAction::Undo => {
                if let Some(latest) = self.undos.last().map(|undo| undo.id) {
                    self.undo(latest);
                }
            }
        }
        Ok(())
    }

    /// Forgets one Result: everything the History keeps for it. The menu
    /// stays open.
    fn forget_result(
        &mut self,
        front_end: FrontEndId,
        result: &ResultRef,
        now: Instant,
    ) -> Result<(), String> {
        let (session, item) = self.shown_item(front_end, result)?;
        let keys = self.history_keys(session, &result.extension, &item);
        let title = format!("Undo forgetting “{}”", item.title);
        let change = self.history.forget(&keys);
        self.changed(title, change, now);
        Ok(())
    }

    /// Forgets every Result of the Extension that owns `result`, the things
    /// they stand for, and the Aliases that name them. The menu stays open.
    fn clear_extension_history(
        &mut self,
        front_end: FrontEndId,
        result: &ResultRef,
        now: Instant,
    ) -> Result<(), String> {
        let name = result.extension.as_str();
        let (_, item) = self.shown_item(front_end, result)?;
        let ext = self
            .extension_named(name)
            .ok_or_else(|| format!("`{}` has no Action `{CLEAR_EXTENSION_HISTORY}`", item.title))?;
        let live = self
            .sessions
            .get(&front_end)
            .map(|session| session.live[ext.0].results.as_slice())
            .unwrap_or_default();
        // Their Identities go too, whoever else offers them: the History
        // doesn't know which Extension a pick of one came through.
        let identities: HashSet<&str> = self
            .index
            .identities(ext)
            .chain(live.iter().filter_map(|live| live.item.identity.as_deref()))
            .collect();
        let aliases: HashSet<String> = self
            .keywords
            .aliases
            .iter()
            .filter(|(_, alias)| alias.extension == name)
            .map(|(keyword, _)| alias_id(keyword))
            .collect();
        let change = self.history.forget_where(|(extension, id)| {
            extension == name
                || (extension == IDENTITIES && identities.contains(id))
                || (extension == OWN && aliases.contains(id))
        });
        self.store.forget_usage(Some(name));
        let title = format!("Undo clearing the history of {name}");
        self.changed(title, change, now);
        Ok(())
    }

    /// The Item of a Result shown in a Front end's session, with the session.
    fn shown_item(
        &self,
        front_end: FrontEndId,
        result: &ResultRef,
    ) -> Result<(&Session, ItemRef<'_>), String> {
        let gone = || "that Result is gone".to_owned();
        let session = self.sessions.get(&front_end).ok_or_else(gone)?;
        let source = self
            .source_of(session, &result.extension, &result.id)
            .ok_or_else(gone)?;
        let (_, item) = self.item(session, source).ok_or_else(gone)?;
        Ok((session, item))
    }

    /// The ids the picks of the Aliases that name a Result are kept under.
    fn alias_ids<'a>(
        &'a self,
        extension: &'a str,
        id: &'a str,
    ) -> impl Iterator<Item = String> + 'a {
        self.keywords
            .aliases
            .iter()
            .filter(move |(_, alias)| alias.extension == extension && alias.item == id)
            .map(|(keyword, _)| alias_id(keyword))
    }

    /// Everything the History may keep for one Result: its own picks, its
    /// Identity's and those of every other Result of that Identity, and the
    /// picks of the Aliases that name it.
    fn history_keys(&self, session: &Session, extension: &str, item: &Item) -> Vec<ResultKey> {
        let mut keys = vec![(extension.to_owned(), item.id.clone())];
        if let Some(identity) = &item.identity {
            keys.push((IDENTITIES.to_owned(), identity.clone()));
            for source in self.sources_of(session, IDENTITIES, identity) {
                if let Some((other, item)) = self.item(session, source) {
                    keys.push((other.to_owned(), item.id.clone()));
                }
            }
        }
        keys.extend(
            self.alias_ids(extension, &item.id)
                .map(|id| (OWN.to_owned(), id)),
        );
        keys
    }

    /// Saves a change to the History, which can be undone for [`UNDO_FOR`],
    /// and ranks every session again. A change of nothing is let go.
    fn changed(&mut self, title: String, change: Change, now: Instant) {
        if change.is_empty() {
            return;
        }
        self.store.save(&self.history);
        let id = self.next_id;
        self.next_id += 1;
        self.undos.push(Undoable {
            id,
            title,
            change,
            until: now + UNDO_FOR,
        });
        self.load_undo_item();
    }

    /// Undoes the change `id` to the History, if it can still be undone.
    fn undo(&mut self, id: u64) {
        let Some(n) = self.undos.iter().position(|undo| undo.id == id) else {
            return;
        };
        let undo = self.undos.remove(n);
        self.history.undo(undo.change);
        self.store.save(&self.history);
        self.load_undo_item();
    }

    /// Shows the Undo Item for the latest change to the History, or hides it
    /// when there is nothing left to undo, and ranks every session again.
    fn load_undo_item(&mut self) {
        if self
            .own
            .last()
            .is_some_and(|own| matches!(own.does, OwnAction::Undo))
        {
            self.own.pop();
        }
        if let Some(latest) = self.undos.last() {
            self.own
                .push(Own::new(undo_item(&latest.title), OwnAction::Undo));
        }
        self.mark_late();
    }

    /// Adds the Actions the Daemon offers on a Result the History keeps
    /// anything for: Forget, and clearing its Extension's History. They go
    /// with the Item's own Actions on the Daemon's own Results (ADR 0005),
    /// and after the Contributed actions on the rest. Only Results with
    /// a History get them, so a row costs nothing more until something is
    /// picked through it.
    fn offer_forgetting(
        &self,
        extension: &str,
        item: &mut Item,
        contributed: &mut Vec<OfferedAction>,
    ) {
        if self.history.is_empty() {
            return;
        }
        let picked = |extension: &str, id: &str| self.history.picks(extension, id).is_some();
        let remembered = picked(extension, &item.id)
            || item
                .identity
                .as_deref()
                .is_some_and(|identity| picked(IDENTITIES, identity))
            || self
                .alias_ids(extension, &item.id)
                .any(|alias| picked(OWN, &alias));
        if !remembered {
            return;
        }
        let forget = action_titled(FORGET, "Forget".into());
        if extension == OWN {
            item.actions.push(forget);
            return;
        }
        let clear = action_titled(
            CLEAR_EXTENSION_HISTORY,
            format!("Clear the history of {extension}"),
        );
        for action in [forget, clear] {
            contributed.push(OfferedAction {
                extension: OWN.to_owned(),
                action,
            });
        }
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
        let when = self.when(now);
        let at = when.at;
        let Some(session) = self.sessions.get(&front_end) else {
            return;
        };
        let source = self.source_of(session, &result.extension, &result.id);
        let row = session
            .shown
            .iter()
            .position(|(shown, _)| Some(*shown) == source);
        if self.history.settings().usage_log {
            self.store.log(Entry {
                at,
                used: Used::Pick {
                    query: session.query.clone(),
                    extension: result.extension.clone(),
                    id: result.id.clone(),
                    action: action.id.clone(),
                    row,
                },
            });
        }
        if let Some((_, alias)) = session
            .via_alias
            .as_ref()
            .filter(|(named, _)| Some(*named) == source)
        {
            self.history.touch(OWN, &alias_id(alias), when);
        } else if let Some(Source::Live(ext, _)) = source
            && self.extensions[ext.0].fallback()
        {
            // Frecency orders the fallbacks among themselves, but a fallback
            // is never a Learned choice, which would take over from the one
            // among the real matches.
            self.history.touch(&result.extension, &result.id, when);
        } else {
            let key = source
                .and_then(|source| self.history_key(session, source))
                .unwrap_or((&result.extension, &result.id));
            // What ranked above it, nearest first: what it was picked over.
            // The same thing from another Extension wasn't.
            let above: Vec<ResultKey> = row
                .map(|at| {
                    session.shown[..at]
                        .iter()
                        .rev()
                        .filter_map(|&(shown, _)| self.history_key(session, shown))
                        .filter(|&above| above != key)
                        .map(|(extension, id)| (extension.to_owned(), id.to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            let (extension, id) = (key.0.to_owned(), key.1.to_owned());
            let query = session.key.clone();
            self.history.pick(&extension, &id, &query, &above, when);
            if extension == IDENTITIES {
                // The Result itself too, so it goes ahead of the same thing
                // from other Extensions.
                self.history.touch(&result.extension, &result.id, when);
            }
        }
        self.store.save(&self.history);
        self.refresh_empty(Update::Late, None);
    }

    fn set_config(&mut self, config: Table) {
        self.config = config;
        self.load_keywords();
        self.load_settings();
        let empty_query = EmptyQuery::from_config(&self.config);
        if empty_query != self.empty_query {
            self.empty_query = empty_query;
            self.mark_late();
        }
        let kinds = KindOrder::from_config(&self.config);
        if kinds != self.kinds {
            self.kinds = kinds;
            self.mark_late();
        }
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
    /// again if one that ranks changed.
    fn load_settings(&mut self) {
        let settings = Settings::from_config(&self.config);
        let old = *self.history.settings();
        if settings != old {
            self.history.set_settings(settings);
            let usage_log = old.usage_log;
            if (Settings {
                usage_log,
                ..settings
            }) != old
            {
                self.mark_late();
            }
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
        self.repin();
        self.load_undo_item();
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
        if !ids.is_empty() {
            for id in ids {
                self.refresh(id, now);
            }
            // A refresh asks fallbacks what real matches left rows empty for.
            self.send_due_live_queries(now);
        }

        let versions = (self.index.version(), self.history.version());
        if versions != self.hot_versions {
            self.hot_versions = versions;
            self.hot_due = self.hot_due.min(now + HOT_SETTLE);
        }
        if self.hot_due <= now {
            self.plan_hot(now);
        }
        self.index.trim();
        // Once the menu has what it shows, so its first keystroke finds what
        // it reads in the CPU's cache.
        if std::mem::take(&mut self.warm) {
            std::hint::black_box(self.index.warm());
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

    /// Ranks one session's Results and sends them. Late Results rank with the
    /// rest while [`Session::reranks`]. After that the rows on
    /// screen stay where they are, and everything new goes below them, except
    /// an answer, which takes the top row.
    fn refresh(&mut self, id: FrontEndId, now: Instant) {
        let Some(mut session) = self.sessions.remove(&id) else {
            return;
        };
        let fresh = session.update == Update::Fresh;
        let keep = !fresh && !session.reranks(now);
        session.update = Update::None;

        if session.query.is_empty() {
            session.narrowing.forget();
            let mut ranked = self.empty_query(&session, now);
            if keep {
                // The rows on screen stay put, as for any query.
                let visible: Vec<Source> = session
                    .shown
                    .iter()
                    .take(session.rows)
                    .map(|&(source, _)| source)
                    .filter(|source| ranked.contains(source))
                    .collect();
                ranked.retain(|source| !visible.contains(source));
                ranked.splice(0..0, visible);
            }
            session.via_alias = None;
            let results = ranked
                .iter()
                .filter_map(|&source| self.row(&session, source, None))
                .collect();
            let shown = ranked.into_iter().map(|source| (source, false)).collect();
            session.send_results(shown, results, fresh);
            self.sessions.insert(id, session);
            return;
        }

        let mut candidates = Vec::new();
        self.scored.clear();
        let version = self.index.version();
        let narrow = session
            .narrowing
            .narrows(&session.query, &session.folded, version);
        let within = narrow.then_some(session.narrowing.matched.as_slice());
        self.kind_places.update(&self.kinds, self.index.kinds());
        let Daemon {
            index,
            matcher,
            tiers,
            kind_places,
            scored,
            ..
        } = self;
        index.search(
            &session.pattern,
            matcher,
            session.folded.filter(),
            within,
            |slot, ranked, score, matched| {
                scored.push(slot);
                if let Some(tier) = tiers.texts(ranked.texts(), &session.folded, matched) {
                    let kind = kind_places.place(slot.segment, ranked.kind());
                    let source = Source::Indexed(slot);
                    candidates.push(Candidate::new(
                        source,
                        tier,
                        kind,
                        score,
                        ranked.title_len(),
                    ));
                }
            },
        );
        session.narrowing.found(
            &session.query,
            &session.folded,
            version,
            self.scored.iter().copied(),
        );
        // Up to here the candidates are in slot order.
        let indexed = candidates.len();
        let mut fallbacks = Vec::new();
        for (ext, live) in session.live.iter().enumerate() {
            if live.results.is_empty() {
                continue;
            }
            let fallback = self.extensions[ext].fallback();
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
                let kind = self.kinds.place(&result.item.kind);
                let mut candidate = Candidate::new(
                    Source::Live(ExtensionId(ext), n as u32),
                    tier,
                    kind,
                    score,
                    len,
                );
                candidate.standing = if result.answer {
                    Standing::Answer
                } else if fallback {
                    Standing::Fallback
                } else {
                    Standing::Match
                };
                if candidate.standing == Standing::Fallback {
                    fallbacks.push(candidate);
                } else {
                    candidates.push(candidate);
                }
            }
        }
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
                let kind = self.kinds.place(&own.item.kind);
                candidates.push(Candidate::new(
                    Source::Own(n as u32),
                    tier,
                    kind,
                    score,
                    len,
                ));
            }
        }
        let indexed = self.add_near_misses(&mut session, &mut candidates, indexed, narrow, fresh);
        let primary = self.learn(&mut session, &mut candidates, indexed, now);
        self.add_fallbacks(&mut session, &mut candidates, fallbacks, now);

        let mut answers: Vec<Candidate> = Vec::new();
        candidates.retain(|candidate| {
            let answer = candidate.standing == Standing::Answer;
            if answer {
                answers.push(*candidate);
            }
            !answer
        });
        answers.sort_unstable_by(Candidate::rank);

        let mut kept: Vec<Candidate> = Vec::new();
        if keep {
            let visible = session.shown.iter().take(session.rows);
            // A fallback on screen moves down for a real match come late: it
            // never ranks above one.
            for (source, _) in visible.filter(|(_, answer)| !answer) {
                if let Some(at) = candidates
                    .iter()
                    .position(|c| c.source == *source && c.standing != Standing::Fallback)
                {
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
        let shown = ranked
            .iter()
            .map(|candidate| (candidate.source, candidate.standing == Standing::Answer))
            .collect();
        session.send_results(shown, results, fresh);
        self.sessions.insert(id, session);
    }

    /// Adds the fallback Extensions' results to `candidates`, after what
    /// the Daemon learned is applied to the real matches there, if those
    /// leave rows on screen empty, and asks the query of every fallback not
    /// asked it yet. With the rows full, its results would sit below them
    /// unseen. Frecency orders them among themselves, but takes no other part:
    /// a fallback is never a Learned choice, nor lifted a Tier.
    fn add_fallbacks(
        &self,
        session: &mut Session,
        candidates: &mut Vec<Candidate>,
        mut fallbacks: Vec<Candidate>,
        now: Instant,
    ) {
        if candidates.len() >= session.rows {
            return;
        }
        for (ext, live) in self.extensions.iter().zip(&mut session.live) {
            if live.asked || !ext.fallback() || ext.sender().is_none() {
                continue;
            }
            if let Some(debounce) = ext.debounce() {
                *live = Live::asking(session.query.clone(), None, now + debounce);
            }
        }
        if !self.history.is_empty() {
            let when = self.when(now);
            for fallback in &mut fallbacks {
                if let Some((extension, item)) = self.item(session, fallback.source) {
                    (fallback.frecency, fallback.direct) = self.frecency(extension, &item, when);
                }
            }
        }
        candidates.append(&mut fallbacks);
    }

    /// Adds the near misses of the session's query, Indexed items and the
    /// Daemon's own, to `candidates`, unless the real matches already fill the
    /// rows: near misses rank below every real match, so they would never be
    /// seen, except those on screen when the query hasn't changed.
    /// `narrowed` is whether this search narrowed the last one's matches;
    /// then near misses are looked for among the last one's, if it looked for
    /// them and allowed as many typos. The candidates before `indexed` are the
    /// Indexed items, in slot order, and stay so; returns how many there are
    /// now.
    fn add_near_misses(
        &mut self,
        session: &mut Session,
        candidates: &mut Vec<Candidate>,
        indexed: usize,
        narrowed: bool,
        fresh: bool,
    ) -> usize {
        let last = session.narrowing.nearly_for.take();
        let narrow =
            narrowed && last.is_some_and(|last| last.narrows_near_misses_of(&session.folded));
        let typos = session.folded.tolerates_typos();
        let look = typos && candidates.len() < session.rows;
        let mut nearly = std::mem::take(&mut session.narrowing.nearly);

        let Daemon {
            index,
            own,
            tiers,
            kinds,
            kind_places,
            ..
        } = self;
        let folded = &session.folded;
        let known = &candidates[..];
        // The near miss `source` is, unless it is a candidate already.
        let mut near_miss = |source: Source| {
            let (texts, len, kind) = match source {
                Source::Indexed(slot) => {
                    if known[..indexed]
                        .binary_search_by_key(&source, |c| c.source)
                        .is_ok()
                    {
                        return None;
                    }
                    let ranked = index.ranked(slot)?;
                    // Most rows fail on their letters: their texts aren't
                    // looked up.
                    let (letters, initials) = ranked.letters();
                    if !folded.may_nearly_match(letters, initials) {
                        return None;
                    }
                    let kind = kind_places.place(slot.segment, ranked.kind());
                    (ranked.texts(), ranked.title_len(), kind)
                }
                Source::Own(n) => {
                    if known[indexed..].iter().any(|c| c.source == source) {
                        return None;
                    }
                    let own = own.get(n as usize)?;
                    let len = own.haystack.title.len() as u32;
                    (own.haystack.texts(), len, kinds.place(&own.item.kind))
                }
                Source::Live(..) => return None,
            };
            tiers
                .near_miss_texts(texts, folded)
                .then(|| Candidate::new(source, Tier::NearMiss, kind, 0, len))
        };
        let mut found = Vec::new();
        if look {
            let (within, all) = if narrow {
                (&nearly[..], None)
            } else {
                (&[][..], Some(index.starting_with(folded.typo_initials())))
            };
            let slots = within.iter().copied().chain(all.into_iter().flatten());
            let own = (0..own.len() as u32).map(Source::Own);
            found.extend(
                slots
                    .map(Source::Indexed)
                    .chain(own)
                    .filter_map(&mut near_miss),
            );
        } else if typos && !fresh {
            let shown = session.shown.iter().take(session.rows);
            found.extend(shown.filter_map(|&(source, _)| near_miss(source)));
        }

        nearly.clear();
        if look {
            nearly.extend(&session.narrowing.matched);
            nearly.extend(found.iter().filter_map(|candidate| match candidate.source {
                Source::Indexed(slot) => Some(slot),
                _ => None,
            }));
            nearly.sort_unstable();
            // A slot can be both a match and a near miss of another word.
            nearly.dedup();
            session.narrowing.nearly_for = Some(session.folded.clone());
        }
        session.narrowing.nearly = nearly;
        // The Indexed items first, in slot order, then the Daemon's own.
        found.sort_unstable_by_key(|candidate| candidate.source);
        let own = found.partition_point(|candidate| matches!(candidate.source, Source::Indexed(_)));
        candidates.extend(found.drain(own..));
        let added = found.len();
        candidates.splice(indexed..indexed, found);
        let indexed = indexed + added;
        candidates[..indexed].sort_unstable_by_key(|candidate| candidate.source);
        indexed
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
        let when = self.when(now);
        let at = when.at;
        if !self.history.is_empty() {
            let settings = *self.history.settings();
            self.cache_frecent();
            for &(slot, picks, counts) in &self.frecent {
                if let Ok(n) =
                    candidates[..indexed].binary_search_by_key(&Source::Indexed(slot), |c| c.source)
                {
                    let frecency = picks.frecency(when, &settings);
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
                    (candidate.frecency, candidate.direct) = self.frecency(extension, &item, when);
                }
            }
            for candidate in candidates.iter_mut() {
                if candidate.frecency >= settings.lift_threshold {
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
                frecency: Some(self.history.frecency(OWN, &alias_id(keyword), when)),
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
        for slot in self.index.keyed(first) {
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
                        self.kinds.place(&item.kind),
                        session.max_score,
                        item.title.chars().count() as u32,
                    );
                    (candidate.frecency, candidate.direct) = self.frecency(extension, &item, when);
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
                        && !self.history_key(session, candidate.source).is_some_and(
                            |(extension, id)| {
                                passed_over.iter().any(|(e, i)| e == extension && i == id)
                            },
                        )
                }
            };
            if named {
                candidate.class = Class::Named;
            }
        }
        primary
    }

    /// What the Empty query shows: the Results most used at this time of day,
    /// then the Fixed menu, each thing once. The most used rank by their
    /// picks near this time of day, then by all their picks; an Alias's
    /// picks count for the Result it names. Of the Results of one Identity,
    /// the one picked through most stands for it.
    fn empty_query(&mut self, session: &Session, now: Instant) -> Vec<Source> {
        let when = self.when(now);
        let half_life = self.history.settings().half_life_secs;
        let mut most: Vec<MostUsed> = Vec::new();
        if self.empty_query.most_used > 0 && !self.history.is_empty() {
            self.cache_frecent();
            // In slot order, an Item's Identity's picks next to its own.
            for &(slot, picks, counts) in &self.frecent {
                let source = Source::Indexed(slot);
                if most.last().is_none_or(|last| last.source != source) {
                    most.push(MostUsed::new(source));
                }
                let entry = most.last_mut().expect("just pushed");
                let used = picks.habit(when, half_life);
                if counts != Counts::Direct {
                    entry.used = used;
                }
                if counts != Counts::Shared {
                    entry.direct = used.overall;
                }
            }
            for (n, own) in self.own.iter().enumerate() {
                if let Some(picks) = self.history.picks(OWN, &own.item.id) {
                    most.push(MostUsed {
                        used: picks.habit(when, half_life),
                        ..MostUsed::new(Source::Own(n as u32))
                    });
                }
            }
            let mut aliases: Vec<_> = self.keywords.aliases.iter().collect();
            aliases.sort_unstable_by_key(|&(keyword, _)| keyword);
            for (keyword, alias) in aliases {
                let Some(picks) = self.history.picks(OWN, &alias_id(keyword)) else {
                    continue;
                };
                let Some(source) = self.source_of(session, &alias.extension, &alias.item) else {
                    continue;
                };
                let used = picks.habit(when, half_life);
                match most.iter_mut().find(|entry| entry.source == source) {
                    Some(entry) => entry.used = entry.used.and(used),
                    None => most.push(MostUsed {
                        used,
                        ..MostUsed::new(source)
                    }),
                }
            }
        }

        let identity = |source: Source| self.identity(session, source);
        let mut stands_for: HashMap<&str, usize> = HashMap::new();
        let mut keep = vec![true; most.len()];
        for n in 0..most.len() {
            let Some(id) = identity(most[n].source) else {
                continue;
            };
            let Some(&other) = stands_for.get(id) else {
                stands_for.insert(id, n);
                continue;
            };
            let (best, rest) = if most[n].direct > most[other].direct {
                (n, other)
            } else {
                (other, n)
            };
            // They share their Identity's picks, but an Alias may name one.
            most[best].used = most[best].used.max(most[rest].used);
            keep[rest] = false;
            stands_for.insert(id, best);
        }
        let mut kept = keep.iter();
        most.retain(|_| *kept.next().expect("one per Result"));
        let rank = |a: &MostUsed, b: &MostUsed| a.used.rank(&b.used).then(a.source.cmp(&b.source));
        let wanted = self.empty_query.most_used;
        if most.len() > wanted && wanted > 0 {
            most.select_nth_unstable_by(wanted - 1, rank);
        }
        most.truncate(wanted);
        most.sort_unstable_by(rank);

        let mut shown: Vec<Source> = most.into_iter().map(|entry| entry.source).collect();
        for (extension, id) in &self.empty_query.menu {
            if shown.len() >= MAX_RESULTS {
                break;
            }
            let Some(source) = self.source_of(session, extension, id) else {
                continue;
            };
            let same = |&shown: &Source| {
                shown == source || identity(shown).is_some_and(|id| identity(source) == Some(id))
            };
            if !shown.iter().any(same) {
                shown.push(source);
            }
        }
        shown
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
                frecent.extend(slots.map(|slot| (slot, picks, Counts::Shared)));
            } else if let Some(ext) = self.extension_named(extension)
                && let Some(slot) = self.index.find(ext, id)
            {
                let identified = self.index.identity(slot).is_some();
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
        let indexed = self.index.identified(id).map(Source::Indexed);
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
        let identity = |candidate: &Candidate| self.identity(session, candidate.source);
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
    fn frecency(&self, extension: &str, item: &Item, when: When) -> (f64, f64) {
        let own = self.history.frecency(extension, &item.id, when);
        match &item.identity {
            Some(identity) => (self.history.frecency(IDENTITIES, identity, when), own),
            None => (own, own),
        }
    }

    /// A Result's Item and the name of the Extension that owns it.
    fn item<'a>(&'a self, session: &'a Session, source: Source) -> Option<(&'a str, ItemRef<'a>)> {
        match source {
            Source::Indexed(slot) => {
                let item = self.index.item(slot)?;
                Some((
                    self.extensions[slot.extension().0].name(),
                    ItemRef::Resident(item),
                ))
            }
            Source::Live(ext, n) => Some((
                self.extensions[ext.0].name(),
                ItemRef::Borrowed(&session.live[ext.0].results.get(n as usize)?.item),
            )),
            Source::Own(n) => Some((OWN, ItemRef::Borrowed(&self.own.get(n as usize)?.item))),
        }
    }

    /// A Result's id and Identity, and the name of the Extension that owns
    /// it, without reading an Indexed item off its Shelf.
    fn names<'a>(
        &'a self,
        session: &'a Session,
        source: Source,
    ) -> Option<(&'a str, &'a str, Option<&'a str>)> {
        let item = match source {
            Source::Indexed(slot) => {
                return Some((
                    self.extensions[slot.extension().0].name(),
                    self.index.id(slot)?,
                    self.index.identity(slot),
                ));
            }
            Source::Live(ext, n) => &session.live[ext.0].results.get(n as usize)?.item,
            Source::Own(n) => &self.own.get(n as usize)?.item,
        };
        let extension = match source {
            Source::Live(ext, _) => self.extensions[ext.0].name(),
            _ => OWN,
        };
        Some((extension, &item.id, item.identity.as_deref()))
    }

    /// What the History keeps a Result under, as [`history_key`] gives it.
    fn history_key<'a>(
        &'a self,
        session: &'a Session,
        source: Source,
    ) -> Option<(&'a str, &'a str)> {
        let (extension, id, identity) = self.names(session, source)?;
        Some(match identity {
            Some(identity) => (IDENTITIES, identity),
            None => (extension, id),
        })
    }

    /// A Result's Identity, if it has one.
    fn identity<'a>(&'a self, session: &'a Session, source: Source) -> Option<&'a str> {
        match source {
            Source::Indexed(slot) => self.index.identity(slot),
            _ => self.names(session, source)?.2,
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
        let (live_title, mut live_starts);
        let (owner, item, title): (Option<ExtensionId>, ItemRef, Text) = match source {
            Source::Indexed(slot) => (
                Some(slot.extension()),
                ItemRef::Resident(self.index.item(slot)?),
                self.index.texts(slot)?.title,
            ),
            Source::Live(ext, n) => {
                let item = &session.live[ext.0].results.get(n as usize)?.item;
                live_title = rank::folded(&item.title);
                live_starts = Vec::new();
                rank::word_starts(live_title.slice(..), Some(&item.title), &mut live_starts);
                let title = Text {
                    chars: live_title.slice(..),
                    starts: &live_starts,
                };
                (Some(ext), ItemRef::Borrowed(item), title)
            }
            Source::Own(n) => {
                let own = self.own.get(n as usize)?;
                (
                    None,
                    ItemRef::Borrowed(&own.item),
                    own.haystack.texts().title,
                )
            }
        };
        self.highlights.clear();
        if session
            .pattern
            .indices(title.chars, &mut self.matcher, &mut self.highlights)
            .is_some()
        {
            self.highlights.sort_unstable();
            self.highlights.dedup();
        } else {
            self.highlights.clear();
            // A near miss: the words it nearly matches.
            if session.folded.tolerates_typos() {
                self.tiers
                    .near_miss_highlights(title, &session.folded, &mut self.highlights);
            }
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
        let (extension, mut contributed) = match owner {
            Some(owner) => (
                self.extensions[owner.0].name().to_owned(),
                self.contributed_for(owner, item.kind.name(), &item.actions),
            ),
            None => (OWN.to_owned(), Vec::new()),
        };
        self.offer_forgetting(&extension, &mut item, &mut contributed);
        Some(ShownResult {
            extension,
            item,
            contributed,
            highlights: self.highlights.clone(),
        })
    }

    /// Other running Extensions' Actions for Items of `kind`, in Extension
    /// order. When a shortcut clashes, the owner's binding in `own` wins, then
    /// the earlier Extension's.
    fn contributed_for(
        &self,
        owner: ExtensionId,
        kind: &str,
        own: &[Action],
    ) -> Vec<OfferedAction> {
        let Some(offered) = self.contributed.get(kind) else {
            return Vec::new();
        };
        let mut taken: Vec<&str> = own
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
            let own: BTreeMap<(String, String), Action> = self
                .index
                .offered(ExtensionId(id))
                .into_iter()
                .map(|(kind, action)| ((kind, action.id.clone()), action))
                .collect();
            for ((kind, _), action) in own {
                if let Ok(kind) = KindName::new(&kind) {
                    entries.push(CatalogEntry {
                        extension: ext.name().to_owned(),
                        kind,
                        action,
                    });
                }
            }
        }
        entries
    }
}
