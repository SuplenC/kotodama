//! Every Indexed item, searched with nucleo on each keystroke, with no IPC
//! to Extensions: two [`Segment`]s per Extension, each replaced whole when its
//! Items change. The first holds the Items the search shows, the second those
//! only the Extension's app shows, so the search skips the second whole and
//! never tests a row for it.
//!
//! What a search reads is all in memory. The rest of an Item is read back
//! off its Shelf when a Result shows it, and stays resident while it is hot
//! or was shown lately.
//!
//! # Owns
//!
//! - [`Index`]: every Extension's Segments, the search over them and keeping Items resident.
//! - [`Slot`], [`Reach`] and [`ItemRef`]: where an Item is, what a search reads, and an Item read
//!   back.
//!
//! # Not here
//!
//! - One Extension's rows: `segment.rs`. The payloads: `payload.rs`.
//! - Building new versions and planning the hot Items: `indexer.rs`, `daemon/hot.rs`.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::BuildHasherDefault;
use std::hash::Hasher;
use std::ops::Deref;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use kotodama_protocol::Action;
use kotodama_protocol::Item;
use nucleo_matcher::Matcher;
use nucleo_matcher::pattern::Pattern;

use crate::daemon::ExtensionId;
use crate::rank::Filter;
use crate::rank::Letters;
use crate::rank::Matched;
use crate::rank::Texts;
use crate::segment::Edit;
use crate::segment::Ranked;
use crate::segment::Row;
use crate::segment::Segment;

/// How many Segments each Extension has: the Items the search shows, then
/// those its app alone shows.
pub const SIDES: usize = 2;

/// Where an Indexed item lives: one of its Extension's Segments and its row
/// there. Slots order by Extension, then by Segment and row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slot {
    pub segment: u32,
    pub row: Row,
}

impl Slot {
    pub fn extension(self) -> ExtensionId {
        ExtensionId(self.segment as usize / SIDES)
    }
}

/// One Extension's change as a change to each of its Segments, given what
/// they hold: an Item goes to the Segment its `app_only` names, and leaves
/// the other one if it was there. A Segment the change doesn't touch gets no
/// edit, so it is never copied for nothing.
pub fn split(edit: Edit, segments: [&Segment; SIDES]) -> [Vec<Edit>; SIDES] {
    let mut split: [Vec<Edit>; SIDES] = Default::default();
    match edit {
        Edit::Add(items) => {
            let app_only = items.iter().filter(|item| item.app_only).count();
            // A batch for one side, nearly every batch, goes on as it came: a
            // copy of a large first batch would stay in the Daemon's resident
            // memory after it is freed.
            let added: [Vec<Item>; SIDES] = if app_only == 0 {
                [items, Vec::new()]
            } else if app_only == items.len() {
                [Vec::new(), items]
            } else {
                // Within one Segment a later Item replaces an earlier one
                // with its id. Across the two, only the last may stay.
                let (searched, app_only) = last_of_each(items).into_iter().partition(|item| !item.app_only);
                [searched, app_only]
            };
            let moved: [Vec<String>; SIDES] = std::array::from_fn(|side| {
                let other = SIDES - 1 - side;
                if segments[side].is_empty() {
                    return Vec::new();
                }
                added[other]
                    .iter()
                    .filter(|item| segments[side].holds(&item.id))
                    .map(|item| item.id.clone())
                    .collect()
            });
            for (side, (moved, added)) in moved.into_iter().zip(added).enumerate() {
                if !moved.is_empty() {
                    split[side].push(Edit::Remove(moved));
                }
                if !added.is_empty() {
                    split[side].push(Edit::Add(added));
                }
            }
        }
        Edit::Remove(ids) => {
            for (side, segment) in segments.into_iter().enumerate() {
                let held: Vec<String> = ids.iter().filter(|id| segment.holds(id)).cloned().collect();
                if !held.is_empty() {
                    split[side].push(Edit::Remove(held));
                }
            }
        }
    }
    split
}

/// What each Segment's placeholders must do: by Segment, those to stand
/// aside, `true`, as an Item of their Identity that doesn't yield is in
/// some Segment, and those to come back, `false`, as none is any more.
/// Empty, at the cost of one test per Segment, while no Segment has one.
fn steps(segments: &[Arc<Segment>]) -> Vec<(usize, Vec<(String, bool)>)> {
    let mut steps = Vec::new();
    for (at, segment) in segments.iter().enumerate() {
        let moved: Vec<(String, bool)> = segment
            .placeholders()
            .filter_map(|(id, identity, aside)| {
                let claimed = identity.is_some_and(|identity| segments.iter().any(|other| other.claims(identity)));
                (claimed != aside).then(|| (id.to_owned(), claimed))
            })
            .collect();
        if !moved.is_empty() {
            steps.push((at, moved));
        }
    }
    steps
}

