//! Every Indexed item, kept in memory and searched with nucleo on each
//! keystroke, with no IPC to Extensions: one [`Segment`] per Extension, each
//! replaced whole when its Items change.

use std::sync::Arc;

use kotodama_protocol::Item;
use nucleo_matcher::Matcher;
use nucleo_matcher::pattern::Pattern;

use crate::daemon::ExtensionId;
use crate::rank::{Filter, Letters, Matched, Texts};
use crate::segment::{Edit, Row, Segment};

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

#[derive(Default)]
pub struct Index {
    /// By Extension.
    segments: Vec<Arc<Segment>>,
    /// Bumped on every change, so a search narrowed from an earlier one knows
    /// when it can't be trusted.
    version: u64,
}

impl Index {
    pub fn new(extensions: usize) -> Self {
        Index {
            segments: (0..extensions).map(|_| Arc::default()).collect(),
            version: 0,
        }
    }

    /// Makes room for one more Extension's Items.
    pub fn add_extension(&mut self) {
        self.segments.push(Arc::default());
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Makes one change to an Extension's Items, here and now.
    pub fn edit(&mut self, extension: ExtensionId, edit: Edit) {
        self.version += 1;
        Arc::make_mut(&mut self.segments[extension.0]).edit(edit);
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
        self.replace(extension, Arc::default());
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

    /// The Items whose keyword is `keyword`, given in lowercase.
    pub fn keyed<'a>(&'a self, keyword: &'a str) -> impl Iterator<Item = Slot> + 'a {
        self.segments
            .iter()
            .enumerate()
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

    /// The slots, in order, whose Item has a word starting with one of
    /// `initials`, as [`Haystack::initials`](crate::rank::Haystack::initials)
    /// gives them.
    pub fn starting_with(&self, initials: Letters) -> impl Iterator<Item = Slot> + '_ {
        self.segments
            .iter()
            .enumerate()
            .flat_map(move |(n, segment)| {
                segment
                    .initials()
                    .iter()
                    .enumerate()
                    .filter(move |&(_, &item)| item.meets(initials))
                    .map(move |(row, _)| Slot {
                        segment: n as u32,
                        row: row as Row,
                    })
            })
    }

    pub fn item(&self, slot: Slot) -> Option<&Item> {
        self.segments.get(slot.segment as usize)?.item(slot.row)
    }

    pub fn texts(&self, slot: Slot) -> Option<Texts<'_>> {
        self.segments.get(slot.segment as usize)?.texts(slot.row)
    }

    /// The title's length in characters, of an Item that is there.
    pub fn title_len(&self, slot: Slot) -> u32 {
        self.segments[slot.segment as usize].title_len(slot.row)
    }

    /// The name of the kind of an Item that is there.
    pub fn kind(&self, slot: Slot) -> &str {
        let segment = &self.segments[slot.segment as usize];
        &segment.kinds()[segment.kind(slot.row)]
    }

    /// One Extension's Items.
    pub fn items(&self, extension: ExtensionId) -> impl Iterator<Item = &Item> {
        self.segments[extension.0].items()
    }

    /// Scores every Item against `pattern`, or only the slots in `within` when
    /// the caller knows nothing else can match. Appends `(slot, score)` for
    /// each match, with what matched, to `out`, in slot order if `within` is.
    /// `filter` is as [`Texts::score`] takes it.
    pub fn search(
        &self,
        pattern: &Pattern,
        matcher: &mut Matcher,
        filter: Filter,
        within: Option<&[Slot]>,
        out: &mut Vec<(Slot, u32, Matched)>,
    ) {
        match within {
            Some(slots) => {
                for &slot in slots {
                    let Some(segment) = self.segments.get(slot.segment as usize) else {
                        continue;
                    };
                    if segment
                        .every_letter()
                        .get(slot.row as usize)
                        .is_some_and(|&letters| filter.may_match(letters))
                        && let Some(texts) = segment.texts(slot.row)
                        && let Some((score, matched)) = texts.score(pattern, matcher, filter)
                    {
                        out.push((slot, score, matched));
                    }
                }
            }
            None => {
                for (n, segment) in self.segments.iter().enumerate() {
                    for (row, &letters) in segment.every_letter().iter().enumerate() {
                        if filter.may_match(letters)
                            && let Some(texts) = segment.texts(row as Row)
                            && let Some((score, matched)) = texts.score(pattern, matcher, filter)
                        {
                            let slot = Slot {
                                segment: n as u32,
                                row: row as Row,
                            };
                            out.push((slot, score, matched));
                        }
                    }
                }
            }
        }
    }
}
