//! The messages a Daemon and a Front end exchange once the handshake is done.
//! They are tagged like the Extension messages: `{"query": {"text": "fir"}}`, or
//! just `"closed"` for a variant without fields.
//!
//! # Owns
//!
//! - [`FromFrontEnd`], [`ToFrontEnd`], and what they carry: [`ShownResult`], [`PathCompletion`],
//!   [`Scope`] and its [`Intro`], [`Problem`], [`ListMatch`], the references to a Result and an
//!   Action.
//! - Where the Daemon's socket is: [`socket_path`].
//!
//! # Not here
//!
//! - Items and Actions: `item.rs`. Views and jobs: `view.rs`. The Extension messages: `message.rs`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::Action;
use crate::Item;
use crate::Job;
use crate::Table;
use crate::View;
use crate::item::ItemError;
use crate::item::ItemOwned;

/// From a Front end to the Daemon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FromFrontEnd {
    /// The search text changed. The Daemon answers with [`ToFrontEnd::Results`],
    /// and again whenever late Results arrive.
    Query { text: String },
    /// Which Results are on screen: `rows` of them, from the one at `top`. Once
    /// the selection has moved, or 150 ms after the query changed, Late Results go
    /// below these rows, so neither the rows on screen nor any
    /// scrolled past move. The Daemon assumes the top 10 until told.
    Viewport { top: u32, rows: u32 },
    /// The user moved the selection off the top row of the Results. Until the
    /// query changes, Late Results go below the rows on screen.
    SelectionMoved,
    /// Runs one Action of a Result's Action list.
    RunAction {
        result: ResultRef,
        /// The [`ToFrontEnd::Results`] `version` on screen when the user chose
        /// `result`. The Daemon refuses a Late Result that a later version
        /// added or moved up a row: the user hasn't seen it there.
        shown: u32,
        action: ActionRef,
        /// Argument values by Argument name.
        #[serde(default)]
        arguments: BTreeMap<String, String>,
    },
    /// The user submitted a form View: field values by field id.
    SubmitForm {
        extension: String,
        view: String,
        values: Table,
    },
    /// The user left a View, as with Esc. Never sent for closing the menu,
    /// which keeps its Views open, since an Extension may take this as
    /// "cancel".
    ViewPopped { extension: String, view: String },
    /// The user typed `text` into an open list View to filter it; empty
    /// clears the filter. The Daemon answers with
    /// [`ToFrontEnd::FilteredView`], or, for a View whose Extension filters it
    /// itself, passes the text on and the Extension updates the View.
    FilterView {
        extension: String,
        view: String,
        text: String,
    },
    /// Answers [`ToFrontEnd::Close`]: the window is gone and focus is back on the
    /// window the user came from, so the Daemon may type or paste into it.
    Closed,
    /// Asks a menu to open, as `kotodama open` does from a compositor
    /// keybind. The Daemon passes it on as [`ToFrontEnd::Open`] to one menu,
    /// another Front end that has searched: the one shown last, else the one
    /// that connected last. It answers with [`ToFrontEnd::Reached`].
    Open {
        /// The search text to open with; empty clears it.
        query: String,
        /// A View to open on, as `<extension>/<item id>`: the Daemon runs that
        /// Item's primary Action, and passes the request on only once the
        /// View it pushes is there, or after a second without it.
        view: Option<String>,
    },
    /// Asks a menu to open if it is closed and close if it is open, as
    /// `kotodama toggle` does. It goes to one menu, as [`FromFrontEnd::Open`]
    /// does, and the Daemon answers with [`ToFrontEnd::Reached`].
    Toggle,
    /// Undoes the change to the History that [`ToFrontEnd::OfferUndo`]
    /// offered, as its key does. Nothing happens once it can't be undone.
    Undo { change: u64 },
    /// The menu became visible, however it opened: on [`ToFrontEnd::Open`]
    /// or [`ToFrontEnd::Toggle`], on the Front end's own keybind, or again
    /// after a View closed it. A Front end should send it every time. The
    /// Empty query shows what is used at this time of day, so the Daemon
    /// answers with new [`ToFrontEnd::Results`] when the query is empty and
    /// the time of day has moved to another part since it last ranked them.
    /// It also warms the CPU cache with what the first keystroke reads.
    Shown,
    /// Asks for the paths that complete `text` in the file field `field` of
    /// the open form View `view`, as the user types into it. The Daemon
    /// answers with [`ToFrontEnd::PathCompletions`], unless a newer request
    /// for the same field came first; a field that isn't a file field gets
    /// no answer.
    CompletePath {
        extension: String,
        view: String,
        field: String,
        text: String,
    },
}

