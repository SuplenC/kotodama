//! The messages a Daemon and a Front end exchange once the handshake is done.
//! They are tagged like the Extension messages: `{"query": {"text": "fir"}}`, or
//! just `"closed"` for a variant without fields.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::item::ItemOwned;
use crate::{Action, Item, Job, Table, View};

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
        /// View it pushes is there, or after a second without it (ADR 0014).
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
    /// The Extensions that are down. Their Results are gone until they come back.
    Status {
        down: Vec<String>,
    },
    /// Close the menu, then answer [`FromFrontEnd::Closed`].
    Close,
    /// An Action failed; show `message` and stay open.
    ActionFailed {
        message: String,
    },
    /// `contributed` holds, for a list View, the Contributed actions on its
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
    PopView {
        extension: String,
        view: String,
    },
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
    JobProgress {
        extension: String,
        job: Job,
    },
    /// Open the menu, as another Front end asked with [`FromFrontEnd::Open`].
    /// When `view` is given, the View it names is already on top, unless it
    /// failed to open, in which case an [`ToFrontEnd::ActionFailed`] follows.
    Open {
        query: String,
        view: Option<String>,
    },
    /// Open the menu if it is closed, or close it if it is open.
    Toggle,
    /// The History just changed, as by Forget or "Clear history", and the
    /// change can be undone: show `title`, such as `Forgot “Firefox”`, with
    /// the key that sends [`FromFrontEnd::Undo`]. It takes the place of the
    /// offer before it. Shown without taking the keys or the selection, until
    /// [`ToFrontEnd::WithdrawUndo`] names the same `change`.
    OfferUndo {
        change: u64,
        title: String,
    },
    /// The change can't be undone any more: it was undone, or its time ran
    /// out. Hide its offer, if it is still shown.
    WithdrawUndo {
        change: u64,
    },
    /// Replace the search text with `text`, as when the user enters an app,
    /// which types its word and a space. The Daemon has taken `text` as the
    /// query already, and its Results follow, so the Front end sends no
    /// [`FromFrontEnd::Query`] for it.
    SetQuery {
        text: String,
    },
    /// The answer to [`FromFrontEnd::Open`] and [`FromFrontEnd::Toggle`],
    /// sent to the Front end that asked: how many menus the request was
    /// passed to. With none running, the Daemon waits a moment for one to
    /// connect first; `0` means none did, so nothing opened.
    Reached {
        menus: u32,
    },
}

/// One Result as the Front end shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ShownResultOwned")]
pub struct ShownResult {
    /// The Extension that owns the Item.
    pub extension: String,
    pub item: Item,
    /// Other Extensions' Actions for this Item's kind, grouped by Extension,
    /// then the Daemon's own, under the Extension name `kotodama`, such as
    /// `forget` on a Result the History keeps anything for. The Action panel
    /// lists them after the Item's own Actions, so none is ever the primary
    /// Action. A shortcut that clashes with the owner's is dropped.
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
    type Error = String;
    fn try_from(wire: ShownResultOwned) -> Result<Self, String> {
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
    pub examples: Vec<Example>,
}

/// A query to type in an app, and what it gives, such as `15min in sec` and
/// `900 s`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Example {
    pub query: String,
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

/// Where the Daemon serves Front ends: `$XDG_RUNTIME_DIR/kotodama.sock`, or
/// `None` when `XDG_RUNTIME_DIR` isn't set.
pub fn socket_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(|dir| PathBuf::from(dir).join("kotodama.sock"))
}
