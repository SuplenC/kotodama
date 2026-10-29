//! The Quickshell Front end: the `Kotodama` QML module.
//!
//! [`KotodamaSearch`](bridge::qobject::KotodamaSearch) is a `QAbstractListModel`
//! over the Daemon's Results. It talks to the Daemon in the fastest Wire format
//! they share and hands QML each Result's fields through model roles, so
//! Results never pass through JavaScript. It also keeps the stack of Views
//! Extensions push, the jobs they report, and each Result's Action panel. While
//! a list View is on top, the model holds that View's Items instead of the
//! Results: those that match its filter, as the Daemon finds them.
//! `KotodamaMenu.qml` is the window that draws them; a host shell can use it as
//! it is, set its look, or draw its own.
//!
//! # Owns
//!
//! - `SearchRust`, the state behind `KotodamaSearch`.
//! - What QML's calls do: the query and the app it is in, the intro card, running Actions and
//!   holding one that asks first, filters and forms, the selection, the frames on screen, and the
//!   Toast.
//!
//! # Not here
//!
//! - The names QML sees: `bridge`.
//! - The list model's roles and Qt's row signals: `model`.
//! - What the Daemon's messages do: `updates`; the connection: `connection`.
//! - The rows: `rows`; the Views, forms and jobs: `views`, as QML maps: `variant`.
//! - The plugin's entry points: `plugin`.

mod bridge;
mod connection;
mod model;
mod plugin;
mod rows;
mod scope;
mod shortcut;
mod updates;
mod variant;
mod views;

use std::collections::BTreeMap;
use std::pin::Pin;
use std::time::Instant;

use bridge::qobject::KotodamaSearch;
use bridge::qobject::QStringList;
use connection::Writer;
use cxx_qt::CxxQtType;
use cxx_qt::Initialize;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::Intro;
use kotodama_protocol::Problem;
use kotodama_protocol::ResultRef;
use kotodama_protocol::Scope;
use kotodama_protocol::UNDO_SHORTCUT;
use kotodama_protocol::Value;
use rows::Row;
use rows::Rows;
use views::Views;

/// The color of the matched characters until the window sets one: the default
/// `accent` of `KotodamaMenu.qml`, which sets it.
const DEFAULT_HIGHLIGHT_COLOR: &str = "#89b4fa";

/// The state behind `KotodamaSearch`.
#[derive(Debug)]
pub struct SearchRust {
    query: QString,
    highlight_color: QString,
    connected: bool,
    down: QStringList,
    problems: i32,
    /// Microseconds from the query that Results were last asked for to the
    /// model showing them.
    latency: i32,
    /// What the list shows: the Results, or the Items of the list View on top.
    rows: Rows,
    /// The Results, kept here while a list View's Items are shown instead.
    results: Option<Vec<Row>>,
    /// The list View whose Items are shown, by Extension and id.
    listed: Option<(String, String)>,
    /// The row the list has selected, by what it shows.
    selected: Option<ResultRef>,
    /// Whether the user moved the selection off the top row of the Results
    /// since the query last changed, and the Daemon knows.
    selection_moved: bool,
    /// The version of the Results in the model, or last in it while a list
    /// View's Items are shown instead.
    version: u32,
    /// The version of the Results kept in `results`.
    kept_version: u32,
    /// The version of the Results in the frame being drawn.
    drawing: u32,
    /// The version of the Results in the last frame on screen, which a run
    /// confirms to the Daemon.
    on_screen: u32,
    views: Views,
    view: variant::Map,
    depth: i32,
    jobs: variant::List,
    completions: variant::List,
    completing_field: i32,
    field_errors: variant::List,
    toast: QString,
    /// The change to the History the Toast offers to undo.
    offered: Option<u64>,
    scope: QString,
    intro_about: QString,
    intro_note: QString,
    intro_examples: variant::List,
    confirming: QString,
    /// The run of an Action that asks first, sent once the user confirms.
    unconfirmed: Option<FromFrontEnd>,
    /// The intro card shown, which the two above hold.
    intro: Option<Intro>,
    /// The word of the app the search is in, which `query` follows.
    word: Option<String>,
    /// The query last sent, the app's word and all.
    sent: Option<String>,
    writer: Option<Writer>,
    /// When the oldest query still waiting for its Results was sent. Results
    /// for a later query are measured from it too, so typing faster than the
    /// Daemon answers shows up as latency rather than hiding.
    asked: Option<Instant>,
}

