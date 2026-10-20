//! One Extension's Indexed items, laid out for scanning: what a keystroke
//! reads is worked out once, when an Item arrives, and kept in dense arrays
//! and one arena of text, so a search runs down them without following a
//! pointer per Item (ADR 0016).
//!
//! A Segment is built off the keystroke path and then only read, so a search
//! can share it with the thread building the next one. Each Extension has its
//! own, so a change to one Extension's Items copies only its Segment.
//!
//! Of the rest of an Item, a Segment keeps in memory only what ranking reads:
//! its id, Identity, keyword and Actions. The whole Item waits on a
//! [`Shelf`], off the heap, until a Result shows it.

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::Path;
use std::sync::Arc;

use kotodama_protocol::Item;
use nucleo_matcher::{Utf32Str, Utf32String};

use crate::payload::{Place, Shelf, encode};
use crate::rank::{Folded, Letters, Text, Texts};

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
    /// the keywords'.
    starts: u32,
    title_starts: u32,
    words_starts: u32,
    subtitle_initials: Letters,
    /// Every letter of the title and keywords.
    letters: Letters,
    /// The Item's kind, as an index into [`Segment::kinds`].
    kind: u16,
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

/// An Action the Segment's Items offer: an Item kind and an Action id, and
/// the rows whose Item has it.
#[derive(Clone, Debug)]
struct Offered {
    kind: u16,
    action: Box<str>,
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
        Segment::new(self.dir.clone())
    }

    /// Starts the next version of the Segment, which Front ends will see
    /// whole: the rows emptied in the last one are free from now on.
    pub fn next_version(&mut self) {
        self.free.append(&mut self.freed);
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
                        self.add(item, &mut shelved);
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
            let shelf = Shelf::new(self.dir.as_deref());
            if let Err(err) = self.reshelve(shelf) {
                log::warn!("can't copy Items to a new file: {err}");
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    fn add(&mut self, item: Item, shelved: &mut Shelved) {
        let row = match self.ids.get(item.id.as_str()) {
            Some(&row) => {
                self.release(row);
                row
            }
            None => {
                let row = match self.free.pop() {
                    Some(row) => row,
                    None => {
                        self.held.push(None);
                        self.every_letter.push(Letters::NONE);
                        self.initials.push(Letters::NONE);
                        self.keys.push(Keys::default());
                        (self.held.len() - 1) as Row
                    }
                };
                self.ids.insert(item.id.as_str().into(), row);
                row
            }
        };
        let folded = Folded::new(&item);
        let starts = self.starts.len() as u32;
        self.starts.extend(&folded.title_starts);
        self.starts.extend(&folded.words_starts);
        let kind = item.kind.name();
        let kind = match self.kinds.iter().position(|known| known == kind) {
            Some(n) => n,
            None => {
                self.kinds.push(kind.to_owned());
                self.kinds.len() - 1
            }
        } as u16;
        let keys = Keys {
            title: self.push_text(&folded.title),
            words: self.push_text(&folded.words),
            subtitle: self.push_text(&folded.subtitle),
            starts,
            title_starts: folded.title_starts.len() as u32,
            words_starts: folded.words_starts.len() as u32,
            subtitle_initials: folded.subtitle_initials,
            letters: folded.letters,
            kind,
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
        let actions =
            item.actions
                .iter()
                .map(|action| {
                    let n =
                        match self.offered.iter().position(|offered| {
                            offered.kind == kind && *offered.action == *action.id
                        }) {
                            Some(n) => n,
                            None => {
                                self.offered.push(Offered {
                                    kind,
                                    action: action.id.as_str().into(),
                                    rows: BTreeSet::new(),
                                });
                                self.offered.len() - 1
                            }
                        };
                    self.offered[n].rows.insert(row);
                    n as u32
                })
                .collect();
        encode(&item, &mut shelved.item);
        let place = Place {
            at: shelved.at + shelved.bytes.len() as u64,
            len: shelved.item.len() as u32,
        };
        shelved.bytes.extend_from_slice(&shelved.item);
        self.end = place.end();
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

    fn remove(&mut self, id: &str) {
        let Some(row) = self.ids.remove(id) else {
            return;
        };
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
            + (keys.title_starts + keys.words_starts) as usize * size_of::<u32>();
    }

    /// Puts the payloads of the Items just added on the Shelf. Should the
    /// file fail, every payload moves to memory.
    fn shelve(&mut self, shelved: &Shelved) {
        if shelved.bytes.is_empty() {
            return;
        }
        let shelf = Arc::clone(
            self.shelf
                .get_or_insert_with(|| Shelf::new(self.dir.as_deref())),
        );
        let Err(err) = shelf.put(shelved.at, &shelved.bytes) else {
            return;
        };
        log::error!("keeping Items in memory: can't write them to their file: {err}");
        let memory = Shelf::in_memory();
        let mut before = Vec::new();
        let moved = shelf
            .read(
                Place {
                    at: 0,
                    len: shelved.at as u32,
                },
                &mut before,
            )
            .and_then(|()| memory.put(0, &before))
            .and_then(|()| memory.put(shelved.at, &shelved.bytes));
        if let Err(err) = moved {
            log::error!("Items are lost: can't read them back: {err}");
        }
        self.shelf = Some(memory);
    }

    /// Moves the payloads rows still hold to `shelf`, dropping the garbage:
    /// one read of the old Shelf and one write of the new.
    fn reshelve(&mut self, shelf: Arc<Shelf>) -> io::Result<()> {
        let Some(old) = self.shelf.clone() else {
            self.shelf = Some(shelf);
            self.end = 0;
            return Ok(());
        };
        let mut all = Vec::new();
        old.read(
            Place {
                at: 0,
                len: self.end as u32,
            },
            &mut all,
        )?;
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
        shelf.put(0, &kept)?;
        for (held, place) in self.held.iter_mut().zip(places) {
            if let Some(held) = held {
                held.place = place;
            }
        }
        self.shelf = Some(shelf);
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
            let starts = keys.starts as usize
                ..(keys.starts + keys.title_starts + keys.words_starts) as usize;
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
        self.held(row)?;
        let keys = &self.keys[row as usize];
        let starts = keys.starts as usize;
        let words = starts + keys.title_starts as usize;
        Some(Texts {
            title: Text {
                chars: self.text(keys.title),
                starts: &self.starts[starts..words],
            },
            words: Text {
                chars: self.text(keys.words),
                starts: &self.starts[words..words + keys.words_starts as usize],
            },
            subtitle: self.text(keys.subtitle),
            initials: self.initials[row as usize],
            subtitle_initials: keys.subtitle_initials,
            letters: keys.letters,
        })
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

    /// The title's length in characters.
    pub fn title_len(&self, row: Row) -> u32 {
        self.keys[row as usize].title.len() as u32
    }

    /// The kind of the Item in `row`, as an index into [`Segment::kinds`].
    pub fn kind(&self, row: Row) -> usize {
        self.keys[row as usize].kind.into()
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
mod tests {
    use super::*;
    use kotodama_protocol::{Action, ItemKind};

    fn item(id: &str, title: &str, actions: &[&str]) -> Item {
        Item {
            id: id.into(),
            kind: ItemKind::Url {
                url: format!("https://example.org/{id}"),
            },
            identity: None,
            title: title.into(),
            subtitle: Some("A subtitle long enough to fill the Shelf".into()),
            icon: None,
            keyword: None,
            keywords: vec![],
            hidden: false,
            actions: actions
                .iter()
                .map(|&id| Action {
                    id: id.into(),
                    title: format!("{id} it"),
                    shortcut: None,
                    arguments: vec![],
                })
                .collect(),
        }
    }

    fn read(segment: &Segment, id: &str) -> Option<Item> {
        let (shelf, place) = segment.payload(segment.find(id)?)?;
        shelf.get(place)
    }

    #[test]
    fn items_read_back_as_last_sent_after_the_shelf_drops_its_garbage() {
        let dir = tempfile::tempdir().unwrap();
        for mut segment in [Segment::default(), Segment::new(Some(dir.path().into()))] {
            changed_over_and_over(&mut segment);
        }
    }

    fn changed_over_and_over(segment: &mut Segment) {
        let ids: Vec<String> = (0..100).map(|n| format!("item-{n}")).collect();
        let shelves: std::collections::HashSet<u64> = (0..20)
            .map(|round| {
                let items = ids
                    .iter()
                    .map(|id| item(id, &format!("{id} round {round}"), &["open"]))
                    .collect();
                segment.edit(Edit::Add(items));
                segment.shelf.as_ref().unwrap().id()
            })
            .collect();
        assert!(shelves.len() > 1, "the Shelf was copied");
        for id in &ids {
            assert_eq!(read(segment, id).unwrap().title, format!("{id} round 19"));
        }
    }

    #[test]
    fn an_earlier_version_reads_its_own_items_after_a_later_one_replaced_them() {
        let mut segment = Segment::default();
        segment.edit(Edit::Add(vec![item("a", "First", &["open"])]));
        let earlier = segment.clone();
        segment.next_version();
        segment.edit(Edit::Add(vec![item("a", "Second", &["open"])]));
        assert_eq!(read(&earlier, "a").unwrap().title, "First");
        assert_eq!(read(&segment, "a").unwrap().title, "Second");
    }

    #[test]
    fn each_action_on_offer_comes_with_the_first_row_whose_item_has_it() {
        let mut segment = Segment::default();
        segment.edit(Edit::Add(vec![
            item("a", "A", &["open"]),
            item("b", "B", &["open", "copy"]),
            item("c", "C", &["copy"]),
        ]));
        segment.edit(Edit::Remove(vec!["a".into()]));
        let offered: Vec<_> = segment.offered().collect();
        assert_eq!(offered, [("url", "open", 1), ("url", "copy", 1)]);
    }
}
