//! Indexing off the keystroke path: Extensions' changes to their
//! Items are made on a thread of their own, into a new version of each
//! changed Segment, which the Daemon puts in whole between two events. A keystroke never waits for
//! a change, and never sees half of one.
//!
//! Changes that queue up while the thread works are made together, one new
//! version of each Segment for all of them.
//!
//! The thread also reads, off their Shelves, the Items the Daemon keeps
//! resident: the hot ones, which it plans here from a copy of the History's
//! picks, and those a keyword or an Alias names. Each read goes out with the
//! version it was read from, so the thread answering keystrokes never reads
//! a payload for them, nor plans.
//!
//! # Owns
//!
//! - [`Indexer`]: the `index` thread and the changes it coalesces.
//! - [`Indexed`], [`Kept`] and [`Plan`]: what it sends back, and the hot Items it plans.
//!
//! # Not here
//!
//! - Putting a new version in: `daemon/from_extension.rs`. The Segments: `segment.rs`.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::hash::BuildHasherDefault;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::thread;

use crate::clock::When;
use crate::daemon::Event;
use crate::daemon::ExtensionId;
use crate::history;
use crate::history::Habit;
use crate::index;
use crate::index::Index;
use crate::index::Read;
use crate::index::SIDES;
use crate::index::Slot;
use crate::index::SlotHasher;
use crate::most_used::Ahead;
use crate::most_used::Usage;
use crate::most_used::Versions;
use crate::segment::Edit;
use crate::segment::Row;
use crate::segment::Segment;

/// A new version of an Extension's Segments, those that changed, for the
/// Daemon to put in together: an Item moved from one to the other is never
/// in neither, nor in both.
pub struct Indexed {
    pub(crate) extension: ExtensionId,
    /// The last change they hold.
    pub(crate) change: u64,
    /// Each with where it goes in the Index.
    pub(crate) segments: Vec<(usize, Arc<Segment>)>,
    /// Items of them to keep resident, read since the last version.
    pub(crate) reads: Vec<Read>,
}

/// Items to keep resident, read off the Segments the Daemon has.
pub struct Kept {
    /// The hot Items, in slot order, when they were planned again.
    pub(crate) hot: Option<Vec<Slot>>,
    /// The Empty query's most used Results, ranked ahead as the hot Items
    /// were planned.
    pub(crate) ahead: Option<Ahead>,
    pub(crate) reads: Vec<Read>,
}

/// The Segments and the reads by count: their rows would fill a log.
impl fmt::Debug for Indexed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Indexed")
            .field("extension", &self.extension)
            .field("change", &self.change)
            .field("segments", &self.segments.len())
            .field("reads", &self.reads.len())
            .finish()
    }
}

/// The reads by count, as [`Indexed`] has them.
impl fmt::Debug for Kept {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Kept")
            .field("hot", &self.hot)
            .field("ahead", &self.ahead)
            .field("reads", &self.reads.len())
            .finish()
    }
}

/// How long before a time of day an Item picked then is hot.
const HOT_LEAD_SECS: i64 = 15 * 60;
/// How long after a time of day an Item picked then stays hot.
const HOT_GRACE_SECS: i64 = 30 * 60;
/// Learned choices take up to one in this many of the hot Items before the
/// time-of-day habit takes the rest, and then whatever room the habit
/// leaves. A quarter of 256 is about the queries a user types often enough
/// to teach a choice; the habit keeps the most room, as it predicts the
/// Empty query and the first keystrokes.
const LEARNED_SHARE: usize = 4;

/// What the hot Items are worked out from: a copy of what the Empty query
/// ranks by, the Learned choices, and the time.
pub struct Plan {
    pub usage: Usage,
    /// The Items that are a query's Learned choice, strongest first.
    pub learned: Arc<[Slot]>,
    pub when: When,
    pub capacity: usize,
    /// What `usage` was gathered from, for the rows ranked ahead.
    pub versions: Versions,
}

impl Plan {
    /// The hot Items, in slot order, and the Empty query's most used Results
    /// ranked ahead: for the current time, and for the start of the coming part of the
    /// day.
    pub fn run(&self) -> (Vec<Slot>, Ahead) {
        let mut habits = Vec::new();
        let ranked = [self.when, history::next_part(self.when)]
            .map(|when| (when, self.usage.rank(when, &mut habits)))
            .into();
        let ahead = Ahead {
            versions: self.versions,
            ranked,
        };
        let hot = self.hot(ahead.slots(), &mut habits);
        (hot, ahead)
    }

    /// The hot Items, in slot order, up to the capacity. The Items the
    /// Empty query shows go first, then the strongest Learned choices, up to
    /// a [`LEARNED_SHARE`]th of the capacity, then the Items picked most
    /// near this time of day, from a little before it to a little after,
    /// then Learned choices again while there is room.
    fn hot(&self, shown: impl Iterator<Item = Slot>, habits: &mut Vec<Habit>) -> Vec<Slot> {
        let capacity = self.capacity;
        if capacity == 0 {
            return Vec::new();
        }
        let mut hot = Hot {
            slots: Vec::with_capacity(capacity),
            taken: HashSet::default(),
            capacity,
        };
        hot.take(shown);
        let mut learned = self.learned.iter().copied();
        hot.take(learned.by_ref().take(capacity / LEARNED_SHARE));
        hot.take(self.habit(habits));
        hot.take(learned);
        let mut slots = hot.slots;
        slots.sort_unstable();
        slots
    }