impl Default for SearchRust {
    fn default() -> Self {
        SearchRust {
            query: QString::default(),
            highlight_color: QString::from(DEFAULT_HIGHLIGHT_COLOR),
            connected: false,
            down: QStringList::default(),
            problems: 0,
            latency: 0,
            rows: Rows::default(),
            results: None,
            listed: None,
            selected: None,
            selection_moved: false,
            version: 0,
            kept_version: 0,
            drawing: 0,
            on_screen: 0,
            views: Views::default(),
            view: variant::Map::default(),
            depth: 0,
            jobs: variant::List::default(),
            completions: variant::List::default(),
            completing_field: -1,
            field_errors: variant::List::default(),
            toast: QString::default(),
            offered: None,
            scope: QString::default(),
            intro_about: QString::default(),
            intro_note: QString::default(),
            intro_examples: variant::List::default(),
            confirming: QString::default(),
            unconfirmed: None,
            intro: None,
            word: None,
            sent: None,
            writer: None,
            asked: None,
        }
    }
}

impl Initialize for KotodamaSearch {
    fn initialize(self: Pin<&mut Self>) {
        let qt_thread = self.qt_thread();
        connection::spawn(move |updates| qt_thread.queue(move |search| search.apply(updates)).is_ok());
    }
}

impl KotodamaSearch {
    /// Sends the query unless the Daemon has it already: one the scope was
    /// left for, such as the menu opening on what it showed before, may
    /// have the same text as the box.
    fn set_query(mut self: Pin<&mut Self>, query: QString) {
        if self.query != query {
            self.as_mut().cancel_confirm();
            self.as_mut().rust_mut().query = query;
            self.as_mut().query_changed();
        }
        let text = scope::full(self.word.as_deref(), &String::from(&self.query));
        if self.sent.as_deref() != Some(text.as_str()) {
            self.as_mut().send_query(text);
        }
    }

    /// Shows the search in the app of `scope` when the box starts with its
    /// word, as the Daemon read the query: the box keeps the rest, which is
    /// the query already. Nothing is sent: the Daemon has the query.
    fn enter_scope(mut self: Pin<&mut Self>, scope: Option<Scope>) {
        let Some(scope) = scope else {
            return;
        };
        if self.word.is_some() {
            return;
        }
        let query = String::from(&self.query);
        let Some(rest) = scope::behind(&query, &scope.word) else {
            return;
        };
        let rest = QString::from(rest);
        let mut this = self.as_mut().rust_mut();
        this.word = Some(scope.word);
        this.scope = QString::from(&scope.title);
        this.query = rest.clone();
        self.as_mut().scope_changed();
        self.as_mut().query_changed();
        self.as_mut().query_replaced(rest);
    }

    /// Forgets the scope, without sending anything: what is typed next
    /// searches everything, and is sent even if the Daemon has it, so its
    /// Results come back to say whether it is in an app. Returns the word
    /// of the app it was in.
    fn drop_scope(mut self: Pin<&mut Self>) -> Option<String> {
        let word = self.as_mut().rust_mut().word.take()?;
        let mut this = self.as_mut().rust_mut();
        this.scope = QString::default();
        this.sent = None;
        self.as_mut().scope_changed();
        self.as_mut().set_intro(None);
        Some(word)
    }

    /// Shows `intro` as the intro card, or none.
    fn set_intro(mut self: Pin<&mut Self>, intro: Option<Intro>) {
        if self.intro == intro {
            return;
        }
        let about = intro.as_ref().map(|intro| QString::from(&intro.about));
        let note = intro
            .as_ref()
            .and_then(|intro| intro.note.as_deref())
            .map(QString::from);
        let examples = intro.as_ref().map(|intro| variant::examples(&intro.examples));
        let mut this = self.as_mut().rust_mut();
        this.intro = intro;
        this.intro_about = about.unwrap_or_default();
        this.intro_note = note.unwrap_or_default();
        this.intro_examples = examples.unwrap_or_default();
        self.as_mut().intro_about_changed();
        self.as_mut().intro_note_changed();
        self.as_mut().intro_examples_changed();
    }

    fn leave_app(mut self: Pin<&mut Self>) -> bool {
        if self.as_mut().drop_scope().is_none() {
            return false;
        }
        let text = QString::default();
        self.as_mut().set_query(text.clone());
        self.as_mut().query_replaced(text);
        true
    }

    fn enter(self: Pin<&mut Self>, row: i32) -> bool {
        if !self.row(row).is_some_and(|row| row.enters) {
            return false;
        }
        self.run(row)
    }

    fn leave_scope(mut self: Pin<&mut Self>) -> bool {
        let Some(word) = self.as_mut().drop_scope() else {
            return false;
        };
        let text = QString::from(&word);
        self.as_mut().set_query(text.clone());
        self.as_mut().query_replaced(text);
        true
    }

