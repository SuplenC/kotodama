//! What a running Extension sends: Items, Views, Live query Results and the
//! outcome of an Action.
//!
//! # Owns
//!
//! - Taking in Items and their edits, and the index's answer once they land.
//! - Views an Extension updates, and the filters typed into them.
//! - Live query Results, and the outcome of each Action the Daemon ran.
//!
//! # Not here
//!
//! - Starting and stopping the Extension: `extensions.rs`.
//! - Ranking what it sent: `search.rs`.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::time::Duration;
use std::time::Instant;

use kotodama_protocol::ActionRef;
use kotodama_protocol::FromExtension;
use kotodama_protocol::Item;
use kotodama_protocol::LiveResult;
use kotodama_protocol::OfferedAction;
use kotodama_protocol::Outcome;
use kotodama_protocol::ToExtension;
use kotodama_protocol::ToFrontEnd;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;

use super::Daemon;
use super::Desktop;
use super::Extension;
use super::ExtensionId;
use super::FrontEndId;
use super::Launcher;
use super::MAX_LIVE_RESULTS;
use super::Output;
use super::Status;
use super::Update;
use crate::history::Store;
use crate::indexer::Indexed;
use crate::reserved;
use crate::segment::Edit;
use crate::views::Open;

impl<L: Launcher, D: Desktop, S: Store> Daemon<L, D, S> {
    pub(super) fn extension_message(&mut self, id: ExtensionId, mut message: FromExtension, now: Instant) {
        let name = self.extensions[id.0].name().to_owned();
        let released = reserved::release_message(&mut message);
        if released > 0 {
            log::warn!("{name}: dropped {released} key bindings the menu keeps for itself");
        }
        match message {
            FromExtension::AddItems { items } => self.add_items(id, &name, items, now),
            FromExtension::RemoveItems { ids } => {
                if let Some(hidden) = self.hidden.get_mut(&id) {
                    for item in &ids {
                        hidden.remove(item);
                    }
                }
                self.edit_items(id, Edit::Remove(ids), now);
            }
            FromExtension::LiveResults {
                live_query,
                results,
                done,
            } => self.live_results(id, live_query, results, done),
            FromExtension::ActionDone { invocation, outcome } => self.action_done(id, invocation, outcome, now),
            FromExtension::WithdrawPick { invocation } => self.withdraw_pick(id, invocation),
            FromExtension::Status { usable, note } => {
                match (usable, &note) {
                    (true, _) => log::debug!("{name} can do something"),
                    (false, Some(note)) => log::info!("{name} can't do anything now: {note}"),
                    (false, None) => log::info!("{name} can't do anything now"),
                }
                self.said(id, |ext| ext.status = Some(Status { usable, note }));
            }
            FromExtension::PushView(view) => {
                let contributed = self.view_contributed(id, &view);
                self.forget_filters(id, &view.id);
                self.views.push(id, view.clone());
                self.broadcast(|| ToFrontEnd::PushView {
                    extension: name.clone(),
                    view: view.clone(),
                    contributed: contributed.clone(),
                });
            }
            FromExtension::UpdateView(view) => self.update_view(id, &name, &view),
            FromExtension::PopView { view } => {
                self.forget_filters(id, &view);
                self.views.pop(id, &view);
                self.broadcast(|| ToFrontEnd::PopView {
                    extension: name.clone(),
                    view: view.clone(),
                });
            }
            FromExtension::ReplaceView { view, with } => {
                let contributed = self.view_contributed(id, &with);
                self.forget_filters(id, &view);
                self.forget_filters(id, &with.id);
                self.views.pop(id, &view);
                self.views.push(id, with.clone());
                self.broadcast(|| ToFrontEnd::ReplaceView {
                    extension: name.clone(),
                    view: view.clone(),
                    with: with.clone(),
                    contributed: contributed.clone(),
                });
            }
            FromExtension::JobProgress(job) => self.job_progress(id, &name, job),
            FromExtension::RequestActionCatalog => self.send_catalog(id, &name),
        }
    }

    /// Adds or replaces the Items of the Extension `id`, called `name`, but
    /// for those of another Extension's kind, which it is told it may not send.
    pub(super) fn add_items(&mut self, id: ExtensionId, name: &str, mut items: Vec<Item>, now: Instant) {
        let ext = &self.extensions[id.0];
        items.retain(|item| {
            let theirs = item.kind.extension().is_some_and(|owner| owner != name);
            if theirs {
                let reason = format!(
                    "Item `{}` has another Extension's kind `{}`; name your own kinds `{name}.<kind>`",
                    item.id,
                    item.kind.name()
                );
                log::warn!("{name}: {reason}");
                ext.send(ToExtension::Rejected { reason });
            }
            !theirs
        });
        // A hidden Item is kept out of the index: the Segment drops
        // it, and the Daemon keeps it for `kotodama open --view`.
        let hidden = self.hidden.entry(id).or_default();
        for item in &items {
            if item.hidden {
                hidden.insert(item.id.clone(), item.clone());
            } else {
                hidden.remove(&item.id);
            }
        }
        self.edit_items(id, Edit::Add(items), now);
    }