/// From the Daemon to a Front end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToFrontEnd {
    /// The whole ranked list for the current query, best first. It replaces the
    /// previous list.
    Results {
        /// Counts up with every list sent to this Front end. It names the list
        /// on screen in [`FromFrontEnd::RunAction`]'s `shown`.
        version: u32,
        results: Vec<ShownResult>,
        /// The app the query is in, when it starts with an Extension's word:
        /// `emoji fire` searches the emoji alone. A Front end shows it apart
        /// from the rest of the query, as a chip.
        scope: Option<Scope>,
    },
    /// The Extensions that are down, whose Results are gone until they come
    /// back, and what the user should fix. Sent when a Front end connects and
    /// whenever either changes, each time whole.
    Status {
        /// The names of the Extensions that are down.
        down: Vec<String>,
        /// Everything wrong the user can fix: a config file that doesn't
        /// parse, a setting that isn't one or can't be read, a Manifest
        /// refused, a keyword two things ask for, an Extension that can't
        /// start or keeps stopping. Empty when nothing is.
        #[serde(default)]
        problems: Vec<Problem>,
    },
    /// Close the menu, then answer [`FromFrontEnd::Closed`].
    Close,
    /// An Action failed: show `message` and stay open.
    ActionFailed { message: String },
    /// Show `view`, an Extension's View, on top. `contributed` holds, for a
    /// list View, the Contributed actions on its
    /// Items by Item kind name, grouped as [`ShownResult::contributed`] has
    /// them; an Item's own Action keeps a shortcut one of them also binds.
    /// Empty for other Views.
    PushView {
        extension: String,
        view: View,
        contributed: BTreeMap<String, Vec<OfferedAction>>,
    },
    /// Replaces the open View with the same id. A list View being filtered
    /// is followed by its [`ToFrontEnd::FilteredView`] for the new Items.
    UpdateView {
        extension: String,
        view: View,
        contributed: BTreeMap<String, Vec<OfferedAction>>,
    },
    /// Close the open View `view`.
    PopView { extension: String, view: String },
    /// Pops the open View `view` and pushes `with`, in one step, so the
    /// search never shows in between.
    ReplaceView {
        extension: String,
        view: String,
        with: View,
        contributed: BTreeMap<String, Vec<OfferedAction>>,
    },
    /// The Items of a list View that match the filter `text`, best first.
    /// Never sent for an empty filter, which shows every Item in order.
    FilteredView {
        extension: String,
        view: String,
        text: String,
        matches: Vec<ListMatch>,
    },
    /// A job's whole state, for the progress View that follows it.
    JobProgress { extension: String, job: Job },
    /// Open the menu, as another Front end asked with [`FromFrontEnd::Open`].
    /// When `view` is given, the View it names is already on top, unless it
    /// failed to open, in which case an [`ToFrontEnd::ActionFailed`] follows.
    Open { query: String, view: Option<String> },
    /// Open the menu if it is closed, or close it if it is open.
    Toggle,
    /// The History just changed, as by Forget or "Clear history", and the
    /// change can be undone: show `title`, such as `Forgot “Firefox”`, with
    /// the key that sends [`FromFrontEnd::Undo`]. It takes the place of the
    /// offer before it. Shown without taking the keys or the selection, until
    /// [`ToFrontEnd::WithdrawUndo`] names the same `change`.
    OfferUndo { change: u64, title: String },
    /// The change can't be undone any more: it was undone, or its time ran
    /// out. Hide its offer, if it is still shown.
    WithdrawUndo { change: u64 },
    /// Replace the search text with `text`, as when the user enters an app,
    /// which types its word and a space. The Daemon has taken `text` as the
    /// query already, and its Results follow, so the Front end sends no
    /// [`FromFrontEnd::Query`] for it.
    SetQuery { text: String },
    /// The answer to [`FromFrontEnd::Open`] and [`FromFrontEnd::Toggle`],
    /// sent to the Front end that asked: how many menus the request was
    /// passed to. With none running, the Daemon waits a moment for one to
    /// connect first; `0` means none did, so nothing opened.
    Reached { menus: u32 },
    /// The answer to [`FromFrontEnd::CompletePath`], naming the field and
    /// the `text` it completes, so a Front end drops an answer to text no
    /// longer typed. For a partial path, the entries of its directory that
    /// match what follows the last `/`, best first and folders before files
    /// within a match tier; for no text, the newest files the field takes
    /// from the places downloads and documents land.
    PathCompletions {
        extension: String,
        view: String,
        field: String,
        text: String,
        entries: Vec<PathCompletion>,
    },
}

/// One path offered to complete a file field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathCompletion {
    /// The absolute path. A folder's has no `/` at the end: add one to
    /// complete inside it.
    pub path: PathBuf,
    /// Its name, as a row shows it.
    pub title: String,
    pub folder: bool,
    /// When it was last modified, in seconds since the Unix epoch; `None`
    /// when the file system can't tell.
    pub modified: Option<u64>,
    /// The positions, in characters, of the title's characters that matched
    /// what was typed.
    pub highlights: Vec<u32>,
}