    fn set_highlight_color(mut self: Pin<&mut Self>, color: QString) {
        if self.highlight_color == color {
            return;
        }
        self.as_mut().rust_mut().highlight_color = color;
        self.as_mut().highlight_color_changed();
        let rows = self.rows.len();
        self.as_mut().rows_changed(0..rows, &[model::MARKUP]);
    }

    fn send_query(mut self: Pin<&mut Self>, text: String) {
        let now = Instant::now();
        let mut this = self.as_mut().rust_mut();
        this.selection_moved = false;
        let sent = this.send(&FromFrontEnd::Query { text: text.clone() });
        if sent {
            this.asked.get_or_insert(now);
        }
        this.sent = sent.then_some(text);
    }

    fn row(&self, row: i32) -> Option<&Row> {
        usize::try_from(row).ok().and_then(|row| self.rows.get(row))
    }

    fn run(self: Pin<&mut Self>, row: i32) -> bool {
        self.run_action(row, 0)
    }

    fn actions(&self, row: i32) -> variant::List {
        self.row(row).map(variant::actions).unwrap_or_default()
    }

    fn hints(&self, row: i32) -> QString {
        self.row(row).map(|row| QString::from(&row.hints())).unwrap_or_default()
    }

    fn run_listed(self: Pin<&mut Self>, row: i32, group: &QString, id: &QString) -> bool {
        let group = String::from(group);
        let id = String::from(id);
        let at = self
            .row(row)
            .and_then(|found| found.place_of((!group.is_empty()).then_some(group.as_str()), &id));
        match at.and_then(|at| i32::try_from(at).ok()) {
            Some(at) => self.run_action(row, at),
            None => false,
        }
    }

    fn run_action(mut self: Pin<&mut Self>, row: i32, n: i32) -> bool {
        let Some(row) = self.row(row) else {
            return false;
        };
        let Ok(n) = usize::try_from(n) else {
            return false;
        };
        let Some(action) = row.action(n) else {
            return false;
        };
        let asks = row.confirmation(n);
        let message = FromFrontEnd::RunAction {
            result: row.result.clone(),
            shown: self.on_screen,
            action,
            arguments: BTreeMap::default(),
        };
        if let Some(asks) = asks {
            let mut this = self.as_mut().rust_mut();
            this.unconfirmed = Some(message);
            this.confirming = QString::from(&asks);
            self.as_mut().confirming_changed();
            true
        } else {
            self.as_mut().cancel_confirm();
            self.as_mut().rust_mut().send(&message)
        }
    }

    fn confirm(mut self: Pin<&mut Self>) -> bool {
        let Some(message) = self.as_mut().rust_mut().unconfirmed.take() else {
            return false;
        };
        self.as_mut().cancel_confirm();
        self.as_mut().rust_mut().send(&message)
    }

    fn cancel_confirm(mut self: Pin<&mut Self>) -> bool {
        if self.confirming.is_empty() {
            return false;
        }
        let mut this = self.as_mut().rust_mut();
        this.unconfirmed = None;
        this.confirming = QString::default();
        self.as_mut().confirming_changed();
        true
    }

    fn run_shortcut(self: Pin<&mut Self>, row: i32, key: i32, modifiers: i32) -> bool {
        let Some(n) = self.row(row).and_then(|found| {
            found.actions().position(|entry| {
                entry
                    .shortcut
                    .is_some_and(|bound| shortcut::matches(bound, key, modifiers))
            })
        }) else {
            return false;
        };
        self.run_action(row, n as i32)
    }

    fn filter(mut self: Pin<&mut Self>, text: &QString) -> bool {
        let text = String::from(text);
        let Some(message) = self.as_mut().rust_mut().views.set_filter(&text) else {
            return false;
        };
        self.as_mut().rust_mut().send(&message);
        if text.is_empty() {
            // No answer comes: every Item shows again, in the form last
            // sent, which may have waited for the Daemon's matches.
            self.as_mut().show_view();
        }
        true
    }

    fn pop_view(mut self: Pin<&mut Self>) -> bool {
        let Some(popped) = self.as_mut().rust_mut().views.pop_top() else {
            return false;
        };
        self.as_mut().rust_mut().send(&popped);
        self.as_mut().show_view();
        true
    }

    fn set_text(mut self: Pin<&mut Self>, field: i32, text: &QString) -> bool {
        // The field shows what was typed already, so the view isn't told.
        let Ok(field) = usize::try_from(field) else {
            return false;
        };
        let value = Value::String(String::from(text));
        self.as_mut().rust_mut().views.set_value(field, value)
    }

