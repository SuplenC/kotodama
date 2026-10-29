//! One Extension's Indexed items, laid out for scanning: what a keystroke
//! reads is worked out once, when an Item arrives, and kept in dense arrays
//! and one arena of text, so a search runs down them without following a
//! pointer per Item.
//!
//! A Segment is built off the keystroke path and then only read, so a search
//! can share it with the thread building the next one. Each Extension has its
//! own, so a change to one Extension's Items copies only its Segment.
//!
//! Of the rest of an Item, a Segment keeps in memory only what ranking reads:
//! its id, Identity, keyword and Actions. The whole Item waits on a
//! [`Shelf`], off the heap, until a Result shows it.
//!
//! # Owns
//!
//! - [`Segment`]: one Extension's rows, scanned a block at a time, and [`Edit`], a change to them.
//! - [`Ranked`]: a row's keys for ranking, its texts read on demand.
//!
//! # Not here
//!
//! - The Segments together and the search: `index.rs`. The payloads: `payload.rs`.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Arc;

use kotodama_protocol::Item;
use nucleo_matcher::Matcher;
use nucleo_matcher::Utf32Str;
use nucleo_matcher::Utf32String;
use nucleo_matcher::pattern::Pattern;

use crate::payload::Place;
use crate::payload::Shelf;
use crate::payload::encode;
use crate::rank::BLOCK;
use crate::rank::Filter;
use crate::rank::Folded;
use crate::rank::Letters;
use crate::rank::Mask;
use crate::rank::Matched;
use crate::rank::Searched;
use crate::rank::Text;
use crate::rank::Texts;
use crate::rank::fold_text;

/// Whether `text` folds to `folded`, without making a copy of an ASCII
/// `text` to fold it.
fn folds_to(text: &str, folded: &str) -> bool {
    if text.is_ascii() {
        text.eq_ignore_ascii_case(folded) && !folded.bytes().any(|byte| byte.is_ascii_uppercase())
    } else {
        fold_text(text) == folded
    }
}

/// Where an Item lives in its Segment. Stable until the Item is removed, and
/// not taken by another Item until the change after that.
pub type Row = u32;

/// Where a text lies in a Segment's arenas.
#[derive(Clone, Copy, Debug, Default)]
struct Span {
    start: u32,
    /// The text's length, with [`WIDE`] set when it is in the arena of
    /// `char`s rather than the ASCII one.
    len: u32,
}

const WIDE: u32 = 1 << 31;

impl Span {
    fn len(self) -> usize {
        (self.len & !WIDE) as usize
    }

    fn range(self) -> std::ops::Range<usize> {
        self.start as usize..self.start as usize + self.len()
    }
}

/// What a search reads of a row once its letters let it through.
#[derive(Clone, Copy, Debug, Default)]
struct Keys {
    title: Span,
    words: Span,
    subtitle: Span,
    /// Where the title's word starts are in the arena of them, followed by
    /// the keywords', then, with [`WEAK_AT`] set in `words_starts`, where
    /// in `words` the weak keywords begin.
    starts: u32,
    title_starts: u32,
    /// How many of the keywords' word starts there are, with [`WEAK_AT`] set
    /// when the Item has weak keywords.
    words_starts: u32,
    subtitle_initials: Letters,
    /// Every letter of the title and keywords.
    letters: Letters,
    /// The Item's kind, as an index into [`Segment::kinds`].
    kind: u16,
    /// Whether `words` starts with the Item's own keyword.
    keyword: bool,
    /// Whether the row holds an Item: a search reads it here, beside the
    /// rest, rather than in the Segment's `held`.
    filled: bool,
}

// A row's keys are what a search reads of every row its letters let
// through: they stay one cache line or less, with no padding left. Weak
// keywords, which most Items lack, cost a row nothing here.
const _: () = assert!(size_of::<Keys>() == 56);

/// Set in [`Keys::words_starts`] when the Item has weak keywords. A count
/// of word starts never reaches it: a text is shorter than [`WIDE`].
const WEAK_AT: u32 = 1 << 31;

