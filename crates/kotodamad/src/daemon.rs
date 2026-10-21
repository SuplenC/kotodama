//! The Daemon's state machine, fed one [`Event`] at a time by the main
//! loop. It owns the index, the Front ends' sessions and the Extensions' state,
//! and never blocks: everything it sends goes into a channel that another
//! thread drains, so a stuck or dead Extension can't stall it.
//!
//! Starting processes, typing into windows and saving the History are behind
//! [`Launcher`], [`Desktop`] and [`Store`], so it runs the same in tests as in
//! `kotodamad`.
//!
//! # Owns
//!
//! - [`Daemon`]: the state of every Extension, Front end and session, and [`Daemon::handle`], the
//!   one entry point.
//! - [`Event`] and [`Output`], and the [`Launcher`] and [`Desktop`] it acts through.
//! - The order of the Results (`Candidate::rank`), the sessions (`Session`) and the Daemon's own
//!   Results (`Own`).
//! - Its methods, by concern, in `daemon/`: each Extension's run (`extensions.rs`), what it sends
//!   (`from_extension.rs`), the Front ends (`front_ends.rs`), opening on a View (`openings.rs`),
//!   picks (`picks.rs`) and undoing them (`undo.rs`), the config (`settings.rs`), the search
//!   (`search.rs`), the Empty query (`empty.rs`), the rows (`rows.rs`) and the hot Items
//!   (`hot.rs`).
//!
//! # Not here
//!
//! - The index and the matching: `index.rs`, `segment.rs`, `rank.rs`.
//! - The processes, the socket and the desktop: `host.rs`, `server.rs`, `desktop.rs`.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::TrySendError;
use std::time::Duration;
use std::time::Instant;

use kotodama_protocol::Action;
use kotodama_protocol::ActionRef;
use kotodama_protocol::CatalogEntry;
use kotodama_protocol::Clipboard;
use kotodama_protocol::FromExtension;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::Job;
use kotodama_protocol::LiveResult;
use kotodama_protocol::NamespacedKind;
use kotodama_protocol::Problem;
use kotodama_protocol::ResultRef;
use kotodama_protocol::Scope;
use kotodama_protocol::ShownResult;
use kotodama_protocol::Table;
use kotodama_protocol::ToExtension;
use kotodama_protocol::ToFrontEnd;
use nucleo_matcher::Config;
use nucleo_matcher::Matcher;
use nucleo_matcher::pattern::CaseMatching;
use nucleo_matcher::pattern::Normalization;
use nucleo_matcher::pattern::Pattern;

use crate::apps::Apps;
use crate::clock::Clock;
use crate::clock::When;
use crate::config;
use crate::conflict::Conflict;
use crate::discovery::Discovered;
use crate::discovery::Found;
use crate::discovery::Stamp;
use crate::empty_query::EmptyQuery;
use crate::engine::DesktopSettings;
use crate::engine::Engine;
use crate::history;
use crate::history::Change;
use crate::history::Habit;
use crate::history::History;
use crate::history::Store;
use crate::history::query_key;
use crate::index::Index;
use crate::index::Reach;
use crate::index::SIDES;
use crate::index::Slot;
use crate::indexer::Indexed;
use crate::indexer::Indexer;
use crate::indexer::Kept;
use crate::keywords::Keywords;
use crate::kind_order::KindOrder;
use crate::kind_order::KindPlaces;
use crate::most_used::Ahead;
use crate::most_used::Picked;
use crate::most_used::Versions;
use crate::narrowing::Narrowing;
use crate::open_view::Opening;
use crate::paths::Overrides;
use crate::pick::Pick;
use crate::problem;
use crate::problem::Problems;
use crate::rank;
use crate::rank::Haystack;
use crate::rank::Tier;
use crate::rank::Tiers;
use crate::views::Views;

