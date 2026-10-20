//! Watching directories, and acting once they settle after a change, so a
//! package install or a `git clone` that writes many files is acted on once,
//! whole.
//!
//! A directory that doesn't exist yet is watched for through the deepest
//! directory on its path that does, and watched itself once it appears. One that is removed, or replaced by a
//! rename, is watched again once it is back. Only the directories asked for
//! are watched: each [`Watcher::watch`] call replaces the last.
//!
//! A watch follows the symlinks on the way to its directory once, when it is
//! made. A NixOS or home-manager switch doesn't change the directories a
//! profile holds: it replaces the profile's symlink (`/run/current-system`,
//! `/etc/profiles/per-user/<user>`, `~/.nix-profile` and the generation
//! links behind them) with one to a new generation. So the directory each
//! symlink on the way sits in is watched too, and a symlink replaced there
//! counts as a change: the directories behind it are watched again where it
//! leads now.

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
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
    /// Each with how it's watched: one watched for a missing directory, or
    /// for a symlink in it, is watched on its own.
    dirs: HashMap<PathBuf, RecursiveMode>,
    /// The directories asked for that don't exist, watched through the
    /// deepest directory on their path that does.
    missing: Vec<PathBuf>,
    /// Directories forgotten since the last [`Watcher::watch`], whose
    /// watches, which may follow a directory renamed elsewhere, it removes.
    gone: Vec<PathBuf>,
    /// The symlinks on the way to the directories asked for, each with the
    /// directories whose watches follow it, to watch again once it's
    /// replaced. The directory each is in is watched.
    links: HashMap<PathBuf, Vec<PathBuf>>,
}

impl Watched {
    /// Whether `path` is a directory being watched. A directory that is gone
    /// can't be looked at, but if it was watched it was a directory.
    pub fn contains(&self, path: &Path) -> bool {
        self.dirs.contains_key(path)
    }

