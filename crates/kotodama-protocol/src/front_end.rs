//! The messages a Daemon and a Front end exchange once the handshake is done.
//! They are tagged like the Extension messages: `{"query": {"text": "fir"}}`, or
//! just `"closed"` for a variant without fields.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Action, Item, Job, Table, View};

/// From a Front end to the Daemon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FromFrontEnd {
    /// The search text changed. The Daemon answers with [`ToFrontEnd::Results`],
    /// and again whenever late Results arrive.
    Query { text: String },
    /// How many Results fit on screen. Results that arrive after the list is
    /// shown go below these rows (ADR 0003). The Daemon assumes 10 until told.
    Viewport { rows: u32 },
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
}

/// From the Daemon to a Front end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToFrontEnd {
    /// The whole ranked list for the current query, best first. It replaces the
    /// previous list.
    Results {
        results: Vec<ResultRow>,
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
}

/// One Result as the Front end shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResultRow {
    /// The Extension that owns the Item.
    pub extension: String,
    pub item: Item,
    /// Other Extensions' Actions for this Item's kind, grouped by Extension. The
    /// Action panel lists them after the Item's own Actions, so none is ever the
    /// primary Action. A shortcut that clashes with the owner's is dropped.
    pub contributed: Vec<ExtensionAction>,
    /// The positions, in characters, of the title's characters that matched the
    /// query.
    pub highlights: Vec<u32>,
}

/// An Action together with the Extension that runs it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionAction {
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
