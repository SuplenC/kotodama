//! Indexing off the keystroke path (ADR 0017): Extensions' changes to their
//! Items are made on a thread of their own, into a new version of each
//! changed Extension's Segment, which the Daemon puts in whole between two
//! events. A keystroke never waits for a change, and never sees half of one.
//!
//! Changes that queue up while the thread works are made together, one new
//! version of each Segment for all of them.
//!
//! The thread also reads, off their Shelves, the Items the Daemon keeps
//! resident: the hot ones, which it plans here from a copy of the History's
//! picks, and those a keyword or an Alias names. Each read goes out with the
//! version it was read from, so the thread answering keystrokes never reads
//! a payload for them, nor plans.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::clock::{DAY_SECS, When};
use crate::daemon::{Event, ExtensionId};
use crate::history::Picks;
use crate::index::{self, Read, Slot};
use crate::segment::{Edit, Row, Segment};

/// A new version of one Extension's Segment, for the Daemon to put in.
pub struct Indexed {
    pub(crate) extension: ExtensionId,
    /// The last change it holds.
    pub(crate) change: u64,
    pub(crate) segment: Arc<Segment>,
    /// Items of it to keep resident, read since the last version.
    pub(crate) reads: Vec<Read>,
}

/// Items to keep resident, read off the Segments the Daemon has now.
pub struct Kept {
    /// The hot Items, in slot order, when they were planned again.
    pub(crate) hot: Option<Vec<Slot>>,
    pub(crate) reads: Vec<Read>,
}

/// How long before a time of day an Item picked then is hot.
const HOT_LEAD_SECS: i64 = 15 * 60;
/// How long after a time of day an Item picked then stays hot.
const HOT_GRACE_SECS: i64 = 30 * 60;

/// What the hot Items are worked out from: a copy of the picks of each
/// Indexed item picked, and the time.
pub struct Plan {
    pub picked: Vec<(Slot, Picks)>,
    pub when: When,
    pub half_life: f64,
    pub capacity: usize,
}

impl Plan {
    /// The hot Items, in slot order: those picked most near this time of
    /// day, from a little before it to a little after, up to the capacity.
    pub fn hot(&self) -> Vec<Slot> {
        if self.capacity == 0 {
            return Vec::new();
        }
        let when = self.when;
        let around = [-HOT_GRACE_SECS, 0, HOT_LEAD_SECS].map(|by| When {
            at: when.at.saturating_add_signed(by),
            time_of_day: (i64::from(when.time_of_day) + by).rem_euclid(DAY_SECS as i64) as u32,
        });
        let mut hot: Vec<(f64, Slot)> = Vec::new();
        for &(slot, picks) in &self.picked {
            let near = around
                .iter()
                .map(|&when| picks.habit(when, self.half_life).near_now)
                .fold(0.0, f64::max);
            if near > 0.0 {
                hot.push((near, slot));
            }
        }
        hot.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut slots: Vec<Slot> = Vec::with_capacity(self.capacity.min(hot.len()));
        for (_, slot) in hot {
            if slots.len() == self.capacity {
                break;
            }
            if !slots.contains(&slot) {
                slots.push(slot);
            }
        }
        slots.sort_unstable();
        slots
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
    /// Reads the Items to keep in the Extension's Segment that moved since
    /// they were sent.
    fn read(&mut self, segments: &[Arc<Segment>], extension: ExtensionId, reads: &mut Vec<Read>) {
        let Some(segment) = segments.get(extension.0) else {
            return;
        };
        let n = extension.0 as u32;
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
        for n in 0..segments.len() {
            self.read(segments, ExtensionId(n), &mut reads);
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
        // By Extension, the last change each new version holds.
        let mut changed: Vec<(ExtensionId, u64)> = Vec::new();
        let mut plan = None;
        let mut pinned = false;
        for job in std::iter::once(first).chain(queue.try_iter()) {
            match job {
                Job::Edit {
                    extension,
                    change,
                    edit,
                } => {
                    if segments.len() <= extension.0 {
                        segments.resize_with(extension.0 + 1, empty);
                    }
                    // Copies the version the Daemon has, once for all the
                    // changes to it waiting.
                    let segment = Arc::make_mut(&mut segments[extension.0]);
                    match changed.iter_mut().find(|(ext, _)| *ext == extension) {
                        Some((_, last)) => *last = change,
                        None => {
                            segment.next_version();
                            changed.push((extension, change));
                        }
                    }
                    segment.edit(edit);
                }
                Job::Clear(extension) => {
                    if let Some(segment) = segments.get_mut(extension.0) {
                        *segment = empty();
                    }
                    changed.retain(|(ext, _)| *ext != extension);
                    keeping.sent.retain(|slot, _| slot.extension() != extension);
                }
                Job::Plan(next) => plan = Some(next),
                Job::Pin(aliases) => {
                    keeping.aliases = aliases;
                    pinned = true;
                }
            }
        }
        for (extension, change) in changed {
            let segment = Arc::clone(&segments[extension.0]);
            let mut reads = Vec::new();
            keeping.read(&segments, extension, &mut reads);
            if !send(Event::Indexed(Indexed {
                extension,
                change,
                segment,
                reads,
            })) {
                return;
            }
        }
        let hot = plan.map(|plan| plan.hot());
        if hot.is_none() && !pinned {
            continue;
        }
        if let Some(hot) = &hot {
            keeping.hot.clone_from(hot);
        }
        let reads = keeping.read_all(&segments);
        if !send(Event::Kept(Kept { hot, reads })) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            actions: vec![Action {
                id: "open".into(),
                title: "Open".into(),
                shortcut: None,
                arguments: vec![],
            }],
        }
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
        let versions: Vec<(usize, u64)> = built
            .iter()
            .map(|indexed| (indexed.extension.0, indexed.change))
            .collect();
        assert_eq!(versions, [(0, 4), (1, 2)]);
        let first = &built[0].segment;
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
        let segment = &built[0].segment;
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

    #[test]
    fn the_hot_items_are_planned_and_read_here() {
        let when = When {
            at: 10 * DAY_SECS,
            time_of_day: 8 * 3600,
        };
        let mut history = crate::history::History::default();
        history.pick("apps", "b", "", &[], when);
        let picks = *history.picks("apps", "b").unwrap();
        let sent = sent(vec![
            edit(0, 1, Edit::Add(vec![item("a"), item("b")])),
            Job::Plan(Plan {
                picked: vec![(slot(1), picks)],
                when,
                half_life: history.settings().half_life_secs,
                capacity: 8,
            }),
        ]);
        assert_eq!(sent, [vec![], vec![slot(1)]]);
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