impl Keys {
    /// How many of the keywords' word starts there are.
    fn words_starts(&self) -> u32 {
        self.words_starts & !WEAK_AT
    }

    /// How many entries of the arena of word starts the row takes.
    fn starts_len(&self) -> u32 {
        self.title_starts + self.words_starts() + u32::from(self.words_starts & WEAK_AT != 0)
    }
}

/// A filled row, as ranking reads it: its keys, looked up once, and its
/// texts only when asked for, as most rows looked at for a near miss turn
/// out not to be one on their letters alone.
#[derive(Clone, Copy)]
pub struct Ranked<'a> {
    segment: &'a Segment,
    row: Row,
    keys: &'a Keys,
}

impl<'a> Ranked<'a> {
    /// What a search reads of the row, as [`Segment::texts`] gives it.
    #[inline]
    pub fn texts(self) -> Texts<'a> {
        self.segment.keys_texts(self.row, self.keys)
    }

    /// The title's length in characters.
    #[inline]
    pub fn title_len(self) -> u32 {
        self.keys.title.len() as u32
    }

    /// The Item's kind, as an index into [`Segment::kinds`].
    #[inline]
    pub fn kind(self) -> u16 {
        self.keys.kind
    }

    /// Every letter of the title and keywords, and the letters their words
    /// start with, as [`Texts`] has them.
    #[inline]
    pub fn letters(self) -> (Letters, Letters) {
        (self.keys.letters, self.segment.initials[self.row as usize])
    }
}

/// One change to an Extension's Items, as it sent it.
#[derive(Debug)]
pub enum Edit {
    /// Adds Items, or replaces those with the same ids. A hidden one
    /// removes the Item with its id.
    Add(Vec<Item>),
    Remove(Vec<String>),
}

/// What a Segment keeps of an Item besides its search keys: what ranking
/// reads of it, and where the rest is.
#[derive(Clone, Debug)]
struct Held {
    id: Arc<str>,
    identity: Option<Arc<str>>,
    /// In lowercase.
    keyword: Option<Arc<str>>,
    /// Its Actions, as indexes into [`Segment::offered`].
    actions: Box<[u32]>,
    /// Where the whole Item is on the Segment's Shelf.
    place: Place,
}

/// An Action the Segment's Items offer: an Item kind, an Action id and its
/// keywords, and the rows whose Item has it.
#[derive(Clone, Debug)]
struct Offered {
    kind: u16,
    action: Box<str>,
    /// The Action's keywords, folded as the query is.
    keywords: Box<[Box<str>]>,
    rows: BTreeSet<Row>,
}

#[derive(Clone, Default)]
pub struct Segment {
    /// Every letter of each row's texts, or none for an empty row: a search
    /// passes over the rows that lack one of the query's without reading
    /// anything else of them.
    every_letter: Vec<Letters>,
    /// The word initials of each row's title and keywords, or none for an
    /// empty row, so looking for near misses passes over most rows.
    initials: Vec<Letters>,
    keys: Vec<Keys>,
    /// The folded texts that are ASCII, one byte a character, as nucleo
    /// reads them.
    ascii: Vec<u8>,
    /// The rest, a `char` a character.
    wide: Vec<char>,
    /// Where the words of titles and keywords start.
    starts: Vec<u32>,
    /// How many bytes of the arenas no row uses any more.
    garbage: usize,
    held: Vec<Option<Held>>,
    /// Rows free to take.
    free: Vec<Row>,
    /// Rows emptied in this version, free from the next one on: a Front end
    /// may show the Item that was there until it has seen this version, and
    /// a new Item in its row would take its place on screen.
    freed: Vec<Row>,
    ids: HashMap<Arc<str>, Row>,
    /// The rows whose Item has a keyword, by it in lowercase.
    keywords: HashMap<Arc<str>, Vec<Row>>,
    /// The rows whose Item has an Identity, by it.
    identities: HashMap<Arc<str>, Vec<Row>>,
    /// The name of each kind of Item the Segment has held.
    kinds: Vec<String>,
    offered: Vec<Offered>,
    /// Where a new Shelf goes: a file in this directory, or memory.
    dir: Option<Arc<Path>>,
    shelf: Option<Arc<Shelf>>,
    /// Where the next payload goes on the Shelf.
    end: u64,
    /// How many bytes of the Shelf no row uses any more.
    shelf_garbage: u64,
    /// Whether this version added, removed or changed what a search reads
    /// of a row. A version that only changed Items' Actions ranks every
    /// query as the last one did.
    keys_changed: bool,
    /// Whether this version offers an Action keyword no version before did,
    /// or is a Segment emptied, so the Daemon's table of them is built again.
    verbs_changed: bool,
}

