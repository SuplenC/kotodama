//! Each Extension's run: starting its process, the handshake, and what
//! happens when it ends or the Extension directories change.
//!
//! # Owns
//!
//! - Starting, restarting with backoff, and retiring an Extension's process.
//! - Forgetting what a run sent once it ends.
//! - Taking in a new set of discovered Extensions.
//! - Showing how an Extension's job is doing.
//!
//! # Not here
//!
//! - What a running Extension sends: `from_extension.rs`.
//! - Finding the Extensions on disk: `crate::discovery`.

use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc::SyncSender;
use std::time::Instant;

use kotodama_protocol::ContributedTo;
use kotodama_protocol::Job;
use kotodama_protocol::JobState;
use kotodama_protocol::ToExtension;
use kotodama_protocol::ToFrontEnd;
use kotodama_protocol::ViewContent;
use kotodama_protocol::is_reserved_shortcut;

use super::Daemon;
use super::Desktop;
use super::Ended;
use super::Extension;
use super::ExtensionId;
use super::FIRST_RETRY;
use super::FrontEndId;
use super::HANDSHAKE_TIMEOUT;
use super::Launcher;
use super::Live;
use super::STABLE;
use super::State;
use super::Update;
use crate::config::extension_settings;
use crate::discovery::Found;
use crate::history::Store;

impl<L: Launcher, D: Desktop, S: Store> Daemon<L, D, S> {
    pub(super) fn start(&mut self, id: ExtensionId, now: Instant) {
        let ext = &mut self.extensions[id.0];
        if matches!(ext.state, State::Failed { .. }) {
            self.failed_changed = true;
            self.problems.dirty = true;
        }
        ext.generation += 1;
        ext.state = State::Starting {
            deadline: now + HANDSHAKE_TIMEOUT,
        };
        ext.catalog = false;
        ext.config = None;
        ext.hung.set(false);
        ext.stopping = false;
        self.launcher.start(id, ext.generation, &ext.found);
    }

    pub(super) fn extension_up(
        &mut self,
        id: ExtensionId,
        generation: u64,
        sender: SyncSender<ToExtension>,
        now: Instant,
    ) {
        let ext = &mut self.extensions[id.0];
        if ext.generation != generation || ext.stopping || !matches!(ext.state, State::Starting { .. }) {
            return;
        }
        let table = extension_settings(&self.config, ext.name(), ext.found.inline);
        ext.state = State::Up { sender, since: now };
        ext.ended = None;
        self.problems.dirty = true;
        ext.send(ToExtension::Config(table.clone()));
        ext.config = Some(table);

        self.ask_typed(id, now);
        self.extensions_changed();
    }

    /// Sends the Extension `id` whatever is typed already, as if just typed,
    /// if it takes Live queries.
    pub(super) fn ask_typed(&mut self, id: ExtensionId, now: Instant) {
        let ext = &self.extensions[id.0];
        let Some(debounce) = ext.asked() else {
            return;
        };
        let searched = self.index.searched();
        for session in self.sessions.values_mut() {
            if let Some((query, prefix)) = session.live_query(id, ext, searched) {
                session.live[id.0] = Live::asking(query, prefix, now + debounce);
            }
        }
    }

    pub(super) fn extension_down(&mut self, id: ExtensionId, ended: Ended, now: Instant) {
        let ext = &mut self.extensions[id.0];
        self.problems.dirty = true;
        let how = match ended {
            Ended::CantStart(reason) | Ended::Refused(reason) => {
                log::warn!(
                    "extension {}: {reason}; it starts again once its program or Manifest changes",
                    ext.name()
                );
                ext.state = State::Failed {
                    stamp: ext.found.stamp(),
                    reason,
                };
                ext.ended = None;
                self.failed_changed = true;
                self.forget_run(id);
                return;
            }
            // Stopped for not answering, its exit status is the Daemon's
            // doing.
            Ended::Stopped(_) if ext.hung.get() => "stopped answering".to_owned(),
            Ended::Stopped(how) => format!("stopped ({how})"),
        };
        if let State::Up { since, .. } = ext.state
            && now.duration_since(since) >= STABLE
        {
            ext.failures = 0;
        }
        let delay = FIRST_RETRY
            .saturating_mul(1 << ext.failures.min(16))
            .min(self.engine.retry_max);
        ext.failures = ext.failures.saturating_add(1);
        let give_up_after = self.engine.give_up_after;
        if give_up_after > 0 && ext.failures >= give_up_after {
            let reason = format!("{how}, {} times in a row", ext.failures);
            log::warn!(
                "extension {}: {reason}; it starts again once its program or Manifest changes",
                ext.name()
            );
            ext.state = State::Failed {
                stamp: ext.found.stamp(),
                reason,
            };
            ext.ended = None;
            self.failed_changed = true;
            self.forget_run(id);
            return;
        }
        ext.state = State::Down { retry_at: now + delay };
        log::warn!("extension {} {how}; restarting in {delay:?}", ext.name());
        ext.ended = Some(how);
        self.forget_run(id);
    }