    /// Whether `event` brings a directory asked for that didn't exist, or
    /// one on the way to it.
    fn appeared(&self, event: &Event) -> bool {
        !matches!(event.kind, EventKind::Access(_))
            && event.paths.iter().any(|path| {
                self.missing
                    .iter()
                    .any(|dir| path.starts_with(dir) || dir.starts_with(path))
            })
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

    /// Forgets the watched directories behind each symlink `event` makes,
    /// removes or renames, so the next [`Watcher::watch`] watches them where
    /// the links lead now. Returns whether it touched any link.
    fn forget_relinked(&mut self, event: &Event) -> bool {
        if !matches!(
            event.kind,
            EventKind::Create(_)
                | EventKind::Remove(_)
                | EventKind::Modify(notify::event::ModifyKind::Name(_))
        ) {
            return false;
        }
        let mut relinked = false;
        for path in &event.paths {
            let Some(behind) = self.links.get(path) else {
                continue;
            };
            relinked = true;
            for dir in behind {
                if self.dirs.remove(dir).is_some() {
                    self.gone.push(dir.clone());
                }
            }
        }
        relinked
    }
}

/// The symlinks on the way to `path` that can be replaced, each named as it
/// is in a directory reached without following any, as the event for its
/// replacement names it. Followed as the kernel would, as far as the path
/// exists.
fn links_to(path: &Path) -> Vec<PathBuf> {
    // Where the kernel gives up with ELOOP.
    const MAX_FOLLOWED: usize = 40;
    let mut links = Vec::new();
    let mut at = PathBuf::new();
    // What is left to walk, the next part last.
    let mut rest: Vec<PathBuf> = parts(path);
    let mut followed = 0;
    while let Some(part) = rest.pop() {
        match part.to_str() {
            Some(".") => continue,
            Some("..") => {
                at.pop();
                continue;
            }
            _ => {}
        }
        let next = at.join(&part);
        match fs::read_link(&next) {
            Ok(target) if followed < MAX_FOLLOWED => {
                followed += 1;
                if replaceable(&at) {
                    links.push(next);
                }
                // A relative target goes on from the link's directory.
                rest.extend(parts(&target));
            }
            _ => at = next,
        }
    }
    links
}

/// `path`'s parts, the last first, ready to be popped.
fn parts(path: &Path) -> Vec<PathBuf> {
    path.components()
        .rev()
        .map(|part| PathBuf::from(part.as_os_str()))
        .collect()
}

/// Whether a symlink in `dir` can be replaced: not when nobody may write
/// there, as in a Nix store path, which never changes.
fn replaceable(dir: &Path) -> bool {
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    fs::metadata(dir).is_ok_and(|meta| meta.permissions().mode() & 0o222 != 0)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

impl Watcher {
    /// Starts a watcher that watches each directory `mode`, and nothing yet.
    /// Each change `matters` approves, and each directory asked for
    /// appearing, starts a wait; once nothing more happened for `settle`,
    /// the directories asked for are watched again and `settled` runs, on a
    /// thread of the watcher's own called `name`. `matters` runs for every
    /// change, on `notify`'s thread, so it should be quick.
    pub fn start(
        name: &str,
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
            .name(name.into())
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
            // A directory watched going, or a link to one replaced, is acted
            // on too, to watch it again.
            let gone = watched.forget_gone(&event) | watched.forget_relinked(&event);
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
        // The links first: one replaced from now on is noticed, and one
        // replaced before is followed by the watches made below.
        let routes: Vec<Vec<PathBuf>> = dirs.iter().map(|dir| links_to(dir)).collect();
        for parent in routes.iter().flatten().filter_map(|link| link.parent()) {
            watch_beside(notify, &old, &mut dirs_now, parent);
        }
        let mut missing = Vec::new();
        for dir in &dirs {
            if old.get(dir) == Some(&mode) || notify.watch(dir, mode).is_ok() {
                dirs_now.insert(dir.clone(), mode);
            } else {
                missing.push(dir.clone());
            }
        }
        // A missing directory is watched for where its path still exists.
        for dir in &missing {
            if let Some(there) = dir.ancestors().skip(1).find(|there| there.is_dir()) {
                watch_beside(notify, &old, &mut dirs_now, there);
            }
        }
        // Each link with what is watched for each directory behind it.
        let mut links: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        for (dir, route) in dirs.iter().zip(routes) {
            let Some(watched) = dir.ancestors().find(|there| dirs_now.contains_key(*there)) else {
                continue;
            };
            for link in route {
                links.entry(link).or_default().push(watched.to_owned());
            }
        }
        for gone in old.keys().filter(|dir| !dirs_now.contains_key(*dir)) {
            let _ = notify.unwatch(gone);
        }
        let mut watched = lock(&self.0.watched);
        // One that went, or whose link was replaced, while this ran is
        // forgotten, to be watched again.
        dirs_now.retain(|dir, _| {
            (!old.contains_key(dir) || watched.dirs.contains_key(dir))
                && !watched.gone.contains(dir)
        });
        watched.dirs = dirs_now;
        watched.missing = missing;
        watched.links = links;
        drop(watched);
        *lock(&self.0.wanted) = dirs;
    }
}

/// Watches `dir` on its own, for what is made, removed or renamed in it,
/// unless it is watched already.
fn watch_beside(
    notify: &mut RecommendedWatcher,
    old: &HashMap<PathBuf, RecursiveMode>,
    dirs_now: &mut HashMap<PathBuf, RecursiveMode>,
    dir: &Path,
) {
    if dirs_now.contains_key(dir) {
        return;
    }
    if let Some(mode) = old.get(dir) {
        dirs_now.insert(dir.to_owned(), *mode);
    } else if dir.is_dir() && notify.watch(dir, RecursiveMode::NonRecursive).is_ok() {
        dirs_now.insert(dir.to_owned(), RecursiveMode::NonRecursive);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::links_to;

    #[test]
    fn the_links_on_the_way_are_found_where_they_can_be_replaced() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        // ~/.nix-profile -> profiles/profile -> profile-2-link -> the
        // generation, whose `share` links into a store that never changes.
        let generation = root.join("store/generation");
        let package = root.join("store/package");
        fs::create_dir_all(&generation).unwrap();
        fs::create_dir_all(package.join("share/applications")).unwrap();
        symlink(package.join("share"), generation.join("share")).unwrap();
        fs::create_dir(root.join("profiles")).unwrap();
        symlink(&generation, root.join("profiles/profile-2-link")).unwrap();
        symlink("profile-2-link", root.join("profiles/profile")).unwrap();
        symlink("profiles/profile", root.join("nix-profile")).unwrap();
        fs::set_permissions(&generation, fs::Permissions::from_mode(0o555)).unwrap();

        let links = links_to(&root.join("nix-profile/share/applications"));

        fs::set_permissions(&generation, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            links,
            [
                root.join("nix-profile"),
                root.join("profiles/profile"),
                root.join("profiles/profile-2-link"),
            ]
        );
        // As far as the path exists.
        assert_eq!(links_to(&root.join("nix-profile/missing/deeper")).len(), 3);
        assert!(links_to(&root.join("store/package/share")).is_empty());
    }

    #[test]
    fn a_loop_of_links_ends() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        symlink("two", root.join("one")).unwrap();
        symlink("one", root.join("two")).unwrap();
        assert_eq!(links_to(&root.join("one/dir")).len(), 40);
    }
}