/// The payloads of the Items one change adds, before they go on the Shelf.
struct Shelved {
    /// Where on the Shelf they go.
    at: u64,
    bytes: Vec<u8>,
    /// The Item being encoded.
    item: Vec<u8>,
}

/// Below this, a Shelf is never copied to drop its garbage.
const SMALL_SHELF: u64 = 1 << 16;

impl Segment {
    /// An empty Segment whose Items' payloads go to a file in `dir`, or stay
    /// in memory without one.
    pub fn new(dir: Option<Arc<Path>>) -> Self {
        Segment {
            dir,
            ..Segment::default()
        }
    }

    /// An empty Segment that keeps payloads where this one does.
    pub fn emptied(&self) -> Self {
        Segment {
            keys_changed: true,
            verbs_changed: true,
            ..Segment::new(self.dir.clone())
        }
    }

    /// Whether this version offers an Action keyword no version before it
    /// did, or is an emptied Segment.
    pub fn verbs_changed(&self) -> bool {
        self.verbs_changed
    }

    /// Whether this version added, removed or changed what a search reads
    /// of a row, rather than only Items' Actions.
    pub fn keys_changed(&self) -> bool {
        self.keys_changed
    }

    /// Starts the next version of the Segment, which Front ends will see
    /// whole: the rows emptied in the last one are free from here on.
    pub fn next_version(&mut self) {
        self.free.append(&mut self.freed);
        self.keys_changed = false;
        self.verbs_changed = false;
    }

