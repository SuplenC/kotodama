//! The Front ends: their connections, what they send, and what goes back.
//!
//! # Owns
//!
//! - A Front end's session, from connecting to disconnecting.
//! - Handling each message a Front end sends, and asking the menu for a Front end that isn't one.
//! - Output for the desktop, held until the menu has closed.
//!
//! # Not here
//!
//! - Opening the menu on a View: `openings.rs`.
//! - Running a picked Action: `picks.rs`.

use std::cell::Cell;
use std::sync::mpsc::SyncSender;
use std::time::Instant;

use kotodama_protocol::Clipboard;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::ResultRef;
use kotodama_protocol::Table;
use kotodama_protocol::ToExtension;
use kotodama_protocol::ToFrontEnd;
use nucleo_matcher::pattern::Pattern;

use super::ArgumentForm;
use super::CLEAR_EXTENSION_HISTORY;
use super::CLOSE_TIMEOUT;
use super::Daemon;
use super::Desktop;
use super::ExtensionId;
use super::FORGET;
use super::FrontEndId;
use super::LastSearch;
use super::Launcher;
use super::Live;
use super::MENU_WAIT;
use super::Missing;
use super::OWN;
use super::Output;
use super::PendingOutput;
use super::Session;
use super::Undoable;
use super::Update;
use super::read_query;
use crate::arguments;
use crate::history;
use crate::history::Store;
use crate::index::Reach;
use crate::open_view::Opening;
use crate::pick::PICK;
use crate::problem;
use crate::rank;
use crate::usage::Entry;
use crate::usage::Used;

impl<L: Launcher, D: Desktop, S: Store> Daemon<L, D, S> {
    /// Nothing goes into a window, so there is no waiting for the menus to
    /// close. A paste still waiting goes first, so it can't take the
    /// clipboard back afterwards.
    pub(super) fn copy(&mut self, clipboard: Clipboard) {
        self.deliver_output();
        self.broadcast(|| ToFrontEnd::Close);
        self.desktop.output(Output::Copy(clipboard));
    }

    /// Closes every Front end, then types or pastes once they have all closed,
    /// so the text lands in the window the user came from.
    pub(super) fn output(&mut self, output: Output, now: Instant) {
        if self.pending_output.is_some() {
            self.deliver_output();
        }
        self.broadcast(|| ToFrontEnd::Close);
        self.pending_output = Some(PendingOutput {
            output,
            waiting: self.sessions.keys().copied().collect(),
            deadline: now + CLOSE_TIMEOUT,
        });
        self.closed(None);
    }

    /// A Front end answered Close, or went away.
    pub(super) fn closed(&mut self, front_end: Option<FrontEndId>) {
        let Some(pending) = &mut self.pending_output else {
            return;
        };
        pending.waiting.retain(|&id| Some(id) != front_end);
        if pending.waiting.is_empty() {
            self.deliver_output();
        }
    }

    pub(super) fn deliver_output(&mut self) {
        if let Some(pending) = self.pending_output.take() {
            self.desktop.output(pending.output);
        }
    }

    pub(super) fn broadcast(&self, message: impl Fn() -> ToFrontEnd) {
        for session in self.sessions.values() {
            session.send(message());
        }
    }

    /// Sends to the menu `kotodama open` and `toggle` from `from` go to.
    pub(super) fn to_menu(&self, from: FrontEndId, message: ToFrontEnd) {
        if let Some(session) = self.menu(from).and_then(|id| self.sessions.get(&id)) {
            session.send(message);
        }
    }

