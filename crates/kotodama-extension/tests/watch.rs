//! The directory watcher, against the real file system.

#![cfg(feature = "watch")]

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;

use kotodama_extension::watch::RecursiveMode;
use kotodama_extension::watch::Watcher;
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
        "watch",
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
fn a_directory_renamed_away_is_no_longer_watched() {
    let root = TempDir::new().unwrap();
    let (dir, elsewhere) = (root.path().join("scripts"), root.path().join("old"));
    fs::create_dir(&dir).unwrap();
    let (_watcher, changes) = watch(vec![dir.clone()], RecursiveMode::NonRecursive, |_| true);
    fs::rename(&dir, &elsewhere).unwrap();
    assert!(noticed(&changes));
    fs::write(elsewhere.join("file"), "x").unwrap();
    assert!(quiet(&changes));
}

#[test]
fn directories_no_longer_asked_for_are_no_longer_watched() {
    let root = TempDir::new().unwrap();
    let (one, two) = (root.path().join("one"), root.path().join("two"));
    fs::create_dir(&one).unwrap();
    fs::create_dir(&two).unwrap();
    let (watcher, changes) = watch(vec![one.clone(), two.clone()], RecursiveMode::NonRecursive, |_| true);
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
        "watch",
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
    let (touched, changes) = mpsc::channel();
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
    assert!(noticed(&changes));
    // Watching the directory itself, once it's there, is the caller's call.
    watcher.watch(vec![dir.clone()]);
    while changes.recv_timeout(QUIET).is_ok() {}
    fs::write(dir.join("script"), "x").unwrap();
    assert!(noticed(&changes));
}

/// Points the symlink `link` at `target` as `ln -sfn` and Nix do: a new link
/// renamed over the old one.
fn relink(target: &Path, link: &Path) {
    let new = link.with_extension("new");
    std::os::unix::fs::symlink(target, &new).unwrap();
    fs::rename(&new, link).unwrap();
}

fn is_desktop_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "desktop")
}

#[test]
fn a_profile_link_switched_to_a_new_generation_is_watched_there() {
    let root = TempDir::new().unwrap();
    let (one, two) = (root.path().join("gen-1"), root.path().join("gen-2"));
    fs::create_dir_all(one.join("share/applications")).unwrap();
    fs::create_dir_all(two.join("share/applications")).unwrap();
    // nix-profile -> profiles/profile -> gen-1, as Nix lays a profile out.
    let profiles = root.path().join("profiles");
    fs::create_dir(&profiles).unwrap();
    relink(&one, &profiles.join("profile"));
    let profile = root.path().join("nix-profile");
    relink(&profiles.join("profile"), &profile);
    let (_watcher, changes) = watch(
        vec![profile.join("share/applications")],
        RecursiveMode::Recursive,
        is_desktop_file,
    );

    // A switch replaces the link furthest in, and maybe others with it:
    // noticed once.
    fs::write(two.join("share/applications/new.desktop"), "x").unwrap();
    relink(&two, &profiles.join("profile"));
    relink(&profiles.join("profile"), &profile);
    assert!(noticed(&changes));
    assert!(quiet(&changes));

    // The old generation isn't watched any more, the new one is.
    fs::write(one.join("share/applications/old.desktop"), "x").unwrap();
    assert!(quiet(&changes));
    fs::write(two.join("share/applications/later.desktop"), "x").unwrap();
    assert!(noticed(&changes));
}

#[test]
fn a_directory_a_new_generation_brings_is_noticed_and_then_watched() {
    let root = TempDir::new().unwrap();
    let (one, two) = (root.path().join("gen-1"), root.path().join("gen-2"));
    fs::create_dir_all(&one).unwrap();
    fs::create_dir_all(two.join("share/kotodama/extensions")).unwrap();
    let system = root.path().join("current-system");
    relink(&one, &system);
    let dir = system.join("share/kotodama/extensions");
    let inside = dir.clone();
    let (_watcher, changes) = watch(vec![dir], RecursiveMode::NonRecursive, move |path| {
        path.starts_with(&inside)
    });

    relink(&two, &system);
    assert!(noticed(&changes));
    fs::create_dir(two.join("share/kotodama/extensions/new")).unwrap();
    assert!(noticed(&changes));
}

#[test]
fn a_profile_made_by_the_first_switch_is_noticed_and_then_watched() {
    let root = TempDir::new().unwrap();
    let generation = root.path().join("gen-1");
    fs::create_dir_all(generation.join("share/applications")).unwrap();
    // No profile yet, nor the directory its link goes in.
    let profile = root.path().join("per-user/alice");
    let (_watcher, changes) = watch(
        vec![profile.join("share/applications")],
        RecursiveMode::Recursive,
        is_desktop_file,
    );

    fs::create_dir(root.path().join("per-user")).unwrap();
    relink(&generation, &profile);
    assert!(noticed(&changes));
    fs::write(generation.join("share/applications/app.desktop"), "x").unwrap();
    assert!(noticed(&changes));
}