    /// Makes one change within this version.
    pub fn edit(&mut self, edit: Edit) {
        match edit {
            Edit::Add(items) => {
                // The payloads go on the Shelf together, in one write.
                let mut shelved = Shelved {
                    at: self.end,
                    bytes: Vec::new(),
                    item: Vec::new(),
                };
                for item in items {
                    // A hidden Item stays out of the search.
                    if item.hidden {
                        self.remove(&item.id);
                    } else {
                        self.add(&item, &mut shelved);
                    }
                }
                self.shelve(&shelved);
            }
            Edit::Remove(ids) => {
                for id in ids {
                    self.remove(&id);
                }
            }
        }
        self.compact();
        if self.shelf_garbage * 2 >= self.end.max(SMALL_SHELF) {
            let fresh = Shelf::new(self.dir.as_deref());
            if let Err(err) = self.reshelve(fresh) {
                log::warn!("can't copy Items to a new file: {err}");
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    fn add(&mut self, item: &Item, shelved: &mut Shelved) {
        let folded = Folded::new(item);
        let row = if let Some(&row) = self.ids.get(item.id.as_str()) {
            if self.same_keys(row, item, &folded) {
                self.reshelve_row(row, item, shelved);
                return;
            }
            self.release(row);
            row
        } else {
            let row = if let Some(row) = self.free.pop() {
                row
            } else {
                self.held.push(None);
                self.every_letter.push(Letters::NONE);
                self.initials.push(Letters::NONE);
                self.keys.push(Keys::default());
                (self.held.len() - 1) as Row
            };
            self.ids.insert(item.id.as_str().into(), row);
            row
        };
        self.keys_changed = true;
        let starts = self.starts.len() as u32;
        self.starts.extend(&folded.title_starts);
        self.starts.extend(&folded.words_starts);
        let mut words_starts = folded.words_starts.len() as u32;
        if let Some(weak_at) = folded.weak_at {
            self.starts.push(weak_at);
            words_starts |= WEAK_AT;
        }
        let kind = item.kind.name();
        let kind = if let Some(n) = self.kinds.iter().position(|known| known == kind) {
            n
        } else {
            self.kinds.push(kind.to_owned());
            self.kinds.len() - 1
        } as u16;
        let keys = Keys {
            title: self.push_text(&folded.title),
            words: self.push_text(&folded.words),
            subtitle: self.push_text(&folded.subtitle),
            starts,
            title_starts: folded.title_starts.len() as u32,
            words_starts,
            keyword: folded.keyword,
            subtitle_initials: folded.subtitle_initials,
            letters: folded.letters,
            kind,
            filled: true,
        };
        let at = row as usize;
        self.keys[at] = keys;
        self.every_letter[at] = folded.every_letter;
        self.initials[at] = folded.initials;

        let keyword = item
            .keyword
            .as_ref()
            .map(|keyword| list(&mut self.keywords, &keyword.to_lowercase(), row));
        let identity = item
            .identity
            .as_deref()
            .map(|identity| list(&mut self.identities, identity, row));
        let actions = self.offer(kind, row, item);
        let place = self.put(item, shelved);
        let id = self
            .ids
            .get_key_value(item.id.as_str())
            .map(|(id, _)| Arc::clone(id))
            .expect("listed above");
        self.held[at] = Some(Held {
            id,
            identity,
            keyword,
            actions,
            place,
        });
    }

    /// Whether `item`, sent again for `row`, is searched by the same keys:
    /// the same kind, Identity, keyword and folded texts. Only its Actions,
    /// its icon or its kind's field may differ.
    fn same_keys(&self, row: Row, item: &Item, folded: &Folded) -> bool {
        let Some(held) = self.held(row) else {
            return false;
        };
        let keys = &self.keys[row as usize];
        let texts = self.keys_texts(row, keys);
        let weak_at = folded.weak_at.unwrap_or(folded.words.len() as u32);
        self.kinds[keys.kind as usize] == item.kind.name()
            && held.identity.as_deref() == item.identity.as_deref()
            && held.keyword.as_deref() == item.keyword.as_ref().map(|keyword| keyword.to_lowercase()).as_deref()
            && keys.keyword == folded.keyword
            && keys.letters == folded.letters
            && keys.subtitle_initials == folded.subtitle_initials
            && self.every_letter[row as usize] == folded.every_letter
            && texts.initials == folded.initials
            && texts.weak_at == weak_at
            && texts.title.chars == folded.title.slice(..)
            && texts.title.starts == folded.title_starts.as_slice()
            && texts.words.chars == folded.words.slice(..)
            && texts.words.starts == folded.words_starts.as_slice()
            && texts.subtitle == folded.subtitle.slice(..)
    }

    /// Puts `item`, sent again for `row` with the same search keys, on the
    /// Shelf, and offers its Actions: the row keeps its keys and its texts.
    fn reshelve_row(&mut self, row: Row, item: &Item, shelved: &mut Shelved) {
        let Some(held) = self.held.get_mut(row as usize).and_then(Option::take) else {
            return;
        };
        for &n in &held.actions {
            self.offered[n as usize].rows.remove(&row);
        }
        self.shelf_garbage += u64::from(held.place.len);
        let actions = self.offer(self.keys[row as usize].kind, row, item);
        let place = self.put(item, shelved);
        self.held[row as usize] = Some(Held { actions, place, ..held });
    }

    /// Lists `row` among those offering each of `item`'s Actions, and
    /// returns where those are in [`Segment::offered`].
    fn offer(&mut self, kind: u16, row: Row, item: &Item) -> Box<[u32]> {
        item.actions
            .iter()
            .map(|action| {
                let same = |offered: &Offered| {
                    offered.kind == kind
                        && *offered.action == *action.id
                        && offered.keywords.len() == action.keywords.len()
                        && offered
                            .keywords
                            .iter()
                            .zip(&action.keywords)
                            .all(|(folded, keyword)| folds_to(keyword, folded))
                };
                let n = if let Some(n) = self.offered.iter().position(same) {
                    n
                } else {
                    self.verbs_changed |= !action.keywords.is_empty();
                    self.offered.push(Offered {
                        kind,
                        action: action.id.as_str().into(),
                        keywords: action
                            .keywords
                            .iter()
                            .map(|keyword| fold_text(keyword).into_boxed_str())
                            .collect(),
                        rows: BTreeSet::new(),
                    });
                    self.offered.len() - 1
                };
                self.offered[n].rows.insert(row);
                n as u32
            })
            .collect()
    }

    /// Encodes `item` after the payloads `shelved` holds, and returns where
    /// it goes on the Shelf.
    fn put(&mut self, item: &Item, shelved: &mut Shelved) -> Place {
        encode(item, &mut shelved.item);
        let place = Place {
            at: shelved.at + shelved.bytes.len() as u64,
            len: shelved.item.len() as u32,
        };
        shelved.bytes.extend_from_slice(&shelved.item);
        self.end = place.end();
        place
    }

    fn remove(&mut self, id: &str) {
        let Some(row) = self.ids.remove(id) else {
            return;
        };
        self.keys_changed = true;
        self.release(row);
        let at = row as usize;
        self.every_letter[at] = Letters::NONE;
        self.initials[at] = Letters::NONE;
        self.keys[at] = Keys::default();
        self.freed.push(row);
    }

    /// Empties `row` of everything but its search keys, and counts what it
    /// kept of the arenas and the Shelf as garbage.
    fn release(&mut self, row: Row) {
        let Some(held) = self.held.get_mut(row as usize).and_then(Option::take) else {
            return;
        };
        if let Some(keyword) = &held.keyword {
            unlist(&mut self.keywords, keyword, row);
        }
        if let Some(identity) = &held.identity {
            unlist(&mut self.identities, identity, row);
        }
        for &n in &held.actions {
            self.offered[n as usize].rows.remove(&row);
        }
        self.shelf_garbage += u64::from(held.place.len);
        let keys = &self.keys[row as usize];
        self.garbage += [keys.title, keys.words, keys.subtitle]
            .iter()
            .map(|span| text_bytes(*span))
            .sum::<usize>()
            + keys.starts_len() as usize * size_of::<u32>();
    }

    /// Puts the payloads of the Items just added on the Shelf. Should the
    /// file fail, every payload moves to memory.
    fn shelve(&mut self, shelved: &Shelved) {
        if shelved.bytes.is_empty() {
            return;
        }
        let store = Arc::clone(self.shelf.get_or_insert_with(|| Shelf::new(self.dir.as_deref())));
        let Err(err) = store.put(shelved.at, &shelved.bytes) else {
            return;
        };
        log::error!("keeping Items in memory: can't write them to their file: {err}");
        let memory = Shelf::in_memory();
        let mut before = Vec::new();
        let moved = store
            .read_up_to(0, shelved.at, &mut before)
            .and_then(|()| memory.put(0, &before))
            .and_then(|()| memory.put(shelved.at, &shelved.bytes));
        if let Err(err) = moved {
            log::error!("Items are lost: can't read them back: {err}");
        }
        self.shelf = Some(memory);
    }

    /// Moves the payloads rows still hold to `to`, dropping the garbage:
    /// one read of the old Shelf and one write of the new.
    fn reshelve(&mut self, to: Arc<Shelf>) -> io::Result<()> {
        let Some(old) = self.shelf.clone() else {
            self.shelf = Some(to);
            self.end = 0;
            return Ok(());
        };
        let mut all = Vec::new();
        old.read_up_to(0, self.end, &mut all)?;
        let mut kept = Vec::with_capacity(all.len().saturating_sub(self.shelf_garbage as usize));
        let mut places = Vec::with_capacity(self.held.len());
        for held in &self.held {
            let Some(held) = held else {
                places.push(Place::default());
                continue;
            };
            let place = Place {
                at: kept.len() as u64,
                len: held.place.len,
            };
            kept.extend_from_slice(&all[held.place.at as usize..held.place.end() as usize]);
            places.push(place);
        }
        let end = kept.len() as u64;
        to.put(0, &kept)?;
        for (held, place) in self.held.iter_mut().zip(places) {
            if let Some(held) = held {
                held.place = place;
            }
        }
        self.shelf = Some(to);
        self.end = end;
        self.shelf_garbage = 0;
        Ok(())
    }

    fn push_text(&mut self, text: &Utf32String) -> Span {
        match text.slice(..) {
            Utf32Str::Ascii(bytes) => {
                let start = self.ascii.len() as u32;
                self.ascii.extend_from_slice(bytes);
                Span {
                    start,
                    len: bytes.len() as u32,
                }
            }
            Utf32Str::Unicode(chars) => {
                let start = self.wide.len() as u32;
                self.wide.extend_from_slice(chars);
                Span {
                    start,
                    len: chars.len() as u32 | WIDE,
                }
            }
        }
    }

    /// Copies the texts rows still use into new arenas, once at least half
    /// of the arenas is garbage.
    fn compact(&mut self) {
        let size = self.ascii.len() + self.wide.len() * size_of::<char>() + self.starts.len() * 4;
        if self.garbage * 2 < size.max(4096) {
            return;
        }
        let ascii = std::mem::take(&mut self.ascii);
        let wide = std::mem::take(&mut self.wide);
        let old_starts = std::mem::take(&mut self.starts);
        for at in 0..self.held.len() {
            if self.held[at].is_none() {
                continue;
            }
            let keys = self.keys[at];
            let starts = keys.starts as usize..(keys.starts + keys.starts_len()) as usize;
            let moved = Keys {
                title: self.copy_text(&ascii, &wide, keys.title),
                words: self.copy_text(&ascii, &wide, keys.words),
                subtitle: self.copy_text(&ascii, &wide, keys.subtitle),
                starts: self.starts.len() as u32,
                ..keys
            };
            self.starts.extend_from_slice(&old_starts[starts]);
            self.keys[at] = moved;
        }
        self.ascii.shrink_to_fit();
        self.wide.shrink_to_fit();
        self.starts.shrink_to_fit();
        self.garbage = 0;
    }

    fn copy_text(&mut self, ascii: &[u8], wide: &[char], span: Span) -> Span {
        if span.len & WIDE != 0 {
            let start = self.wide.len() as u32;
            self.wide.extend_from_slice(&wide[span.range()]);
            Span { start, ..span }
        } else {
            let start = self.ascii.len() as u32;
            self.ascii.extend_from_slice(&ascii[span.range()]);
            Span { start, ..span }
        }
    }

    #[inline]
    fn text(&self, span: Span) -> Utf32Str<'_> {
        if span.len & WIDE != 0 {
            Utf32Str::Unicode(&self.wide[span.range()])
        } else {
            Utf32Str::Ascii(&self.ascii[span.range()])
        }
    }

    fn held(&self, row: Row) -> Option<&Held> {
        self.held.get(row as usize)?.as_ref()
    }

    /// What a search reads of `row`, if it holds an Item.
    pub fn texts(&self, row: Row) -> Option<Texts<'_>> {
        let keys = self.keys.get(row as usize).filter(|keys| keys.filled)?;
        Some(self.keys_texts(row, keys))
    }

    /// The texts `keys`, the keys of `row`, point to.
    #[inline]
    fn keys_texts(&self, row: Row, keys: &Keys) -> Texts<'_> {
        let starts = keys.starts as usize;
        let words = starts + keys.title_starts as usize;
        let end = words + keys.words_starts() as usize;
        let weak_at = if keys.words_starts & WEAK_AT == 0 {
            keys.words.len() as u32
        } else {
            self.starts[end]
        };
        Texts {
            title: Text {
                chars: self.text(keys.title),
                starts: &self.starts[starts..words],
            },
            words: Text {
                chars: self.text(keys.words),
                starts: &self.starts[words..end],
            },
            weak_at,
            keyword: keys.keyword,
            subtitle: self.text(keys.subtitle),
            initials: self.initials[row as usize],
            subtitle_initials: keys.subtitle_initials,
            letters: keys.letters,
        }
    }

