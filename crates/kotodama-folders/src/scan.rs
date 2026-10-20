//! Searching the folders, and searching them again when they change, on a
//! thread of its own, so an Action never waits for a search.
//!
//! Only the folders a search went through are watched, each on its own and
//! not what is below it: a project's files are never watched, however many
//! there are, and neither is anything below the depth.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use notify::event::{CreateKind, EventKind, ModifyKind, RemoveKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::projects::{self, Project, ProjectFolder};
use crate::scripts::{self, Script};

/// How long the folders must stay still before they are searched again, so a
/// `git clone` or a copy of many scripts is searched once, whole.
const SETTLE: Duration = Duration::from_millis(200);

/// The folders to search.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folders {
    pub projects: Vec<ProjectFolder>,
    pub scripts: Vec<PathBuf>,
}

/// What a search of every folder found.
#[derive(Debug, Default)]
pub struct Scan {
    pub projects: Vec<Project>,
    pub scripts: Vec<Script>,
}

/// Searches every folder.
pub fn scan(folders: &Folders) -> (Scan, Vec<PathBuf>) {
    let found = projects::find(&folders.projects);
    let mut dirs = found.dirs;
    let mut scripts = scripts::Found::default();
    for folder in &folders.scripts {
        scripts::find(folder, &mut scripts);
    }
    dirs.extend(scripts.dirs);
    (
        Scan {
            projects: found.projects,
            scripts: scripts.scripts,
        },
        dirs,
    )
}

enum Request {
    Folders(Folders),
    Touched,
}

/// Searches folders on a thread of its own, and again whenever they change.
pub struct Scanner(Sender<Request>);

impl Scanner {
    /// Starts the thread. It calls `scanned` with each search's results.
    pub fn start(scanned: impl Fn(Scan) + Send + 'static) -> io::Result<Self> {
        let (requests, received) = mpsc::channel();
        let touched = requests.clone();
        let filter = Arc::new(Mutex::new(Filter::default()));
        let watching = Arc::clone(&filter);
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else {
                return;
            };
            let filter = watching.lock().unwrap_or_else(|poison| poison.into_inner());
            if filter.matters(&event) {
                let _ = touched.send(Request::Touched);
            }
        })
        .map_err(|err| eprintln!("kotodama-folders: not watching the folders: {err}"))
        .ok();
        thread::Builder::new()
            .name("scan".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                let mut watch = Watch {
                    watcher,
                    watched: HashSet::new(),
                };
                let mut folders = Folders::default();
                while let Ok(request) = received.recv() {
                    match request {
                        Request::Folders(new) => folders = new,
                        Request::Touched => loop {
                            match received.recv_timeout(SETTLE) {
                                Ok(Request::Folders(new)) => folders = new,
                                Ok(Request::Touched) => {}
                                Err(RecvTimeoutError::Timeout) => break,
                                Err(RecvTimeoutError::Disconnected) => return,
                            }
                        },
                    }
                    let (scan, mut dirs) = scan(&folders);
                    // A folder that isn't there yet is watched for through
                    // its parent.
                    for folder in folders
                        .projects
                        .iter()
                        .map(|folder| &folder.path)
                        .chain(&folders.scripts)
                    {
                        if !folder.is_dir()
                            && let Some(parent) = folder.parent()
                        {
                            dirs.push(parent.to_owned());
                        }
                    }
                    watch.set(dirs);
                    *filter.lock().unwrap_or_else(|poison| poison.into_inner()) = Filter {
                        folders: folders.clone(),
                        watched: watch.watched.clone(),
                    };
                    scanned(scan);
                }
            })?;
        Ok(Scanner(requests))
    }

    /// Searches `folders` now, and watches them from now on.
    pub fn search(&self, folders: Folders) {
        let _ = self.0.send(Request::Folders(folders));
    }
}

struct Watch {
    watcher: Option<RecommendedWatcher>,
    watched: HashSet<PathBuf>,
}

impl Watch {
    /// Watches `dirs`, each on its own, and nothing else.
    fn set(&mut self, dirs: Vec<PathBuf>) {
        let Some(watcher) = &mut self.watcher else {
            return;
        };
        let dirs: HashSet<PathBuf> = dirs.into_iter().collect();
        for gone in self.watched.difference(&dirs) {
            let _ = watcher.unwatch(gone);
        }
        let mut watched = HashSet::with_capacity(dirs.len());
        for dir in dirs {
            if self.watched.contains(&dir)
                || watcher.watch(&dir, RecursiveMode::NonRecursive).is_ok()
            {
                watched.insert(dir);
            }
        }
        self.watched = watched;
    }
}

/// What the watcher needs to tell which changes matter.
#[derive(Default)]
struct Filter {
    folders: Folders,
    /// The folders watched.
    watched: HashSet<PathBuf>,
}

impl Filter {
    /// Whether `event` can change what a search finds. In a scripts folder
    /// any change can, a header edit included. In a projects folder only a
    /// folder coming or going can, or the files that make a repository or
    /// list its submodules, so saving or deleting files at the top of a
    /// project searches nothing.
    fn matters(&self, event: &notify::Event) -> bool {
        if matches!(event.kind, EventKind::Access(_)) {
            return false;
        }
        event.paths.iter().any(|path| {
            if self
                .folders
                .scripts
                .iter()
                .any(|folder| path.starts_with(folder))
            {
                return true;
            }
            if !self
                .folders
                .projects
                .iter()
                .any(|folder| path.starts_with(&folder.path))
            {
                return false;
            }
            let telling = path
                .file_name()
                .is_some_and(|name| name == ".git" || name == ".gitmodules" || name == "HEAD");
            // What is gone can't be looked at: it was a folder if it was
            // watched.
            let folder = || self.watched.contains(path) || Path::is_dir(path);
            telling
                || match event.kind {
                    EventKind::Create(CreateKind::Folder)
                    | EventKind::Remove(RemoveKind::Folder) => true,
                    EventKind::Create(CreateKind::File) | EventKind::Remove(RemoveKind::File) => {
                        false
                    }
                    EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(ModifyKind::Name(_)) => folder(),
                    EventKind::Modify(_) => false,
                    _ => true,
                }
        })
    }
}
