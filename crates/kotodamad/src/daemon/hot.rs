//! The hot Items and the Empty query ranked ahead, so a menu opening or a
//! keyword typed never waits for a read.
//!
//! # Owns
//!
//! - Planning which Items stay resident and the Empty query's most used, on the indexing thread
//!   when there is one.
//! - Keeping resident every Item a keyword or an Alias names.
//!
//! # Not here
//!
//! - Keeping the Items and their payloads: `crate::index` and `crate::payload`.
//! - What the Usage counts: `crate::most_used`.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use super::Daemon;
use super::Desktop;
use super::ExtensionId;
use super::HOT_EVERY;
use super::Launcher;
use crate::history::IDENTITIES;
use crate::history::Store;
use crate::index::Slot;
use crate::indexer::Plan;
use crate::most_used::Usage;
use crate::most_used::Versions;

impl<L: Launcher, D: Desktop, S: Store> Daemon<L, D, S> {
    /// Those of `slots` that are resident, by Extension name and id.
    pub(super) fn resident(&self, slots: &[Slot]) -> Vec<(String, String)> {
        slots
            .iter()
            .filter(|&&slot| self.index.is_resident(slot))
            .filter_map(|&slot| {
                let extension = self.extensions[slot.extension().0].name();
                Some((extension.to_owned(), self.index.id(slot)?.to_owned()))
            })
            .collect()
    }

    /// Whether the hot Items are planned, and the Empty query ranked ahead.
    pub(super) fn plans(&self) -> bool {
        self.hot_capacity > 0 && !self.history.is_empty()
    }

    /// The versions of what the Empty query and the hot Items are worked
    /// out from: the Index, the History, and the rest (`ranks_version`).
    pub(super) fn versions(&self) -> Versions {
        (self.index.version(), self.history.version(), self.ranks_version)
    }

    /// Works out the hot Items, up to the hot capacity, and reads their
    /// payloads, so the menu shows them without waiting; and ranks the
    /// Empty query's most used Results for now and for the coming part of
    /// the day, so a menu opening sends them ranked already. All of it
    /// happens on the indexing thread, when there is one.
    pub(super) fn plan_hot(&mut self, now: Instant) {
        self.hot_due = now + HOT_EVERY;
        self.hot_versions = self.versions();
        let when = self.when(now);
        let plan = if self.plans() {
            Plan {
                usage: self.usage(),
                learned: self.cache_learned(when.at),
                when,
                capacity: self.hot_capacity,
                versions: self.hot_versions,
            }
        } else {
            self.ahead = None;
            // Nothing hot to let go of.
            if self.index.hot().is_empty() {
                return;
            }
            Plan {
                usage: Usage {
                    picked: Arc::default(),
                    others: Vec::new(),
                    wanted: 0,
                    left_out: Vec::new(),
                },
                learned: Arc::new([]),
                when,
                capacity: 0,
                versions: self.hot_versions,
            }
        };
        if let Some(indexer) = &mut self.indexer {
            indexer.plan(plan);
        } else {
            let (hot, ahead) = plan.run();
            self.index.set_hot(hot);
            self.ahead = self.plans().then_some(ahead);
        }
    }

    /// The Items that are a query's Learned choice, strongest first, each
    /// once. The order holds at any time: every choice's picks decay alike.
    pub(super) fn cache_learned(&mut self, at: u64) -> Arc<[Slot]> {
        let versions = (self.index.version(), self.history.version());
        if self.learned_versions == Some(versions) {
            return Arc::clone(&self.learned);
        }
        let mut choices: Vec<(f64, Slot)> = Vec::new();
        for (extension, id, strength) in self.history.choices(at) {
            if extension == IDENTITIES {
                choices.extend(self.index.identified(id).map(|slot| (strength, slot)));
            } else if let Some(ext) = self.extension_named(extension)
                && let Some(slot) = self.index.find(ext, id)
            {
                choices.push((strength, slot));
            }
        }
        choices.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut seen = HashSet::new();
        self.learned = choices
            .into_iter()
            .map(|(_, slot)| slot)
            .filter(|&slot| seen.insert(slot))
            .collect();
        self.learned_versions = Some(versions);
        Arc::clone(&self.learned)
    }

    /// Keeps resident every Item a keyword or an Alias names, so a keyword
    /// typed never waits for a read. The indexing thread reads them, when
    /// there is one; otherwise they are read now.
    pub(super) fn repin(&mut self) {
        let aliases = self.aliased();
        if let Some(indexer) = &mut self.indexer {
            indexer.pin(aliases);
        }
        self.repin_index();
    }

    /// The Items the Aliases name, by Extension and id.
    pub(super) fn aliased(&self) -> Vec<(ExtensionId, String)> {
        self.keywords
            .aliases
            .values()
            .filter_map(|alias| Some((self.extension_named(&alias.extension)?, alias.item.clone())))
            .collect()
    }

    /// Works out again which Items the Index keeps resident for a keyword or
    /// an Alias, after the Items or the Aliases changed. Reads those that
    /// aren't resident only when there is no indexing thread to.
    pub(super) fn repin_index(&mut self) {
        let mut pinned = self.index.keyed_all();
        for (ext, id) in self.aliased() {
            pinned.extend(self.index.find(ext, &id));
        }
        pinned.sort_unstable();
        pinned.dedup();
        self.index.pin(pinned);
        if self.indexer.is_none() {
            self.index.read_pinned();
        }
    }
}
