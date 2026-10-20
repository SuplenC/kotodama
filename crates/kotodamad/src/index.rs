//! Every Indexed item, kept in memory and searched with nucleo on each
//! keystroke, with no IPC to Extensions.

use std::collections::HashMap;

use kotodama_protocol::Item;
use nucleo_matcher::Matcher;
use nucleo_matcher::pattern::Pattern;

use crate::daemon::ExtensionId;
use crate::rank::{Filter, Haystack, Matched};

/// Where an Indexed item lives. Stable until the Item is removed.
pub type Slot = u32;

pub struct Entry {
    pub extension: ExtensionId,
    pub item: Item,
    pub haystack: Haystack,
}

impl Entry {
    fn new(extension: ExtensionId, item: Item) -> Self {
        Entry {
            extension,
            haystack: Haystack::new(&item),
            item,
        }
    }
}

#[derive(Default)]
pub struct Index {
    slots: Vec<Option<Entry>>,
    /// The word initials of each slot's Item, as [`Haystack::initials`] gives
    /// them, or 0 for an empty slot. Kept apart from the Entries, so looking
    /// for near misses passes over most slots without reading them.
    initials: Vec<u64>,
    /// Every letter of each slot's Item, as [`Haystack::every_letter`] gives
    /// them, or 0 for an empty slot: a search passes over the slots that lack
    /// one of the query's without reading them.
    letters: Vec<u64>,
    free: Vec<Slot>,
    /// Each Extension's Items by id.
    ids: Vec<HashMap<String, Slot>>,
    /// The Items that have a keyword, by it in lowercase.
    keywords: HashMap<String, Vec<Slot>>,
    /// The Items that have an Identity, by it.
    identities: HashMap<String, Vec<Slot>>,
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

    /// Makes room for one more Extension's Items.
    pub fn add_extension(&mut self) {
        self.ids.push(HashMap::new());
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Adds an Item, or replaces the one with the same id.
    pub fn add(&mut self, extension: ExtensionId, item: Item) {
        self.version += 1;
        let keyword = item.keyword.as_deref().map(str::to_lowercase);
        let slot = match self.ids[extension.0].get(&item.id) {
            Some(&slot) => {
                self.unkey(slot);
                self.slots[slot as usize] = Some(Entry::new(extension, item));
                slot
            }
            None => {
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
                self.ids[extension.0].insert(id, slot);
                slot
            }
        };
        if self.initials.len() <= slot as usize {
            self.initials.resize(slot as usize + 1, 0);
            self.letters.resize(slot as usize + 1, 0);
        }
        let haystack = self.entry(slot).map(|entry| &entry.haystack);
        let initials = haystack.map_or(0, Haystack::initials);
        let letters = haystack.map_or(0, Haystack::every_letter);
        self.initials[slot as usize] = initials;
        self.letters[slot as usize] = letters;
        if let Some(keyword) = keyword {
            self.keywords.entry(keyword).or_default().push(slot);
        }
        if let Some(identity) = self
            .entry(slot)
            .and_then(|entry| entry.item.identity.clone())
        {
            self.identities.entry(identity).or_default().push(slot);
        }
    }

    pub fn remove(&mut self, extension: ExtensionId, id: &str) {
        if let Some(slot) = self.ids[extension.0].remove(id) {
            self.version += 1;
            self.empty(slot);
        }
    }

    /// Frees a slot.
    fn empty(&mut self, slot: Slot) {
        self.unkey(slot);
        self.slots[slot as usize] = None;
        self.initials[slot as usize] = 0;
        self.letters[slot as usize] = 0;
        self.free.push(slot);
    }

    /// Forgets the keyword and the Identity of the Item in `slot`.
    fn unkey(&mut self, slot: Slot) {
        let Some(item) = self.slots.get(slot as usize).and_then(Option::as_ref) else {
            return;
        };
        let item = &item.item;
        if let Some(keyword) = &item.keyword {
            unlist(&mut self.keywords, &keyword.to_lowercase(), slot);
        }
        if let Some(identity) = &item.identity {
            unlist(&mut self.identities, identity, slot);
        }
    }

    /// The Items whose keyword is `keyword`, given in lowercase.
    pub fn keyed(&self, keyword: &str) -> &[Slot] {
        self.keywords.get(keyword).map_or(&[], Vec::as_slice)
    }

    /// The Items, of any Extension, whose Identity is `identity`.
    pub fn identified(&self, identity: &str) -> &[Slot] {
        self.identities.get(identity).map_or(&[], Vec::as_slice)
    }

    /// Removes every Item of one Extension. Returns whether there were any.
    pub fn clear(&mut self, extension: ExtensionId) -> bool {
        let ids = std::mem::take(&mut self.ids[extension.0]);
        if ids.is_empty() {
            return false;
        }
        self.version += 1;
        for slot in ids.into_values() {
            self.empty(slot);
        }
        true
    }

    pub fn find(&self, extension: ExtensionId, id: &str) -> Option<Slot> {
        self.ids[extension.0].get(id).copied()
    }

    /// The slots, in order, whose Item has a word starting with one of
    /// `initials`, as [`Haystack::initials`] gives them.
    pub fn starting_with(&self, initials: u64) -> impl Iterator<Item = Slot> {
        self.initials
            .iter()
            .enumerate()
            .filter(move |&(_, &item)| item & initials != 0)
            .map(|(slot, _)| slot as Slot)
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
    /// each match, with what matched, to `out`, in slot order if `within` is.
    /// `filter` is as [`Haystack::score`] takes it.
    pub fn search(
        &self,
        pattern: &Pattern,
        matcher: &mut Matcher,
        filter: Filter,
        within: Option<&[Slot]>,
        out: &mut Vec<(Slot, u32, Matched)>,
    ) {
        let mut score = |slot: Slot| {
            if self
                .letters
                .get(slot as usize)
                .is_some_and(|&letters| filter.may_match(letters))
                && let Some(entry) = self.entry(slot)
                && let Some((score, matched)) = entry.haystack.score(pattern, matcher, filter)
            {
                out.push((slot, score, matched));
            }
        };
        match within {
            Some(slots) => slots.iter().copied().for_each(&mut score),
            None => (0..self.slots.len() as Slot).for_each(score),
        }
    }
}

/// Takes `slot` off the list kept under `name`, and the list off `lists` once
/// it is empty.
fn unlist(lists: &mut HashMap<String, Vec<Slot>>, name: &str, slot: Slot) {
    if let Some(slots) = lists.get_mut(name) {
        slots.retain(|&other| other != slot);
        if slots.is_empty() {
            lists.remove(name);
        }
    }
}