/// Stands aside each placeholder an Item of its Identity that doesn't
/// yield is in `segments` for, and brings back each one none is for any
/// more. A Segment it changes starts a new version first when `begins`
/// says so for it: not one already changed in the version being made.
/// Returns the Segments it changed.
pub fn settle(segments: &mut [Arc<Segment>], mut begins: impl FnMut(usize) -> bool) -> Vec<usize> {
    let steps = steps(segments);
    let mut changed = Vec::with_capacity(steps.len());
    for (at, steps) in steps {
        let segment = Arc::make_mut(&mut segments[at]);
        if begins(at) {
            segment.next_version();
        }
        segment.step(&steps);
        changed.push(at);
    }
    changed
}

/// `items` with only the last of those that share an id, in order.
fn last_of_each(items: Vec<Item>) -> Vec<Item> {
    let mut seen = HashSet::with_capacity(items.len());
    let mut kept: Vec<Item> = items
        .into_iter()
        .rev()
        .filter(|item| seen.insert(item.id.clone()))
        .collect();
    kept.reverse();
    kept
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

/// Where the Item in `row` of `segment` is: its Shelf's id and its place
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

/// Hashes a [`Slot`] with a multiply per word, as rustc's `FxHash` does. The
/// Daemon makes the Slots, so `SipHash`'s guard against keys picked to collide
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
    /// The Segments the whole search reads.
    Root,
    /// Both of one Extension's Segments: the search in its app.
    App(ExtensionId),
}

impl Reach {
    /// Whether a search with this reach reads Segment `segment`, given which
    /// Segments the whole search reads.
    pub fn reads(self, segment: usize, searched: &[bool]) -> bool {
        match self {
            Reach::Root => searched.get(segment).copied().unwrap_or(false),
            Reach::App(extension) => segment / SIDES == extension.0,
        }
    }
}

/// Which of an Extension's Segments the whole search reads until told
/// otherwise: the Items it shows, and not those only the app shows.
const SEARCHED_BY_DEFAULT: [bool; SIDES] = [true, false];