#[test]
fn only_a_watched_link_replaced_counts_in_its_directory() {
    let root = TempDir::new().unwrap();
    let generation = root.path().join("gen-1");
    fs::create_dir_all(generation.join("share")).unwrap();
    let system = root.path().join("current-system");
    relink(&generation, &system);
    let (_watcher, changes) = watch(vec![system.join("share")], RecursiveMode::NonRecursive, |_| false);
    fs::write(root.path().join("unrelated"), "x").unwrap();
    relink(&generation, &root.path().join("booted-system"));
    assert!(quiet(&changes));
    relink(&generation, &system);
    assert!(noticed(&changes));
}

#[test]
fn reading_what_changed_once_it_settles_is_not_another_change() {
    // Regression: #240.
    let root = TempDir::new().unwrap();
    let dir = root.path().join("config");
    fs::create_dir(&dir).unwrap();
    let file = dir.join("user.toml");
    fs::write(&file, "v = 1").unwrap();
    let (settled, changes) = mpsc::channel();
    let watcher = Watcher::start(
        "watch",
        RecursiveMode::NonRecursive,
        SETTLE,
        {
            let file = file.clone();
            move |event, _| event.paths.contains(&file)
        },
        {
            let file = file.clone();
            move |_| {
                let _ = fs::read_to_string(&file);
                let _ = settled.send(());
            }
        },
    )
    .unwrap();
    watcher.watch(vec![dir.clone()]);

    fs::write(&file, "v = 2").unwrap();

    assert!(noticed(&changes));
    assert!(quiet(&changes));
}

/// Each thread of this process with how often it has woken: its voluntary context switches.
#[cfg(target_os = "linux")]
fn wakeups_by_thread() -> HashMap<String, u64> {
    let mut wakeups = HashMap::new();
    for task in fs::read_dir("/proc/self/task").unwrap().flatten() {
        let status = fs::read_to_string(task.path().join("status")).unwrap_or_default();
        let count = status
            .lines()
            .find_map(|line| line.strip_prefix("voluntary_ctxt_switches:"))
            .and_then(|count| count.trim().parse::<u64>().ok());
        if let Some(count) = count {
            wakeups.insert(task.file_name().to_string_lossy().into_owned(), count);
        }
    }
    wakeups
}

/// Set in the process the wakeup test runs itself in.
#[cfg(target_os = "linux")]
const ALONE: &str = "KOTODAMA_WATCH_TEST_ALONE";

#[cfg(target_os = "linux")]
#[test]
fn reads_in_a_watched_directory_never_wake_the_watcher() {
    const NAME: &str = "reads_in_a_watched_directory_never_wake_the_watcher";
    // Regression: #240.
    if std::env::var_os(ALONE).is_none() {
        // In a process of its own: under `cargo test` the other tests' watchers start, wake and
        // end beside it, and their threads can't be told from its own.
        let run = Command::new(std::env::current_exe().unwrap())
            .args([NAME, "--exact", "--test-threads=1", "--nocapture"])
            .env(ALONE, "1")
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&run.stdout);
        assert!(
            run.status.success() && out.contains("1 passed"),
            "{out}{}",
            String::from_utf8_lossy(&run.stderr)
        );
        return;
    }
    let root = TempDir::new().unwrap();
    let dir = root.path().join("share");
    fs::create_dir(&dir).unwrap();
    // Two files read in turn, as the kernel merges a run of identical events.
    let (one, two) = (dir.join("one"), dir.join("two"));
    fs::write(&one, "x").unwrap();
    fs::write(&two, "x").unwrap();
    let threads = wakeups_by_thread();
    let (_watcher, changes) = watch(vec![dir.clone()], RecursiveMode::NonRecursive, |_| true);
    thread::sleep(SETTLE);
    // The threads the watcher started.
    let before: HashMap<String, u64> = wakeups_by_thread()
        .into_iter()
        .filter(|(thread, _)| !threads.contains_key(thread))
        .collect();
    assert!(!before.is_empty(), "the watcher started no thread");

    // Spaced, so each read would wake the watcher on its own rather than in a batch.
    for _ in 0..50 {
        let _ = fs::read(&one).unwrap();
        let _ = fs::read(&two).unwrap();
        let _ = fs::read_dir(&dir).unwrap().count();
        thread::sleep(Duration::from_millis(2));
    }

    assert!(quiet(&changes));
    let after = wakeups_by_thread();
    let woke: u64 = before
        .iter()
        .map(|(thread, count)| after.get(thread).map_or(0, |now| now.saturating_sub(*count)))
        .sum();
    assert!(woke <= 2, "the watcher woke {woke} times for reads");
}