/// One Result as the Front end shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ShownResultOwned")]
pub struct ShownResult {
    /// The Extension that owns the Item.
    pub extension: String,
    /// The Item the Result shows.
    pub item: Item,
    /// Other Extensions' Actions for this Item's kind or targeted at its
    /// Identity, grouped by Extension, then the Daemon's own, under the
    /// Extension name `kotodama`, such as `forget` on a Result the History
    /// keeps anything for. The Action panel lists them after the Item's own
    /// Actions, so none is ever the primary Action, and lists every
    /// [`Destructive`](crate::ActionRole::Destructive) Action but the primary
    /// after them all. A shortcut that clashes with the owner's is dropped.
    pub contributed: Vec<OfferedAction>,
    /// The positions, in characters, of the title's characters that matched the
    /// query.
    pub highlights: Vec<u32>,
    /// The Alias the query named this Result by, which the row shows as a
    /// badge.
    pub alias: Option<String>,
    /// Why the Result can't run, which the row shows greyed out in place of
    /// its subtitle: the Item an Alias names is gone, or its Extension is
    /// down. Such a Result carries no Actions.
    pub unavailable: Option<String>,
}

/// A [`ShownResult`] as it arrives, before its Item is checked.
#[derive(Deserialize)]
struct ShownResultOwned {
    extension: String,
    item: ItemOwned,
    contributed: Vec<OfferedAction>,
    highlights: Vec<u32>,
    alias: Option<String>,
    unavailable: Option<String>,
}

impl TryFrom<ShownResultOwned> for ShownResult {
    type Error = ItemError;
    fn try_from(wire: ShownResultOwned) -> Result<Self, ItemError> {
        let item = if wire.unavailable.is_some() {
            wire.item.into_item_without_actions()?
        } else {
            Item::try_from(wire.item)?
        };
        Ok(ShownResult {
            extension: wire.extension,
            item,
            contributed: wire.contributed,
            highlights: wire.highlights,
            alias: wire.alias,
            unavailable: wire.unavailable,
        })
    }
}

/// One thing the user should fix, as [`ToFrontEnd::Status`] lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    /// Where it is: a config key and the file that sets it, a Manifest's
    /// path, a keyword, or an Extension.
    pub about: String,
    /// What is wrong, and what to do about it where that isn't plain.
    pub message: String,
}

impl Problem {
    /// The Daemon's own Result that opens the list of problems, which a
    /// Front end runs with [`Problem::show`] when the user asks for the
    /// list, as by clicking its count.
    #[must_use]
    pub fn list() -> ResultRef {
        ResultRef {
            extension: "kotodama".into(),
            id: "problems".into(),
        }
    }

    /// The Action of [`Problem::list`] that opens it.
    #[must_use]
    pub fn show() -> ActionRef {
        ActionRef {
            extension: "kotodama".into(),
            id: "show".into(),
        }
    }
}

/// The kind of the Daemon's entry Item for an Extension's app: running it
/// enters the app, and a Front end runs it on Tab too.
pub const APP_ENTRY_KIND: &str = "kotodama.app";

/// The app a query is in: the word it starts with, and the Extension whose
/// app it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    /// The app's word, in lowercase, as the query starts with it before a
    /// space.
    pub word: String,
    /// The Extension's name as the user reads it, such as `Emoji`.
    pub title: String,
    /// The app's icon: a freedesktop icon name or a path.
    pub icon: Option<String>,
    /// What the app does, shown above its rows while nothing is typed past
    /// its word. `None` once something is.
    #[serde(default)]
    pub intro: Option<Intro>,
}

/// The card an app shows before anything is typed in it, from its
/// Extension's Manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intro {
    /// What the app does and how to use it, in a sentence or two.
    pub about: String,
    /// Queries to type in the app, under `about`.
    pub examples: Vec<Example>,
    /// What the Extension says the user should know at the moment, from its
    /// [`FromExtension::Status`](crate::FromExtension::Status), such as why it
    /// can't do anything.
    #[serde(default)]
    pub note: Option<String>,
}

/// A query to type in an app, and what it gives, such as `15min in sec` and
/// `900 s`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Example {
    /// What to type.
    pub query: String,
    /// What it gives.
    pub result: String,
}

/// One Item of a filtered list View.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListMatch {
    /// The Item's position in the View's `items`.
    pub item: u32,
    /// The positions, in characters, of the title's characters that matched.
    /// Empty past the first few hundred matches, which are seldom seen and
    /// would cost as much again to highlight.
    pub highlights: Vec<u32>,
}

/// A Contributed action as a Result offers it: the Action and the Extension
/// that runs it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfferedAction {
    pub extension: String,
    pub action: Action,
}

/// Names a Result: the Extension that owns its Item and the Item's id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResultRef {
    pub extension: String,
    pub id: String,
}

/// Names an Action: the Extension that runs it and the Action's id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActionRef {
    pub extension: String,
    pub id: String,
}

/// Where the Daemon serves Front ends: `$KOTODAMA_SOCKET`, else
/// `$XDG_RUNTIME_DIR/kotodama.sock`, or `None` when neither is set. A
/// Daemon whose config moves its socket needs its Front ends pointed there
/// with `KOTODAMA_SOCKET`.
#[must_use]
pub fn socket_path() -> Option<PathBuf> {
    let set = |var| std::env::var_os(var).filter(|value: &std::ffi::OsString| !value.is_empty());
    set("KOTODAMA_SOCKET")
        .map(PathBuf::from)
        .or_else(|| set("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join("kotodama.sock")))
}