    /// Every letter of each row's texts, none for an empty row.
    pub fn every_letter(&self) -> &[Letters] {
        &self.every_letter
    }

    /// The word initials of each row's title and keywords, none for an empty
    /// row.
    pub fn initials(&self) -> &[Letters] {
        &self.initials
    }

    /// Calls `found` with each row, in order, whose bit `test` sets. `test`
    /// reads the rows' letters and initials, as [`Segment::every_letter`] and
    /// [`Segment::initials`] give them, a [`BLOCK`] at a time, so it can test
    /// a block in one vectorised loop; past the last row they are empty, and
    /// never found. Only the rows found are read further.
    #[inline]
    pub fn rows_where(&self, test: impl Fn(&[Letters; BLOCK], &[Letters; BLOCK]) -> Mask, mut found: impl FnMut(Row)) {
        let (letters, letters_left) = self.every_letter.as_chunks::<BLOCK>();
        let (initials, initials_left) = self.initials.as_chunks::<BLOCK>();
        for (n, (letters, initials)) in letters.iter().zip(initials).enumerate() {
            each_bit(test(letters, initials), n * BLOCK, &mut found);
        }
        if !letters_left.is_empty() {
            let mut last = ([Letters::NONE; BLOCK], [Letters::NONE; BLOCK]);
            last.0[..letters_left.len()].copy_from_slice(letters_left);
            last.1[..initials_left.len()].copy_from_slice(initials_left);
            let filled = (1 << letters_left.len()) - 1;
            each_bit(test(&last.0, &last.1) & filled, letters.len() * BLOCK, &mut found);
        }
    }

