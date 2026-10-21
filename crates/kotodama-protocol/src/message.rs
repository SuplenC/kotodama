//! The messages a Daemon and an Extension exchange once the handshake is done.
//!
//! Every message is an enum variant, tagged in JSON by its snake_case name:
//! `{"remove_items": {"ids": ["a"]}}`, or just `"request_action_catalog"` for a
//! variant without fields.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Action, Bytes, Fraction, Item, ItemRef, Job, KindName, Table, View};

/// From the Daemon to an Extension.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToExtension {
    /// The Extension's own `[extensions.<name>]` config table, sent right after
    /// the handshake and again whenever it changes. Extensions never read config
    /// files themselves.
    Config(Table),
    /// Starts a Live query. The Daemon sends one only to an Extension whose
    /// Manifest asks for Live queries, and only once the query has been still for
    /// the Manifest's debounce.
    LiveQuery {
        /// Names this query in the results and in its cancellation.
        id: u64,
        /// The query, without the prefix that routed it here.
        query: String,
        /// The Manifest prefix the query started with, if a prefix routed it.
        /// In the Extension's app, which routes every query to it, its first
        /// prefix, as though typed.
        prefix: Option<String>,
    },
    /// The query changed or the menu closed: results for `id` are no longer
    /// wanted.
    CancelLiveQuery { id: u64 },
    /// Runs one of this Extension's Actions on an Item. For a Contributed action
    /// the Item belongs to another Extension, and `item` is all this Extension
    /// sees of it.
    RunAction {
        /// Names this run in the [`FromExtension::ActionDone`] that answers it.
        invocation: u64,
        action: String,
        item: ItemRef,
        /// Argument values by Argument name.
        #[serde(default)]
        arguments: BTreeMap<String, String>,
    },
    /// The user submitted a form View: field values by field id.
    SubmitForm { view: String, values: Table },
    /// The user left one of this Extension's Views, as with Esc. Closing the
    /// menu doesn't send this: the View is still open, and shows again when
    /// the menu does, so an Extension may take this as "cancel".
    ViewPopped { view: String },
    /// The user typed into a list View whose `filters_itself` is set:
    /// `text` is the whole filter, empty once cleared. Answer with an
    /// `update_view` holding the Items that match.
    FilterView { view: String, text: String },
    /// The whole Action catalog, sent after
    /// [`FromExtension::RequestActionCatalog`] and again whenever it changes.
    /// Only Extensions whose Manifest asks for it receive it.
    ActionCatalog { entries: Vec<CatalogEntry> },
}

/// From an Extension to the Daemon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FromExtension {
    /// Indexes Items. An Item whose id is already indexed replaces the old one,
    /// which is how an Item is updated.
    AddItems {
        items: Vec<Item>,
    },
    RemoveItems {
        ids: Vec<String>,
    },
    /// Results for a Live query, streamed in as many batches as the Extension
    /// likes.
    LiveResults {
        /// The id of the [`ToExtension::LiveQuery`] these results answer.
        live_query: u64,
        results: Vec<LiveResult>,
        /// No more results will follow for this query.
        #[serde(default)]
        done: bool,
    },
    /// Answers a [`ToExtension::RunAction`].
    ActionDone {
        invocation: u64,
        outcome: Outcome,
    },
    PushView(View),
    /// Replaces the open View with the same id.
    UpdateView(View),
    PopView {
        view: String,
    },
    /// Pops the open View `view` and pushes `with` in one step, as a wizard
    /// goes from one step to the next: the search never shows in between.
    /// Pushes `with` alone if `view` isn't open.
    ReplaceView {
        view: String,
        with: View,
    },
    JobProgress(Job),
    /// Asks for the Action catalog; allowed only if the Manifest says so.
    RequestActionCatalog,
    /// Withdraws the pick that running an Action made, as when an app the
    /// Action launched closed right after: the History becomes as if the user
    /// had never picked the Result. Only the Extension that ran the Action may
    /// withdraw its pick, for a minute after [`ToExtension::RunAction`].
    /// Withdrawing a pick the Daemon doesn't know, or one it no longer keeps,
    /// does nothing.
    WithdrawPick {
        /// The `invocation` of the [`ToExtension::RunAction`] that ran it.
        invocation: u64,
    },
    /// Whether the Extension can do anything now, sent whenever that changes.
    /// The search shows its app's entry Item only while it can, so a user on
    /// another compositor, or before setting the Extension up, never enters
    /// an app with nothing in it. Until an Extension sends one, it counts as
    /// able once it has sent Items or if its Manifest asks for Live queries.
    Status {
        usable: bool,
        /// What the user should know as they enter the app, which its intro
        /// card shows: why it can't do anything, such as `Hyprland isn't
        /// running`, or what it isn't doing, such as a paused clipboard
        /// history.
        #[serde(default)]
        note: Option<String>,
    },
}

/// One Live query result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LiveResult {
    pub item: Item,
    /// How well the result matches the query, from 0 to 1. The Daemon maps it
    /// onto the scale it ranks Indexed items on.
    pub score: Fraction,
    /// Marks the one result that answers the query outright, such as a
    /// calculator's sum. The Daemon puts it in the top row, and its History
    /// keeps nothing of it: running it adds to neither Frecency nor Learned
    /// choices, only to the Usage log. Only an Extension whose Live query
    /// debounce is 0 may send one; a result the History should learn is not
    /// an answer.
    #[serde(default)]
    pub answer: bool,
}

/// How an Action finished.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Done; the menu closes.
    Close,
    /// Done; the menu stays open, say because the Action pushed a View.
    KeepOpen,
    /// The menu closes and the Daemon types `text` into the window that had focus.
    Type { text: String },
    /// The menu closes and the Daemon pastes into the window that had focus.
    Paste(Clipboard),
    /// The menu closes and the Daemon puts this on the clipboard, without
    /// pasting it.
    Copy(Clipboard),
    /// The Action failed; the menu stays open and shows `message`.
    Failed { message: String },
    /// The user confirmed a proposal: the Daemon runs this Action from the
    /// Action catalog, as if the user had picked it. Only an
    /// Extension whose Manifest reads the catalog may answer with it, and only
    /// as the outcome of an Action the user ran, so it never runs on its own.
    RunCatalogAction {
        /// The Extension that offers the Action, as the catalog names it.
        extension: String,
        kind: KindName,
        action: String,
        /// Argument values by Argument name. One that is required but missing
        /// is asked for in a form, as for any Action.
        #[serde(default)]
        arguments: BTreeMap<String, String>,
    },
}

/// What to paste.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Clipboard {
    Text(String),
    Image {
        /// The image's MIME type, such as `image/png`.
        mime: String,
        data: Bytes,
    },
}

/// One Action on offer, as the Action catalog lists it. It never includes Items.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEntry {
    /// The Extension that runs the Action.
    pub extension: String,
    /// The kind of Item it runs on.
    pub kind: KindName,
    pub action: Action,
}
