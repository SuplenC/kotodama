//! What the Daemon's messages do to the search, on the Qt thread: each
//! batch the connection hands over is applied in order, then shown once, so
//! the menu draws what the batch adds up to.
//!
//! # Owns
//!
//! - Applying an [`Update`]: the Results, the status, the Views, the jobs, the Toast's offers, and
//!   the Daemon coming and going.
//! - Which list the model shows: the Results, or the Items of the list View on top, kept selected
//!   as they change.
//!
//! # Not here
//!
//! - Reading the messages off the socket: `connection`.
//! - The View stack itself: `views`; the rows and their diff: `rows`.
//! - What QML's calls do: `lib.rs`.

use std::pin::Pin;
use std::sync::OnceLock;

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use kotodama_protocol::Job;
use kotodama_protocol::Scope;

use crate::bridge::qobject::KotodamaSearch;
use crate::connection::Update;
use crate::connection::Writer;
use crate::model::QtModel;
use crate::rows;
use crate::rows::Row;
use crate::scope;
use crate::variant;
use crate::views::Open;
use crate::views::Shown;

/// `KOTODAMA_LOG=debug` logs each query's latency.
fn debug() -> bool {
    static DEBUG: OnceLock<bool> = OnceLock::new();
    *DEBUG.get_or_init(|| std::env::var("KOTODAMA_LOG").as_deref() == Ok("debug"))
}