    /// Where the whole Item in `row` is.
    pub fn payload(&self, row: Row) -> Option<(&Arc<Shelf>, Place)> {
        let place = self.held(row)?.place;
        Some((self.shelf.as_ref()?, place))
    }

    /// The id of the Item in `row`.
    pub fn id(&self, row: Row) -> Option<&str> {
        self.held(row).map(|held| &*held.id)
    }

    /// The Identity of the Item in `row`, if it has one.
    pub fn identity(&self, row: Row) -> Option<&str> {
        self.held(row)?.identity.as_deref()
    }

    /// Every Identity the Segment's Items have.
    pub fn identities(&self) -> impl Iterator<Item = &str> {
        self.identities.keys().map(|identity| &**identity)
    }

    /// Each Action the Segment's Items offer, by Item kind and Action id,
    /// with the first row whose Item has it.
    pub fn offered(&self) -> impl Iterator<Item = (&str, &str, Row)> {
        self.offered.iter().filter_map(|offered| {
            let first = *offered.rows.first()?;
            Some((&*self.kinds[offered.kind as usize], &*offered.action, first))
        })
    }

    /// The rows whose Item, of kind `kind`, offers the Action `action`, in row order.
    pub fn offering(&self, kind: &str, action: &str) -> Vec<Row> {
        let mut rows: Vec<Row> = self
            .offered
            .iter()
            .filter(|offered| self.kinds[offered.kind as usize] == kind && *offered.action == *action)
            .flat_map(|offered| offered.rows.iter().copied())
            .collect();
        // The same Action with other keywords is offered apart.
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    /// Every keyword of the Actions the Segment's Items offer, folded, with
    /// repeats.
    pub fn verbs(&self) -> impl Iterator<Item = &str> {
        self.offered
            .iter()
            .filter(|offered| !offered.rows.is_empty())
            .flat_map(|offered| offered.keywords.iter().map(|keyword| &**keyword))
    }

    /// The id of the first of the Actions of the Item in `row` with a
    /// keyword `names` accepts, given folded.
    pub fn named_action(&self, row: Row, names: impl Fn(&str) -> bool) -> Option<&str> {
        let held = self.held(row)?;
        held.actions
            .iter()
            .map(|&n| &self.offered[n as usize])
            .find(|offered| offered.keywords.iter().any(|keyword| names(keyword)))
            .map(|offered| &*offered.action)
    }

    /// Scores the Item in `row`, if there is one, as [`Texts::score`] does,
    /// with `filter`'s letter already known to start one of its words. Reads
    /// only its title and keywords unless it matches, or its subtitle is
    /// searched: most rows a search reads, nucleo turns down. On a match,
    /// gives what ranking reads of it too.
    #[inline]
    pub fn score(
        &self,
        row: Row,
        pattern: &Pattern,
        matcher: &mut Matcher,
        filter: Filter,
    ) -> Option<(Ranked<'_>, u32, Matched)> {
        let keys = self.keys.get(row as usize).filter(|keys| keys.filled)?;
        let searched = Searched {
            title: self.text(keys.title),
            words: self.text(keys.words),
            subtitle_initials: keys.subtitle_initials,
        };
        let subtitle = || self.text(keys.subtitle);
        let (score, hits) = searched.score(subtitle, pattern, matcher, filter)?;
        let ranked = Ranked {
            segment: self,
            row,
            keys,
        };
        Some((ranked, score, hits))
    }

    /// What ranking reads of `row`, if it holds an Item.
    #[inline]
    pub fn ranked(&self, row: Row) -> Option<Ranked<'_>> {
        let keys = self.keys.get(row as usize).filter(|keys| keys.filled)?;
        Some(Ranked {
            segment: self,
            row,
            keys,
        })
    }

