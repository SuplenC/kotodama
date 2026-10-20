//! The Quickshell Front end: the `Kotodama` QML module.
//!
//! [`KotodamaSearch`](qobject::KotodamaSearch) is a `QAbstractListModel` over
//! the Daemon's Results. It talks to the Daemon in the fastest Wire format they
//! share and hands QML each Result's fields through model roles, so Results
//! never pass through JavaScript. It also keeps the stack of Views Extensions
//! push, the jobs they report, and each Result's Action panel. While a list View
//! is on top, the model holds that View's Items instead of the Results: those
//! that match its filter, as the Daemon finds them.
//! `KotodamaMenu.qml` is the window that draws them; a host shell can use it as
//! it is, set its look, or draw its own.

mod connection;
mod plugin;
mod rows;
mod shortcut;
mod variant;
mod views;

use std::pin::Pin;
use std::sync::OnceLock;
use std::time::Instant;

use cxx_qt::{CxxQtType, Initialize, Threading};
use cxx_qt_lib::{QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant};
use kotodama_protocol::{FromFrontEnd, ResultRef, Value};

use connection::{Update, Writer};
use rows::{Model, Row, Rows};
use views::{Open, Shown, Views};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++Qt" {
        include!(<QtCore/QAbstractListModel>);
        #[qobject]
        type QAbstractListModel;
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvector.h");
        type QVector_i32 = cxx_qt_lib::QVector<i32>;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        /// The search: the text typed, and the Daemon's ranked Results for it.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, query, READ, WRITE = set_query, NOTIFY)]
        #[qproperty(QString, highlight_color, cxx_name = "highlightColor", READ, WRITE = set_highlight_color, NOTIFY)]
        #[qproperty(bool, connected, READ, NOTIFY)]
        #[qproperty(QStringList, down, READ, NOTIFY)]
        #[qproperty(i32, latency, READ, NOTIFY)]
        /// The top View, as a map; empty at the search. See `variant::view`.
        #[qproperty(QMap_QString_QVariant, view, READ, NOTIFY)]
        /// How many Views are open.
        #[qproperty(i32, depth, READ, NOTIFY)]
        /// The jobs still running, oldest first. See `variant::jobs`.
        #[qproperty(QList_QVariant, jobs, READ, NOTIFY)]
        type KotodamaSearch = super::SearchRust;

        fn set_query(self: Pin<&mut KotodamaSearch>, query: QString);
        fn set_highlight_color(self: Pin<&mut KotodamaSearch>, color: QString);

        /// Runs the primary Action of the Result in `row`. False if there is
        /// none, or the Daemon isn't there to run it.
        #[qinvokable]
        fn run(self: Pin<&mut KotodamaSearch>, row: i32) -> bool;

        /// The Action panel of the Result in `row`: the Item's own Actions,
        /// then Contributed actions grouped by Extension. See
        /// `variant::actions`.
        #[qinvokable]
        fn actions(self: &KotodamaSearch, row: i32) -> QList_QVariant;

        /// Runs the `n`th Action of the Result in `row`'s Action panel.
        #[qinvokable]
        #[cxx_name = "runAction"]
        fn run_action(self: Pin<&mut KotodamaSearch>, row: i32, n: i32) -> bool;

        /// Runs the Action of the Result in `row` bound to `key` (a `Qt::Key`)
        /// with `modifiers` held, as a key event gives them. False if none is.
        #[qinvokable]
        #[cxx_name = "runShortcut"]
        fn run_shortcut(self: Pin<&mut KotodamaSearch>, row: i32, key: i32, modifiers: i32)
        -> bool;

        /// Filters the list View on top by `text`: the Daemon matches it as it
        /// does the search, or the View's Extension does. False if the top
        /// View isn't a list, or `text` is its filter already. The top View's
        /// `filter` is what was typed.
        #[qinvokable]
        fn filter(self: Pin<&mut KotodamaSearch>, text: QString) -> bool;

        /// Pops the top View, as Esc does, and tells its Extension the user
        /// left it. False if there is none. Closing the menu pops nothing:
        /// it opens again where it was.
        #[qinvokable]
        #[cxx_name = "popView"]
        fn pop_view(self: Pin<&mut KotodamaSearch>) -> bool;

        /// Sets the text of the top form's `field`th field, or the value of
        /// the option chosen in a select.
        #[qinvokable]
        #[cxx_name = "setText"]
        fn set_text(self: Pin<&mut KotodamaSearch>, field: i32, text: QString) -> bool;

        /// Ticks or unticks the top form's `field`th field, a checkbox.
        #[qinvokable]
        #[cxx_name = "setChecked"]
        fn set_checked(self: Pin<&mut KotodamaSearch>, field: i32, checked: bool) -> bool;

        /// Submits the top form. False if the top View isn't a form, or the
        /// Daemon isn't there to take it.
        #[qinvokable]
        fn submit(self: Pin<&mut KotodamaSearch>) -> bool;

        /// The row the list has selected, which a list View keeps selected as
        /// its rows change while its Item is listed. See `selectRow`.
        #[qinvokable]
        fn select(self: Pin<&mut KotodamaSearch>, row: i32);

        /// Tells the Daemon which rows are on screen.
        #[qinvokable]
        fn viewport(self: Pin<&mut KotodamaSearch>, top: i32, rows: i32);

        /// Answers `closeRequested` once the window is gone.
        #[qinvokable]
        fn closed(self: Pin<&mut KotodamaSearch>);

        /// The Daemon asks the window to close, before it types or pastes into
        /// the window underneath. Hide it, then call `closed()`.
        #[qsignal]
        #[cxx_name = "closeRequested"]
        fn close_requested(self: Pin<&mut KotodamaSearch>);

        /// `kotodama open`: show the window with `query` typed. `view` is the
        /// View asked for, or empty; it is on top already, so the first frame
        /// shows it.
        #[qsignal]
        #[cxx_name = "openRequested"]
        fn open_requested(self: Pin<&mut KotodamaSearch>, query: QString, view: QString);

        /// `kotodama toggle`: show the window if it is hidden, hide it if not.
        #[qsignal]
        #[cxx_name = "toggleRequested"]
        fn toggle_requested(self: Pin<&mut KotodamaSearch>);

        /// A list View's rows changed: select `row`, where the Item selected
        /// went, or the top row if it went away.
        #[qsignal]
        #[cxx_name = "selectRow"]
        fn select_row(self: Pin<&mut KotodamaSearch>, row: i32);

        #[qsignal]
        #[cxx_name = "actionFailed"]
        fn action_failed(self: Pin<&mut KotodamaSearch>, message: QString);
    }

    impl cxx_qt::Threading for KotodamaSearch {}
    impl cxx_qt::Initialize for KotodamaSearch {}

    unsafe extern "RustQt" {
        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut KotodamaSearch>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );

        #[inherit]
        fn index(self: &KotodamaSearch, row: i32, column: i32, parent: &QModelIndex)
        -> QModelIndex;

        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut KotodamaSearch>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut KotodamaSearch>);

        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        unsafe fn begin_remove_rows(
            self: Pin<&mut KotodamaSearch>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        #[inherit]
        #[cxx_name = "endRemoveRows"]
        unsafe fn end_remove_rows(self: Pin<&mut KotodamaSearch>);

        #[inherit]
        #[cxx_name = "beginMoveRows"]
        unsafe fn begin_move_rows(
            self: Pin<&mut KotodamaSearch>,
            source_parent: &QModelIndex,
            source_first: i32,
            source_last: i32,
            destination_parent: &QModelIndex,
            destination_child: i32,
        ) -> bool;
        #[inherit]
        #[cxx_name = "endMoveRows"]
        unsafe fn end_move_rows(self: Pin<&mut KotodamaSearch>);
    }

    extern "RustQt" {
        #[cxx_override]
        fn data(self: &KotodamaSearch, index: &QModelIndex, role: i32) -> QVariant;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &KotodamaSearch) -> QHash_i32_QByteArray;

        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &KotodamaSearch, parent: &QModelIndex) -> i32;
    }
}