    /// Starts each Extension that couldn't start whose program changed
    /// since.
    pub(super) fn programs_changed(&mut self, now: Instant) {
        for id in 0..self.extensions.len() {
            let ext = &mut self.extensions[id];
            if let State::Failed { stamp, .. } = &ext.state
                && *stamp != ext.found.stamp()
            {
                log::info!("extension {}'s program changed; starting it", ext.name());
                ext.failures = 0;
                self.start(ExtensionId(id), now);
            }
        }
    }

    /// The directories of the programs of the Extensions that can't start,
    /// to watch for a change that may let them, when that changed since it
    /// last said.
    pub fn failed_programs(&mut self) -> Option<Vec<PathBuf>> {
        if !std::mem::take(&mut self.failed_changed) {
            return None;
        }
        let mut dirs: Vec<PathBuf> = (self.extensions.iter())
            .filter(|ext| matches!(ext.state, State::Failed { .. }))
            .filter_map(|ext| ext.found.program().parent().map(Path::to_owned))
            .filter(|dir| dir.is_absolute())
            .collect();
        dirs.sort();
        dirs.dedup();
        Some(dirs)
    }

    /// Drops everything one run of an Extension left behind: its Items, Live
    /// results and jobs, and the Actions it was running, which fail.
    /// Shows every Front end how a job is doing. A running job is kept, so a
    /// menu opened later shows it; a finished one is announced on the desktop,
    /// then forgotten.
    pub(super) fn job_progress(&mut self, id: ExtensionId, name: &str, job: Job) {
        self.broadcast(|| ToFrontEnd::JobProgress {
            extension: name.to_owned(),
            job: job.clone(),
        });
        if job.state == JobState::Running {
            self.jobs.insert((id, job.id.clone()), job);
        } else {
            self.jobs.remove(&(id, job.id.clone()));
            let body = job.message.unwrap_or_else(|| {
                match job.state {
                    JobState::Failed => "Failed",
                    _ => "Done",
                }
                .to_owned()
            });
            self.desktop.notify(job.title, body);
        }
    }

    pub(super) fn forget_run(&mut self, id: ExtensionId) {
        let name = self.extensions[id.0].name().to_owned();
        self.said(id, |ext| {
            ext.status = None;
            ext.has_items = false;
        });
        for alias in self.keywords.aliases.values() {
            if alias.extension == name
                && let Some(slot) = self.index.find(id, &alias.item)
                && let Some(item) = self.index.item(slot)
            {
                self.last_seen.insert((name.clone(), alias.item.clone()), item);
            }
        }
        let cleared = self.index.clear(id);
        if !cleared.is_empty() {
            self.repin_index();
            self.mark_late();
        }
        if let Some(indexer) = &mut self.indexer {
            self.stale[id.0] = indexer.clear(id);
            indexer.free(cleared);
        } else if self.index.settle() {
            // The placeholders its Items stood in for come back. On the
            // indexing thread they come back in a version of their own.
            self.repin_index();
            self.mark_late();
        }
        self.live_queries.retain(|_, (_, ext)| *ext != id);
        // Its running jobs will never finish.
        let stopped: Vec<String> = self
            .jobs
            .keys()
            .filter(|(ext, _)| *ext == id)
            .map(|(_, job)| job.clone())
            .collect();
        for job in stopped {
            if let Some(mut job) = self.jobs.remove(&(id, job)) {
                job.state = JobState::Failed;
                job.message = Some(format!("{name} stopped"));
                self.job_progress(id, &name, job);
            }
        }
        self.hidden.remove(&id);
        self.targets.forget(id);
        // Its Views can't be answered any more.
        for session in self.sessions.values_mut() {
            session.filters.retain(|(ext, _, _)| *ext != id);
        }
        for view in self.views.pop_all(id) {
            self.broadcast(|| ToFrontEnd::PopView {
                extension: name.clone(),
                view: view.clone(),
            });
        }
        // Its Items can't be picked any more.
        let picking: Vec<FrontEndId> = self
            .picks
            .iter()
            .filter(|(_, pick)| pick.target == id)
            .map(|(&front_end, _)| front_end)
            .collect();
        for front_end in picking {
            self.cancel_pick(front_end);
        }
        for session in self.sessions.values_mut() {
            let live = &mut session.live[id.0];
            if !live.results.is_empty() {
                session.update = session.update.max(Update::Late);
            }
            *live = Live::default();
        }
        let sessions = &self.sessions;
        self.invocations.retain(|_, (front_end, ext)| {
            if *ext == id
                && let Some(session) = sessions.get(front_end)
            {
                session.send(ToFrontEnd::ActionFailed {
                    message: format!("{name} stopped before the Action finished"),
                });
            }
            *ext != id
        });
        // A View it was asked to open will never come.
        let invocations = &self.invocations;
        let (stopped, waiting) = std::mem::take(&mut self.openings).into_iter().partition(|opening| {
            opening
                .running
                .is_some_and(|running| !invocations.contains_key(&running))
        });
        self.openings = waiting;
        for opening in stopped {
            let failure = format!("{name} stopped before its View opened");
            self.open_menus(&opening, Some(failure));
        }
        self.extensions_changed();
    }

