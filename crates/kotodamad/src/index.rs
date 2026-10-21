//! Every Indexed item, searched with nucleo on each keystroke, with no IPC
//! to Extensions: one [`Segment`] per Extension, each replaced whole when its
//! Items change.
//!
//! What a search reads is all in memory. The rest of an Item is read back
//! off its Shelf when a Result shows it, and stays resident while it is hot
//! or was shown lately (ADR 0017).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use kotodama_protocol::{Action, Item};
use nucleo_matcher::Matcher;
use nucleo_matcher::pattern::Pattern;

use crate::daemon::ExtensionId;
use crate::rank::{Filter, Letters, Matched, Texts};
use crate::segment::{Edit, Ranked, Row, Segment};

/// Where an Indexed item lives: its Extension's Segment and its row there.
/// Slots order by Extension, then by row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slot {
    pub segment: u32,
    pub row: Row,
}

impl Slot {
    pub fn extension(self) -> ExtensionId {
        ExtensionId(self.segment as usize)
    }
}

/// An Item, read back off its Shelf or borrowed from where it is kept.
pub enum ItemRef<'a> {
    Resident(Arc<Item>),
    Borrowed(&'a Item),
}

impl Deref for ItemRef<'_> {
    type Target = Item;

    fn deref(&self) -> &Item {
        match self {
            ItemRef::Resident(item) => item,
            ItemRef::Borrowed(item) => item,
        }
    }
}

/// An Item read back off its Shelf.
struct Resident {
    /// Where it was read from, to tell it from another Item in the same
    /// slot since.
    shelf: u64,
    at: u64,
    item: Arc<Item>,
    /// When it was last read, in [`Index::reads`].
    read: u64,
}

/// An Item read off its Shelf on another thread, for an [`Index`] holding
/// the same version of its Segment to keep resident.
pub struct Read {
    pub slot: Slot,
    shelf: u64,
    at: u64,
    item: Arc<Item>,
}

impl Read {
    /// Where it was read from: its Shelf's id and its place there.
    pub fn from(&self) -> (u64, u64) {
        (self.shelf, self.at)
    }
}

/// Where the Item in `row` of `segment` is now: its Shelf's id and its place
/// there.
pub fn place_of(segment: &Segment, row: Row) -> Option<(u64, u64)> {
    let (shelf, place) = segment.payload(row)?;
    Some((shelf.id(), place.at))
}

/// Reads the Item in `slot` of `segments`, by Extension, off its Shelf.
pub fn read(segments: &[Arc<Segment>], slot: Slot) -> Option<Read> {
    let (shelf, place) = segments.get(slot.segment as usize)?.payload(slot.row)?;
    Some(Read {
        slot,
        shelf: shelf.id(),
        at: place.at,
        item: Arc::new(shelf.get(place)?),
    })
}

/// Hashes a [`Slot`] with a multiply per word, as rustc's FxHash does. The
/// Daemon makes the Slots, so SipHash's guard against keys picked to collide
/// buys nothing, and would cost each row a keystroke shows.
#[derive(Default)]
pub struct SlotHasher(u64);

impl Hasher for SlotHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(byte.into());
        }
    }

    #[inline]
    fn write_u32(&mut self, n: u32) {
        self.write_u64(n.into());
    }

    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

/// How many Items that aren't hot stay resident after they were shown: the
/// Results of a few keystrokes.
pub const SHOWN_RESIDENT: usize = 256;

/// Which Segments a search reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Every Segment but those behind a Prefix: the search.
    Root,
    /// One Extension's Segment alone: the search behind its Prefix.
    Scope(ExtensionId),
}

impl Reach {
    /// Whether a search with this reach reads Segment `segment`, given which
    /// Segments are behind a Prefix.
    pub fn reads(self, segment: usize, scoped: &[bool]) -> bool {
        match self {
            Reach::Root => !scoped.get(segment).copied().unwrap_or(false),
            Reach::Scope(extension) => extension.0 == segment,
        }
    }
}