use qobject::{KotodamaSearch, QStringList, QVector_i32};

// Roles start at Qt::UserRole.
const TITLE: i32 = 0x100;
const SUBTITLE: i32 = 0x101;
const ICON: i32 = 0x102;
/// The title as StyledText, its matched characters in `highlightColor`.
const MARKUP: i32 = 0x103;
const EXTENSION: i32 = 0x104;

pub struct SearchRust {
    query: QString,
    highlight_color: QString,
    connected: bool,
    down: QStringList,
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
    views: Views,
    view: variant::Map,
    depth: i32,
    jobs: variant::List,
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
            highlight_color: QString::from("#89b4fa"),
            connected: false,
            down: QStringList::default(),
            latency: 0,
            rows: Rows::default(),
            results: None,
            listed: None,
            selected: None,
            views: Views::default(),
            view: variant::Map::default(),
            depth: 0,
            jobs: variant::List::default(),
            writer: None,
            asked: None,
        }
    }
}

impl Initialize for KotodamaSearch {
    fn initialize(self: Pin<&mut Self>) {
        let qt_thread = self.qt_thread();
        connection::spawn(move |updates| {
            qt_thread.queue(move |search| search.apply(updates)).is_ok()
        });
    }
}

/// `KOTODAMA_LOG=debug` logs each query's latency.
fn debug() -> bool {
    static DEBUG: OnceLock<bool> = OnceLock::new();
    *DEBUG.get_or_init(|| std::env::var("KOTODAMA_LOG").as_deref() == Ok("debug"))
}