    pub(super) fn front_end_up(
        &mut self,
        id: FrontEndId,
        sender: SyncSender<ToFrontEnd>,
        disconnect: Box<dyn FnOnce() + Send>,
    ) {
        let session = Session {
            sender,
            disconnect: Some(disconnect),
            hung: Cell::new(false),
            query: String::new(),
            reach: Reach::Root,
            scope: None,
            entered: None,
            rest: 0,
            key: String::new(),
            folded: rank::Query::default(),
            pattern: Pattern::default(),
            max_score: 0,
            rows: self.engine.rows,
            typed: self.started,
            selection_moved: false,
            last_search: LastSearch::default(),
            live: self.extensions.iter().map(|_| Live::default()).collect(),
            shown: Vec::new(),
            version: 0,
            risen: Vec::new(),
            via_alias: None,
            searched: false,
            asked_for_menu: false,
            update: Update::None,
            empty_ranked_in: None,
            filters: Vec::new(),
        };
        // `kotodama check` reads the first Status alone, so it must be up to
        // date.
        if self.problems.dirty {
            self.update_problems();
        }
        session.send(ToFrontEnd::Status {
            down: self.down.clone(),
            problems: self.problems.sent.clone(),
        });
        if let Some(offer) = self.latest_undo().and_then(Undoable::offer) {
            session.send(offer);
        }
        for ((ext, _), job) in &self.jobs {
            session.send(ToFrontEnd::JobProgress {
                extension: self.extensions[ext.0].name().to_owned(),
                job: job.clone(),
            });
        }
        // A menu that restarted shows the Views it had, bottom first, with
        // nothing typed into them: a list that filters itself lists every
        // Item again.
        for open in self.views.iter() {
            if open.filters_itself() {
                self.extensions[open.extension.0].send(ToExtension::FilterView {
                    view: open.view.id.clone(),
                    text: String::new(),
                });
            }
            session.send(ToFrontEnd::PushView {
                extension: self.extensions[open.extension.0].name().to_owned(),
                view: open.view.clone(),
                contributed: self.view_contributed(open.extension, &open.view),
            });
        }
        self.sessions.insert(id, session);
    }

    pub(super) fn front_end_down(&mut self, id: FrontEndId) {
        if let Some(session) = self.sessions.remove(&id) {
            for (ext, live) in session.live.iter().enumerate() {
                if let Some(query) = live.sent {
                    self.live_queries.remove(&query);
                    self.extensions[ext].send(ToExtension::CancelLiveQuery { id: query });
                }
            }
        }
        self.argument_forms.retain(|_, form| form.front_end != id);
        self.problems_open.remove(&id);
        // The command waits connected for its answer: one that left has
        // given up, and its menu would open for nobody.
        self.awaiting_menu.retain(|&(from, ..)| from != id);
        if let Some(pick) = self.picks.remove(&id) {
            self.pick_lists.pop(pick.target, &pick.view);
        }
        self.closed(Some(id));
    }

