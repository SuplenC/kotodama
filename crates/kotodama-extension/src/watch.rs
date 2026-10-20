//! Watching directories, and acting once they settle after a change, so a
//! package install or a `git clone` that writes many files is acted on once,
//! whole.
//!
//! A directory that doesn't exist yet is watched for through its parent, and
//! watched itself once it appears. One that is removed, or replaced by a
//! rename, is watched again once it is back. Only the directories asked for
//! are watched: each [`Watcher::watch`] call replaces the last.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::Duration;

pub use notify::{self, Event, EventKind, RecursiveMode};
use notify::{RecommendedWatcher, Watcher as _};

use crate::STACK_SIZE;

/// Watches directories until it is dropped.
pub struct Watcher(Arc<Inner>);

struct Inner {
    /// Held across every call into `notify`, which runs on a thread of its
    /// own; the event handler never takes it, so the two can't deadlock.
    notify: Mutex<RecommendedWatcher>,
    mode: RecursiveMode,
    /// The directories asked for.
    wanted: Mutex<Vec<PathBuf>>,
    /// What is watched, which the event handler reads and updates.
    watched: Arc<Mutex<Watched>>,
}

/// The directories being watched, for telling which changes matter.
#[derive(Debug, Default)]
pub struct Watched {
    /// Each with how it's watched: a parent watched for a missing directory
    /// is watched on its own.
    dirs: HashMap<PathBuf, RecursiveMode>,
    /// The directories asked for that don't exist, watched through their
    /// parent.
    missing: Vec<PathBuf>,
    /// Directories forgotten since the last [`Watcher::watch`], whose
    /// watches, which may follow a directory renamed elsewhere, it removes.
    gone: Vec<PathBuf>,
}

impl Watched {
    /// Whether `path` is a directory being watched. A directory that is gone
    /// can't be looked at, but if it was watched it was a directory.
    pub fn contains(&self, path: &Path) -> bool {
        self.dirs.contains_key(path)
    }

    /// Whether `event` brings a directory asked for that didn't exist.
    fn appeared(&self, event: &Event) -> bool {
        !matches!(event.kind, EventKind::Access(_))
            && event
                .paths
                .iter()
                .any(|path| self.missing.iter().any(|dir| path.starts_with(dir)))
    }