#[derive(Default)]
pub struct Index {
    /// [`SIDES`] per Extension, in Extension order: the Items the search
    /// shows, then those only its app shows.
    segments: Vec<Arc<Segment>>,
    /// By Segment, whether the whole search reads it. It skips the rest
    /// whole.
    searched: Vec<bool>,
    /// Bumped on every change.
    version: u64,
    /// Bumped on every change to what a search reads, so a search narrowed
    /// from an earlier one knows when it can't be trusted. A change to
    /// Items' Actions alone leaves it.
    keys_version: u64,
    /// Bumped whenever a Segment put in may offer Action keywords others
    /// didn't, or is emptied: the Daemon's table of them is built again.
    verbs_version: u64,
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
            segments: (0..extensions * SIDES).map(|_| Arc::default()).collect(),
            searched: SEARCHED_BY_DEFAULT.repeat(extensions),
            ..Index::default()
        }
    }

    /// Keeps the payloads of Items indexed from here on in files in `dir`.
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
        for searched in SEARCHED_BY_DEFAULT {
            self.segments.push(Arc::new(Segment::new(self.dir.clone())));
            self.searched.push(searched);
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// The version of what a search reads: the same until an Item is
    /// added, removed or changed other than in its Actions.
    pub fn keys_version(&self) -> u64 {
        self.keys_version
    }

    /// Makes the whole search read the Segments `searched` marks, by
    /// Segment, and skip the rest. A search narrowed from one before this
    /// can't be trusted, so it counts as a change.
    pub fn set_searched(&mut self, searched: Vec<bool>) {
        if searched != self.searched {
            self.searched = searched;
            self.version += 1;
            self.keys_version += 1;
        }
    }

    /// By Segment, whether the whole search reads it.
    pub fn searched(&self) -> &[bool] {
        &self.searched
    }

    /// Where one Extension's Segments are.
    pub fn segments_of(extension: ExtensionId) -> Range<usize> {
        extension.0 * SIDES..(extension.0 + 1) * SIDES
    }

    /// Up to `most` of one Extension's Items, in row order: those the search
    /// shows, then those only its app shows, each in the order they were
    /// sent, but for the rows a removed Item left and a later one took.
    pub fn first(&self, extension: ExtensionId, most: usize) -> Vec<Slot> {
        let mut first = Vec::new();
        for n in Self::segments_of(extension) {
            let Some(segment) = self.segments.get(n) else {
                break;
            };
            let rows = (0..segment.every_letter().len() as Row)
                .filter(|&row| segment.ranked(row).is_some())
                .take(most - first.len());
            first.extend(rows.map(|row| Slot { segment: n as u32, row }));
        }
        first
    }

    /// Makes one change to an Extension's Items, here, at once. Returns
    /// whether it changed what a search reads, rather than only Actions.
    pub fn edit(&mut self, extension: ExtensionId, edit: Edit) -> bool {
        self.version += 1;
        let at = Self::segments_of(extension).start;
        let edits = split(edit, [&self.segments[at], &self.segments[at + 1]]);
        let mut edited = [false; SIDES];
        for (side, edits) in edits.into_iter().enumerate() {
            if edits.is_empty() {
                continue;
            }
            let segment = Arc::make_mut(&mut self.segments[at + side]);
            segment.next_version();
            for edit in edits {
                segment.edit(edit);
            }
            edited[side] = true;
        }
        let settled = settle(&mut self.segments, |n| !(n / SIDES == extension.0 && edited[n % SIDES]));
        let mut keys_changed = false;
        let changed = (0..SIDES).filter(|&side| edited[side]).map(|side| at + side);
        for n in changed.chain(settled) {
            let segment = &self.segments[n];
            keys_changed |= segment.keys_changed();
            if segment.verbs_changed() {
                self.verbs_version += 1;
            }
        }
        if keys_changed {
            self.keys_version += 1;
        }
        keys_changed
    }

    /// Stands aside or brings back the placeholders whose Identity an Item
    /// that doesn't yield took or left, here, at once, as [`Index::edit`]
    /// does after its change: for a change made elsewhere, such as
    /// [`Index::clear`]. Returns whether it changed what a search reads.
    pub fn settle(&mut self) -> bool {
        let settled = settle(&mut self.segments, |_| true);
        if settled.is_empty() {
            return false;
        }
        self.version += 1;
        self.keys_version += 1;
        if settled.iter().any(|&n| self.segments[n].verbs_changed()) {
            self.verbs_version += 1;
        }
        true
    }

    /// Puts in the Segment at `at`, built elsewhere, and returns the one it
    /// replaces.
    pub fn replace(&mut self, at: usize, segment: Arc<Segment>) -> Arc<Segment> {
        self.version += 1;
        if segment.keys_changed() {
            self.keys_version += 1;
        }
        if segment.verbs_changed() {
            self.verbs_version += 1;
        }
        std::mem::replace(&mut self.segments[at], segment)
    }

    /// Removes every Item of one Extension. Returns the Segments that held
    /// them, none if there were none.
    pub fn clear(&mut self, extension: ExtensionId) -> Vec<Arc<Segment>> {
        let mut cleared = Vec::new();
        for at in Self::segments_of(extension) {
            if self.segments[at].is_empty() {
                continue;
            }
            let empty = self.segments[at].emptied();
            cleared.push(self.replace(at, Arc::new(empty)));
        }
        cleared
    }

    fn slots<'a>(segment: usize, rows: impl IntoIterator<Item = &'a Row> + 'a) -> impl Iterator<Item = Slot> + 'a {
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
            .filter(move |&(n, _)| reach.reads(n, &self.searched))
            .flat_map(move |(n, segment)| Self::slots(n, segment.keyed(keyword)))
    }

    /// The Items, of any Extension, whose Identity is `identity`.
    pub fn identified<'a>(&'a self, identity: &'a str) -> impl Iterator<Item = Slot> + 'a {
        self.segments
            .iter()
            .enumerate()
            .flat_map(move |(n, segment)| Self::slots(n, segment.identified(identity)))
    }

    /// Where one Extension's Item is, in whichever of its Segments.
    pub fn find(&self, extension: ExtensionId, id: &str) -> Option<Slot> {
        Self::segments_of(extension).find_map(|at| {
            let row = self.segments.get(at)?.find(id)?;
            Some(Slot {
                segment: at as u32,
                row,
            })
        })
    }

    /// The slots within `reach`, in order, whose Item has a word starting
    /// with one of `initials`, as
    /// [`Haystack::initials`](crate::rank::Haystack::initials) gives them.
    pub fn starting_with(&self, initials: Letters, reach: Reach) -> Vec<Slot> {
        let mut slots = Vec::new();
        for (n, segment) in self.segments.iter().enumerate() {
            if !reach.reads(n, &self.searched) {
                continue;
            }
            segment.rows_where(
                |_, starts| Letters::meet_block(starts, initials),
                |row| slots.push(Slot { segment: n as u32, row }),
            );
        }
        slots
    }

    /// The whole Item in `slot`, read back off its Shelf unless it is
    /// resident.
    pub fn item(&self, slot: Slot) -> Option<Arc<Item>> {
        let segment = self.segments.get(slot.segment as usize)?;
        let (store, place) = segment.payload(slot.row)?;
        let read = self.reads.get() + 1;
        self.reads.set(read);
        let mut resident = self.resident.borrow_mut();
        if let Some(kept) = resident.get_mut(&slot)
            && kept.shelf == store.id()
            && kept.at == place.at
        {
            kept.read = read;
            return Some(Arc::clone(&kept.item));
        }
        let item = Arc::new(store.get_with(place, &mut self.read_buf.borrow_mut())?);
        self.shelf_reads.set(self.shelf_reads.get() + 1);
        resident.insert(
            slot,
            Resident {
                shelf: store.id(),
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
        let Some((store, place)) = self
            .segments
            .get(slot.segment as usize)
            .and_then(|segment| segment.payload(slot.row))
        else {
            return false;
        };
        self.resident
            .borrow()
            .get(&slot)
            .is_some_and(|kept| kept.shelf == store.id() && kept.at == place.at)
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
    /// and reads them back at once. Lets go of the rest but those shown last.
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
        for Read { slot, shelf, at, item } in reads {
            let read = self.reads.get() + 1;
            self.reads.set(read);
            resident.insert(slot, Resident { shelf, at, item, read });
        }
    }

    /// Every Item with a keyword, in slot order.
    pub fn keyed_all(&self) -> Vec<Slot> {
        let mut keyed: Vec<Slot> = self
            .segments
            .iter()
            .enumerate()
            .flat_map(|(n, segment)| Self::slots(n, segment.keyed_rows()))
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
        let kept = |slot: &Slot| hot.binary_search(slot).is_ok() || pinned.binary_search(slot).is_ok();
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
        let keys = self.segments.iter().fold(0, |read, segment| read ^ segment.touch());
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
        self.segments[Self::segments_of(extension)]
            .iter()
            .flat_map(|segment| segment.identities())
    }

    /// Each Action one Extension's Items offer, by Item kind and Action id,
    /// as the first Item in slot order with it in each Segment has it.
    pub fn offered(&self, extension: ExtensionId) -> Vec<(String, Action)> {
        let mut offered = Vec::new();
        for at in Self::segments_of(extension) {
            let segment = at as u32;
            offered.extend(self.segments[at].offered().filter_map(|(kind, id, row)| {
                let item = self.item(Slot { segment, row })?;
                let action = item.actions.iter().find(|action| action.id == id)?;
                Some((kind.to_owned(), action.clone()))
            }));
        }
        offered
    }

    /// The slots of one Extension's Items of kind `kind` that offer the Action `action`.
    pub fn offering<'a>(
        &'a self,
        extension: ExtensionId,
        kind: &'a str,
        action: &'a str,
    ) -> impl Iterator<Item = Slot> + 'a {
        Self::segments_of(extension).flat_map(move |at| {
            let segment = at as u32;
            self.segments[at]
                .offering(kind, action)
                .into_iter()
                .map(move |row| Slot { segment, row })
        })
    }

    /// Every keyword of the Actions the Indexed items offer, folded, with
    /// repeats.
    pub fn verbs(&self) -> impl Iterator<Item = &str> {
        self.segments.iter().flat_map(|segment| segment.verbs())
    }

    /// Counts up whenever the Action keywords [`Index::verbs`] gives may
    /// have changed.
    pub fn verbs_version(&self) -> u64 {
        self.verbs_version
    }

    /// The id of the first Action of the Item in `slot` with a keyword
    /// `names` accepts, given folded.
    pub fn named_action(&self, slot: Slot, names: impl Fn(&str) -> bool) -> Option<&str> {
        self.segments.get(slot.segment as usize)?.named_action(slot.row, names)
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
                        && let Some((ranked, score, matched)) = segment.score(slot.row, pattern, matcher, filter)
                    {
                        found(slot, ranked, score, matched);
                    }
                }
            }
            None => {
                for (n, segment) in self.segments.iter().enumerate() {
                    // A Segment out of reach costs this test, never a row.
                    if !reach.reads(n, &self.searched) {
                        continue;
                    }
                    // Two passes: the rows' dense letters alone, a block at a
                    // time in one vectorised loop, then only the rows that
                    // pass are scored.
                    segment.rows_where(
                        |letters, initials| filter.may_match_block(letters, initials),
                        |row| {
                            if let Some((ranked, score, matched)) = segment.score(row, pattern, matcher, filter) {
                                let slot = Slot { segment: n as u32, row };
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
mod tests;
