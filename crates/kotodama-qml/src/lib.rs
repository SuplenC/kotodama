//! The Quickshell Front end: the `Kotodama` QML module.
//!
//! [`KotodamaSearch`](qobject::KotodamaSearch) is a `QAbstractListModel` over
//! the Daemon's Results. It talks to the Daemon in the fastest Wire format they
//! share and hands QML each Result's fields through model roles, so Results
//! never pass through JavaScript. `KotodamaMenu.qml` is the window that draws
//! them; a host shell can use it as it is, set its look, or draw its own.

mod connection;
mod plugin;
mod rows;

use std::pin::Pin;
use std::sync::OnceLock;
use std::time::Instant;

use cxx_qt::{CxxQtType, Initialize, Threading};
use cxx_qt_lib::{QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant};
use kotodama_protocol::FromFrontEnd;

use connection::{Update, Writer};
use rows::{Replace, Row};

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
        type KotodamaSearch = super::SearchRust;

        fn set_query(self: Pin<&mut KotodamaSearch>, query: QString);
        fn set_highlight_color(self: Pin<&mut KotodamaSearch>, color: QString);

        /// Runs the primary Action of the Result in `row`. False if there is
        /// none, or the Daemon isn't there to run it.
        #[qinvokable]
        fn run(self: Pin<&mut KotodamaSearch>, row: i32) -> bool;

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
        /// View asked for, or empty.
        #[qsignal]
        #[cxx_name = "openRequested"]
        fn open_requested(self: Pin<&mut KotodamaSearch>, query: QString, view: QString);

        /// `kotodama toggle`: show the window if it is hidden, hide it if not.
        #[qsignal]
        #[cxx_name = "toggleRequested"]
        fn toggle_requested(self: Pin<&mut KotodamaSearch>);

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
    rows: Vec<Row>,
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
            rows: Vec::new(),
            writer: None,
            asked: None,
        }
    }
}

impl Initialize for KotodamaSearch {
    fn initialize(self: Pin<&mut Self>) {
        let qt_thread = self.qt_thread();
        connection::spawn(move |update| {
            qt_thread.queue(move |search| search.apply(update)).is_ok()
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

    fn run(mut self: Pin<&mut Self>, row: i32) -> bool {
        let Some(row) = usize::try_from(row).ok().and_then(|row| self.rows.get(row)) else {
            return false;
        };
        let Some(action) = row.primary.clone() else {
            return false;
        };
        let message = FromFrontEnd::RunAction {
            result: row.result.clone(),
            action,
            arguments: Default::default(),
        };
        self.as_mut().rust_mut().send(&message)
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

    /// Takes what the connection read, on the Qt thread.
    fn apply(mut self: Pin<&mut Self>, update: Update) {
        match update {
            Update::Connected(writer) => {
                let mut this = self.as_mut().rust_mut();
                this.writer = Some(writer);
                this.asked = None;
                self.as_mut().set_connected(true);
                // A Daemon that restarted has no idea what is typed.
                let text = String::from(&self.query);
                if !text.is_empty() {
                    self.as_mut().send_query(text);
                }
            }
            Update::Rows(rows) => {
                self.as_mut().replace(rows);
                if let Some(asked) = self.as_mut().rust_mut().asked.take() {
                    let micros = asked.elapsed().as_micros().min(i32::MAX as u128) as i32;
                    if debug() {
                        eprintln!("kotodama: query to model update: {micros} µs");
                    }
                    self.as_mut().rust_mut().latency = micros;
                    self.as_mut().latency_changed();
                }
            }
            Update::Down(down) => {
                self.as_mut().rust_mut().down = down.iter().map(QString::from).collect();
                self.as_mut().down_changed();
            }
            Update::Close => self.as_mut().close_requested(),
            Update::Open { query, view } => {
                let view = view.as_deref().map(QString::from).unwrap_or_default();
                self.as_mut().open_requested(QString::from(&query), view);
            }
            Update::Toggle => self.as_mut().toggle_requested(),
            Update::ActionFailed(message) => {
                self.as_mut().action_failed(QString::from(&message));
            }
            Update::Disconnected => {
                let mut this = self.as_mut().rust_mut();
                this.writer = None;
                this.asked = None;
                self.as_mut().set_connected(false);
                self.as_mut().replace(Vec::new());
            }
        }
    }

    fn set_connected(mut self: Pin<&mut Self>, connected: bool) {
        if self.connected != connected {
            self.as_mut().rust_mut().connected = connected;
            self.as_mut().connected_changed();
        }
    }

    /// Puts `rows` in place of the current rows: those both lists have change
    /// in place, and the rest are inserted or removed, so the view keeps its
    /// delegates and its current row.
    fn replace(mut self: Pin<&mut Self>, mut rows: Vec<Row>) {
        let plan = Replace::new(self.rows.len(), rows.len());
        let root = QModelIndex::default();
        if !plan.removed.is_empty() {
            // SAFETY: the rows are removed between begin and end, as Qt requires.
            unsafe {
                self.as_mut().begin_remove_rows(
                    &root,
                    plan.removed.start as i32,
                    plan.removed.end as i32 - 1,
                );
                self.as_mut().rust_mut().rows.truncate(plan.removed.start);
                self.as_mut().end_remove_rows();
            }
        }
        let tail = rows.split_off(plan.changed.end);
        self.as_mut()
            .rust_mut()
            .rows
            .splice(plan.changed.clone(), rows);
        self.as_mut().rows_changed(plan.changed, &[]);
        if !plan.inserted.is_empty() {
            // SAFETY: the rows are inserted between begin and end, as Qt requires.
            unsafe {
                self.as_mut().begin_insert_rows(
                    &root,
                    plan.inserted.start as i32,
                    plan.inserted.end as i32 - 1,
                );
                self.as_mut().rust_mut().rows.extend(tail);
                self.as_mut().end_insert_rows();
            }
        }
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
