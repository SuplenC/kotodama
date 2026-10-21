//! The `KotodamaSearch` `QObject` as Qt and QML see it: its properties,
//! invokables and signals, and the `QAbstractListModel` members it inherits or
//! overrides. cxx-qt generates the C++ class and the glue from this module, so
//! the names here are the ones QML calls.
//!
//! # Owns
//!
//! - The QML-visible surface of the search, and its documentation.
//!
//! # Not here
//!
//! - The state behind it (`SearchRust`) and what the invokables do: `lib.rs`.
//! - The model's roles and overrides: `model`.
//! - What the Daemon's messages do to it: `updates`.

use crate::SearchRust;

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
        /// What the search box shows; in an app, what follows the app's word.
        #[qproperty(QString, query, READ, WRITE = set_query, NOTIFY)]
        /// The color the matched characters of a title are set in, any color
        /// QML accepts.
        #[qproperty(QString, highlight_color, cxx_name = "highlightColor", READ, WRITE = set_highlight_color, NOTIFY)]
        /// Whether the Daemon is there.
        #[qproperty(bool, connected, READ, NOTIFY)]
        /// The Extensions that are down, by name.
        #[qproperty(QStringList, down, READ, NOTIFY)]
        /// How many problems the Daemon knows of, which the user should
        /// fix; `showProblems` lists them.
        #[qproperty(i32, problems, READ, NOTIFY)]
        /// Microseconds from the query that Results were last asked for to
        /// the model showing them.
        #[qproperty(i32, latency, READ, NOTIFY)]
        /// The top View, as a map; empty at the search. See `variant::view`.
        #[qproperty(QMap_QString_QVariant, view, READ, NOTIFY)]
        /// How many Views are open.
        #[qproperty(i32, depth, READ, NOTIFY)]
        /// The jobs still running, oldest first. See `variant::jobs`.
        #[qproperty(QList_QVariant, jobs, READ, NOTIFY)]
        /// The Toast: what the change to the History that can be undone did,
        /// such as `Forgot “Firefox”`, or empty when there is none.
        #[qproperty(QString, toast, READ, NOTIFY)]
        /// The app the search is in, by the title a chip shows; empty for
        /// the whole search. `query` is then what follows the app's word.
        #[qproperty(QString, scope, READ, NOTIFY)]
        /// The app's intro card, shown while nothing is typed in it: what
        /// it does, and `introExamples`, each `{query, result}`. Empty
        /// elsewhere, and once something is typed.
        #[qproperty(QString, intro_about, cxx_name = "introAbout", READ, NOTIFY)]
        /// What the app's Extension wants the user to know, such as why it
        /// can't do anything, shown on the intro card; or empty.
        #[qproperty(QString, intro_note, cxx_name = "introNote", READ, NOTIFY)]
        /// What the footer asks while an Action that asks first waits for
        /// Enter again, such as `Power Off? ↵ Confirm · Esc Cancel`; empty
        /// when none waits.
        #[qproperty(QString, confirming, READ, NOTIFY)]
        /// The examples on the app's intro card, each `{query, result}`. See
        /// `variant::examples`.
        #[qproperty(QList_QVariant, intro_examples, cxx_name = "introExamples", READ, NOTIFY)]
        type KotodamaSearch = super::SearchRust;

        fn set_query(self: Pin<&mut KotodamaSearch>, query: QString);
        fn set_highlight_color(self: Pin<&mut KotodamaSearch>, color: QString);

        /// Runs the primary Action of the Result in `row`. False if there is
        /// none, or the Daemon isn't there to run it. One that asks first
        /// waits for `confirm`, with `confirming` set.
        #[qinvokable]
        fn run(self: Pin<&mut KotodamaSearch>, row: i32) -> bool;

        /// Runs the Action waiting for confirmation. False if none waits, or
        /// the Daemon isn't there to run it.
        #[qinvokable]
        fn confirm(self: Pin<&mut KotodamaSearch>) -> bool;

        /// Drops the Action waiting for confirmation, running nothing. False
        /// if none waits. Typing, moving the selection, or a View opening or
        /// closing drops it too.
        #[qinvokable]
        #[cxx_name = "cancelConfirm"]
        fn cancel_confirm(self: Pin<&mut KotodamaSearch>) -> bool;

        /// The Action panel of the Result in `row`: the Item's own Actions,
        /// then Contributed actions grouped by Extension. See
        /// `variant::actions`.
        #[qinvokable]
        fn actions(self: &KotodamaSearch, row: i32) -> QList_QVariant;

        /// The keys the Result in `row` takes, such as
        /// `↵ Open · Tab Enter app · ⌃K Actions`, or empty. See `Row::hints`.
        #[qinvokable]
        fn hints(self: &KotodamaSearch, row: i32) -> QString;

        /// Runs the `n`th Action of the Result in `row`'s Action panel. One
        /// that asks first waits for `confirm`, with `confirming` set.
        #[qinvokable]
        #[cxx_name = "runAction"]
        fn run_action(self: Pin<&mut KotodamaSearch>, row: i32, n: i32) -> bool;

        /// Runs the Action of the Result in `row` bound to `key` (a `Qt::Key`)
        /// with `modifiers` held, as a key event gives them. False if none is.
        #[qinvokable]
        #[cxx_name = "runShortcut"]
        fn run_shortcut(self: Pin<&mut KotodamaSearch>, row: i32, key: i32, modifiers: i32) -> bool;

        /// Filters the list View on top by `text`: the Daemon matches it as it
        /// does the search, or the View's Extension does. False if the top
        /// View isn't a list, or `text` is its filter already. The top View's
        /// `filter` is what was typed.
        #[qinvokable]
        fn filter(self: Pin<&mut KotodamaSearch>, text: &QString) -> bool;

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
        fn set_text(self: Pin<&mut KotodamaSearch>, field: i32, text: &QString) -> bool;

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

        /// Undoes the change the Toast offers to undo, and hides it. False if
        /// there is none, or the Daemon isn't there to undo it.
        #[qinvokable]
        fn undo(self: Pin<&mut KotodamaSearch>) -> bool;

        /// Whether `key` (a `Qt::Key`) with `modifiers` held is the key that
        /// undoes what the Toast offers to.
        #[qinvokable]
        #[cxx_name = "isUndoKey"]
        fn is_undo_key(self: &KotodamaSearch, key: i32, modifiers: i32) -> bool;

        /// Opens the list of problems in a View. False if there are none, or
        /// the Daemon isn't there to list them.
        #[qinvokable]
        #[cxx_name = "showProblems"]
        fn show_problems(self: Pin<&mut KotodamaSearch>) -> bool;

        /// Tells the Daemon which rows are on screen.
        #[qinvokable]
        fn viewport(self: Pin<&mut KotodamaSearch>, top: i32, rows: i32);

        /// The window is about to draw a frame of the model as it stands.
        #[qinvokable]
        #[cxx_name = "frameStarted"]
        fn frame_started(self: Pin<&mut KotodamaSearch>);

        /// The frame `frameStarted` began reached the screen: its Results are
        /// the ones the user sees, and so the ones a run confirms.
        #[qinvokable]
        fn painted(self: Pin<&mut KotodamaSearch>);

        /// Answers `closeRequested` once the window is gone.
        #[qinvokable]
        fn closed(self: Pin<&mut KotodamaSearch>);

        /// The window became visible, however it was opened. Call it every
        /// time, so the Daemon ranks the Empty query again for the time of
        /// day.
        #[qinvokable]
        fn shown(self: Pin<&mut KotodamaSearch>);

        /// Leaves the app, as Backspace on an empty search box does: the
        /// whole search again, with the app's word alone typed. False if the
        /// search isn't in an app.
        #[qinvokable]
        #[cxx_name = "leaveScope"]
        fn leave_scope(self: Pin<&mut KotodamaSearch>) -> bool;

        /// Leaves the app for the whole search with nothing typed, as Esc in
        /// one does. False if the search isn't in an app.
        #[qinvokable]
        #[cxx_name = "leaveApp"]
        fn leave_app(self: Pin<&mut KotodamaSearch>) -> bool;

        /// Enters the app whose entry Item is in `row`, as Tab on it does.
        /// False if the row isn't one.
        #[qinvokable]
        fn enter(self: Pin<&mut KotodamaSearch>, row: i32) -> bool;

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

        /// An Action failed: `message` says why.
        #[qsignal]
        #[cxx_name = "actionFailed"]
        fn action_failed(self: Pin<&mut KotodamaSearch>, message: QString);

        /// The search box shows `text` in place of what it shows: the rest of
        /// the query once it is in an app, or a query the Daemon typed.
        /// `query` is `text` already.
        #[qsignal]
        #[cxx_name = "queryReplaced"]
        fn query_replaced(self: Pin<&mut KotodamaSearch>, text: QString);
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
        fn index(self: &KotodamaSearch, row: i32, column: i32, parent: &QModelIndex) -> QModelIndex;

        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(self: Pin<&mut KotodamaSearch>, parent: &QModelIndex, first: i32, last: i32);
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut KotodamaSearch>);

        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        unsafe fn begin_remove_rows(self: Pin<&mut KotodamaSearch>, parent: &QModelIndex, first: i32, last: i32);
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