    /// The Items picked near this time of day, from a little before it to a
    /// little after, most first.
    fn habit(&self, habits: &mut Vec<Habit>) -> Vec<Slot> {
        let picked = &*self.usage.picked;
        let mut near = vec![0.0f64; picked.slots.len()];
        for by in [-HOT_GRACE_SECS, 0, HOT_LEAD_SECS] {
            picked.picks.habits(self.when.plus(by), habits);
            for (near, habit) in near.iter_mut().zip(habits.iter()) {
                *near = near.max(habit.near_now);
            }
        }
        let mut habit: Vec<(f64, Slot)> = near
            .into_iter()
            .zip(&picked.slots)
            .filter(|&(near, _)| near > 0.0)
            .map(|(near, &(slot, _))| (near, slot))
            .collect();
        habit.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        habit.into_iter().map(|(_, slot)| slot).collect()
    }
}

/// The hot Items as they are taken, each once, up to the capacity.
struct Hot {
    slots: Vec<Slot>,
    taken: HashSet<Slot, BuildHasherDefault<SlotHasher>>,
    capacity: usize,
}

impl Hot {
    /// Takes those of `slots` not taken already, in order, until full.
    fn take(&mut self, slots: impl IntoIterator<Item = Slot>) {
        for slot in slots {
            if self.slots.len() == self.capacity {
                return;
            }
            if self.taken.insert(slot) {
                self.slots.push(slot);
            }
        }
    }
}

enum Job {
    Edit {
        extension: ExtensionId,
        change: u64,
        edit: Edit,
    },
    /// The Extension stopped: its Items are gone.
    Clear(ExtensionId),
    /// Work out the hot Items again.
    Plan(Plan),
    /// These Items, by Extension and id, are what the Aliases name.
    Pin(Vec<(ExtensionId, String)>),
    /// Versions the Index has replaced, to free here.
    Free(Vec<Arc<Segment>>),
}

/// The thread that makes changes to the Index, and how to give it more.
pub struct Indexer {
    jobs: Sender<Job>,
    /// Numbers each change.
    changes: u64,
    /// The Items the Aliases name, as the thread was told last.
    aliases: Vec<(ExtensionId, String)>,
}

impl Indexer {
    /// Starts the thread, which sends each new version to `events` and keeps
    /// payloads in files in `dir`, or in memory. It stops when the Indexer is
    /// dropped.
    pub fn start(events: Sender<Event>, dir: Option<Arc<Path>>) -> Self {
        let (jobs, queue) = mpsc::channel();
        if let Err(err) = thread::Builder::new()
            .name("index".into())
            .spawn(move || work(&queue, dir.as_ref(), |event| events.send(event).is_ok()))
        {
            log::error!("can't start the thread that indexes Items: {err}");
        }
        Indexer {
            jobs,
            changes: 0,
            aliases: Vec::new(),
        }
    }

    /// Hands a change to the thread. Returns its number.
    pub fn edit(&mut self, extension: ExtensionId, edit: Edit) -> u64 {
        self.changes += 1;
        let _ = self.jobs.send(Job::Edit {
            extension,
            change: self.changes,
            edit,
        });
        self.changes
    }

    /// Tells the thread an Extension's Items are gone. Returns the number of
    /// the last change: the versions holding it or earlier ones are stale.
    pub fn clear(&mut self, extension: ExtensionId) -> u64 {
        let _ = self.jobs.send(Job::Clear(extension));
        self.changes
    }

    /// Has the thread work out the hot Items and read them; they come back
    /// as an [`Event::Kept`].
    pub fn plan(&mut self, plan: Plan) {
        let _ = self.jobs.send(Job::Plan(plan));
    }

    /// Hands the thread versions of Segments the Index replaced, to free
    /// there: freeing one frees each of its rows' allocations, thousands
    /// for an Extension with thousands of Items, which the thread
    /// answering keystrokes would otherwise do.
    pub fn free(&mut self, segments: Vec<Arc<Segment>>) {
        if !segments.is_empty() {
            let _ = self.jobs.send(Job::Free(segments));
        }
    }

    /// Tells the thread the Items the Aliases name, to read and keep read.
    pub fn pin(&mut self, aliases: Vec<(ExtensionId, String)>) {
        if aliases != self.aliases {
            self.aliases.clone_from(&aliases);
            let _ = self.jobs.send(Job::Pin(aliases));
        }
    }
}

/// What the thread keeps resident in the Daemon, and where each was read.
#[derive(Default)]
struct Keeping {
    hot: Vec<Slot>,
    aliases: Vec<(ExtensionId, String)>,
    /// Where each Item sent was read from, so it is sent again only once it
    /// moved.
    sent: HashMap<Slot, (u64, u64)>,
}