impl KotodamaSearch {
    fn set_query(mut self: Pin<&mut Self>, query: QString) {
        if self.query == query {
            return;
        }
        let text = String::from(&query);
        self.as_mut().rust_mut().query = query;
        self.as_mut().query_changed();
        self.as_mut().send_query(text);
    }

    fn set_highlight_color(mut self: Pin<&mut Self>, color: QString) {
        if self.highlight_color == color {
            return;
        }
        self.as_mut().rust_mut().highlight_color = color;
        self.as_mut().highlight_color_changed();
        let rows = self.rows.len();
        self.as_mut().rows_changed(0..rows, &[MARKUP]);
    }

    fn send_query(mut self: Pin<&mut Self>, text: String) {
        let now = Instant::now();
        let mut this = self.as_mut().rust_mut();
        if this.send(&FromFrontEnd::Query { text }) {
            this.asked.get_or_insert(now);
        }
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

    fn run_action(mut self: Pin<&mut Self>, row: i32, n: i32) -> bool {
        let Some(row) = self.row(row) else {
            return false;
        };
        let Some(action) = usize::try_from(n).ok().and_then(|n| row.action(n)) else {
            return false;
        };
        let message = FromFrontEnd::RunAction {
            result: row.result.clone(),
            action,
            arguments: Default::default(),
        };
        self.as_mut().rust_mut().send(&message)
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

    fn filter(mut self: Pin<&mut Self>, text: QString) -> bool {
        let text = String::from(&text);
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

    fn set_text(mut self: Pin<&mut Self>, field: i32, text: QString) -> bool {
        // The field shows what was typed already, so the view isn't told.
        let Ok(field) = usize::try_from(field) else {
            return false;
        };
        let value = Value::String(String::from(&text));
        self.as_mut().rust_mut().views.set_value(field, value)
    }

    fn set_checked(mut self: Pin<&mut Self>, field: i32, checked: bool) -> bool {
        let Ok(field) = usize::try_from(field) else {
            return false;
        };
        self.as_mut()
            .rust_mut()
            .views
            .set_value(field, Value::Bool(checked))
    }

    fn submit(mut self: Pin<&mut Self>) -> bool {
        let Some(message) = self.views.submit() else {
            return false;
        };
        self.as_mut().rust_mut().send(&message)
    }

    fn select(mut self: Pin<&mut Self>, row: i32) {
        let selected = self.row(row).map(|row| row.result.clone());
        self.as_mut().rust_mut().selected = selected;
    }

    fn viewport(mut self: Pin<&mut Self>, top: i32, rows: i32) {
        let message = FromFrontEnd::Viewport {
            top: top.max(0) as u32,
            rows: rows.max(0) as u32,
        };
        self.as_mut().rust_mut().send(&message);
    }

    fn closed(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().send(&FromFrontEnd::Closed);
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = usize::try_from(index.row())
            .ok()
            .and_then(|row| self.rows.get(row))
        else {
            return QVariant::default();
        };
        let text = match role {
            TITLE => QString::from(&row.title),
            SUBTITLE => QString::from(&row.subtitle),
            ICON => QString::from(&row.icon),
            MARKUP => QString::from(&row.markup(&String::from(&self.highlight_color))),
            EXTENSION => QString::from(&row.result.extension),
            _ => return QVariant::default(),
        };
        QVariant::from(&text)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut roles = QHash::<QHashPair_i32_QByteArray>::default();
        for (role, name) in [
            (TITLE, "title"),
            (SUBTITLE, "subtitle"),
            (ICON, "icon"),
            (MARKUP, "markup"),
            (EXTENSION, "extension"),
        ] {
            roles.insert(role, QByteArray::from(name));
        }
        roles
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        // A list has rows only at the top.
        if parent.is_valid() {
            0
        } else {
            self.rows.len() as i32
        }
    }

    /// Takes what the connection read, on the Qt thread, and shows what it
    /// adds up to.
    fn apply(mut self: Pin<&mut Self>, updates: Vec<Update>) {
        let mut shown = Shown::Same;
        for update in updates {
            // What QML is asked to do, such as open on the top View, sees
            // what came before it.
            if update.signals() {
                self.as_mut()
                    .show(std::mem::replace(&mut shown, Shown::Same));
            }
            shown = shown.max(self.as_mut().apply_one(update));
        }
        self.as_mut().show(shown);
    }

    fn show(self: Pin<&mut Self>, shown: Shown) {
        match shown {
            Shown::Same => {}
            Shown::Rows => self.show_rows(),
            Shown::View => self.show_view(),
        }
    }

    /// Takes one update and says what to show again.
    fn apply_one(mut self: Pin<&mut Self>, update: Update) -> Shown {
        match update {
            Update::Connected(writer) => {
                let mut this = self.as_mut().rust_mut();
                this.writer = Some(writer);
                this.asked = None;
                self.as_mut().set_connected(true);
                // A Daemon that restarted has no idea what is typed. Even
                // nothing typed has Results: those of the Empty query.
                let text = String::from(&self.query);
                self.as_mut().send_query(text);
                Shown::Same
            }
            Update::Rows(rows) => {
                if self.results.is_some() {
                    self.as_mut().rust_mut().results = Some(rows);
                } else {
                    self.as_mut().replace(rows);
                }
                if let Some(asked) = self.as_mut().rust_mut().asked.take() {
                    let micros = asked.elapsed().as_micros().min(i32::MAX as u128) as i32;
                    if debug() {
                        eprintln!("kotodama: query to model update: {micros} µs");
                    }
                    self.as_mut().rust_mut().latency = micros;
                    self.as_mut().latency_changed();
                }
                Shown::Same
            }
            Update::Down(down) => {
                self.as_mut().rust_mut().down = down.iter().map(QString::from).collect();
                self.as_mut().down_changed();
                Shown::Same
            }
            Update::Close => {
                self.as_mut().close_requested();
                Shown::Same
            }
            Update::Open { query, view } => {
                let view = view.as_deref().map(QString::from).unwrap_or_default();
                self.as_mut().open_requested(QString::from(&query), view);
                Shown::Same
            }
            Update::Toggle => {
                self.as_mut().toggle_requested();
                Shown::Same
            }
            Update::ActionFailed(message) => {
                self.as_mut().action_failed(QString::from(&message));
                Shown::Same
            }
            Update::PushView {
                extension,
                view,
                contributed,
            } => {
                self.as_mut()
                    .rust_mut()
                    .views
                    .push(extension, view, contributed);
                Shown::View
            }
            Update::ViewUpdated {
                extension,
                view,
                contributed,
            } => {
                if self
                    .as_mut()
                    .rust_mut()
                    .views
                    .update(extension, view, contributed)
                {
                    Shown::View
                } else {
                    Shown::Same
                }
            }
            Update::ReplaceView {
                extension,
                view,
                with,
                contributed,
            } => {
                self.as_mut()
                    .rust_mut()
                    .views
                    .replace(extension, &view, with, contributed);
                Shown::View
            }
            Update::FilteredView {
                extension,
                view,
                text,
                matches,
            } => self
                .as_mut()
                .rust_mut()
                .views
                .filtered(&extension, &view, &text, matches),
            Update::PopView { extension, view } => {
                self.as_mut().rust_mut().views.pop(&extension, &view);
                Shown::View
            }
            Update::JobProgress { extension, job } => {
                let followed = self
                    .views
                    .top()
                    .is_some_and(|open| open.follows(&extension, &job.id));
                self.as_mut().rust_mut().views.progress(extension, job);
                if followed {
                    let view = variant::view(&self.views);
                    self.as_mut().rust_mut().view = view;
                    self.as_mut().view_changed();
                }
                self.as_mut().show_jobs();
                Shown::Same
            }
            Update::Disconnected => {
                let mut this = self.as_mut().rust_mut();
                this.writer = None;
                this.asked = None;
                this.views.reset();
                // The Results of a Daemon that is gone, kept under a list
                // View, go too.
                this.results = None;
                self.as_mut().set_connected(false);
                self.as_mut().show_jobs();
                self.as_mut().replace(Vec::new());
                Shown::View
            }
        }
    }

    /// Shows the top View after the stack changed: its map, the depth, and,
    /// for a list View, its Items in place of the Results.
    fn show_view(mut self: Pin<&mut Self>) {
        let view = variant::view(&self.views);
        let depth = self.views.depth() as i32;
        let mut this = self.as_mut().rust_mut();
        this.view = view;
        let deeper = this.depth != depth;
        this.depth = depth;
        self.as_mut().view_changed();
        if deeper {
            self.as_mut().depth_changed();
        }
        self.as_mut().show_rows();
    }

    /// Shows the rows of the list View on top, or the Results if there is
    /// none.
    fn show_rows(mut self: Pin<&mut Self>) {
        let top = self
            .views
            .top()
            .filter(|open| open.is_list())
            .map(|open| (open.extension.clone(), open.view.id.clone()));
        let Some(top) = top else {
            self.as_mut().rust_mut().listed = None;
            if let Some(results) = self.as_mut().rust_mut().results.take() {
                self.as_mut().replace(results);
            }
            return;
        };
        if self.results.is_none() {
            let results = self.rows.iter().cloned().collect();
            self.as_mut().rust_mut().results = Some(results);
        }
        let same = self.listed.as_ref() == Some(&top);
        self.as_mut().rust_mut().listed = Some(top);
        // The rows are made from the Views while the model changes, so they
        // are set aside meanwhile; nothing a view does on the model's signals
        // reaches them.
        let views = std::mem::take(&mut self.as_mut().rust_mut().views);
        if let Some(listed) = views.top().and_then(Open::listed) {
            rows::diff(&mut QtModel(self.as_mut()), &listed);
        }
        self.as_mut().rust_mut().views = views;
        if same {
            // `selected` is still the Item selected before the diff: a list
            // view applies model changes to its current row only when it
            // lays out, after this, and `selectRow` settles that row first.
            let row = self
                .selected
                .as_ref()
                .and_then(|selected| self.rows.iter().position(|row| row.result == *selected))
                .unwrap_or(0);
            let selected = self.rows.get(row).map(|row| row.result.clone());
            self.as_mut().rust_mut().selected = selected;
            self.as_mut().select_row(row as i32);
        }
    }

    fn show_jobs(mut self: Pin<&mut Self>) {
        let jobs = variant::jobs(&self.views);
        self.as_mut().rust_mut().jobs = jobs;
        self.as_mut().jobs_changed();
    }

    fn set_connected(mut self: Pin<&mut Self>, connected: bool) {
        if self.connected != connected {
            self.as_mut().rust_mut().connected = connected;
            self.as_mut().connected_changed();
        }
    }

    /// Puts the Results in place of the current rows, by position: see
    /// [`rows::replace`].
    fn replace(self: Pin<&mut Self>, rows: Vec<Row>) {
        rows::replace(&mut QtModel(self), rows);
    }

    /// Tells views the rows in `range` changed, in `roles` or, if empty, in all.
    fn rows_changed(mut self: Pin<&mut Self>, range: std::ops::Range<usize>, roles: &[i32]) {
        if range.is_empty() {
            return;
        }
        let root = QModelIndex::default();
        let first = self.index(range.start as i32, 0, &root);
        let last = self.index(range.end as i32 - 1, 0, &root);
        let mut changed = QVector_i32::default();
        for &role in roles {
            changed.append(role);
        }
        self.as_mut().data_changed(&first, &last, &changed);
    }
}

/// The model's rows, changed between the signals Qt requires around each
/// change.
struct QtModel<'a>(Pin<&'a mut KotodamaSearch>);

impl Model for QtModel<'_> {
    fn rows(&mut self) -> &mut Rows {
        &mut self.0.as_mut().rust_mut().get_mut().rows
    }

    fn begin_remove(&mut self, rows: std::ops::Range<usize>) {
        // SAFETY: `rows::diff` and `rows::replace` remove the rows before
        // `end_remove`, as Qt requires.
        unsafe {
            self.0.as_mut().begin_remove_rows(
                &QModelIndex::default(),
                rows.start as i32,
                rows.end as i32 - 1,
            );
        }
    }

    fn end_remove(&mut self) {
        // SAFETY: after a `begin_remove`.
        unsafe { self.0.as_mut().end_remove_rows() }
    }

    fn begin_insert(&mut self, rows: std::ops::Range<usize>) {
        // SAFETY: the rows are inserted before `end_insert`, as Qt requires.
        unsafe {
            self.0.as_mut().begin_insert_rows(
                &QModelIndex::default(),
                rows.start as i32,
                rows.end as i32 - 1,
            );
        }
    }

    fn end_insert(&mut self) {
        // SAFETY: after a `begin_insert`.
        unsafe { self.0.as_mut().end_insert_rows() }
    }

    fn begin_move(&mut self, from: usize, before: usize) {
        let root = QModelIndex::default();
        // SAFETY: the row moves before `end_move`, as Qt requires; `before`
        // is never `from` or `from + 1`, the moves Qt refuses.
        let moving = unsafe {
            self.0
                .as_mut()
                .begin_move_rows(&root, from as i32, from as i32, &root, before as i32)
        };
        debug_assert!(moving, "Qt refused to move row {from} before {before}");
    }

    fn end_move(&mut self) {
        // SAFETY: after a `begin_move`.
        unsafe { self.0.as_mut().end_move_rows() }
    }

    fn changed(&mut self, rows: std::ops::Range<usize>) {
        self.0.as_mut().rows_changed(rows, &[]);
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