impl KotodamaSearch {
    /// Takes what the connection read, on the Qt thread, and shows what it
    /// adds up to.
    pub(crate) fn apply(mut self: Pin<&mut Self>, updates: Vec<Update>) {
        let mut shown = Shown::Same;
        for update in updates {
            // What QML is asked to do, such as open on the top View, sees
            // what came before it.
            if update.signals() {
                self.as_mut().show(std::mem::replace(&mut shown, Shown::Same));
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
            Update::Connected(writer) => self.daemon_connected(writer),
            Update::Rows(version, rows, scope) => self.take_results(version, rows, scope),
            Update::Status(down, problems) => self.take_status(&down, problems),
            Update::Close => {
                self.as_mut().close_requested();
                Shown::Same
            }
            Update::Open { query, view } => {
                // The menu opens on `query` itself, whatever it showed before.
                self.as_mut().drop_scope();
                let view = view.as_deref().map(QString::from).unwrap_or_default();
                self.as_mut().open_requested(QString::from(&query), view);
                Shown::Same
            }
            Update::Toggle => {
                self.as_mut().drop_scope();
                self.as_mut().toggle_requested();
                Shown::Same
            }
            Update::ActionFailed(message) => {
                self.as_mut().action_failed(QString::from(&message));
                Shown::Same
            }
            Update::SetQuery(text) => self.take_query(text),
            Update::PushView {
                extension,
                view,
                contributed,
            } => {
                self.as_mut().rust_mut().views.push(extension, view, contributed);
                Shown::View
            }
            Update::ViewUpdated {
                extension,
                view,
                contributed,
            } => {
                if self.as_mut().rust_mut().views.update(extension, view, contributed) {
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
            Update::JobProgress { extension, job } => self.take_progress(extension, job),
            Update::OfferUndo { change, title } => {
                self.as_mut().set_toast(Some((change, title)));
                Shown::Same
            }
            Update::WithdrawUndo { change } => {
                if self.offered == Some(change) {
                    self.as_mut().set_toast(None);
                }
                Shown::Same
            }
            Update::Disconnected => self.daemon_gone(),
        }
    }

    /// Starts over with a Daemon that connected, by sending it what is typed.
    fn daemon_connected(mut self: Pin<&mut Self>, writer: Writer) -> Shown {
        let mut this = self.as_mut().rust_mut();
        this.writer = Some(writer);
        this.asked = None;
        // A Daemon that restarted counts its Results from the start.
        this.version = 0;
        this.kept_version = 0;
        this.drawing = 0;
        this.on_screen = 0;
        this.selection_moved = false;
        self.as_mut().set_connected(true);
        // A Daemon that restarted has no idea what is typed. Even nothing
        // typed has Results: those of the Empty query.
        let text = scope::full(self.word.as_deref(), &String::from(&self.query));
        self.as_mut().send_query(text);
        Shown::Same
    }

    /// Shows the Results, or keeps them while a list View's Items are shown,
    /// and the app the query is in.
    fn take_results(mut self: Pin<&mut Self>, version: u32, rows: Vec<Row>, scope: Option<Scope>) -> Shown {
        if self.results.is_some() {
            let mut this = self.as_mut().rust_mut();
            this.results = Some(rows);
            this.kept_version = version;
        } else {
            self.as_mut().rust_mut().version = version;
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
        let intro = scope.as_ref().and_then(|scope| scope.intro.clone());
        self.as_mut().enter_scope(scope);
        self.as_mut().set_intro(intro);
        Shown::Same
    }

    /// Shows the Extensions that are down and how many problems there are.
    fn take_status(mut self: Pin<&mut Self>, down: &[String], problems: usize) -> Shown {
        self.as_mut().rust_mut().down = down.iter().map(QString::from).collect();
        self.as_mut().down_changed();
        let problems = i32::try_from(problems).unwrap_or(i32::MAX);
        if problems != self.problems {
            self.as_mut().rust_mut().problems = problems;
            self.as_mut().problems_changed();
        }
        Shown::Same
    }

    /// Shows the query the Daemon typed, outside any app.
    fn take_query(mut self: Pin<&mut Self>, text: String) -> Shown {
        self.as_mut().drop_scope();
        // The Daemon took the text as the query, and its Results, the app and
        // its intro card, follow: sending it would only ask again.
        let shown = QString::from(&text);
        self.as_mut().rust_mut().sent = Some(text);
        self.as_mut().set_query(shown.clone());
        self.as_mut().query_replaced(shown);
        Shown::Same
    }

    /// Shows a job's latest state, in the jobs and in a progress View
    /// following it.
    fn take_progress(mut self: Pin<&mut Self>, extension: String, job: Job) -> Shown {
        let followed = self.views.top().is_some_and(|open| open.follows(&extension, &job.id));
        self.as_mut().rust_mut().views.progress(extension, job);
        if followed {
            let view = variant::view(&self.views);
            self.as_mut().rust_mut().view = view;
            self.as_mut().view_changed();
        }
        self.as_mut().show_jobs();
        Shown::Same
    }

    /// Forgets what the Daemon that went away said: its Views, jobs, Toast and
    /// Results.
    fn daemon_gone(mut self: Pin<&mut Self>) -> Shown {
        self.as_mut().set_toast(None);
        let mut this = self.as_mut().rust_mut();
        this.writer = None;
        this.asked = None;
        this.sent = None;
        this.views.reset();
        // The Results kept under a list View go with the Daemon too.
        this.results = None;
        self.as_mut().set_connected(false);
        self.as_mut().show_jobs();
        self.as_mut().replace(Vec::new());
        Shown::View
    }

    /// Shows the top View after the stack changed: its map, the depth, and,
    /// for a list View, its Items in place of the Results.
    pub(crate) fn show_view(mut self: Pin<&mut Self>) {
        let view = variant::view(&self.views);
        let depth = self.views.depth() as i32;
        let mut this = self.as_mut().rust_mut();
        this.view = view;
        let deeper = this.depth != depth;
        this.depth = depth;
        self.as_mut().view_changed();
        if deeper {
            self.as_mut().cancel_confirm();
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
                let kept = self.kept_version;
                self.as_mut().rust_mut().version = kept;
                self.as_mut().replace(results);
            }
            return;
        };
        if self.results.is_none() {
            let results = self.rows.iter().cloned().collect();
            let mut this = self.as_mut().rust_mut();
            this.results = Some(results);
            this.kept_version = this.version;
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
}