    /// The names of the kinds of Item the Segment has held.
    pub fn kinds(&self) -> &[String] {
        &self.kinds
    }

    pub fn find(&self, id: &str) -> Option<Row> {
        self.ids.get(id).copied()
    }

    /// The rows whose Item's keyword is `keyword`, given in lowercase.
    pub fn keyed(&self, keyword: &str) -> &[Row] {
        self.keywords.get(keyword).map_or(&[], Vec::as_slice)
    }

    /// The rows of every Item with a keyword.
    pub fn keyed_rows(&self) -> impl Iterator<Item = &Row> {
        self.keywords.values().flatten()
    }

    /// The rows whose Item's Identity is `identity`.
    pub fn identified(&self, identity: &str) -> &[Row] {
        self.identities.get(identity).map_or(&[], Vec::as_slice)
    }

    /// Reads every byte a search reads, so the next search finds them in the
    /// CPU's cache. Returns something of them, so the reading isn't
    /// optimized away.
    pub fn touch(&self) -> u64 {
        /// Reads one element of `slice` per cache line.
        fn lines<T>(slice: &[T], read: impl Fn(&T) -> u64) -> u64 {
            let step = (64 / size_of::<T>()).max(1);
            slice.iter().step_by(step).map(read).fold(0, |a, b| a ^ b)
        }
        lines(&self.every_letter, |letters| letters.bits())
            ^ lines(&self.initials, |letters| letters.bits())
            ^ lines(&self.keys, |keys| u64::from(keys.starts))
            ^ lines(&self.ascii, |&byte| u64::from(byte))
            ^ lines(&self.wide, |&char| u64::from(char))
            ^ lines(&self.starts, |&start| u64::from(start))
    }
}

