//! Every Indexed item, kept in memory and searched with nucleo on each
//! keystroke, with no IPC to Extensions.

use std::collections::HashMap;

use kotodama_protocol::Item;
use nucleo_matcher::pattern::Pattern;
use nucleo_matcher::{Matcher, Utf32String};

use crate::daemon::ExtensionId;

/// Where an Indexed item lives. Stable until the Item is removed.
pub type Slot = u32;

pub struct Entry {
    pub extension: ExtensionId,
    pub item: Item,
    /// The title in the form nucleo searches, converted once on the way in.
    pub title: Utf32String,
    /// The keyword and keywords, joined by spaces; empty if there are none.
    words: Utf32String,
}

impl Entry {
    fn new(extension: ExtensionId, item: Item) -> Self {
        let mut words = String::new();
        for word in item.keyword.iter().chain(&item.keywords) {
            if !words.is_empty() {
                words.push(' ');
            }
            words.push_str(word);
        }
        Entry {
            extension,
            title: Utf32String::from(item.title.as_str()),
            words: Utf32String::from(words),
            item,
        }
    }

    /// How well the Item matches: its title's score, or failing that its
    /// words'. `None` if neither matches.
    pub fn score(&self, pattern: &Pattern, matcher: &mut Matcher) -> Option<u32> {
        let title = pattern.score(self.title.slice(..), matcher);
        if self.words.is_empty() {
            return title;
        }
        let words = pattern.score(self.words.slice(..), matcher);
        title.max(words)
    }
}

#[derive(Default)]
pub struct Index {
    slots: Vec<Option<Entry>>,
    free: Vec<Slot>,
    /// Each Extension's Items by id.
    ids: Vec<HashMap<String, Slot>>,
    /// Bumped on every change, so a search narrowed from an earlier one knows
    /// when it can't be trusted.
    version: u64,
}

impl Index {
    pub fn new(extensions: usize) -> Self {
        Index {
            ids: (0..extensions).map(|_| HashMap::new()).collect(),
            ..Index::default()
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Adds an Item, or replaces the one with the same id.
    pub fn add(&mut self, extension: ExtensionId, item: Item) {
        self.version += 1;
        let ids = &mut self.ids[extension.0];
        if let Some(&slot) = ids.get(&item.id) {
            self.slots[slot as usize] = Some(Entry::new(extension, item));
            return;
        }
        let id = item.id.clone();
        let entry = Some(Entry::new(extension, item));
        let slot = match self.free.pop() {
            Some(slot) => {
                self.slots[slot as usize] = entry;
                slot
            }
            None => {
                self.slots.push(entry);
                (self.slots.len() - 1) as Slot
            }
        };
        ids.insert(id, slot);
    }

    pub fn remove(&mut self, extension: ExtensionId, id: &str) {
        if let Some(slot) = self.ids[extension.0].remove(id) {
            self.version += 1;
            self.slots[slot as usize] = None;
            self.free.push(slot);
        }
    }

    /// Removes every Item of one Extension. Returns whether there were any.
    pub fn clear(&mut self, extension: ExtensionId) -> bool {
        let ids = std::mem::take(&mut self.ids[extension.0]);
        if ids.is_empty() {
            return false;
        }
        self.version += 1;
        for slot in ids.into_values() {
            self.slots[slot as usize] = None;
            self.free.push(slot);
        }
        true
    }

    pub fn find(&self, extension: ExtensionId, id: &str) -> Option<Slot> {
        self.ids[extension.0].get(id).copied()
    }

    pub fn entry(&self, slot: Slot) -> Option<&Entry> {
        self.slots.get(slot as usize)?.as_ref()
    }

    /// One Extension's Items.
    pub fn items(&self, extension: ExtensionId) -> impl Iterator<Item = &Item> {
        self.ids[extension.0]
            .values()
            .filter_map(|&slot| self.entry(slot).map(|entry| &entry.item))
    }

    /// Scores every Item against `pattern`, or only the slots in `within` when
    /// the caller knows nothing else can match. Appends `(slot, score)` for
    /// each match to `out`.
    pub fn search(
        &self,
        pattern: &Pattern,
        matcher: &mut Matcher,
        within: Option<&[Slot]>,
        out: &mut Vec<(Slot, u32)>,
    ) {
        let mut score = |slot: Slot| {
            if let Some(entry) = self.entry(slot)
                && let Some(score) = entry.score(pattern, matcher)
            {
                out.push((slot, score));
            }
        };
        match within {
            Some(slots) => slots.iter().copied().for_each(&mut score),
            None => (0..self.slots.len() as Slot).for_each(score),
        }
    }
}
