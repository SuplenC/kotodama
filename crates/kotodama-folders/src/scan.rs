//! Searching the folders, and searching them again when they change, on a
//! thread of its own, so an Action never waits for a search.
//!
//! Only the folders a search went through are watched, each on its own and
//! not what is below it: a project's files are never watched, however many
//! there are, and neither is anything below the depth.

use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use kotodama_extension::STACK_SIZE;
use kotodama_extension::watch::Event;
use kotodama_extension::watch::RecursiveMode;
use kotodama_extension::watch::Watched;
use kotodama_extension::watch::Watcher;
use kotodama_extension::watch::notify::event::CreateKind;
use kotodama_extension::watch::notify::event::EventKind;
use kotodama_extension::watch::notify::event::ModifyKind;
use kotodama_extension::watch::notify::event::RemoveKind;

use crate::projects;
use crate::projects::Project;
use crate::projects::ProjectFolder;
use crate::scripts;
use crate::scripts::Script;

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
    /// The folders changed.
    Touched,
}

/// Searches folders on a thread of its own, and again whenever they change.
pub struct Scanner(Sender<Request>);

impl Scanner {
    /// Starts the thread. It calls `scanned` with each search's results.
    pub fn start(scanned: impl Fn(Scan) + Send + 'static) -> io::Result<Self> {
        let (requests, received) = mpsc::channel();
        let touched = requests.clone();
        // The folders searched, for telling which changes matter.
        let searched = Arc::new(Mutex::new(Folders::default()));
        let filtering = Arc::clone(&searched);
        // No thread of the watcher's own: this one waits for the folders to
        // settle.
        let watcher = Watcher::new(
            RecursiveMode::NonRecursive,
            move |event, watched| {
                let folders = filtering.lock().unwrap_or_else(|poison| poison.into_inner());
                matters(&folders, watched, event)
            },
            move || {
                let _ = touched.send(Request::Touched);
            },
        )
        .map_err(|err| kotodama_extension::log!("not watching the folders: {err}"))
        .ok();
        thread::Builder::new()
            .name("scan".into())
            .stack_size(STACK_SIZE)
            .spawn(move || {
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
                    dirs.extend(
                        folders
                            .projects
                            .iter()
                            .map(|folder| &folder.path)
                            .chain(&folders.scripts)
                            .filter(|folder| !folder.is_dir())
                            .cloned(),
                    );
                    if let Some(watcher) = &watcher {
                        watcher.watch(dirs);
                    }
                    folders.clone_into(&mut searched.lock().unwrap_or_else(|poison| poison.into_inner()));
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

/// Whether `event` can change what a search of `folders` finds. In a scripts
/// folder any change can, a header edit included. In a projects folder only a
/// folder coming or going can, or the files that make a repository or list
/// its submodules, so saving or deleting files at the top of a project
/// searches nothing.
fn matters(folders: &Folders, watched: &Watched, event: &Event) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.iter().any(|path| {
        if folders.scripts.iter().any(|folder| path.starts_with(folder)) {
            return true;
        }
        if !folders.projects.iter().any(|folder| path.starts_with(&folder.path)) {
            return false;
        }
        let telling = path
            .file_name()
            .is_some_and(|name| name == ".git" || name == ".gitmodules" || name == "HEAD");
        // What is gone can't be looked at: it was a folder if it was watched.
        let folder = || watched.contains(path) || Path::is_dir(path);
        telling
            || match event.kind {
                EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder) => true,
                EventKind::Create(CreateKind::File) | EventKind::Remove(RemoveKind::File) => false,
                EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_)) => folder(),
                EventKind::Modify(_) => false,
                _ => true,
            }
    })
}