    /// Sends the Action catalog to the Extension `id`, called `name`, and
    /// keeps it up to date from then on, if its Manifest asks for it.
    pub(super) fn send_catalog(&mut self, id: ExtensionId, name: &str) {
        let ext = &mut self.extensions[id.0];
        if !ext.found.manifest.action_catalog {
            log::warn!("{name} asked for the Action catalog, but its Manifest doesn't");
            ext.send(ToExtension::Rejected {
                reason: "asked for the Action catalog without `action_catalog = true` in the Manifest".into(),
            });
            return;
        }
        ext.catalog = true;
        let catalog = self.compute_catalog();
        self.extensions[id.0].send(ToExtension::ActionCatalog {
            entries: catalog.clone(),
        });
        self.catalog = Some(catalog);
    }

    /// Applies `change` to what the Extension `id` said of itself. The
    /// search shows its app's entry Item from then on only if it is usable,
    /// decided here rather than per keystroke, and the app's intro card
    /// shows its note.
    pub(super) fn said(&mut self, id: ExtensionId, change: impl FnOnce(&mut Extension)) {
        let ext = &mut self.extensions[id.0];
        let (usable, note) = (ext.usable(), ext.note().map(str::to_owned));
        change(ext);
        if ext.usable() != usable {
            self.build_own();
        } else if ext.note() != note.as_deref() {
            self.mark_late();
        }
    }

    /// Changes an Extension's Items: here and now, or on the indexing
    /// thread, whose new version comes back as an [`Event::Indexed`].
    pub(super) fn edit_items(&mut self, id: ExtensionId, edit: Edit, now: Instant) {
        if let Some(indexer) = &mut self.indexer {
            indexer.edit(id, edit);
        } else {
            self.index.edit(id, edit);
            self.repin_index();
            self.items_changed();
            self.items_landed(id);
            self.run_openings(now);
        }
    }

    /// Puts in a new version of an Extension's Items, unless they were
    /// cleared since.
    pub(super) fn indexed(&mut self, indexed: Indexed, now: Instant) {
        let Indexed {
            extension,
            change,
            segments,
            reads,
        } = indexed;
        if change <= self.stale[extension.0] {
            return;
        }
        for (at, segment) in segments {
            self.index.replace(at, segment);
        }
        self.index.keep(reads);
        self.repin_index();
        self.items_changed();
        self.items_landed(extension);
        self.run_openings(now);
    }

    /// Notes that an Extension's Items are in, as its app's entry Item shows
    /// from then on, in the same Results as they do. A hidden Item counts:
    /// it names a View the app opens.
    pub(super) fn items_landed(&mut self, id: ExtensionId) {
        if self.extensions[id.0].has_items {
            return;
        }
        let has_items =
            !self.index.first(id, 1).is_empty() || self.hidden.get(&id).is_some_and(|hidden| !hidden.is_empty());
        if has_items {
            self.said(id, |ext| ext.has_items = true);
        }
    }

    /// Replaces an open View. Each menu filtering a list View is sent the
    /// new Items that match its filter along with it.
    pub(super) fn update_view(&mut self, id: ExtensionId, name: &str, view: &View) {
        let contributed = self.view_contributed(id, view);
        let view_id = view.id.clone();
        let filtered =
            self.views.update(id, view.clone()) && self.views.get(id, &view_id).is_some_and(Open::filtered_here);
        if !filtered {
            self.forget_filters(id, &view_id);
        }
        let ids: Vec<FrontEndId> = self.sessions.keys().copied().collect();
        for front_end in ids {
            let Some(session) = self.sessions.get(&front_end) else {
                continue;
            };
            session.send(ToFrontEnd::UpdateView {
                extension: name.to_owned(),
                view: view.clone(),
                contributed: contributed.clone(),
            });
            if let Some(text) = session.filter(id, &view_id).map(str::to_owned) {
                self.send_filtered(front_end, id, &view_id, text);
            }
        }
    }

    /// No menu filters the View any more: it was pushed afresh or popped.
    pub(super) fn forget_filters(&mut self, id: ExtensionId, view: &str) {
        for session in self.sessions.values_mut() {
            session.set_filter(id, view, String::new());
        }
    }

    /// A menu typed `text` into one of an Extension's list Views. The Daemon
    /// filters it and answers, unless the Extension filters it itself.
    pub(super) fn filter_view(&mut self, front_end: FrontEndId, extension: &str, view: &str, text: String) {
        let Some(id) = self.extension_named(extension) else {
            return;
        };
        let Some(open) = self.views.get(id, view) else {
            return;
        };
        if !matches!(open.view.content, ViewContent::List { .. }) {
            return;
        }
        if open.filters_itself() {
            self.extensions[id.0].send(ToExtension::FilterView {
                view: view.to_owned(),
                text,
            });
            return;
        }
        let Some(session) = self.sessions.get_mut(&front_end) else {
            return;
        };
        session.set_filter(id, view, text.clone());
        if !text.is_empty() {
            self.send_filtered(front_end, id, view, text);
        }
    }