impl Keeping {
    /// Reads the Items to keep in Segment `at` that moved since they were
    /// sent.
    fn read(&mut self, segments: &[Arc<Segment>], at: usize, reads: &mut Vec<Read>) {
        let Some(segment) = segments.get(at) else {
            return;
        };
        let n = at as u32;
        let extension = ExtensionId(at / SIDES);
        let keyed = segment.keyed_rows().copied();
        let aliased = self
            .aliases
            .iter()
            .filter(|(ext, _)| *ext == extension)
            .filter_map(|(_, id)| segment.find(id));
        let hot = self.hot.iter().filter(|slot| slot.segment == n).map(|slot| slot.row);
        let mut rows: Vec<Row> = keyed.chain(aliased).chain(hot).collect();
        rows.sort_unstable();
        rows.dedup();
        for row in rows {
            let slot = Slot { segment: n, row };
            let Some(place) = index::place_of(segment, row) else {
                continue;
            };
            if self.sent.get(&slot) == Some(&place) {
                continue;
            }
            if let Some(read) = index::read(segments, slot) {
                self.sent.insert(slot, read.from());
                reads.push(read);
            }
        }
    }

    /// Reads the Items to keep in every Segment that moved since they were
    /// sent.
    fn read_all(&mut self, segments: &[Arc<Segment>]) -> Vec<Read> {
        let mut reads = Vec::new();
        for at in 0..segments.len() {
            self.read(segments, at, &mut reads);
        }
        reads
    }
}

/// Makes the changes from `queue`, all that are waiting at once, and passes
/// each new version, then what to keep resident, to `send`, until it fails
/// or the queue closes.
fn work(queue: &Receiver<Job>, dir: Option<&Arc<Path>>, mut send: impl FnMut(Event) -> bool) {
    let empty = || Arc::new(Segment::new(dir.cloned()));
    let mut segments: Vec<Arc<Segment>> = Vec::new();
    let mut keeping = Keeping::default();
    while let Ok(first) = queue.recv() {
        // By Extension, the last change its new version holds, and which of
        // its Segments changed.
        let mut changed: Vec<(ExtensionId, u64, [bool; SIDES])> = Vec::new();
        let mut plan = None;
        let mut pinned = false;
        for job in std::iter::once(first).chain(queue.try_iter()) {
            match job {
                Job::Edit {
                    extension,
                    change,
                    edit,
                } => {
                    let places = Index::segments_of(extension);
                    if segments.len() < places.end {
                        segments.resize_with(places.end, empty);
                    }
                    let at = places.start;
                    let edits = index::split(edit, [&segments[at], &segments[at + 1]]);
                    // A change that touches neither Segment still makes a
                    // version, of the first, so whoever waits for it hears.
                    let untouched = edits.iter().all(Vec::is_empty);
                    let n = if let Some(n) = changed.iter().position(|(ext, ..)| *ext == extension) {
                        n
                    } else {
                        changed.push((extension, change, [false; SIDES]));
                        changed.len() - 1
                    };
                    let (_, last, sides) = &mut changed[n];
                    *last = change;
                    for (side, edits) in edits.into_iter().enumerate() {
                        if edits.is_empty() && !(untouched && side == 0) {
                            continue;
                        }
                        // Copies the version the Daemon has, once for all
                        // the changes to it waiting.
                        let segment = Arc::make_mut(&mut segments[at + side]);
                        if !sides[side] {
                            segment.next_version();
                            sides[side] = true;
                        }
                        for edit in edits {
                            segment.edit(edit);
                        }
                    }
                }
                Job::Clear(extension) => {
                    let places = Index::segments_of(extension);
                    for at in places.clone() {
                        if let Some(segment) = segments.get_mut(at) {
                            *segment = empty();
                        }
                    }
                    changed.retain(|(ext, ..)| *ext != extension);
                    keeping.sent.retain(|slot, _| slot.extension() != extension);
                }
                Job::Plan(next) => plan = Some(next),
                Job::Pin(aliases) => {
                    keeping.aliases = aliases;
                    pinned = true;
                }
                Job::Free(segments) => drop(segments),
            }
        }
        for (extension, change, sides) in changed {
            let mut versions = Vec::new();
            let mut reads = Vec::new();
            for at in Index::segments_of(extension) {
                if sides[at % SIDES] {
                    versions.push((at, Arc::clone(&segments[at])));
                    keeping.read(&segments, at, &mut reads);
                }
            }
            if !send(Event::Indexed(Indexed {
                extension,
                change,
                segments: versions,
                reads,
            })) {
                return;
            }
        }
        let (hot, ahead) = plan.map(|plan| plan.run()).unzip();
        if hot.is_none() && !pinned {
            continue;
        }
        if let Some(hot) = &hot {
            keeping.hot.clone_from(hot);
        }
        let reads = keeping.read_all(&segments);
        if !send(Event::Kept(Kept { hot, ahead, reads })) {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