#[derive(Default)]
pub struct Index {
    /// By Extension.
    segments: Vec<Arc<Segment>>,
    /// By Extension, whether its Items are behind a Prefix, which the root
    /// search skips whole.
    scoped: Vec<bool>,
    /// Bumped on every change, so a search narrowed from an earlier one knows
    /// when it can't be trusted.
    version: u64,
    /// Where the Segments keep their payloads: a file in this directory, or
    /// memory.
    dir: Option<Arc<Path>>,
    /// Items read back off their Shelves: the hot ones and those shown last.
    /// Looked up for each row a keystroke shows.
    resident: RefCell<HashMap<Slot, Resident, BuildHasherDefault<SlotHasher>>>,
    /// Counts the Items read, so the latest read are kept.
    reads: Cell<u64>,
    /// Counts the Items read back off their Shelves here.
    shelf_reads: Cell<u64>,
    /// What an Item is read off its Shelf into, kept from one to the next.
    read_buf: RefCell<Vec<u8>>,
    /// The Items expected to be shown soon, in slot order, kept resident.
    hot: Vec<Slot>,
    /// The Items a keyword or an Alias names, in slot order, kept resident
    /// so a keyword typed never waits for a read.
    pinned: Vec<Slot>,
}

impl Index {
    pub fn new(extensions: usize) -> Self {
        Index {
            segments: (0..extensions).map(|_| Arc::default()).collect(),
            ..Index::default()
        }
    }

    /// Keeps the payloads of Items indexed from now on in files in `dir`.
    pub fn keep_payloads_in(&mut self, dir: Arc<Path>) {
        self.dir = Some(dir);
        for segment in &mut self.segments {
            if segment.is_empty() {
                *segment = Arc::new(Segment::new(self.dir.clone()));
            }
        }
    }

    /// Where the Segments keep their payloads.
    pub fn payload_dir(&self) -> Option<Arc<Path>> {
        self.dir.clone()
    }

