//! What the menu shows before anything is typed: the empty query.
//!
//! # Owns
//!
//! - The empty query's sections, an app's empty View and the most used.
//! - The Usage counts they read, cached until the History changes.
//!
//! # Not here
//!
//! - Which sections the empty query has: `crate::empty_query`.
//! - The search once something is typed: `search.rs`.

use std::sync::Arc;

use super::Daemon;
use super::Desktop;
use super::HOT_EVERY;
use super::Launcher;
use super::MAX_RESULTS;
use super::OWN;
use super::OwnAction;
use super::Session;
use super::Source;
use super::alias_id;
use crate::clock::When;
use crate::history::Frecent;
use crate::history::IDENTITIES;
use crate::history::Picks;
use crate::history::Store;
use crate::index::Index;
use crate::index::Slot;
use crate::most_used::Counts;
use crate::most_used::Other;
use crate::most_used::Picked;
use crate::most_used::Usage;

impl<L: Launcher, D: Desktop, S: Store> Daemon<L, D, S> {
    /// What the Empty query shows: the Results most used at this time of day,
    /// then the Fixed menu, or without one the apps' entry Items, each thing
    /// once.
    pub(super) fn empty_query(&mut self, session: &Session, when: When) -> Vec<Source> {
        let mut shown = self.most_used(when);
        let identity = |source: Source| self.identity(session, source);
        let menu: Vec<Source> = match &self.empty_query.menu {
            Some(menu) => menu
                .iter()
                .filter_map(|(extension, id)| self.source_of(session, extension, id))
                .collect(),
            None => (self.own.iter().enumerate())
                .filter(|(_, own)| matches!(own.does, OwnAction::App(_)))
                .map(|(n, _)| Source::Own(n as u32))
                .collect(),
        };
        for source in menu {
            if shown.len() >= MAX_RESULTS {
                break;
            }
            let same =
                |&shown: &Source| shown == source || identity(shown).is_some_and(|id| identity(source) == Some(id));
            if !shown.iter().any(same) {
                shown.push(source);
            }
        }
        shown
    }

    /// What the `n`th app shows with nothing typed in it, under its intro
    /// card: the commands its Manifest names, then its Extension's Items most
    /// used at `when`, then the rest in the order it sent them.
    pub(super) fn empty_app(&mut self, n: usize, when: When) -> Vec<Source> {
        let Some(app) = self.apps.get(n) else {
            return Vec::new();
        };
        let extension = app.extension;
        let mut shown: Vec<Source> = app
            .commands
            .iter()
            .filter_map(|id| self.index.find(extension, id))
            .map(Source::Indexed)
            .collect();
        if !self.history.is_empty() {
            self.cache_frecent();
            let mut left_out = vec![true; self.index.searched().len()];
            left_out[Index::segments_of(extension)].fill(false);
            let usage = Usage {
                picked: Arc::clone(&self.frecent),
                others: Vec::new(),
                wanted: MAX_RESULTS,
                left_out,
            };
            for source in usage.rank(when, &mut self.habits) {
                if !shown.contains(&source) {
                    shown.push(source);
                }
            }
        }
        shown.truncate(MAX_RESULTS);
        for slot in self.index.first(extension, MAX_RESULTS) {
            if shown.len() >= MAX_RESULTS {
                break;
            }
            let source = Source::Indexed(slot);
            if !shown.contains(&source) {
                shown.push(source);
            }
        }
        shown
    }

    /// The Results most used at `when`, as [`Usage::rank`] ranks them: as
    /// they were ranked ahead, when that ranking stands for `when`, or else
    /// ranked here, on the spot.
    pub(super) fn most_used(&mut self, when: When) -> Vec<Source> {
        let versions = self.versions();
        if let Some(ahead) = &self.ahead
            && let Some(ranked) = ahead.at(versions, when, HOT_EVERY.as_secs())
        {
            return ranked.to_vec();
        }
        if self.empty_query.most_used == 0 || self.history.is_empty() {
            return Vec::new();
        }
        self.ranked_here += 1;
        let usage = self.usage();
        usage.rank(when, &mut self.habits)
    }

    /// What the Empty query's most used Results rank by: the picked
    /// Indexed items the whole search shows, then the Daemon's own Items
    /// and the Results the Aliases name. A Live query result is never among
    /// them: the Empty query asks no Live query.
    pub(super) fn usage(&mut self) -> Usage {
        self.cache_frecent();
        let mut others = Vec::new();
        for (n, own) in self.own.iter().enumerate() {
            if let Some(&picks) = self.history.picks(OWN, &own.item.id) {
                others.push(Other {
                    source: Source::Own(n as u32),
                    group: None,
                    picks,
                    alias: false,
                });
            }
        }
        let mut aliases: Vec<_> = self.keywords.aliases.iter().collect();
        aliases.sort_unstable_by_key(|&(keyword, _)| keyword);
        for (keyword, alias) in aliases {
            let Some(&picks) = self.history.picks(OWN, &alias_id(keyword)) else {
                continue;
            };
            let Some(source) = self.held_source(&alias.extension, &alias.item) else {
                continue;
            };
            let group = match source {
                Source::Indexed(slot) => self.group(slot),
                _ => None,
            };
            others.push(Other {
                source,
                group,
                picks,
                alias: true,
            });
        }
        Usage {
            picked: Arc::clone(&self.frecent),
            others,
            wanted: self.empty_query.most_used,
            left_out: self.index.searched().iter().map(|&read| !read).collect(),
        }
    }

    /// The first Item in slot order with the Identity of the Item in
    /// `slot`, which stands for that Identity; `None` without one.
    pub(super) fn group(&self, slot: Slot) -> Option<Slot> {
        self.index.identified(self.index.identity(slot)?).min()
    }

    /// Rebuilds the list of picked Indexed items if the index or the History
    /// changed since it was built, or the half-life their picks decay at.
    pub(super) fn cache_frecent(&mut self) {
        let versions = (self.index.version(), self.history.version());
        let half_life = self.history.settings().half_life_secs;
        #[expect(
            clippy::float_cmp,
            reason = "both are the setting as read: any change to it, however small, changes how picks decay"
        )]
        let unchanged = self.frecent_versions == Some(versions) && self.frecent.picks.half_life() == half_life;
        if unchanged {
            return;
        }
        let mut frecent = Vec::with_capacity(self.frecent.slots.len());
        for (extension, id, picks) in self.history.picked() {
            if extension == IDENTITIES {
                let group = self.index.identified(id).min();
                let slots = self.index.identified(id);
                frecent.extend(slots.map(|slot| (slot, picks, Counts::Shared, group)));
            } else if let Some(ext) = self.extension_named(extension)
                && let Some(slot) = self.index.find(ext, id)
            {
                let group = self.group(slot);
                let counts = if group.is_some() { Counts::Direct } else { Counts::Both };
                frecent.push((slot, picks, counts, group));
            }
        }
        frecent.sort_unstable_by_key(|&(slot, ..)| slot);
        let picks: Vec<Picks> = frecent.iter().map(|&(_, picks, ..)| picks).collect();
        self.frecent = Arc::new(Picked {
            slots: frecent.iter().map(|&(slot, _, counts, _)| (slot, counts)).collect(),
            groups: frecent.iter().map(|&(.., group)| group).collect(),
            picks: Frecent::new(&picks, half_life),
        });
        self.frecent_versions = Some(versions);
    }
}
