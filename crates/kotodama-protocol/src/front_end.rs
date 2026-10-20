//! The messages a Daemon and a Front end exchange once the handshake is done.
//! They are tagged like the Extension messages: `{"query": {"text": "fir"}}`, or
//! just `"closed"` for a variant without fields.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{Action, Item, Job, Table, View};

/// From a Front end to the Daemon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FromFrontEnd {
    /// The search text changed. The Daemon answers with [`ToFrontEnd::Results`],
    /// and again whenever late Results arrive.
    Query { text: String },
    /// Which Results are on screen: `rows` of them, from the one at `top`. Results
    /// that arrive after the list is shown go below these rows, so neither the
    /// rows on screen nor any scrolled past ever move (ADR 0003). The Daemon
    /// assumes the top 10 until told.
    Viewport { top: u32, rows: u32 },
    /// Runs one Action of a Result's Action list.
    RunAction {
        result: ResultRef,
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
    /// Asks every other Front end to open, as `kotodama open` does from a
    /// compositor keybind. The Daemon passes it on as [`ToFrontEnd::Open`].
    Open {
        /// The search text to open with; empty clears it.
        query: String,
        /// A View to open on, as `<extension>/<item id>`: the Daemon runs that
        /// Item's primary Action, and passes the request on only once the
        /// View it pushes is there, or after a second without it (ADR 0014).
        view: Option<String>,
    },
    /// Asks every other Front end to open if it is closed and close if it is
    /// open, as `kotodama toggle` does.
    Toggle,
}

/// From the Daemon to a Front end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToFrontEnd {
    /// The whole ranked list for the current query, best first. It replaces the
    /// previous list.
    Results {
        results: Vec<ShownResult>,
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
}

/// One Result as the Front end shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