/// Calls `found` with `first` plus the place of each bit set in `bits`,
/// lowest first.
#[inline]
fn each_bit(mut bits: Mask, first: usize, found: &mut impl FnMut(Row)) {
    while bits != 0 {
        found((first + bits.trailing_zeros() as usize) as Row);
        bits &= bits - 1;
    }
}

/// How many bytes of its arena a text takes.
fn text_bytes(span: Span) -> usize {
    if span.len & WIDE != 0 {
        span.len() * size_of::<char>()
    } else {
        span.len()
    }
}

/// Puts `row` on the list kept under `name`, and returns the name as the
/// lists keep it.
fn list(lists: &mut HashMap<Arc<str>, Vec<Row>>, name: &str, row: Row) -> Arc<str> {
    if let Some(name) = lists.get_key_value(name).map(|(name, _)| Arc::clone(name)) {
        lists.entry(Arc::clone(&name)).or_default().push(row);
        return name;
    }
    let name: Arc<str> = name.into();
    lists.insert(Arc::clone(&name), vec![row]);
    name
}

/// Takes `row` off the list kept under `name`, and the list off `lists` once
/// it is empty.
fn unlist(lists: &mut HashMap<Arc<str>, Vec<Row>>, name: &str, row: Row) {
    if let Some(rows) = lists.get_mut(name) {
        rows.retain(|&other| other != row);
        if rows.is_empty() {
            lists.remove(name);
        }
    }
}

#[cfg(test)]
mod tests;
