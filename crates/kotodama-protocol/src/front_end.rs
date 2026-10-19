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
    /// The user popped a View.
    ViewPopped { extension: String, view: String },
    /// Answers [`ToFrontEnd::Close`]: the window is gone and focus is back on the
    /// window the user came from, so the Daemon may type or paste into it.
    Closed,
    /// Asks every other Front end to open, as `kotodama open` does from a
    /// compositor keybind. The Daemon passes it on as [`ToFrontEnd::Open`].
    Open {
        /// The search text to open with; empty clears it.
        query: String,
        /// A View to open on, by id.
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
    PushView {
        extension: String,
        view: View,
    },
    /// Replaces the open View with the same id.
    UpdateView {
        extension: String,
        view: View,
    },
    PopView {
        extension: String,
        view: String,
    },
    JobProgress {
        extension: String,
        job: Job,
    },
    /// Open the menu, as another Front end asked with [`FromFrontEnd::Open`].
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
    /// Other Extensions' Actions for this Item's kind, grouped by Extension. The
    /// Action panel lists them after the Item's own Actions, so none is ever the
    /// primary Action. A shortcut that clashes with the owner's is dropped.
    pub contributed: Vec<OfferedAction>,
    /// The positions, in characters, of the title's characters that matched the
    /// query.
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
