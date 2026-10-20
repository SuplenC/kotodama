//! Indexing off the keystroke path (ADR 0017): Extensions' changes to their
//! Items are made on a thread of their own, into a new version of each
//! changed Extension's Segment, which the Daemon puts in whole between two
//! events. A keystroke never waits for a change, and never sees half of one.
//!
//! Changes that queue up while the thread works are made together, one new
//! version of each Segment for all of them.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::daemon::{Event, ExtensionId};
use crate::segment::{Edit, Segment};

/// A new version of one Extension's Segment, for the Daemon to put in.
pub struct Indexed {
    pub(crate) extension: ExtensionId,
    /// The last change it holds.
    pub(crate) change: u64,
    pub(crate) segment: Arc<Segment>,
}

enum Job {
    Edit {
        extension: ExtensionId,
        change: u64,
        edit: Edit,
    },
    /// The Extension stopped: its Items are gone.
    Clear(ExtensionId),
}

/// The thread that makes changes to the Index, and how to give it more.
pub struct Indexer {
    jobs: Sender<Job>,
    /// Numbers each change.
    changes: u64,
}

impl Indexer {
    /// Starts the thread, which sends each new version to `events` and keeps
    /// payloads in files in `dir`, or in memory. It stops when the Indexer is
    /// dropped.
    pub fn start(events: Sender<Event>, dir: Option<Arc<Path>>) -> Self {
        let (jobs, queue) = mpsc::channel();
        if let Err(err) = thread::Builder::new().name("index".into()).spawn(move || {
            work(queue, dir, |indexed| {
                events.send(Event::Indexed(indexed)).is_ok()
            })
        }) {
            log::error!("can't start the thread that indexes Items: {err}");
        }
        Indexer { jobs, changes: 0 }
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
}

/// Makes the changes from `queue`, all that are waiting at once, and passes
/// each new version to `send`, until it fails or the queue closes.
fn work(queue: Receiver<Job>, dir: Option<Arc<Path>>, mut send: impl FnMut(Indexed) -> bool) {
    let empty = || Arc::new(Segment::new(dir.clone()));
    let mut segments: Vec<Arc<Segment>> = Vec::new();
    while let Ok(first) = queue.recv() {
        // By Extension, the last change each new version holds.
        let mut changed: Vec<(ExtensionId, u64)> = Vec::new();
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
                }
            }
        }
        for (extension, change) in changed {
            let segment = Arc::clone(&segments[extension.0]);
            if !send(Indexed {
                extension,
                change,
                segment,
            }) {
                return;
            }
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
        work(queue, None, |indexed| {
            built.push(indexed);
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