    /// Forgets the watched directories `event` removes or renames, so the
    /// next [`Watcher::watch`] watches what is there now. Returns whether it
    /// forgot any.
    fn forget_gone(&mut self, event: &Event) -> bool {
        if !matches!(
            event.kind,
            EventKind::Remove(_) | EventKind::Modify(notify::event::ModifyKind::Name(_))
        ) {
            return false;
        }
        let mut forgot = false;
        for path in &event.paths {
            if self.dirs.remove(path).is_some() {
                self.gone.push(path.clone());
                forgot = true;
            }
        }
        forgot
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

impl Watcher {
    /// Starts a watcher that watches each directory `mode`, and nothing yet.
    /// Each change `matters` approves, and each directory asked for
    /// appearing, starts a wait; once nothing more happened for `settle`,
    /// the directories asked for are watched again and `settled` runs, on a
    /// thread of the watcher's own. `matters` runs for every change, on
    /// `notify`'s thread, so it should be quick.
    pub fn start(
        mode: RecursiveMode,
        settle: Duration,
        matters: impl Fn(&Event, &Watched) -> bool + Send + 'static,
        mut settled: impl FnMut(&Watcher) + Send + 'static,
    ) -> notify::Result<Self> {
        let (touched, touches) = mpsc::channel::<()>();
        let watcher = Watcher::new(mode, matters, move || {
            let _ = touched.send(());
        })?;
        // Weak, so dropping the last `Watcher` drops `notify`, whose handler
        // holds `touched`, which ends the thread.
        let weak: Weak<Inner> = Arc::downgrade(&watcher.0);
        thread::Builder::new()
            .name("watch".into())
            .stack_size(STACK_SIZE)
            .spawn(move || {
                while touches.recv().is_ok() {
                    loop {
                        match touches.recv_timeout(settle) {
                            Ok(()) => continue,
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let watcher = Watcher(inner);
                    // A directory that appeared needs watching itself.
                    watcher.refresh();
                    settled(&watcher);
                }
            })?;
        Ok(watcher)
    }

    /// A watcher with no thread of its own, for an Extension that waits for
    /// changes to settle on a thread it has anyway: `touched` runs, on
    /// `notify`'s thread, for each change `matters` approves and each
    /// directory asked for appearing, and the Extension calls
    /// [`Watcher::watch`] again once they settle, to watch what appeared.
    pub fn new(
        mode: RecursiveMode,
        matters: impl Fn(&Event, &Watched) -> bool + Send + 'static,
        touched: impl Fn() + Send + 'static,
    ) -> notify::Result<Self> {
        let watched = Arc::new(Mutex::new(Watched::default()));
        let handler_watched = Arc::clone(&watched);
        let notify = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let Ok(event) = event else {
                return;
            };
            let mut watched = lock(&handler_watched);
            let matters = watched.appeared(&event) || matters(&event, &watched);
            // A directory watched going is acted on too, to watch for it.
            let gone = watched.forget_gone(&event);
            drop(watched);
            if matters || gone {
                touched();
            }
        })?;
        Ok(Watcher(Arc::new(Inner {
            notify: Mutex::new(notify),
            mode,
            wanted: Mutex::new(Vec::new()),
            watched,
        })))
    }

    /// Watches `dirs`, and stops watching any other. A directory already
    /// watched costs nothing.
    pub fn watch(&self, dirs: Vec<PathBuf>) {
        let mut notify = lock(&self.0.notify);
        self.watch_locked(&mut notify, dirs);
    }

    /// Watches the directories asked for again, for those that appeared or
    /// came back since.
    fn refresh(&self) {
        // Read under the `notify` lock, so a `watch` running meanwhile isn't
        // undone with the directories it replaced.
        let mut notify = lock(&self.0.notify);
        let dirs = lock(&self.0.wanted).clone();
        self.watch_locked(&mut notify, dirs);
    }

    fn watch_locked(&self, notify: &mut RecommendedWatcher, dirs: Vec<PathBuf>) {
        let mode = self.0.mode;
        let (old, gone) = {
            let mut watched = lock(&self.0.watched);
            (watched.dirs.clone(), std::mem::take(&mut watched.gone))
        };
        // A watch follows its directory, even renamed elsewhere.
        for dir in &gone {
            let _ = notify.unwatch(dir);
        }
        let mut dirs_now: HashMap<PathBuf, RecursiveMode> = HashMap::with_capacity(dirs.len());
        let mut missing = Vec::new();
        for dir in &dirs {
            if old.get(dir) == Some(&mode) || notify.watch(dir, mode).is_ok() {
                dirs_now.insert(dir.clone(), mode);
            } else {
                missing.push(dir.clone());
            }
        }
        for dir in &missing {
            let Some(parent) = dir.parent() else {
                continue;
            };
            if dirs_now.contains_key(parent) {
                continue;
            }
            if old.contains_key(parent)
                || (parent.is_dir() && notify.watch(parent, RecursiveMode::NonRecursive).is_ok())
            {
                let parent_mode = old
                    .get(parent)
                    .copied()
                    .unwrap_or(RecursiveMode::NonRecursive);
                dirs_now.insert(parent.to_owned(), parent_mode);
            }
        }
        for gone in old.keys().filter(|dir| !dirs_now.contains_key(*dir)) {
            let _ = notify.unwatch(gone);
        }
        let mut watched = lock(&self.0.watched);
        // One that went while this ran is forgotten, to be watched again.
        dirs_now.retain(|dir, _| !old.contains_key(dir) || watched.dirs.contains_key(dir));
        watched.dirs = dirs_now;
        watched.missing = missing;
        drop(watched);
        *lock(&self.0.wanted) = dirs;
    }
}