    pub(super) fn front_end_message(&mut self, id: FrontEndId, message: FromFrontEnd, now: Instant) {
        match message {
            FromFrontEnd::Query { text } => {
                // A query typed in the search answers no pick.
                if !self.picks.is_empty() {
                    self.cancel_pick(id);
                }
                self.query(id, text, now);
            }
            FromFrontEnd::Viewport { top, rows } => {
                if let Some(session) = self.sessions.get_mut(&id) {
                    // Rows scrolled past are protected as much as those on screen.
                    session.rows = top as usize + rows as usize;
                }
            }
            FromFrontEnd::SelectionMoved => {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.selection_moved = true;
                }
            }
            FromFrontEnd::RunAction {
                result,
                shown,
                action,
                arguments,
            } => {
                let daemons = action.extension == OWN;
                let run = if daemons && action.id == PICK && result.extension == OWN {
                    self.run_pick(id, &result, now)
                } else if self.unseen(id, &result, shown) {
                    Err("that Result wasn't on screen yet".to_owned())
                } else if daemons && action.id == FORGET {
                    self.forget_result(id, &result, now)
                } else if daemons && action.id == CLEAR_EXTENSION_HISTORY {
                    self.clear_extension_history(id, &result, now)
                } else if result.extension == OWN {
                    self.run_own(id, &result, &action, &arguments, now)
                } else {
                    self.run_action(id, &result, &action, arguments, Missing::AskInForm, now)
                };
                self.failed(id, run);
            }
            FromFrontEnd::SubmitForm {
                extension,
                view,
                values,
            } => {
                if extension == OWN {
                    self.submit_arguments(id, &view, values, now);
                } else if let Some(ext) = self.extension_named(&extension) {
                    self.extensions[ext.0].send(ToExtension::SubmitForm { view, values });
                }
            }
            FromFrontEnd::ViewPopped { extension, view } => {
                if extension == OWN {
                    if view == problem::PROBLEMS {
                        self.problems_open.remove(&id);
                    }
                    self.take_argument_form(id, &view);
                    self.pick_popped(id, &view);
                } else if let Some(ext) = self.extension_named(&extension) {
                    self.forget_filters(ext, &view);
                    self.views.pop(ext, &view);
                    self.extensions[ext.0].send(ToExtension::ViewPopped { view });
                }
            }
            FromFrontEnd::FilterView { extension, view, text } if extension == OWN => self.filter_pick(id, &view, text),
            FromFrontEnd::FilterView { extension, view, text } => self.filter_view(id, &extension, &view, text),
            FromFrontEnd::Closed => self.closed(Some(id)),
            // The menu may open on the Empty query, which may have been
            // ranked hours ago.
            FromFrontEnd::Open { .. } | FromFrontEnd::Toggle => {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.asked_for_menu = true;
                }
                if let FromFrontEnd::Open { query, .. } = &message {
                    self.enter_on_open(query);
                }
                if self.menu(id).is_some() {
                    self.ask_menu(id, message, now);
                } else {
                    self.awaiting_menu.push((id, message, now + MENU_WAIT));
                }
            }
            FromFrontEnd::Undo { change } => self.undo(change),
            FromFrontEnd::Shown => {
                self.shown_last = Some(id);
                self.shown(id, now);
            }
        }
    }

    /// The menu `kotodama open` and `toggle` from `from` go to: of the Front
    /// ends that never asked for a menu themselves, the one shown last, else
    /// the one that connected last. One menu, not every one: a menu per
    /// screen, or two shells that each place one, would all open at once and
    /// fight over the keyboard, and the Daemon can't tell which screen has
    /// the focus without knowing the compositor.
    pub(super) fn menu(&self, from: FrontEndId) -> Option<FrontEndId> {
        let is_menu =
            |id: &FrontEndId| *id != from && self.sessions.get(id).is_some_and(|session| !session.asked_for_menu);
        self.shown_last
            .filter(is_menu)
            .or_else(|| self.sessions.keys().copied().filter(is_menu).max())
    }

    /// Passes `kotodama open` or `toggle` on to the menu, and tells the Front
    /// end that asked how many menus it reached: one, or none.
    pub(super) fn ask_menu(&mut self, from: FrontEndId, message: FromFrontEnd, now: Instant) {
        let menus = u32::from(self.menu(from).is_some());
        self.warm = true;
        self.refresh_empty(Update::Fresh, Some(from));
        match message {
            FromFrontEnd::Open { query, view } => {
                // Again, for a menu that connected since.
                self.enter_on_open(&query);
                match view {
                    Some(text) => self.open_on_view(from, query, &text, now),
                    None => self.ask_menus(from, ToFrontEnd::Open { query, view: None }),
                }
            }
            FromFrontEnd::Toggle => self.ask_menus(from, ToFrontEnd::Toggle),
            _ => return,
        }
        if let Some(session) = self.sessions.get(&from) {
            session.send(ToFrontEnd::Reached { menus });
        }
    }

    /// `kotodama open --app <word>` opens on the app: the query the menus
    /// send back starts with its word. A Front end that sends it for itself
    /// is in the app at once, whether a menu is there or not.
    pub(super) fn enter_on_open(&mut self, query: &str) {
        if let Some((n, _)) = self.apps.scope_of(query) {
            for session in self.sessions.values_mut() {
                session.entered = Some(n);
            }
        }
    }

    /// Passes on what waited for a menu, once one is there, and answers what
    /// waited too long that it reached none.
    pub(super) fn settle_awaiting_menu(&mut self, now: Instant) {
        if self.awaiting_menu.is_empty() {
            return;
        }
        for (from, message, until) in std::mem::take(&mut self.awaiting_menu) {
            let searched = |id: FrontEndId| self.sessions.get(&id).is_some_and(|s| s.searched);
            if self.menu(from).is_some_and(searched) {
                self.ask_menu(from, message, now);
            } else if until <= now {
                if let Some(session) = self.sessions.get(&from) {
                    session.send(ToFrontEnd::Reached { menus: 0 });
                }
            } else {
                self.awaiting_menu.push((from, message, until));
            }
        }
    }

    /// A Front end's menu became visible: the CPU cache is warmed for its
    /// first keystroke, as when `kotodama open` opens it, and its Empty query
    /// is ranked again if it was last ranked in another part of the day.
    pub(super) fn shown(&mut self, id: FrontEndId, now: Instant) {
        self.warm = true;
        let part = history::part_of_day(self.when(now));
        if let Some(session) = self.sessions.get_mut(&id)
            && session.searched
            && session.query.is_empty()
            && session.empty_ranked_in != Some(part)
        {
            session.update = Update::Fresh;
        }
    }

    /// Whether a late update added `result` or moved it up a row in a later
    /// version of the Results than `shown`, the one on screen when the user
    /// chose it: the user hasn't seen it where it is.
    pub(super) fn unseen(&self, front_end: FrontEndId, result: &ResultRef, shown: u32) -> bool {
        let Some(session) = self.sessions.get(&front_end) else {
            return false;
        };
        let Some(source) = self.source_of(session, &result.extension, &result.id) else {
            return false;
        };
        session
            .risen
            .iter()
            .any(|&(risen, version)| risen == source && version > shown)
    }

    /// Tells the Front end why an Action it asked for didn't run.
    pub(super) fn failed(&self, front_end: FrontEndId, run: Result<(), String>) {
        if let Err(message) = run
            && let Some(session) = self.sessions.get(&front_end)
        {
            session.send(ToFrontEnd::ActionFailed { message });
        }
    }

    /// The user filled in the form that asked for an Action's Arguments: the
    /// form goes, and the Action runs.
    pub(super) fn submit_arguments(&mut self, front_end: FrontEndId, view: &str, values: Table, now: Instant) {
        let Some(form) = self.take_argument_form(front_end, view) else {
            return;
        };
        if let Some(session) = self.sessions.get(&front_end) {
            session.send(ToFrontEnd::PopView {
                extension: OWN.to_owned(),
                view: view.to_owned(),
            });
        }
        let mut arguments = form.arguments;
        arguments.extend(arguments::submitted(values));
        let run = self.run_action(front_end, &form.result, &form.action, arguments, Missing::Fail, now);
        self.failed(front_end, run);
    }

    /// Removes the argument form `view`, if `front_end` is the one showing it.
    pub(super) fn take_argument_form(&mut self, front_end: FrontEndId, view: &str) -> Option<ArgumentForm> {
        if self
            .argument_forms
            .get(view)
            .is_some_and(|form| form.front_end == front_end)
        {
            self.argument_forms.remove(view)
        } else {
            None
        }
    }

    /// Passes `message` on to the menu `from` asks, or holds it back while
    /// the menus wait on a View.
    pub(super) fn ask_menus(&mut self, from: FrontEndId, message: ToFrontEnd) {
        if self.openings.iter().any(Opening::awaited) {
            self.held.push((from, message));
        } else {
            self.to_menu(from, message);
        }
    }

    /// Passes on what was held back, once no View is awaited.
    pub(super) fn release_held(&mut self) {
        if self.held.is_empty() || self.openings.iter().any(Opening::awaited) {
            return;
        }
        for (from, message) in std::mem::take(&mut self.held) {
            self.to_menu(from, message);
        }
    }

    pub(super) fn query(&mut self, id: FrontEndId, text: String, now: Instant) {
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        session.query = text;
        read_query(&self.apps, &mut self.matcher, session);
        let logged = (self.history.settings().usage_log
            && !session.query.is_empty()
            && !session.in_private_app(&self.apps))
        .then(|| session.query.clone());
        session.searched = true;
        session.update = Update::Fresh;
        session.typed = now;
        session.selection_moved = false;
        let searched = self.index.searched();
        for (ext_id, ext) in self.extensions.iter().enumerate() {
            let live = &mut session.live[ext_id];
            if let Some(query) = live.sent.take() {
                self.live_queries.remove(&query);
                ext.send(ToExtension::CancelLiveQuery { id: query });
            }
            *live = Live::default();
            if ext.sender().is_none() {
                continue;
            }
            if let (Some(debounce), Some((query, prefix))) =
                (ext.debounce(), session.live_query(ExtensionId(ext_id), ext, searched))
            {
                session.live[ext_id] = Live::asking(query, prefix, now + debounce);
            }
        }
        if let Some(query) = logged {
            self.store.log(Entry {
                at: self.wall(now),
                used: Used::Query { query },
            });
        }
        // A menu that just connected searches first; what waited for one
        // goes to it.
        self.settle_awaiting_menu(now);
    }
}