    /// The set of running Extensions changed.
    pub(super) fn extensions_changed(&mut self) {
        // The Contributed actions on the open list Views, to tell the menus
        // of those that change.
        let before: Vec<_> = self
            .views
            .iter()
            .filter(|open| matches!(open.view.content, ViewContent::List { .. }))
            .map(|open| {
                let contributed = self.view_contributed(open.extension, &open.view);
                (open.extension, open.view.id.clone(), contributed)
            })
            .collect();
        self.contributed.clear();
        let mut targeted = Vec::new();
        for (id, ext) in self.extensions.iter().enumerate() {
            if ext.sender().is_none() {
                continue;
            }
            for contributed in &ext.found.manifest.contributed_actions {
                let mut action = contributed.action.clone();
                // A Manifest problem, reported when it was read.
                action.shortcut.take_if(|shortcut| is_reserved_shortcut(shortcut));
                match &contributed.to {
                    ContributedTo::Kind(kind) => self
                        .contributed
                        .entry(kind.as_str().to_owned())
                        .or_default()
                        .push((ExtensionId(id), action)),
                    ContributedTo::Identities => targeted.push((ExtensionId(id), action)),
                }
            }
        }
        self.targets.set_offered(targeted);
        self.verbs_from = None;
        self.catalog_dirty = true;
        self.mark_late();
        for (id, view_id, contributed) in before {
            let Some(open) = self.views.get(id, &view_id) else {
                continue;
            };
            if self.view_contributed(id, &open.view) != contributed {
                let view = open.view.clone();
                let name = self.extensions[id.0].name().to_owned();
                self.update_view(id, &name, &view);
            }
        }
    }

    pub(super) fn extension_named(&self, name: &str) -> Option<ExtensionId> {
        self.extensions
            .iter()
            .position(|ext| ext.name() == name && !matches!(ext.state, State::Removed))
            .map(ExtensionId)
    }

    /// Brings the running Extensions in line with what is installed: new ones
    /// start, removed ones stop for good, and one whose Manifest changed
    /// restarts at once with the new Manifest.
    pub(super) fn set_extensions(&mut self, found: Vec<Found>, now: Instant) {
        for id in 0..self.extensions.len() {
            let ext = &self.extensions[id];
            if !matches!(ext.state, State::Removed) && !found.iter().any(|f| f.manifest.name == ext.name()) {
                log::info!("extension {} was removed or switched off", ext.name());
                self.retire(ExtensionId(id));
            }
        }
        for found in found {
            if let Some(id) = self.extensions.iter().position(|ext| ext.name() == found.manifest.name) {
                let ext = &mut self.extensions[id];
                let removed = matches!(ext.state, State::Removed);
                if !removed
                    && ext.found.manifest == found.manifest
                    && ext.found.dir == found.dir
                    && ext.found.env == found.env
                {
                    continue;
                }
                log::info!("extension {} changed; restarting it", found.manifest.name);
                ext.set_found(found);
                ext.failures = 0;
                if !removed {
                    self.retire(ExtensionId(id));
                }
                self.start(ExtensionId(id), now);
            } else {
                log::info!("extension {} was installed", found.manifest.name);
                let id = ExtensionId(self.extensions.len());
                self.extensions.push(Extension::new(found, now));
                self.index.add_extension();
                self.stale.push(0);
                for session in self.sessions.values_mut() {
                    session.live.push(Live::default());
                }
                self.start(id, now);
            }
        }
        self.load_apps();
    }

    /// Ends the current run of an Extension, leaving it removed until started
    /// again: stops its process and forgets what it left behind. Moving on to the next generation
    /// means nothing from the old run, its `ExtensionDown` included, is heard any more.
    pub(super) fn retire(&mut self, id: ExtensionId) {
        let ext = &mut self.extensions[id.0];
        match ext.state {
            State::Starting { .. } | State::Up { .. } => self.launcher.stop(id, ext.generation),
            State::Failed { .. } => self.failed_changed = true,
            State::Down { .. } | State::Removed => {}
        }
        ext.ended = None;
        self.problems.dirty = true;
        ext.generation += 1;
        ext.state = State::Removed;
        self.forget_run(id);
    }
}
