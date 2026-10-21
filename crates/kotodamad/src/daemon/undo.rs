//! Undoing a change to the History, while the offer lasts.
//!
//! # Owns
//!
//! - Saving a change and offering every Front end to undo it.
//! - Undoing it, or letting it expire.
//! - The Undo Result the search shows while one can be undone.
//!
//! # Not here
//!
//! - What changes the History: `picks.rs`.

use std::time::Instant;

use kotodama_protocol::ToFrontEnd;

use super::Daemon;
use super::Desktop;
use super::Done;
use super::ExtensionId;
use super::Launcher;
use super::Own;
use super::OwnAction;
use super::Undoable;
use super::UndoneBy;
use super::Update;
use super::undo_item;
use crate::history::Change;
use crate::history::Store;

impl<L: Launcher, D: Desktop, S: Store> Daemon<L, D, S> {
    /// Saves a change to the History, which can be undone for `[history]
    /// undo_seconds`,
    /// offers every Front end to undo it, and ranks every session again. A
    /// change of nothing is let go.
    pub(super) fn changed(&mut self, done: Done, change: Change, now: Instant) {
        if change.is_empty() {
            return;
        }
        self.store.save(&self.history);
        let id = self.next_id;
        self.next_id += 1;
        let undoable = Undoable {
            id,
            by: UndoneBy::User(done),
            change,
            until: now + self.history.settings().undo_for(),
        };
        if let Some(offer) = undoable.offer() {
            self.broadcast(|| offer.clone());
        }
        self.undos.push(undoable);
        self.load_undo_item();
    }

    /// The latest change to the History the user may still undo.
    pub(super) fn latest_undo(&self) -> Option<&Undoable> {
        self.undos.iter().rev().find(|undo| undo.done().is_some())
    }

    /// Undoes the change `id` to the History, if the user may still undo it.
    pub(super) fn undo(&mut self, id: u64) {
        let Some(n) = self
            .undos
            .iter()
            .position(|undo| undo.id == id && undo.done().is_some())
        else {
            return;
        };
        let undo = self.undos.remove(n);
        self.history.undo(undo.change);
        self.store.save(&self.history);
        self.broadcast(|| ToFrontEnd::WithdrawUndo { change: id });
        self.load_undo_item();
    }

    /// Withdraws the pick that running the Action `invocation` made, if
    /// `ext` ran it and it can still be undone.
    pub(super) fn withdraw_pick(&mut self, ext: ExtensionId, invocation: u64) {
        let Some(n) = self
            .undos
            .iter()
            .position(|undo| undo.id == invocation && matches!(undo.by, UndoneBy::Extension(by) if by == ext))
        else {
            log::debug!(
                "{}: no pick of run {invocation} to withdraw",
                self.extensions[ext.0].name()
            );
            return;
        };
        let undo = self.undos.remove(n);
        self.history.undo(undo.change);
        self.store.save(&self.history);
        self.refresh_empty(Update::Late, None);
    }

    /// Lets go of the changes to the History whose time to be undone ran
    /// out, and withdraws the offers to undo them.
    pub(super) fn expire_undos(&mut self, now: Instant) {
        if self.undos.first().is_none_or(|undo| undo.until > now) {
            return;
        }
        let (expired, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.undos)
            .into_iter()
            .partition(|undo| undo.until <= now);
        self.undos = kept;
        let mut offered = false;
        for undo in expired.iter().filter(|undo| undo.done().is_some()) {
            self.broadcast(|| ToFrontEnd::WithdrawUndo { change: undo.id });
            offered = true;
        }
        if offered {
            self.load_undo_item();
        }
    }

    /// Shows the Undo Item for the latest change to the History, or hides it
    /// when there is nothing left to undo, and ranks every session again.
    pub(super) fn load_undo_item(&mut self) {
        if self.own.last().is_some_and(|own| matches!(own.does, OwnAction::Undo)) {
            self.own.pop();
        }
        if let Some(done) = self.latest_undo().and_then(Undoable::done) {
            self.own.push(Own::new(undo_item(&done.undo), OwnAction::Undo));
        }
        self.mark_late();
    }
}