    fn set_checked(mut self: Pin<&mut Self>, field: i32, checked: bool) -> bool {
        let Ok(field) = usize::try_from(field) else {
            return false;
        };
        self.as_mut().rust_mut().views.set_value(field, Value::Bool(checked))
    }

    fn complete(mut self: Pin<&mut Self>, field: i32, text: &QString) -> bool {
        let Ok(field) = usize::try_from(field) else {
            return false;
        };
        let Some(message) = self.as_mut().rust_mut().views.complete(field, &String::from(text)) else {
            return false;
        };
        // Another field's paths go at once, rather than when this one's come.
        if usize::try_from(self.completing_field).ok() != Some(field) {
            self.as_mut().show_completions();
        }
        self.as_mut().rust_mut().send(&message)
    }

    fn submit(mut self: Pin<&mut Self>) -> bool {
        let Some(message) = self.as_mut().rust_mut().views.submit() else {
            return false;
        };
        self.as_mut().show_field_errors();
        self.as_mut().rust_mut().send(&message)
    }

    fn toggle_option(mut self: Pin<&mut Self>, field: i32, value: &QString) -> bool {
        // The field shows the tick already, so the view isn't told.
        let Ok(field) = usize::try_from(field) else {
            return false;
        };
        self.as_mut()
            .rust_mut()
            .views
            .toggle_option(field, &String::from(value))
    }

    fn select(mut self: Pin<&mut Self>, row: i32) {
        let selected = self.row(row).map(|row| row.result.clone());
        if selected != self.selected {
            self.as_mut().cancel_confirm();
        }
        let mut this = self.as_mut().rust_mut();
        this.selected = selected;
        if row > 0 && this.listed.is_none() && !this.selection_moved {
            this.selection_moved = this.send(&FromFrontEnd::SelectionMoved);
        }
    }

    fn frame_started(mut self: Pin<&mut Self>) {
        let version = self.version;
        self.as_mut().rust_mut().drawing = version;
    }

    fn painted(mut self: Pin<&mut Self>) {
        let drawn = self.drawing;
        self.as_mut().rust_mut().on_screen = drawn;
    }

    fn viewport(mut self: Pin<&mut Self>, top: i32, rows: i32) {
        let message = FromFrontEnd::Viewport {
            top: top.max(0) as u32,
            rows: rows.max(0) as u32,
        };
        self.as_mut().rust_mut().send(&message);
    }

    fn undo(mut self: Pin<&mut Self>) -> bool {
        let Some(change) = self.offered else {
            return false;
        };
        let sent = self.as_mut().rust_mut().send(&FromFrontEnd::Undo { change });
        if sent {
            self.as_mut().set_toast(None);
        }
        sent
    }

    #[expect(
        clippy::unused_self,
        reason = "QML calls it on the search, so the bridge declares it as a member function"
    )]
    fn is_undo_key(&self, key: i32, modifiers: i32) -> bool {
        shortcut::matches(UNDO_SHORTCUT, key, modifiers)
    }

    fn show_problems(mut self: Pin<&mut Self>) -> bool {
        if self.problems == 0 {
            return false;
        }
        let shown = self.on_screen;
        self.as_mut().rust_mut().send(&FromFrontEnd::RunAction {
            result: Problem::list(),
            shown,
            action: Problem::show(),
            arguments: BTreeMap::default(),
        })
    }

    fn closed(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().send(&FromFrontEnd::Closed);
    }

    fn shown(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().send(&FromFrontEnd::Shown);
    }

    /// Shows the Toast for a change to the History, or hides it.
    fn set_toast(mut self: Pin<&mut Self>, offer: Option<(u64, String)>) {
        let (offered, toast) = match offer {
            Some((change, title)) => (Some(change), QString::from(&title)),
            None => (None, QString::default()),
        };
        let mut this = self.as_mut().rust_mut();
        this.offered = offered;
        if this.toast != toast {
            this.toast = toast;
            self.as_mut().toast_changed();
        }
    }
}

impl SearchRust {
    /// Sends to the Daemon, if it is there. A failed write drops the
    /// connection; the reading thread notices and connects again.
    fn send(&mut self, message: &FromFrontEnd) -> bool {
        let Some(writer) = &mut self.writer else {
            return false;
        };
        if let Err(err) = writer.send(message) {
            eprintln!("kotodama: couldn't reach the Daemon: {err}");
            self.writer = None;
            return false;
        }
        true
    }
}
