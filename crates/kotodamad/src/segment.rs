//! One Extension's Indexed items, laid out for scanning: what a keystroke
//! reads is worked out once, when an Item arrives, and kept in dense arrays
//! and one arena of text, so a search runs down them without following a
//! pointer per Item (ADR 0015).
//!
//! A Segment is built off the keystroke path and then only read, so a search
//! can share it with the thread building the next one. Each Extension has its
//! own, so a change to one Extension's Items copies only its Segment.

use std::collections::HashMap;
use std::sync::Arc;

use kotodama_protocol::Item;
use nucleo_matcher::{Utf32Str, Utf32String};

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
    items: Vec<Option<Arc<Item>>>,
    /// Rows free to take.
    free: Vec<Row>,
    /// Rows emptied in this version, free from the next one on: a Front end
    /// may show the Item that was there until it has seen this version, and
    /// a new Item in its row would take its place on screen.
    freed: Vec<Row>,
    ids: HashMap<String, Row>,
    /// The rows whose Item has a keyword, by it in lowercase.
    keywords: HashMap<String, Vec<Row>>,
    /// The rows whose Item has an Identity, by it.
    identities: HashMap<String, Vec<Row>>,
    /// The name of each kind of Item the Segment has held.
    kinds: Vec<String>,
}

impl Segment {
    /// Starts the next version of the Segment, which Front ends will see
    /// whole: the rows emptied in the last one are free from now on.
    pub fn next_version(&mut self) {
        self.free.append(&mut self.freed);
    }

    /// Makes one change within this version.
    pub fn edit(&mut self, edit: Edit) {
        match edit {
            Edit::Add(items) => {
                for item in items {
                    // A hidden Item stays out of the search.
                    if item.hidden {
                        self.remove(&item.id);
                    } else {
                        self.add(item);
                    }
                }
            }
            Edit::Remove(ids) => {
                for id in ids {
                    self.remove(&id);
                }
            }
        }
        self.compact();
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    fn add(&mut self, item: Item) {
        let row = match self.ids.get(&item.id) {
            Some(&row) => {
                self.unkey(row);
                self.drop_texts(row);
                row
            }
            None => {
                let row = match self.free.pop() {
                    Some(row) => row,
                    None => {
                        self.items.push(None);
                        self.every_letter.push(Letters::NONE);
                        self.initials.push(Letters::NONE);
                        self.keys.push(Keys::default());
                        (self.items.len() - 1) as Row
                    }
                };
                self.ids.insert(item.id.clone(), row);
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
        };
        let keys = Keys {
            title: self.push_text(&folded.title),
            words: self.push_text(&folded.words),
            subtitle: self.push_text(&folded.subtitle),
            starts,
            title_starts: folded.title_starts.len() as u32,
            words_starts: folded.words_starts.len() as u32,
            subtitle_initials: folded.subtitle_initials,
            letters: folded.letters,
            kind: kind as u16,
        };
        let at = row as usize;
        self.keys[at] = keys;
        self.every_letter[at] = folded.every_letter;
        self.initials[at] = folded.initials;
        if let Some(keyword) = &item.keyword {
            self.keywords
                .entry(keyword.to_lowercase())
                .or_default()
                .push(row);
        }
        if let Some(identity) = &item.identity {
            self.identities
                .entry(identity.clone())
                .or_default()
                .push(row);
        }
        self.items[at] = Some(Arc::new(item));
    }

    fn remove(&mut self, id: &str) {
        let Some(row) = self.ids.remove(id) else {
            return;
        };
        self.unkey(row);
        self.drop_texts(row);
        let at = row as usize;
        self.items[at] = None;
        self.every_letter[at] = Letters::NONE;
        self.initials[at] = Letters::NONE;
        self.keys[at] = Keys::default();
        self.freed.push(row);
    }

    /// Forgets the keyword and the Identity of the Item in `row`.
    fn unkey(&mut self, row: Row) {
        let Some(item) = self.items.get(row as usize).and_then(Option::as_ref) else {
            return;
        };
        if let Some(keyword) = &item.keyword {
            unlist(&mut self.keywords, &keyword.to_lowercase(), row);
        }
        if let Some(identity) = &item.identity {
            unlist(&mut self.identities, identity, row);
        }
    }

    /// Counts the texts of `row` as garbage.
    fn drop_texts(&mut self, row: Row) {
        let keys = &self.keys[row as usize];
        self.garbage += [keys.title, keys.words, keys.subtitle]
            .iter()
            .map(|span| text_bytes(*span))
            .sum::<usize>()
            + (keys.title_starts + keys.words_starts) as usize * size_of::<u32>();
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
        let old = Segment {
            ascii: std::mem::take(&mut self.ascii),
            wide: std::mem::take(&mut self.wide),
            starts: std::mem::take(&mut self.starts),
            ..Segment::default()
        };
        for at in 0..self.items.len() {
            if self.items[at].is_none() {
                continue;
            }
            let keys = self.keys[at];
            let starts = keys.starts as usize
                ..(keys.starts + keys.title_starts + keys.words_starts) as usize;
            let moved = Keys {
                title: self.copy_text(&old, keys.title),
                words: self.copy_text(&old, keys.words),
                subtitle: self.copy_text(&old, keys.subtitle),
                starts: self.starts.len() as u32,
                ..keys
            };
            self.starts.extend_from_slice(&old.starts[starts]);
            self.keys[at] = moved;
        }
        self.ascii.shrink_to_fit();
        self.wide.shrink_to_fit();
        self.starts.shrink_to_fit();
        self.garbage = 0;
    }

    fn copy_text(&mut self, from: &Segment, span: Span) -> Span {
        if span.len & WIDE != 0 {
            let start = self.wide.len() as u32;
            self.wide.extend_from_slice(&from.wide[span.range()]);
            Span { start, ..span }
        } else {
            let start = self.ascii.len() as u32;
            self.ascii.extend_from_slice(&from.ascii[span.range()]);
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

    /// What a search reads of `row`, if it holds an Item.
    pub fn texts(&self, row: Row) -> Option<Texts<'_>> {
        self.items.get(row as usize)?.as_ref()?;
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

    pub fn item(&self, row: Row) -> Option<&Item> {
        self.items.get(row as usize)?.as_deref()
    }

    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter_map(Option::as_deref)
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
}

/// How many bytes of its arena a text takes.
fn text_bytes(span: Span) -> usize {
    if span.len & WIDE != 0 {
        span.len() * size_of::<char>()
    } else {
        span.len()
    }
}

/// Takes `row` off the list kept under `name`, and the list off `lists` once
/// it is empty.
fn unlist(lists: &mut HashMap<String, Vec<Row>>, name: &str, row: Row) {
    if let Some(rows) = lists.get_mut(name) {
        rows.retain(|&other| other != row);
        if rows.is_empty() {
            lists.remove(name);
        }
    }
}