mod empty;
mod extensions;
mod from_extension;
mod front_ends;
mod hot;
mod openings;
mod picks;
mod rows;
mod search;
mod settings;
mod undo;

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
/// How long after a change to the History it can be undone, unless
/// `[history] undo_seconds` says otherwise.
pub const UNDO_FOR: Duration = Duration::from_secs(history::DEFAULT_UNDO_SECS);

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
    /// How text goes into windows, as the config says, whenever a config load changes it.
    fn configure(&mut self, _settings: DesktopSettings) {}
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
        ended: Ended,
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
    /// The config directory changed, but can't be loaded: the last good
    /// config stays.
    ConfigFailed(config::Error),
    /// The Extension directories changed: these are all the Extensions there are.
    Extensions(Discovered),
    /// A file changed next to the program of an Extension that couldn't
    /// start, which may start this time.
    ProgramsChanged,
    /// A new version of an Extension's Items, indexed off the keystroke path
    /// (see [`Daemon::index_in_background`]).
    Indexed(Indexed),
    /// Items to keep resident, read on the indexing thread: the hot ones,
    /// planned there, and those a keyword or an Alias names.
    Kept(Kept),
}

/// Every field but a Front end's `disconnect`, a closure.
impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::ExtensionUp {
                extension,
                generation,
                sender,
            } => f
                .debug_struct("ExtensionUp")
                .field("extension", extension)
                .field("generation", generation)
                .field("sender", sender)
                .finish(),
            Event::FromExtension {
                extension,
                generation,
                message,
            } => f
                .debug_struct("FromExtension")
                .field("extension", extension)
                .field("generation", generation)
                .field("message", message)
                .finish(),
            Event::ExtensionDown {
                extension,
                generation,
                ended,
            } => f
                .debug_struct("ExtensionDown")
                .field("extension", extension)
                .field("generation", generation)
                .field("ended", ended)
                .finish(),
            Event::FrontEndUp { front_end, sender, .. } => f
                .debug_struct("FrontEndUp")
                .field("front_end", front_end)
                .field("sender", sender)
                .finish_non_exhaustive(),
            Event::FromFrontEnd { front_end, message } => f
                .debug_struct("FromFrontEnd")
                .field("front_end", front_end)
                .field("message", message)
                .finish(),
            Event::FrontEndDown { front_end } => f.debug_struct("FrontEndDown").field("front_end", front_end).finish(),
            Event::Config(config) => f.debug_tuple("Config").field(config).finish(),
            Event::ConfigFailed(err) => f.debug_tuple("ConfigFailed").field(err).finish(),
            Event::Extensions(discovered) => f.debug_tuple("Extensions").field(discovered).finish(),
            Event::ProgramsChanged => f.write_str("ProgramsChanged"),
            Event::Indexed(indexed) => f.debug_tuple("Indexed").field(indexed).finish(),
            Event::Kept(kept) => f.debug_tuple("Kept").field(kept).finish(),
        }
    }
}

/// How a run of an Extension ended, as [`Event::ExtensionDown`] tells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    /// It ran, then stopped: how, as its exit status or what broke the
    /// connection. It is restarted after a while.
    Stopped(String),
    /// Its program couldn't be started, as when it doesn't exist: why.
    CantStart(String),
    /// It speaks a protocol this Daemon doesn't, as the handshake found.
    Refused(String),
}

