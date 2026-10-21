//! Indexing off the keystroke path (ADR 0017): Extensions' changes to their
//! Items are made on a thread of their own, into a new version of each
//! changed Segment, which the Daemon puts in whole between two events. A keystroke never waits for a change, and never sees half of one.
//!
//! Changes that queue up while the thread works are made together, one new
//! version of each Segment for all of them.
//!
//! The thread also reads, off their Shelves, the Items the Daemon keeps
//! resident: the hot ones, which it plans here from a copy of the History's
//! picks, and those a keyword or an Alias names. Each read goes out with the
//! version it was read from, so the thread answering keystrokes never reads
//! a payload for them, nor plans.

use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::clock::{DAY_SECS, When};
use crate::daemon::{Event, ExtensionId};
use crate::history::{self, Habit};
use crate::index::{self, Index, Read, SIDES, Slot, SlotHasher};
use crate::most_used::{Ahead, Usage, Versions};
use crate::segment::{Edit, Row, Segment};

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

/// Items to keep resident, read off the Segments the Daemon has now.
pub struct Kept {
    /// The hot Items, in slot order, when they were planned again.
    pub(crate) hot: Option<Vec<Slot>>,
    /// The Empty query's most used Results, ranked ahead as the hot Items
    /// were planned.
    pub(crate) ahead: Option<Ahead>,
    pub(crate) reads: Vec<Read>,
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
    /// ranked ahead: for now, and for the start of the coming part of the
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
            let when = When {
                at: self.when.at.saturating_add_signed(by),
                time_of_day: (i64::from(self.when.time_of_day) + by).rem_euclid(DAY_SECS as i64)
                    as u32,
            };
            picked.picks.habits(when, habits);
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
    /// These Items, by Extension and id, are what the Aliases name now.
    Pin(Vec<(ExtensionId, String)>),
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
            .spawn(move || work(queue, dir, |event| events.send(event).is_ok()))
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
        let hot = self
            .hot
            .iter()
            .filter(|slot| slot.segment == n)
            .map(|slot| slot.row);
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
fn work(queue: Receiver<Job>, dir: Option<Arc<Path>>, mut send: impl FnMut(Event) -> bool) {
    let empty = || Arc::new(Segment::new(dir.clone()));
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
                    let n = match changed.iter().position(|(ext, ..)| *ext == extension) {
                        Some(n) => n,
                        None => {
                            changed.push((extension, change, [false; SIDES]));
                            changed.len() - 1
                        }
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
mod tests {
    use super::*;
    use crate::daemon::Source;
    use crate::history::{Frecent, History};
    use crate::most_used::{Counts, Picked};
    use kotodama_protocol::{Action, Item, ItemKind};

    fn item(id: &str) -> Item {
        Item {
            id: id.into(),
            kind: ItemKind::Url {
                url: format!("https://example.org/{id}"),
            },
            identity: None,
            title: id.into(),
            subtitle: None,
            icon: None,
            keyword: None,
            keywords: vec![],
            hidden: false,
            app_only: false,
            actions: vec![Action {
                id: "open".into(),
                title: "Open".into(),
                shortcut: None,
                arguments: vec![],
            }],
        }
    }

    /// Each version built, as where each Segment in it goes and the change
    /// it holds.
    fn versions(built: &[Indexed]) -> Vec<(usize, u64)> {
        built
            .iter()
            .flat_map(|indexed| indexed.segments.iter().map(|(at, _)| (*at, indexed.change)))
            .collect()
    }

    /// The Segment at `at` in what was built last.
    fn segment(built: &[Indexed], at: usize) -> &Segment {
        built
            .iter()
            .rev()
            .find_map(|indexed| indexed.segments.iter().find(|(n, _)| *n == at))
            .map(|(_, segment)| &**segment)
            .unwrap()
    }

    /// Runs the thread's work over what is queued already.
    fn built(jobs: Vec<Job>) -> Vec<Indexed> {
        let (sender, queue) = mpsc::channel();
        for job in jobs {
            sender.send(job).unwrap();
        }
        drop(sender);
        let mut built = Vec::new();
        work(queue, None, |event| {
            if let Event::Indexed(indexed) = event {
                built.push(indexed);
            }
            true
        });
        built
    }

    fn edit(extension: usize, change: u64, edit: Edit) -> Job {
        Job::Edit {
            extension: ExtensionId(extension),
            change,
            edit,
        }
    }

    #[test]
    fn changes_waiting_together_make_one_version_of_each_segment() {
        let built = built(vec![
            edit(0, 1, Edit::Add(vec![item("a")])),
            edit(1, 2, Edit::Add(vec![item("b")])),
            edit(0, 3, Edit::Add(vec![item("c")])),
            edit(0, 4, Edit::Remove(vec!["a".into()])),
        ]);
        assert_eq!(versions(&built), [(0, 4), (2, 2)]);
        let first = &built[0].segments[0].1;
        assert_eq!(first.find("a"), None);
        assert!(first.find("c").is_some());
    }

    #[test]
    fn a_row_emptied_in_a_version_is_not_taken_in_the_same_one() {
        let built = built(vec![
            edit(0, 1, Edit::Add(vec![item("a"), item("b")])),
            edit(0, 2, Edit::Remove(vec!["a".into()])),
            edit(0, 3, Edit::Add(vec![item("c")])),
        ]);
        let segment = &built[0].segments[0].1;
        assert_eq!(segment.find("b"), Some(1));
        assert_eq!(segment.find("c"), Some(2));
    }

    /// Runs the thread's work over what is queued already, and gives what it
    /// sent: for each event, the slots of the Items read along with it.
    fn sent(jobs: Vec<Job>) -> Vec<Vec<Slot>> {
        let (sender, queue) = mpsc::channel();
        for job in jobs {
            sender.send(job).unwrap();
        }
        drop(sender);
        let mut sent = Vec::new();
        work(queue, None, |event| {
            let reads = match event {
                Event::Indexed(indexed) => indexed.reads,
                Event::Kept(kept) => kept.reads,
                _ => unreachable!(),
            };
            sent.push(reads.iter().map(|read| read.slot).collect());
            true
        });
        sent
    }

    fn slot(row: Row) -> Slot {
        Slot { segment: 0, row }
    }

    #[test]
    fn items_named_by_a_keyword_or_an_alias_are_read_with_their_version_once() {
        let mut keyed = item("b");
        keyed.keyword = Some("bb".into());
        let sent = sent(vec![
            Job::Pin(vec![(ExtensionId(0), "c".into())]),
            edit(0, 1, Edit::Add(vec![item("a"), keyed, item("c")])),
            edit(0, 2, Edit::Add(vec![item("d")])),
        ]);
        // The version, then the Aliases' Items, read already.
        assert_eq!(sent, [vec![slot(1), slot(2)], vec![]]);
    }

    const EIGHT: When = When {
        at: 10 * DAY_SECS,
        time_of_day: 8 * 3600,
    };

    /// A plan at eight o'clock, of `rows` picked at eight `picks` times
    /// each and at no other time, with the Empty query showing `wanted`.
    fn plan(rows: &[(Row, u32)], learned: &[Row], capacity: usize, wanted: usize) -> Plan {
        let mut history = History::default();
        let mut picked = Vec::new();
        for &(row, picks) in rows {
            let id = row.to_string();
            for _ in 0..picks {
                history.touch("apps", &id, EIGHT);
            }
            picked.push(*history.picks("apps", &id).unwrap());
        }
        let half_life = history.settings().half_life_secs;
        Plan {
            usage: Usage {
                picked: Arc::new(Picked {
                    slots: rows
                        .iter()
                        .map(|&(row, _)| (slot(row), Counts::Both))
                        .collect(),
                    groups: vec![None; rows.len()],
                    picks: Frecent::new(&picked, half_life),
                }),
                others: Vec::new(),
                wanted,
                left_out: Vec::new(),
            },
            learned: learned.iter().map(|&row| slot(row)).collect(),
            when: EIGHT,
            capacity,
            versions: (0, 0, 0),
        }
    }

    fn slots(rows: &[Row]) -> Vec<Slot> {
        rows.iter().map(|&row| slot(row)).collect()
    }

    #[test]
    fn the_hot_items_are_planned_and_read_here() {
        let sent = sent(vec![
            edit(0, 1, Edit::Add(vec![item("a"), item("b")])),
            Job::Plan(plan(&[(1, 1)], &[], 8, 0)),
        ]);
        assert_eq!(sent, [vec![], vec![slot(1)]]);
    }

    #[test]
    fn learned_choices_take_a_quarter_of_the_hot_items_before_the_habit() {
        let habit: Vec<(Row, u32)> = (0..10).map(|row| (row, 10 - row)).collect();

        let (hot, _) = plan(&habit, &[20, 21, 22], 8, 0).run();

        assert_eq!(hot, slots(&[0, 1, 2, 3, 4, 5, 20, 21]));
    }

    #[test]
    fn learned_choices_take_the_room_the_habit_leaves() {
        let (hot, _) = plan(&[(0, 1)], &[20, 21, 22, 23], 4, 0).run();

        assert_eq!(hot, slots(&[0, 20, 21, 22]));
    }

    #[test]
    fn the_items_the_empty_query_shows_are_hot_before_learned_choices() {
        let habit = [(0, 5), (1, 4), (2, 3), (3, 2), (4, 1)];

        let (hot, ahead) = plan(&habit, &[20], 4, 4).run();

        let ranked: Vec<&[Source]> = ahead.ranked.iter().map(|(_, ranked)| &ranked[..]).collect();
        let shown = [0, 1, 2, 3].map(|row| Source::Indexed(slot(row)));
        assert_eq!(ranked, [shown, shown]);
        assert_eq!(hot, slots(&[0, 1, 2, 3]));
    }

    fn app_only(id: &str) -> Item {
        Item {
            app_only: true,
            ..item(id)
        }
    }

    #[test]
    fn an_extensions_items_go_to_the_segment_their_visibility_names() {
        let built = built(vec![edit(1, 1, Edit::Add(vec![item("a"), app_only("b")]))]);

        assert_eq!(versions(&built), [(2, 1), (3, 1)]);
        assert!(segment(&built, 2).find("a").is_some());
        assert_eq!(segment(&built, 2).find("b"), None);
        assert!(segment(&built, 3).find("b").is_some());
    }

    #[test]
    fn an_item_that_changes_visibility_leaves_its_old_segment() {
        let built = built(vec![
            edit(0, 1, Edit::Add(vec![item("a"), item("b")])),
            edit(0, 2, Edit::Add(vec![app_only("a")])),
        ]);

        assert_eq!(built.len(), 1, "both Segments in one version");
        assert_eq!(segment(&built, 0).find("a"), None);
        assert!(segment(&built, 0).find("b").is_some());
        assert!(segment(&built, 1).find("a").is_some());
    }

    #[test]
    fn a_change_to_one_segment_leaves_the_other_alone() {
        let built = built(vec![edit(0, 1, Edit::Add(vec![app_only("b")]))]);

        assert_eq!(versions(&built), [(1, 1)]);
    }

    #[test]
    fn a_change_that_touches_nothing_still_makes_a_version() {
        let built = built(vec![edit(0, 1, Edit::Remove(vec!["gone".into()]))]);

        assert_eq!(versions(&built), [(0, 1)]);
    }

    #[test]
    fn a_cleared_extensions_earlier_changes_are_not_sent() {
        let built = built(vec![
            edit(0, 1, Edit::Add(vec![item("a")])),
            Job::Clear(ExtensionId(0)),
            edit(1, 2, Edit::Add(vec![item("b")])),
        ]);
        assert_eq!(built.len(), 1);
        assert_eq!(built[0].extension, ExtensionId(1));
    }
}
