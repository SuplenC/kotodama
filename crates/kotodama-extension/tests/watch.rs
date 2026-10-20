//! The directory watcher, against the real file system.

#![cfg(feature = "watch")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use kotodama_extension::watch::{RecursiveMode, Watcher};
use tempfile::TempDir;

const SETTLE: Duration = Duration::from_millis(50);
/// Far longer than a settle, for a change that should be noticed.
const NOTICED: Duration = Duration::from_secs(5);
/// For a change that shouldn't be: several settles.
const QUIET: Duration = Duration::from_millis(500);

/// Watches `dirs`, sending `()` each time they settle after a change that
/// `matters`.
fn watch(
    dirs: Vec<PathBuf>,
    mode: RecursiveMode,
    matters: impl Fn(&Path) -> bool + Send + 'static,
) -> (Watcher, Receiver<()>) {
    let (settled, changes) = mpsc::channel();
    let watcher = Watcher::start(
        mode,
        SETTLE,
        move |event, _| event.paths.iter().any(|path| matters(path)),
        move |_| {
            let _ = settled.send(());
        },
    )
    .unwrap();
    watcher.watch(dirs);
    (watcher, changes)
}

fn noticed(changes: &Receiver<()>) -> bool {
    changes.recv_timeout(NOTICED).is_ok()
}

fn quiet(changes: &Receiver<()>) -> bool {
    changes.recv_timeout(QUIET).is_err()
}

#[test]
fn many_changes_at_once_are_acted_on_once_they_settle() {
    let root = TempDir::new().unwrap();
    let dir = root.path().join("apps");
    fs::create_dir(&dir).unwrap();
    let (_watcher, changes) = watch(vec![dir.clone()], RecursiveMode::Recursive, |_| true);
    for n in 0..20 {
        fs::write(dir.join(format!("{n}.desktop")), "x").unwrap();
    }
    assert!(noticed(&changes));
    assert!(quiet(&changes));
}

#[test]
fn changes_that_dont_matter_are_left_alone() {
    let root = TempDir::new().unwrap();
    let dir = root.path().join("apps");
    fs::create_dir(&dir).unwrap();
    let (_watcher, changes) = watch(vec![dir.clone()], RecursiveMode::Recursive, |path| {
        path.extension().is_some_and(|ext| ext == "desktop")
    });
    fs::write(dir.join("notes.txt"), "x").unwrap();
    assert!(quiet(&changes));
    fs::write(dir.join("app.desktop"), "x").unwrap();
    assert!(noticed(&changes));
}

#[test]
fn a_directory_made_later_is_noticed_and_then_watched() {
    let root = TempDir::new().unwrap();
    let dir = root.path().join("apps");
    // What matters is only inside the directory: its appearing is noticed
    // whatever the filter says.
    let inside = dir.clone();
    let (_watcher, changes) = watch(vec![dir.clone()], RecursiveMode::Recursive, move |path| {
        path.starts_with(&inside) && path != inside
    });
    fs::create_dir(&dir).unwrap();
    assert!(noticed(&changes));
    fs::write(dir.join("app.desktop"), "x").unwrap();
    assert!(noticed(&changes));
}

#[test]
fn a_directory_removed_and_made_again_is_still_watched() {
    let root = TempDir::new().unwrap();
    let dir = root.path().join("extensions");
    fs::create_dir(&dir).unwrap();
    let (_watcher, changes) = watch(vec![dir.clone()], RecursiveMode::NonRecursive, |_| true);
    fs::remove_dir(&dir).unwrap();
    assert!(noticed(&changes));
    fs::create_dir(&dir).unwrap();
    assert!(noticed(&changes));
    fs::write(dir.join("manifest.toml"), "x").unwrap();
    assert!(noticed(&changes));
}

#[test]
fn directories_no_longer_asked_for_are_no_longer_watched() {
    let root = TempDir::new().unwrap();
    let (one, two) = (root.path().join("one"), root.path().join("two"));
    fs::create_dir(&one).unwrap();
    fs::create_dir(&two).unwrap();
    let (watcher, changes) = watch(
        vec![one.clone(), two.clone()],
        RecursiveMode::NonRecursive,
        |_| true,
    );
    watcher.watch(vec![two.clone()]);
    fs::write(one.join("file"), "x").unwrap();
    assert!(quiet(&changes));
    fs::write(two.join("file"), "x").unwrap();
    assert!(noticed(&changes));
}

#[test]
fn the_filter_sees_which_directories_are_watched() {
    let root = TempDir::new().unwrap();
    let dir = root.path().join("projects");
    let project = dir.join("project");
    fs::create_dir_all(&project).unwrap();
    let (settled, changes) = mpsc::channel();
    let watcher = Watcher::start(
        RecursiveMode::NonRecursive,
        SETTLE,
        // Only a watched folder going matters, which can't be told from the
        // path once it's gone.
        |event, watched| event.paths.iter().any(|path| watched.contains(path)),
        move |_| {
            let _ = settled.send(());
        },
    )
    .unwrap();
    watcher.watch(vec![dir.clone(), project.clone()]);
    fs::write(dir.join("file"), "x").unwrap();
    assert!(quiet(&changes));
    fs::remove_dir(&project).unwrap();
    assert!(noticed(&changes));
}

#[test]
fn without_a_thread_of_its_own_each_change_is_passed_on_as_it_happens() {
    let root = TempDir::new().unwrap();
    let dir = root.path().join("scripts");
    let (touched, touches) = mpsc::channel();
    let watcher = Watcher::new(
        RecursiveMode::NonRecursive,
        |_, _| true,
        move || {
            let _ = touched.send(());
        },
    )
    .unwrap();
    watcher.watch(vec![dir.clone()]);
    fs::create_dir(&dir).unwrap();
    assert!(noticed(&touches));
    // Watching the directory itself, now it's there, is the caller's call.
    watcher.watch(vec![dir.clone()]);
    while touches.recv_timeout(QUIET).is_ok() {}
    fs::write(dir.join("script"), "x").unwrap();
    assert!(noticed(&touches));
}
