//! The Empty query's most used Results: ranked from a copy of the picks, so
//! the ranking runs on the indexing thread ahead of each part of the day, or
//! on the Daemon's when no ranking made ahead stands for now.
//!
//! # Owns
//!
//! - [`Picked`]: the picked Indexed items and their picks, as of one Index
//!   and History version.
//! - [`Usage`]: everything the most used Results rank by, and the ranking.
//! - [`Ahead`]: the rankings made ahead, and when one stands for now.
//!
//! # Not here
//!
//! - Gathering the picks from the History and the Index:
//!   `Daemon::cache_frecent` and `Daemon::usage`.
//! - The Fixed menu that follows them: `Daemon::empty_query`.

use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::sync::Arc;

use crate::clock::When;
use crate::daemon::Source;
use crate::history::{self, Frecent, Habit, Picks};
use crate::index::{Slot, SlotHasher};

/// What a pick the History kept counts for, for one Indexed item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Counts {
    /// Its Identity's: the Frecency it ranks by.
    Shared,
    /// Picks of the Item itself, which only order the Results of one Identity.
    Direct,
    /// An Item without an Identity's: both.
    Both,
}

/// The picked Indexed items, as of one version of the Index and the History,
/// in slot order: an Item's Identity's picks next to its own.
#[derive(Debug, Default)]
pub struct Picked {
    pub slots: Vec<(Slot, Counts)>,
    /// By entry, the first Item in slot order with its Identity, which every
    /// entry of that Identity shares; `None` without an Identity.
    pub groups: Vec<Option<Slot>>,
    /// By entry, its picks.
    pub picks: Frecent,
}

/// Picks the History keeps for a Result outside [`Picked`]: one of the
/// Daemon's own Items, or the Result an Alias names.
#[derive(Clone, Copy, Debug)]
pub struct Other {
    pub source: Source,
    /// As [`Picked::groups`] has it.
    pub group: Option<Slot>,
    pub picks: Picks,
    /// An Alias's picks, which add to its Result's own.
    pub alias: bool,
}

/// Everything the Empty query's most used Results rank by.
#[derive(Debug)]
pub struct Usage {
    pub picked: Arc<Picked>,
    /// The Daemon's own Items, in their order, then the Aliases' Results, by
    /// keyword.
    pub others: Vec<Other>,
    /// How many to show.
    pub wanted: usize,
    /// By Extension, whether its Indexed items are left out of the ranking:
    /// those behind a Prefix, for the Empty query.
    pub left_out: Vec<bool>,
}

/// One Result as the most used rank it.
struct Used {
    source: Source,
    group: Option<Slot>,
    used: Habit,
    /// How much it was picked through itself rather than its Identity: of
    /// the Results of one Identity, the one picked through most stands for it.
    direct: f64,
}

impl Used {
    fn new(source: Source, group: Option<Slot>) -> Self {
        Used {
            source,
            group,
            used: Habit::default(),
            direct: 0.0,
        }
    }
}

impl Usage {
    /// The most used Results at `when`, most used first, each thing once.
    /// They rank by their picks near this time of day, then by all their
    /// picks; an Alias's picks count for the Result it names. Of the Results
    /// of one Identity, the one picked through most stands for it. `habits`
    /// is scratch space, kept from one call to the next.
    pub fn rank(&self, when: When, habits: &mut Vec<Habit>) -> Vec<Source> {
        if self.wanted == 0 {
            return Vec::new();
        }
        let picked = &*self.picked;
        picked.picks.habits(when, habits);
        let half_life = picked.picks.half_life();
        let mut most: Vec<Used> = Vec::with_capacity(picked.slots.len() + self.others.len());
        let entries = picked.slots.iter().zip(&picked.groups).zip(habits.iter());
        for ((&(slot, counts), &group), &used) in entries {
            if self.left_out.get(slot.segment as usize) == Some(&true) {
                continue;
            }
            let source = Source::Indexed(slot);
            if most.last().is_none_or(|last| last.source != source) {
                most.push(Used::new(source, group));
            }
            let entry = most.last_mut().expect("just pushed");
            if counts != Counts::Direct {
                entry.used = used;
            }
            if counts != Counts::Shared {
                entry.direct = used.overall;
            }
        }
        for other in &self.others {
            let used = other.picks.habit(when, half_life);
            if !other.alias {
                most.push(Used {
                    used,
                    ..Used::new(other.source, other.group)
                });
                continue;
            }
            match most.iter_mut().find(|entry| entry.source == other.source) {
                Some(entry) => entry.used = entry.used.and(used),
                None => most.push(Used {
                    used,
                    ..Used::new(other.source, other.group)
                }),
            }
        }

        let mut stands_for: HashMap<Slot, usize, BuildHasherDefault<SlotHasher>> =
            HashMap::default();
        let mut keep = vec![true; most.len()];
        for n in 0..most.len() {
            let Some(group) = most[n].group else {
                continue;
            };
            let Some(&other) = stands_for.get(&group) else {
                stands_for.insert(group, n);
                continue;
            };
            let (best, rest) = if most[n].direct > most[other].direct {
                (n, other)
            } else {
                (other, n)
            };
            // They share their Identity's picks, but an Alias may name one.
            most[best].used = most[best].used.max(most[rest].used);
            keep[rest] = false;
            stands_for.insert(group, best);
        }
        let mut kept = keep.iter();
        most.retain(|_| *kept.next().expect("one per Result"));
        let rank = |a: &Used, b: &Used| a.used.rank(&b.used).then(a.source.cmp(&b.source));
        if most.len() > self.wanted {
            most.select_nth_unstable_by(self.wanted - 1, rank);
        }
        most.truncate(self.wanted);
        most.sort_unstable_by(rank);
        most.into_iter().map(|entry| entry.source).collect()
    }
}

/// What the Empty query's most used Results were ranked from: the versions
/// of the Index, the History and the rest of what they rank by.
pub type Versions = (u64, u64, u64);

/// The Empty query's most used Results, ranked ahead: for the moment the
/// hot Items were planned, and for the start of the coming part of the day.
#[derive(Debug, Default)]
pub struct Ahead {
    pub versions: Versions,
    pub ranked: Vec<(When, Vec<Source>)>,
}

impl Ahead {
    /// The Results ranked ahead that stand for `when`, if any do: ranked
    /// from `versions`, in the part of the day `when` falls in, at most
    /// `fresh_secs` before it.
    pub fn at(&self, versions: Versions, when: When, fresh_secs: u64) -> Option<&[Source]> {
        if self.versions != versions {
            return None;
        }
        let part = history::part_of_day(when);
        self.ranked
            .iter()
            .find(|(ranked, _)| {
                history::part_of_day(*ranked) == part
                    && ranked.at <= when.at
                    && when.at - ranked.at <= fresh_secs
            })
            .map(|(_, sources)| sources.as_slice())
    }

    /// The Indexed items it shows.
    pub fn slots(&self) -> impl Iterator<Item = Slot> + '_ {
        self.ranked
            .iter()
            .flat_map(|(_, sources)| sources)
            .filter_map(|source| match source {
                Source::Indexed(slot) => Some(*slot),
                _ => None,
            })
    }
}

#[cfg(test)]
mod tests;