/// How many Results a Front end gets for one query.
pub const MAX_RESULTS: usize = 50;
/// How long after a keystroke Late Results may still rerank the rows on
/// screen, unless the selection moves first. Most Live queries answer
/// within it, and the eye has barely found the top row by then.
pub const RERANK_FOR: Duration = Duration::from_millis(150);
/// How long to wait for Front ends to close before typing anyway.
const CLOSE_TIMEOUT: Duration = Duration::from_millis(500);
/// How long `kotodama open --view` waits for its View before opening the
/// menus on the search instead. It covers starting the Extension,
/// as when the keypress also started the Daemon.
const OPEN_VIEW_TIMEOUT: Duration = Duration::from_secs(1);
/// How long `kotodama open` or `toggle` waits for a menu to connect when
/// none is, as at login, when the keypress can come before the menu has
/// started, before it answers that it reached none.
const MENU_WAIT: Duration = Duration::from_secs(2);
/// The first restart delay after a crash; each crash in a row doubles it, up
/// to `[extensions] retry_max_seconds`.
const FIRST_RETRY: Duration = Duration::from_millis(100);
/// An Extension that ran this long before it stopped starts its backoff over.
const STABLE: Duration = Duration::from_secs(10);
/// How many results one Live query may contribute; the rest are dropped.
pub const MAX_LIVE_RESULTS: usize = 500;
/// How long an Extension or Front end may take to say hello before it counts as
/// hung.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// How many Items are hot at most: kept resident, and read when the menu
/// opens.
pub const HOT_CAPACITY: usize = 256;
/// How often the hot Items are worked out again, ahead of the time of day.
const HOT_EVERY: Duration = Duration::from_mins(5);
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
    /// Can't start, or speaks a protocol the Daemon doesn't, for `reason`:
    /// starting it again would fail the same way, so it waits for its
    /// program, as `stamp` was then, or its Manifest to change.
    Failed {
        reason: String,
        stamp: Option<Stamp>,
    },
    /// Uninstalled. The slot stays so `ExtensionId`s keep their meaning, and is
    /// reused if an Extension of the same name comes back.
    Removed,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a fact about one run on its own, with no states they combine into"
)]
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
    /// Asked the Launcher to stop this run; its `ExtensionDown` is on its way.
    stopping: bool,
    /// The Manifest's `live_query.fallback`, read once per Manifest: every
    /// refresh asks it.
    fallback: bool,
    /// What this run last said of whether it can do anything; `None` until
    /// it says.
    status: Option<Status>,
    /// Whether Items of this run have reached the search, or are hidden.
    has_items: bool,
    /// How its last run stopped, while it is down.
    ended: Option<String>,
}

/// What an Extension said of itself in its last `FromExtension::Status`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Status {
    usable: bool,
    note: Option<String>,
}

