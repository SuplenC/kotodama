//! Noticing apps installed, changed or removed.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// How long the directories must stay still before they are read again, so a
/// package install that writes many files is read once, whole.
const SETTLE: Duration = Duration::from_millis(200);

/// Watches each `applications` directory, and calls `changed` once the
/// directories settle after a change. A directory that doesn't exist yet is
/// watched for through its parent, so apps put in it later are noticed.
/// Watching stops when the watcher is dropped.
pub fn watch(
    dirs: Vec<PathBuf>,
    changed: impl Fn() + Send + 'static,
) -> notify::Result<Arc<Mutex<RecommendedWatcher>>> {
    let (touched, touches) = mpsc::channel::<()>();
    let filter = dirs.clone();
    let watcher = Arc::new(Mutex::new(notify::recommended_watcher(
        move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event
                && event
                    .paths
                    .iter()
                    .any(|path| filter.iter().any(|dir| path.starts_with(dir)))
            {
                let _ = touched.send(());
            }
        },
    )?));
    watch_all(&watcher, &dirs);

    let watching = Arc::clone(&watcher);
    thread::Builder::new()
        .name("watch".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            while touches.recv().is_ok() {
                loop {
                    match touches.recv_timeout(SETTLE) {
                        Ok(()) => continue,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                // A directory that appeared needs watching itself.
                watch_all(&watching, &dirs);
                changed();
            }
        })?;
    Ok(watcher)
}

fn watch_all(watcher: &Mutex<RecommendedWatcher>, dirs: &[PathBuf]) {
    let mut watcher = watcher.lock().unwrap_or_else(|poison| poison.into_inner());
    for dir in dirs {
        if watcher.watch(dir, RecursiveMode::Recursive).is_ok() {
            continue;
        }
        if let Some(parent) = dir.parent().filter(|parent| Path::is_dir(parent)) {
            let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
        }
    }
}
