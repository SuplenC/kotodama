//! Opening the menu on a View, as `kotodama open --view` asks.
//!
//! # Owns
//!
//! - Waiting for the Extension that owns the View, and giving up after a while.
//! - Telling the menus what opened, or why it didn't.
//!
//! # Not here
//!
//! - Parsing which View is asked for: `crate::open_view`.
//! - The menu's session: `front_ends.rs`.

use std::collections::BTreeMap;
use std::time::Instant;

use kotodama_protocol::ToExtension;
use kotodama_protocol::ToFrontEnd;

use super::Daemon;
use super::Desktop;
use super::FrontEndId;
use super::Launcher;
use super::OPEN_VIEW_TIMEOUT;
use super::Session;
use super::State;
use super::after_keyword;
use crate::arguments;
use crate::history::Store;
use crate::index::ItemRef;
use crate::open_view::Opening;
use crate::open_view::ViewName;

impl<L: Launcher, D: Desktop, S: Store> Daemon<L, D, S> {
    /// `kotodama open --view <text>`: the menus wait until the View is there.
    pub(super) fn open_on_view(&mut self, from: FrontEndId, query: String, text: &str, now: Instant) {
        let Some(view) = ViewName::parse(text) else {
            let failure = format!("No View `{text}`; name one as <extension>/<item>");
            self.to_menu(
                from,
                ToFrontEnd::Open {
                    query,
                    view: Some(text.to_owned()),
                },
            );
            self.to_menu(from, ToFrontEnd::ActionFailed { message: failure });
            return;
        };
        self.openings.push(Opening {
            from,
            query,
            view,
            running: None,
            opened: false,
            deadline: now + OPEN_VIEW_TIMEOUT,
        });
        self.run_openings(now);
    }

    /// Runs the primary Action of each Item `kotodama open --view` waits on,
    /// once its Extension has indexed it. One that can't run opens the menus
    /// on the search, saying why. Not recorded as a pick: it wasn't picked
    /// from a search.
    pub(super) fn run_openings(&mut self, now: Instant) {
        let mut n = 0;
        while n < self.openings.len() {
            if self.openings[n].running.is_some() {
                n += 1;
                continue;
            }
            match self.run_opening(n, now) {
                Ok(running) => {
                    self.openings[n].running = running;
                    n += 1;
                }
                Err(message) => {
                    let opening = self.openings.remove(n);
                    self.open_menus(&opening, Some(message));
                }
            }
        }
    }

    /// Runs the primary Action of the `n`th Opening's Item: its invocation,
    /// or `None` while the Extension starts, or started so recently that the
    /// Item may still be on its way.
    pub(super) fn run_opening(&mut self, n: usize, now: Instant) -> Result<Option<u64>, String> {
        let opening = &self.openings[n];
        let view = &opening.view;
        let no_view = || format!("No View `{view}`; name one as <extension>/<item>");
        let ext_id = self.extension_named(&view.extension).ok_or_else(no_view)?;
        let ext = &self.extensions[ext_id.0];
        let State::Up { since, .. } = ext.state else {
            return Ok(None);
        };
        let Some(item) = self
            .index
            .find(ext_id, &view.item)
            .and_then(|slot| self.index.item(slot))
            .map(ItemRef::Resident)
            .or_else(|| {
                let hidden = self.hidden.get(&ext_id)?.get(&view.item)?;
                Some(ItemRef::Borrowed(hidden))
            })
        else {
            return if now.duration_since(since) < OPEN_VIEW_TIMEOUT {
                Ok(None)
            } else {
                Err(no_view())
            };
        };
        let primary = item.actions.first().ok_or_else(no_view)?;
        if let Some(absent) = arguments::missing(&primary.arguments, &BTreeMap::new()) {
            return Err(format!("`{}` needs {}", primary.title, absent.name));
        }
        let invocation = self.next_id;
        if !ext.send(ToExtension::RunAction {
            invocation,
            action: primary.id.clone(),
            item: item.to_ref(),
            arguments: BTreeMap::new(),
        }) {
            return Err(format!("{} stopped answering", ext.name()));
        }
        let from = opening.from;
        self.next_id += 1;
        self.invocations.insert(invocation, (from, ext_id));
        Ok(Some(invocation))
    }

    /// Opens the menus for `kotodama open --view`, unless they opened when it
    /// ran late, and shows `failure` if there is one.
    pub(super) fn open_menus(&self, opening: &Opening, failure: Option<String>) {
        if !opening.opened {
            self.to_menu(
                opening.from,
                ToFrontEnd::Open {
                    query: opening.query.clone(),
                    view: Some(opening.view.to_string()),
                },
            );
        }
        if let Some(message) = failure {
            self.to_menu(opening.from, ToFrontEnd::ActionFailed { message });
        }
    }

    /// Opens the menus on the search for each `kotodama open --view` that has
    /// waited long enough. An Action still running is still followed: its
    /// View goes on top when it comes, and the rest of what it does counts.
    pub(super) fn open_overdue(&mut self, now: Instant) {
        let mut n = 0;
        while n < self.openings.len() {
            let opening = &mut self.openings[n];
            if !opening.awaited() || opening.deadline > now {
                n += 1;
            } else if opening.running.is_some() {
                self.open_menus(&self.openings[n], None);
                self.openings[n].opened = true;
                n += 1;
            } else {
                let opening = self.openings.remove(n);
                let failure = format!("No View `{}` turned up", opening.view);
                self.open_menus(&opening, Some(failure));
            }
        }
    }

    /// What the session's query has after the `n`th app's word, if it starts
    /// with it: what entering the app keeps typed.
    pub(super) fn after_word<'q>(&self, session: &'q Session, n: usize) -> Option<&'q str> {
        after_keyword(&session.query, &self.apps.get(n)?.word)
    }
}