/// Whether `found`'s Manifest makes it a fallback, such as web search.
fn is_fallback(found: &Found) -> bool {
    found.manifest.live_query.as_ref().is_some_and(|live| live.fallback)
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
            status: None,
            has_items: false,
            ended: None,
        }
    }

    /// What the user should know while it can't run.
    fn problem(&self) -> Option<Problem> {
        let message = match &self.state {
            State::Failed { reason, .. } => {
                format!("{reason}; it starts again once its program or Manifest changes")
            }
            _ if self.is_down() => format!("{}; restarting", self.ended.as_deref()?),
            _ => return None,
        };
        Some(Problem {
            about: format!("extension {}", self.name()),
            message,
        })
    }

    /// Whether the search shows its app's entry Item: as it last said, else
    /// once its Items are in, or always if it asks for Live queries, so an
    /// Extension that never says keeps its entry Item.
    fn usable(&self) -> bool {
        match &self.status {
            Some(status) => status.usable,
            None => self.has_items || self.found.manifest.live_query.is_some(),
        }
    }

    /// What it last said the user should know, for its app's intro card.
    fn note(&self) -> Option<&str> {
        self.status.as_ref()?.note.as_deref()
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
    /// A disconnected channel means it is gone, and its `ExtensionDown` is on its
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
            State::Down { .. } | State::Failed { .. } => true,
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

    /// The first prefix its Manifest routes Live queries by, if any.
    fn first_prefix(&self) -> Option<String> {
        let live = self.found.manifest.live_query.as_ref()?;
        live.prefixes.first().cloned()
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
pub(crate) enum Source {
    Indexed(Slot),
    /// The `n`th Live query result from an Extension, in arrival order.
    Live(ExtensionId, u32),
    /// The Daemon's own `n`th Item.
    Own(u32),
    /// The Alias the query names, when its Item is gone or its Extension is
    /// down: a row that says so and can't run.
    Unavailable,
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
    /// How well it matches, to break a tie: initials beat a word start, and
    /// an exact title beats an exact keyword.
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
    /// included.
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
    /// Lazy: most pairs differ in the first few keys, and a keystroke compares
    /// thousands of pairs.
    fn rank(&self, other: &Candidate) -> std::cmp::Ordering {
        self.standing
            .cmp(&other.standing)
            .then_with(|| other.class.cmp(&self.class))
            .then_with(|| other.tier.cmp(&self.tier))
            .then_with(|| other.frecency.total_cmp(&self.frecency))
            .then_with(|| other.fine.cmp(&self.fine))
            .then_with(|| self.by_alias.cmp(&other.by_alias))
            .then_with(|| self.kind.cmp(&other.kind))
            .then_with(|| other.score.cmp(&self.score))
            .then_with(|| self.len.cmp(&other.len))
            .then_with(|| self.source.cmp(&other.source))
    }
}

/// Takes the answers out of `candidates`, ranked: they take the top rows,
/// above the rest and the rows on screen.
fn take_answers(candidates: &mut Vec<Candidate>) -> Vec<Candidate> {
    let mut answers: Vec<Candidate> = Vec::new();
    candidates.retain(|candidate| {
        let answer = candidate.standing == Standing::Answer;
        if answer {
            answers.push(*candidate);
        }
        !answer
    });
    answers.sort_unstable_by(Candidate::rank);
    answers
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
    /// Which Items the query searches: those the whole search shows, or,
    /// when it starts with one, that Extension's alone.
    reach: Reach,
    /// The app the query is in, by its place in [`Daemon::apps`].
    scope: Option<usize>,
    /// The app the user entered, by its place in [`Daemon::apps`], while the
    /// query keeps its word. Typing the word starts no app of itself, so
    /// `system monitor` still searches everything.
    entered: Option<usize>,
    /// Where the text searched starts in `query`: past the app's word and its
    /// space, if it starts with one.
    rest: usize,
    /// The query as Learned choices and Aliases are kept under, the app's word
    /// included, so what is learned behind one stays there.
    key: String,
    /// The text searched as tiers are worked out from.
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
    last_search: LastSearch,
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
    /// Whether it has asked for a menu, as `kotodama` does: a Front end
    /// that asks is no menu itself.
    asked_for_menu: bool,
    update: Update,
    /// The part of the day the Empty query was last ranked in, as
    /// [`history::part_of_day`] names it.
    empty_ranked_in: Option<(u64, u32)>,
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

    /// Whether `source` is a Live query's answer, which the History keeps
    /// nothing of.
    fn is_answer(&self, source: Source) -> bool {
        let Source::Live(ext, n) = source else {
            return false;
        };
        self.live[ext.0]
            .results
            .get(n as usize)
            .is_some_and(|result| result.answer)
    }

    /// The text searched: the query past its app's word, inside one.
    fn searched(&self) -> &str {
        &self.query[self.rest..]
    }

    /// Whether the query asks the Extension `id`'s Live queries anything,
    /// given which Segments the whole search reads: the search asks those
    /// whose Items it shows, an app its own Extension alone.
    fn asks(&self, id: ExtensionId, searched: &[bool]) -> bool {
        self.reach.reads(id.0 * SIDES, searched) && !self.searched().is_empty()
    }

    /// The Live query to send the Extension `id` for this query, and the
    /// prefix that routed it: in its app every query, routed by its first
    /// prefix if no prefix or trigger routes it, so the calculator answers
    /// whatever is typed in it as it does after `=`.
    fn live_query(&self, id: ExtensionId, ext: &Extension, searched: &[bool]) -> Option<(String, Option<String>)> {
        if !self.asks(id, searched) {
            return None;
        }
        ext.route(self.searched())
            .or_else(|| (self.reach == Reach::App(id)).then(|| (self.searched().to_owned(), ext.first_prefix())))
    }

    /// Sends `results`, the rows of `shown`, as the next version, for a query
    /// behind `scope`. A late update notes each Result it adds or moves up a
    /// row as risen in that version.
    fn send_results(
        &mut self,
        shown: Vec<(Source, bool)>,
        results: Vec<ShownResult>,
        scope: Option<Scope>,
        fresh: bool,
    ) {
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
            scope,
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
        self.filters.retain(|(ext, id, _)| !(*ext == extension && id == view));
        if !text.is_empty() {
            self.filters.push((extension, view.to_owned(), text));
        }
    }
}

/// What a Session's last search of the Index found, so the next search
/// narrows to it where it can.
#[derive(Default)]
struct LastSearch {
    /// The slots it matched, in slot order.
    narrowing: Narrowing<Slot>,
    /// The index version it searched: the slots of another version may hold
    /// other Items.
    version: u64,
    /// The slots that matched or nearly did, in slot order, if near misses
    /// were looked for. A query that only adds to it, with as many typos for
    /// each word, can only nearly match these.
    nearly: Vec<Slot>,
    /// The query as tiers are worked out from, if `nearly` is for it.
    nearly_for: Option<rank::Query>,
}

impl LastSearch {
    /// The slots `query`, folded as `folded`, need only search at index
    /// version `version`; `None` if it must search every slot.
    fn within(&self, query: &str, folded: &rank::Query, version: u64) -> Option<&[Slot]> {
        if self.version != version {
            return None;
        }
        self.narrowing.within(query, folded)
    }

    /// Remembers that `query`, folded as `folded`, matched `matched` at index
    /// version `version`.
    fn found(&mut self, query: &str, folded: &rank::Query, version: u64, matched: impl IntoIterator<Item = Slot>) {
        self.narrowing.found(query, folded, matched);
        self.version = version;
    }

    /// Forgets the last search, as the Empty query does: the next one starts
    /// from every Item.
    fn forget(&mut self) {
        self.narrowing.forget();
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
    /// Enters the `n`th app: the entry Item of its Extension.
    App(usize),
    ClearHistory,
    /// Undoes the latest change to the History.
    Undo,
    /// Opens the list of problems.
    Problems,
}

impl Own {
    fn new(item: Item, does: OwnAction) -> Self {
        Own {
            haystack: Haystack::new(&item),
            item,
            does,
        }
    }

    /// The best it may match by. An app's entry Item matches at most as a
    /// word start, its title or word typed whole included, so that
    /// GNOME Calculator ranks above the Calculator app for `calc`. A few
    /// picks of the entry Item make it the Learned choice for the word.
    fn tier_cap(&self) -> Tier {
        match self.does {
            OwnAction::App(_) => Tier::WordStart,
            _ => Tier::Exact,
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
        app_only: false,
        actions: vec![Action {
            id: "run".into(),
            title: "Clear".into(),
            shortcut: None,
            arguments: Vec::new(),
            confirm: false,
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
        app_only: false,
        actions: vec![action_titled("run", "Undo".into())],
    }
}

/// An Item known by its id alone, as an Alias names one the Daemon never saw.
fn named_only(id: &str) -> Item {
    Item {
        id: id.to_owned(),
        kind: ItemKind::Namespaced(NamespacedKind::new("kotodama.alias").expect("a valid kind")),
        title: id.to_owned(),
        subtitle: None,
        icon: None,
        keyword: None,
        identity: None,
        keywords: Vec::new(),
        hidden: false,
        app_only: false,
        actions: Vec::new(),
    }
}

/// An Action that takes no Arguments and has no shortcut.
fn action_titled(id: &str, title: String) -> Action {
    Action {
        id: id.to_owned(),
        title,
        shortcut: None,
        arguments: Vec::new(),
        confirm: false,
    }
}

/// A change to the History that can be undone until `until`. Changes are
/// undone by id, so one can be undone whatever changed after it.
struct Undoable {
    id: u64,
    by: UndoneBy,
    change: Change,
    until: Instant,
}

/// Who may undo a change to the History.
enum UndoneBy {
    /// The user, who is offered it in these words.
    User(Done),
    /// The Extension that ran an Action, whose pick the change is, by
    /// withdrawing the pick. The user is never offered it.
    Extension(ExtensionId),
}

impl Undoable {
    /// What it did in words, if the user may undo it.
    fn done(&self) -> Option<&Done> {
        match &self.by {
            UndoneBy::User(done) => Some(done),
            UndoneBy::Extension(_) => None,
        }
    }

    /// Offers a Front end to undo it, if the user may.
    fn offer(&self) -> Option<ToFrontEnd> {
        self.done().map(|done| ToFrontEnd::OfferUndo {
            change: self.id,
            title: done.offer.clone(),
        })
    }
}

/// What a change to the History did, in words.
struct Done {
    /// What a Front end's offer to undo it reads, as `Forgot “Firefox”`.
    offer: String,
    /// What the Undo Result reads while it is the latest.
    undo: String,
}

impl Done {
    #[must_use]
    fn new(offer: impl Into<String>, undo: impl Into<String>) -> Self {
        Done {
            offer: offer.into(),
            undo: undo.into(),
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
    /// How well the keyword makes it match: exact, but for an app's word.
    tier: Tier,
}

/// The id an Alias's picks are kept under, for the Extension [`OWN`].
fn alias_id(keyword: &str) -> String {
    format!("alias:{keyword}")
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

/// Reads `session.query`: the app it is in, if the user entered one and it
/// still starts with its word, and the text
/// searched past it, as the matcher and the tiers take it. A query that
/// moves into another app, or out of one, searches other Items, so the last
/// search can't narrow it.
fn read_query(apps: &Apps, matcher: &mut Matcher, session: &mut Session) {
    let (scope, rest) = match apps.scope_of(&session.query) {
        Some((n, rest)) if session.entered == Some(n) => (Some(n), rest),
        _ => (None, 0),
    };
    let reach = scope
        .and_then(|n| apps.get(n))
        .map_or(Reach::Root, |app| Reach::App(app.extension));
    if reach != session.reach {
        session.last_search.forget();
    }
    session.reach = reach;
    session.scope = scope;
    session.entered = scope;
    session.rest = rest;
    let searched = &session.query[rest..];
    // Folded like the haystacks, so neither case nor accents matter.
    session.pattern = Pattern::parse(&rank::fold_text(searched), CaseMatching::Ignore, Normalization::Never);
    session.folded = rank::Query::new(searched);
    session.max_score = session
        .pattern
        .atoms
        .iter()
        .filter_map(|atom| {
            let needle = atom.needle_text();
            matcher.fuzzy_match(needle, needle)
        })
        .map(u32::from)
        .sum();
    session.key = query_key(&session.query);
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
    /// The versions of what they were worked out from (see
    /// [`versions`](Self::versions)).
    hot_versions: Versions,
    /// Bumped whenever something the Empty query ranks by changes, other
    /// than the Index and the History: the own Items, the Aliases, the
    /// settings, the clock.
    ranks_version: u64,
    /// The Empty query's most used Results, ranked ahead as the hot Items
    /// were last planned.
    ahead: Option<Ahead>,
    /// How many times the Empty query's most used Results were ranked on
    /// this thread, for want of a ranking made ahead.
    ranked_here: u64,
    /// The Items that are a query's Learned choice, strongest first, as of
    /// the Index and History versions in `learned_versions`.
    learned: Arc<[Slot]>,
    learned_versions: Option<(u64, u64)>,
    /// A menu is opening: read what its first keystroke will read.
    warm: bool,
    /// How many times the CPU cache was warmed for a menu.
    warmed: u64,
    matcher: Matcher,
    sessions: BTreeMap<FrontEndId, Session>,
    live_queries: HashMap<u64, (FrontEndId, ExtensionId)>,
    invocations: HashMap<u64, (FrontEndId, ExtensionId)>,
    /// By the id of the form's View.
    argument_forms: HashMap<String, ArgumentForm>,
    /// The Proposals waiting for the user to pick their Item, by the Front
    /// end that shows the list; at most one each.
    picks: HashMap<FrontEndId, Pick>,
    /// The picks' lists, by the Extension whose Items they hold, for the
    /// Daemon to filter as the user types.
    pick_lists: Views,
    next_id: u64,
    config: Table,
    /// The Extensions in the Extension directories, before the config
    /// switches any off, changes or adds to them.
    installed: Vec<Found>,
    engine: Engine,
    /// The config's `[paths]`, for `kotodamad` to follow.
    paths: Overrides,
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
    /// `kotodama open` and `toggle` asked while no menu was connected,
    /// each until it gives up on one: passed on to the first menu that
    /// searches.
    awaiting_menu: Vec<(FrontEndId, FromFrontEnd, Instant)>,
    /// The menu last shown, which `kotodama open` and `toggle` go to.
    shown_last: Option<FrontEndId>,
    launcher: L,
    desktop: D,
    store: S,
    history: History,
    /// The wall clock the History's times come from.
    clock: Clock,
    started: Instant,
    keywords: Keywords,
    /// The Items Aliases name, by Extension name and id, as they were when
    /// their Extension last stopped, so a row can show what an Alias stands
    /// for while it can't run.
    last_seen: HashMap<(String, String), Arc<Item>>,
    empty_query: EmptyQuery,
    kinds: KindOrder,
    /// `kinds` for each Segment's kinds, as of the last search.
    kind_places: KindPlaces,
    /// Each Extension's app.
    apps: Apps,
    /// The keywords two things asked for, as last told to the user.
    conflicts: Vec<Conflict>,
    problems: Problems,
    /// The Front ends showing the list of problems, which is kept up to
    /// date there.
    problems_open: HashSet<FrontEndId>,
    /// Which Extensions can't start changed since
    /// [`failed_programs`](Self::failed_programs) last said.
    failed_changed: bool,
    /// One per Quicklink, then one per app (its entry Item), then the
    /// clear-history Item, then the Undo Item while there is something to
    /// undo.
    own: Vec<Own>,
    /// The changes to the History that can still be undone, oldest first.
    undos: Vec<Undoable>,
    /// The picked Indexed items, by slot, as of the index and History
    /// versions in `frecent_versions`, and their picks. Shared with the
    /// indexing thread, which plans from it.
    frecent: Arc<Picked>,
    frecent_versions: Option<(u64, u64)>,
    /// What the picks of `frecent` count for at one time, in its order: kept
    /// from one query to the next.
    habits: Vec<Habit>,
    /// The same for a search's Frecency.
    frecencies: Vec<f64>,
    /// The slots the last search matched.
    scored: Vec<Slot>,
    tiers: Tiers,
    highlights: Vec<u32>,
}

/// The Extensions and the Front ends by count: the rest is the Index, the
/// History and the sessions, too much for a log line.
impl<L, D, S> fmt::Debug for Daemon<L, D, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Daemon")
            .field("extensions", &self.extensions.len())
            .field("sessions", &self.sessions.len())
            .finish_non_exhaustive()
    }
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
            index: Index::new(0),
            indexer: None,
            stale: Vec::new(),
            hot_capacity: HOT_CAPACITY,
            hot_due: now,
            hot_versions: (0, 0, 0),
            ranks_version: 0,
            ahead: None,
            ranked_here: 0,
            learned: Arc::new([]),
            learned_versions: None,
            warm: false,
            warmed: 0,
            extensions: Vec::new(),
            matcher: Matcher::new(Config::DEFAULT),
            sessions: BTreeMap::new(),
            live_queries: HashMap::new(),
            invocations: HashMap::new(),
            argument_forms: HashMap::new(),
            picks: HashMap::new(),
            pick_lists: Views::default(),
            next_id: 1,
            config: Table::new(),
            installed: found,
            engine: Engine::default(),
            paths: Overrides::default(),
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
            awaiting_menu: Vec::new(),
            shown_last: None,
            launcher,
            desktop,
            store,
            history,
            clock: Clock::system(),
            started: now,
            keywords: Keywords::default(),
            last_seen: HashMap::new(),
            empty_query: EmptyQuery::default(),
            kinds: KindOrder::default(),
            kind_places: KindPlaces::default(),
            apps: Apps::default(),
            conflicts: Vec::new(),
            problems: Problems::default(),
            problems_open: HashSet::new(),
            failed_changed: false,
            own: Vec::new(),
            undos: Vec::new(),
            frecent: Arc::default(),
            frecent_versions: None,
            habits: Vec::new(),
            frecencies: Vec::new(),
            scored: Vec::new(),
            tiers: Tiers::default(),
            highlights: Vec::new(),
        };
        daemon.set_config(config, now);
        daemon
    }

    /// The merged config.
    pub fn config(&self) -> &Table {
        &self.config
    }

    /// The engine's settings, as of the last config.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// The config's `[paths]`, as of the last config.
    pub fn paths(&self) -> &Overrides {
        &self.paths
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Indexes Extensions' Items on a thread of its own from here on, which
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

    /// How many times a menu opening or showing has warmed the CPU cache
    /// with what its first keystroke reads.
    pub fn warmed(&self) -> u64 {
        self.warmed
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

    /// How many times the Empty query's most used Results were ranked on the
    /// thread that answers keystrokes, rather than ahead as the hot Items
    /// were planned.
    pub fn ranked_here(&self) -> u64 {
        self.ranked_here
    }

    /// How many Items the thread that answers keystrokes read back off their
    /// Shelves.
    pub fn shelf_reads(&self) -> u64 {
        self.index.shelf_reads()
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
                ended,
            } => {
                if self.extensions[extension.0].generation == generation {
                    self.extension_down(extension, ended, now);
                }
            }
            Event::FrontEndUp {
                front_end,
                sender,
                disconnect,
            } => self.front_end_up(front_end, sender, disconnect),
            Event::FromFrontEnd { front_end, message } => self.front_end_message(front_end, message, now),
            Event::FrontEndDown { front_end } => self.front_end_down(front_end),
            Event::Config(config) => self.set_config(config, now),
            Event::ConfigFailed(err) => self.config_failed(&err),
            Event::Extensions(Discovered { found, problems }) => {
                self.problems.dirty |= problem::replace(&mut self.problems.manifests, problems);
                self.installed = found;
                self.configure_extensions(now);
            }
            Event::ProgramsChanged => self.programs_changed(now),
            Event::Indexed(indexed) => self.indexed(indexed, now),
            Event::Kept(Kept { hot, ahead, reads }) => {
                self.index.keep(reads);
                match hot {
                    Some(hot) => self.index.hot_read(hot),
                    None => self.index.trim(),
                }
                if ahead.is_some() && self.plans() {
                    self.ahead = ahead;
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
        self.expire_undos(now);
        self.open_overdue(now);
        self.release_held();
        self.settle_awaiting_menu(now);
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
        let menu = self.awaiting_menu.iter().map(|&(_, _, until)| until);
        restarts
            .chain(live)
            .chain(output)
            .chain(undo)
            .chain(opening)
            .chain(menu)
            .min()
    }

    /// When the Daemon next has something to do: what
    /// [`next_deadline`](Self::next_deadline) says, or working out the hot
    /// Items again ahead of the time of day.
    pub fn next_wake(&self) -> Option<Instant> {
        let hot = (self.hot_capacity > 0 && !self.history.is_empty()).then_some(self.hot_due);
        self.next_deadline().into_iter().chain(hot).min()
    }
}