    pub(super) fn send_filtered(&mut self, front_end: FrontEndId, id: ExtensionId, view: &str, text: String) {
        let matches = self.views.filter(id, view, &text, &mut self.matcher, &mut self.tiers);
        if let Some(session) = self.sessions.get(&front_end) {
            session.send(ToFrontEnd::FilteredView {
                extension: self.extensions[id.0].name().to_owned(),
                view: view.to_owned(),
                text,
                matches,
            });
        }
    }

    /// The Contributed actions on a list View's Items, by the Item kinds it
    /// has. Other Views have none.
    pub(super) fn view_contributed(&self, owner: ExtensionId, view: &View) -> BTreeMap<String, Vec<OfferedAction>> {
        let mut by_kind = BTreeMap::new();
        let ViewContent::List { items, .. } = &view.content else {
            return by_kind;
        };
        let mut seen: HashSet<&str> = HashSet::new();
        for item in items {
            let kind = item.kind.name();
            if seen.insert(kind) {
                let offered = self.contributed_for(owner, kind, &[]);
                if !offered.is_empty() {
                    by_kind.insert(kind.to_owned(), offered);
                }
            }
        }
        by_kind
    }

    pub(super) fn items_changed(&mut self) {
        self.catalog_dirty = true;
        self.mark_late();
    }

    pub(super) fn mark_late(&mut self) {
        self.ranks_version += 1;
        for session in self.sessions.values_mut() {
            if session.searched {
                session.update = session.update.max(Update::Late);
            }
        }
    }

    /// Ranks the Empty query again in every session showing it but `except`,
    /// for what the History learned or the time of day now: from scratch
    /// for a menu opening, and otherwise as a late update, which keeps the
    /// rows on screen where they are.
    pub(super) fn refresh_empty(&mut self, update: Update, except: Option<FrontEndId>) {
        for (&id, session) in &mut self.sessions {
            if session.searched && session.query.is_empty() && Some(id) != except {
                session.update = session.update.max(update);
            }
        }
    }

    pub(super) fn live_results(&mut self, id: ExtensionId, live_query: u64, results: Vec<LiveResult>, done: bool) {
        let Some(&(front_end, ext)) = self.live_queries.get(&live_query) else {
            return;
        };
        if ext != id {
            return;
        }
        let answers = self.extensions[id.0].debounce() == Some(Duration::ZERO);
        let Some(session) = self.sessions.get_mut(&front_end) else {
            return;
        };
        let live = &mut session.live[id.0];
        // Nothing to show changes the rows on screen, so they aren't sent again.
        let changed = !results.is_empty();
        let room = MAX_LIVE_RESULTS.saturating_sub(live.results.len());
        if results.len() > room {
            log::warn!(
                "{}: dropping Live results past {MAX_LIVE_RESULTS} for one query",
                self.extensions[id.0].name()
            );
        }
        live.results
            .extend(results.into_iter().take(room).map(|result| LiveResult {
                answer: result.answer && answers,
                ..result
            }));
        if done {
            live.sent = None;
            self.live_queries.remove(&live_query);
        }
        if changed {
            session.update = session.update.max(Update::Late);
        }
    }

    pub(super) fn action_done(&mut self, id: ExtensionId, invocation: u64, outcome: Outcome, now: Instant) {
        if self.invocations.get(&invocation).map(|&(_, ext)| ext) != Some(id) {
            return;
        }
        let Some((front_end, _)) = self.invocations.remove(&invocation) else {
            return;
        };
        // Run for `kotodama open --view`, it answers the menus rather than
        // the command; what it types or copies goes out as for any Action.
        if let Some(n) = self
            .openings
            .iter()
            .position(|opening| opening.running == Some(invocation))
        {
            let opening = self.openings.swap_remove(n);
            match outcome {
                Outcome::KeepOpen => return self.open_menus(&opening, None),
                Outcome::Failed { message } => return self.open_menus(&opening, Some(message)),
                // Closes a menu that was open already, or opened late.
                Outcome::Close => return self.broadcast(|| ToFrontEnd::Close),
                _ => {}
            }
        }
        let session = self.sessions.get(&front_end);
        match outcome {
            Outcome::Close => {
                if let Some(session) = session {
                    session.send(ToFrontEnd::Close);
                }
            }
            Outcome::KeepOpen => {}
            Outcome::Failed { message } => {
                if let Some(session) = session {
                    session.send(ToFrontEnd::ActionFailed { message });
                }
            }
            Outcome::Type { text } => self.output(Output::Type(text), now),
            Outcome::Paste(clipboard) => self.output(Output::Paste(clipboard), now),
            Outcome::Copy(clipboard) => self.copy(clipboard),
            Outcome::RunCatalogAction {
                extension,
                kind,
                action,
                arguments,
            } => {
                let action = ActionRef { extension, id: action };
                let run = self.run_catalog_action(front_end, id, &kind, action, arguments, now);
                self.failed(front_end, run);
            }
        }
    }
}