    /// Makes room for one more Extension's Items.
    pub fn add_extension(&mut self) {
        self.segments.push(Arc::new(Segment::new(self.dir.clone())));
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Puts the Items of the Extensions `scoped` marks, by Extension, behind
    /// their Prefix. A search narrowed from one before this can't be trusted,
    /// so it counts as a change.
    pub fn set_scoped(&mut self, scoped: Vec<bool>) {
        if scoped != self.scoped {
            self.scoped = scoped;
            self.version += 1;
        }
    }

    /// By Extension, whether its Items are behind a Prefix.
    pub fn scoped(&self) -> &[bool] {
        &self.scoped
    }

    /// Up to `most` of one Extension's Items, in row order: the order they
    /// were sent in, but for the rows a removed Item left and a later one took.
    pub fn first(&self, extension: ExtensionId, most: usize) -> Vec<Slot> {
        let Some(segment) = self.segments.get(extension.0) else {
            return Vec::new();
        };
        (0..segment.every_letter().len() as Row)
            .filter(|&row| segment.ranked(row).is_some())
            .take(most)
            .map(|row| Slot {
                segment: extension.0 as u32,
                row,
            })
            .collect()
    }

    /// Makes one change to an Extension's Items, here and now.
    pub fn edit(&mut self, extension: ExtensionId, edit: Edit) {
        self.version += 1;
        let segment = Arc::make_mut(&mut self.segments[extension.0]);
        segment.next_version();
        segment.edit(edit);
    }

    /// Puts in an Extension's Segment, built elsewhere.
    pub fn replace(&mut self, extension: ExtensionId, segment: Arc<Segment>) {
        self.version += 1;
        self.segments[extension.0] = segment;
    }

    /// Removes every Item of one Extension. Returns whether there were any.
    pub fn clear(&mut self, extension: ExtensionId) -> bool {
        if self.segments[extension.0].is_empty() {
            return false;
        }
        let empty = self.segments[extension.0].emptied();
        self.replace(extension, Arc::new(empty));
        true
    }

    fn slots<'a>(
        &'a self,
        segment: usize,
        rows: impl IntoIterator<Item = &'a Row> + 'a,
    ) -> impl Iterator<Item = Slot> + 'a {
        rows.into_iter().map(move |&row| Slot {
            segment: segment as u32,
            row,
        })
    }

    /// The Items within `reach` whose keyword is `keyword`, given in
    /// lowercase.
    pub fn keyed<'a>(&'a self, keyword: &'a str, reach: Reach) -> impl Iterator<Item = Slot> + 'a {
        self.segments
            .iter()
            .enumerate()
            .filter(move |&(n, _)| reach.reads(n, &self.scoped))
            .flat_map(move |(n, segment)| self.slots(n, segment.keyed(keyword)))
    }

    /// The Items, of any Extension, whose Identity is `identity`.
    pub fn identified<'a>(&'a self, identity: &'a str) -> impl Iterator<Item = Slot> + 'a {
        self.segments
            .iter()
            .enumerate()
            .flat_map(move |(n, segment)| self.slots(n, segment.identified(identity)))
    }

    pub fn find(&self, extension: ExtensionId, id: &str) -> Option<Slot> {
        let row = self.segments[extension.0].find(id)?;
        Some(Slot {
            segment: extension.0 as u32,
            row,
        })
    }

    /// The slots within `reach`, in order, whose Item has a word starting
    /// with one of `initials`, as
    /// [`Haystack::initials`](crate::rank::Haystack::initials) gives them.
    pub fn starting_with(&self, initials: Letters, reach: Reach) -> Vec<Slot> {
        let mut slots = Vec::new();
        for (n, segment) in self.segments.iter().enumerate() {
            if !reach.reads(n, &self.scoped) {
                continue;
            }
            segment.rows_where(
                |_, starts| Letters::meet_block(starts, initials),
                |row| {
                    slots.push(Slot {
                        segment: n as u32,
                        row,
                    })
                },
            );
        }
        slots
    }

    /// The whole Item in `slot`, read back off its Shelf unless it is
    /// resident.
    pub fn item(&self, slot: Slot) -> Option<Arc<Item>> {
        let segment = self.segments.get(slot.segment as usize)?;
        let (shelf, place) = segment.payload(slot.row)?;
        let read = self.reads.get() + 1;
        self.reads.set(read);
        let mut resident = self.resident.borrow_mut();
        if let Some(kept) = resident.get_mut(&slot)
            && kept.shelf == shelf.id()
            && kept.at == place.at
        {
            kept.read = read;
            return Some(Arc::clone(&kept.item));
        }
        let item = Arc::new(shelf.get_with(place, &mut self.read_buf.borrow_mut())?);
        self.shelf_reads.set(self.shelf_reads.get() + 1);
        resident.insert(
            slot,
            Resident {
                shelf: shelf.id(),
                at: place.at,
                item: Arc::clone(&item),
                read,
            },
        );
        Some(item)
    }

    /// How many Items were read back off their Shelves on this thread.
    pub fn shelf_reads(&self) -> u64 {
        self.shelf_reads.get()
    }

    /// Whether the Item in `slot` is resident, rather than on its Shelf.
    pub fn is_resident(&self, slot: Slot) -> bool {
        let Some((shelf, place)) = self
            .segments
            .get(slot.segment as usize)
            .and_then(|segment| segment.payload(slot.row))
        else {
            return false;
        };
        self.resident
            .borrow()
            .get(&slot)
            .is_some_and(|kept| kept.shelf == shelf.id() && kept.at == place.at)
    }

    /// The id of the Item in `slot`.
    pub fn id(&self, slot: Slot) -> Option<&str> {
        self.segments.get(slot.segment as usize)?.id(slot.row)
    }

    /// The Identity of the Item in `slot`, if it has one.
    pub fn identity(&self, slot: Slot) -> Option<&str> {
        self.segments.get(slot.segment as usize)?.identity(slot.row)
    }

    /// Makes `hot`, in slot order, the Items kept resident whatever is shown,
    /// and reads them back now. Lets go of the rest but those shown last.
    pub fn set_hot(&mut self, hot: Vec<Slot>) {
        self.hot = hot;
        for n in 0..self.hot.len() {
            self.item(self.hot[n]);
        }
        self.trim();
    }

    /// Makes `hot`, in slot order, the Items kept resident whatever is shown,
    /// without reading them: they were read elsewhere, and kept.
    pub fn hot_read(&mut self, hot: Vec<Slot>) {
        self.hot = hot;
        self.trim();
    }

    /// Keeps Items read on another thread from the version of their Segment
    /// this Index holds. One read from another version is never used.
    pub fn keep(&mut self, reads: Vec<Read>) {
        let resident = self.resident.get_mut();
        for Read {
            slot,
            shelf,
            at,
            item,
        } in reads
        {
            let read = self.reads.get() + 1;
            self.reads.set(read);
            resident.insert(
                slot,
                Resident {
                    shelf,
                    at,
                    item,
                    read,
                },
            );
        }
    }

    /// Every Item with a keyword, in slot order.
    pub fn keyed_all(&self) -> Vec<Slot> {
        let mut keyed: Vec<Slot> = self
            .segments
            .iter()
            .enumerate()
            .flat_map(|(n, segment)| self.slots(n, segment.keyed_rows()))
            .collect();
        keyed.sort_unstable();
        keyed.dedup();
        keyed
    }

    /// Makes `pinned`, in slot order, Items kept resident whatever is shown.
    /// Reads none of them.
    pub fn pin(&mut self, pinned: Vec<Slot>) {
        self.pinned = pinned;
    }

    /// The Items a keyword or an Alias names, kept resident.
    pub fn pinned(&self) -> &[Slot] {
        &self.pinned
    }

    /// Reads back the pinned Items that aren't resident.
    pub fn read_pinned(&self) {
        for &slot in &self.pinned {
            self.item(slot);
        }
    }

    /// The Items kept resident whatever is shown.
    pub fn hot(&self) -> &[Slot] {
        &self.hot
    }

    /// Lets go of resident Items past the hot ones and the
    /// [`SHOWN_RESIDENT`] read last, and of those replaced since.
    pub fn trim(&mut self) {
        let resident = self.resident.get_mut();
        if resident.len() <= self.hot.len() + self.pinned.len() + SHOWN_RESIDENT {
            return;
        }
        let (hot, pinned) = (&self.hot, &self.pinned);
        let kept =
            |slot: &Slot| hot.binary_search(slot).is_ok() || pinned.binary_search(slot).is_ok();
        let mut reads: Vec<u64> = resident
            .iter()
            .filter(|(slot, _)| !kept(slot))
            .map(|(_, kept)| kept.read)
            .collect();
        let keep_from = if reads.len() > SHOWN_RESIDENT {
            let at = reads.len() - SHOWN_RESIDENT;
            *reads.select_nth_unstable(at).1
        } else {
            0
        };
        let segments = &self.segments;
        let kept_slot = kept;
        resident.retain(|slot, kept| {
            let current = segments
                .get(slot.segment as usize)
                .and_then(|segment| segment.payload(slot.row))
                .is_some_and(|(shelf, place)| kept.shelf == shelf.id() && kept.at == place.at);
            current && (kept.read >= keep_from || kept_slot(slot))
        });
    }

    /// Reads what a search reads and the hot Items, so the first keystroke
    /// after the menu opens finds them in the CPU's cache.
    pub fn warm(&self) -> u64 {
        let keys = self
            .segments
            .iter()
            .fold(0, |read, segment| read ^ segment.touch());
        let resident = self.resident.borrow();
        let hot: usize = self
            .hot
            .iter()
            .filter_map(|slot| resident.get(slot))
            .map(|kept| kept.item.title.len() + kept.item.actions.len())
            .sum();
        keys ^ hot as u64
    }

    pub fn texts(&self, slot: Slot) -> Option<Texts<'_>> {
        self.segments.get(slot.segment as usize)?.texts(slot.row)
    }

    /// What ranking reads of the Item in `slot`, if there is one.
    pub fn ranked(&self, slot: Slot) -> Option<Ranked<'_>> {
        self.segments.get(slot.segment as usize)?.ranked(slot.row)
    }

    /// The names of the kinds of Item each Segment has held, in Segment
    /// order.
    pub fn kinds(&self) -> impl Iterator<Item = &[String]> {
        self.segments.iter().map(|segment| segment.kinds())
    }

    /// Every Identity one Extension's Items have.
    pub fn identities(&self, extension: ExtensionId) -> impl Iterator<Item = &str> {
        self.segments[extension.0].identities()
    }

    /// Each Action one Extension's Items offer, by Item kind and Action id,
    /// as the first Item in slot order with it has it.
    pub fn offered(&self, extension: ExtensionId) -> Vec<(String, Action)> {
        let segment = extension.0 as u32;
        self.segments[extension.0]
            .offered()
            .filter_map(|(kind, id, row)| {
                let item = self.item(Slot { segment, row })?;
                let action = item.actions.iter().find(|action| action.id == id)?;
                Some((kind.to_owned(), action.clone()))
            })
            .collect()
    }

    /// The slots of one Extension's Items of kind `kind` that offer the Action `action`.
    pub fn offering<'a>(
        &'a self,
        extension: ExtensionId,
        kind: &str,
        action: &str,
    ) -> impl Iterator<Item = Slot> + 'a {
        let segment = extension.0 as u32;
        self.segments[extension.0]
            .offering(kind, action)
            .map(move |row| Slot { segment, row })
    }

    /// Scores every Item within `reach` against `pattern`, or only the slots
    /// in `within` when the caller knows nothing else can match: a search of
    /// the same reach found them. Calls `found` with each match's slot, what
    /// ranking reads of it, its score and what matched. The matches come in
    /// slot order if `within` is in it. Ranking takes the row from the scan
    /// and doesn't look it up again. `filter` is as [`Texts::score`] takes it.
    pub fn search(
        &self,
        pattern: &Pattern,
        matcher: &mut Matcher,
        filter: Filter,
        reach: Reach,
        within: Option<&[Slot]>,
        mut found: impl FnMut(Slot, Ranked<'_>, u32, Matched),
    ) {
        match within {
            Some(slots) => {
                for &slot in slots {
                    let Some(segment) = self.segments.get(slot.segment as usize) else {
                        continue;
                    };
                    let row = slot.row as usize;
                    if let (Some(&letters), Some(&initials)) =
                        (segment.every_letter().get(row), segment.initials().get(row))
                        && filter.may_match(letters, initials)
                        && let Some((ranked, score, matched)) =
                            segment.score(slot.row, pattern, matcher, filter)
                    {
                        found(slot, ranked, score, matched);
                    }
                }
            }
            None => {
                for (n, segment) in self.segments.iter().enumerate() {
                    // A Segment out of reach costs this test, never a row.
                    if !reach.reads(n, &self.scoped) {
                        continue;
                    }
                    // Two passes: the rows' dense letters alone, a block at a
                    // time in one vectorised loop, then only the rows that
                    // pass are scored.
                    segment.rows_where(
                        |letters, initials| filter.may_match_block(letters, initials),
                        |row| {
                            if let Some((ranked, score, matched)) =
                                segment.score(row, pattern, matcher, filter)
                            {
                                let slot = Slot {
                                    segment: n as u32,
                                    row,
                                };
                                found(slot, ranked, score, matched);
                            }
                        },
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use kotodama_protocol::ItemKind;
    use nucleo_matcher::Config;
    use nucleo_matcher::pattern::{CaseMatching, Normalization};

    use super::*;
    use crate::rank::{self, Haystack, Query, Tier, Tiers};

    /// xorshift64*: the same Items and queries on every run.
    struct Random(u64);

    impl Random {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }

        fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
            from[self.below(from.len())]
        }
    }

    const WORDS: &[&str] = &[
        "Firefox",
        "fire",
        "fox",
        "Terminal",
        "term",
        "Mail",
        "mailbox",
        "Files",
        "Café",
        "naïve",
        "Straße",
        "日本語",
        "😀",
        "getUserName",
        "VSCode",
        "code",
        "Settings",
        "set",
        "git",
        "push",
        "a",
        "x",
        "Ölfass",
        "ŁÓDŹ",
    ];
    const SEPARATORS: &[&str] = &[" ", "-", "_", "", ".", " — "];

    fn words(random: &mut Random, most: usize) -> String {
        let mut text = String::new();
        for n in 0..1 + random.below(most) {
            if n > 0 {
                text.push_str(random.pick(SEPARATORS));
            }
            text.push_str(random.pick(WORDS));
        }
        text
    }

    fn item(random: &mut Random) -> Item {
        let id = format!("item-{}", random.below(40));
        let kind = match random.below(3) {
            0 => ItemKind::Url {
                url: format!("https://example.org/{id}"),
            },
            1 => ItemKind::App {
                desktop_id: format!("{id}.desktop"),
            },
            _ => ItemKind::Project {
                path: format!("/src/{id}").into(),
            },
        };
        Item {
            id,
            kind,
            identity: (random.below(4) == 0).then(|| format!("thing:{}", random.below(5))),
            title: words(random, 3),
            subtitle: (random.below(2) == 0).then(|| words(random, 4)),
            icon: None,
            keyword: (random.below(5) == 0).then(|| random.pick(WORDS).to_lowercase()),
            keywords: (0..random.below(3)).map(|_| words(random, 2)).collect(),
            hidden: random.below(10) == 0,
            actions: (0..1 + random.below(2))
                .map(|n| Action {
                    id: ["open", "copy"][n].into(),
                    title: format!("Action {n}"),
                    shortcut: None,
                    arguments: vec![],
                })
                .collect(),
        }
    }

    fn query(random: &mut Random) -> String {
        let mut text = String::new();
        for n in 0..1 + random.below(2) {
            if n > 0 {
                text.push(' ');
            }
            let word: Vec<char> = random.pick(WORDS).chars().collect();
            let mut word: Vec<char> = word[..1 + random.below(word.len())].to_vec();
            if word.len() > 2 && random.below(3) == 0 {
                // A typo.
                let at = random.below(word.len() - 1);
                word.swap(at, at + 1);
            }
            if random.below(3) == 0 {
                word = word.iter().flat_map(|c| c.to_uppercase()).collect();
            }
            text.extend(word);
        }
        if random.below(5) == 0 {
            text.push(' ');
        }
        text
    }

    /// What the search finds of each Item: nucleo's score, its tier, and
    /// whether it is a near miss, by Extension and id.
    type Found = BTreeMap<(usize, String), (Option<(u32, Option<Tier>)>, bool)>;

    /// Matched, as on a keystroke: the Index's dense layout.
    fn search(index: &Index, text: &str, matcher: &mut Matcher, tiers: &mut Tiers) -> Found {
        let pattern = Pattern::parse(
            &rank::fold_text(text),
            CaseMatching::Ignore,
            Normalization::Never,
        );
        let query = Query::new(text);
        let mut out = Vec::new();
        index.search(
            &pattern,
            matcher,
            query.filter(),
            Reach::Root,
            None,
            |slot, _, score, matched| out.push((slot, score, matched)),
        );
        let mut found = Found::new();
        for (n, segment) in index.segments.iter().enumerate() {
            for row in 0..segment.every_letter().len() as Row {
                let slot = Slot {
                    segment: n as u32,
                    row,
                };
                let Some(texts) = index.texts(slot) else {
                    continue;
                };
                let id = index.id(slot).unwrap().to_owned();
                let near = tiers.near_miss_texts(texts, &query);
                found.insert((n, id), (None, near));
            }
        }
        for (slot, score, matched) in out {
            let tier = tiers.texts(index.texts(slot).unwrap(), &query, matched);
            let id = index.id(slot).unwrap().to_owned();
            found.get_mut(&(slot.segment as usize, id)).unwrap().0 = Some((score, tier));
        }
        found
    }

    /// Matched the naive way: each Item folded afresh, whole.
    fn naive(
        items: &BTreeMap<(usize, String), Item>,
        text: &str,
        matcher: &mut Matcher,
        tiers: &mut Tiers,
    ) -> Found {
        let pattern = Pattern::parse(
            &rank::fold_text(text),
            CaseMatching::Ignore,
            Normalization::Never,
        );
        let query = Query::new(text);
        items
            .iter()
            .map(|(key, item)| {
                let haystack = Haystack::new(item);
                let scored = haystack
                    .score(&pattern, matcher, query.filter())
                    .map(|(score, matched)| (score, tiers.item(&haystack, item, &query, matched)));
                let near = tiers.near_miss(&haystack, item, &query);
                (key.clone(), (scored, near))
            })
            .collect()
    }

    #[test]
    fn a_search_of_the_index_finds_what_a_naive_search_of_every_item_finds() {
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut tiers = Tiers::default();
        for seed in 1..=60u64 {
            let mut random = Random(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
            let mut index = Index::new(2);
            let mut items: BTreeMap<(usize, String), Item> = BTreeMap::new();
            for _ in 0..30 {
                let extension = random.below(2);
                if random.below(4) == 0 {
                    let ids: Vec<String> = (0..1 + random.below(5))
                        .map(|_| format!("item-{}", random.below(40)))
                        .collect();
                    for id in &ids {
                        items.remove(&(extension, id.clone()));
                    }
                    index.edit(ExtensionId(extension), Edit::Remove(ids));
                } else {
                    let batch: Vec<Item> = (0..1 + random.below(8))
                        .map(|_| item(&mut random))
                        .collect();
                    for item in &batch {
                        let key = (extension, item.id.clone());
                        if item.hidden {
                            items.remove(&key);
                        } else {
                            items.insert(key, item.clone());
                        }
                    }
                    index.edit(ExtensionId(extension), Edit::Add(batch));
                }
                let text = query(&mut random);
                assert_eq!(
                    search(&index, &text, &mut matcher, &mut tiers),
                    naive(&items, &text, &mut matcher, &mut tiers),
                    "seed {seed}, query {text:?}"
                );
            }
            let indexed: BTreeSet<(usize, String)> = (0..2)
                .flat_map(|n| {
                    let segment = &index.segments[n];
                    (0..segment.every_letter().len() as Row)
                        .filter_map(move |row| Some((n, segment.id(row)?.to_owned())))
                })
                .collect();
            assert_eq!(indexed, items.keys().cloned().collect(), "seed {seed}");
            for ((extension, id), item) in &items {
                let slot = index.find(ExtensionId(*extension), id).unwrap();
                assert_eq!(index.item(slot).as_deref(), Some(item), "seed {seed}");
                assert_eq!(index.identity(slot), item.identity.as_deref());
            }
        }
    }
}
