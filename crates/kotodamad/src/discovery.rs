//! Finding Extensions: every `<dir>/<anything>/manifest.toml` in the Extension
//! directories.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use kotodama_extension::watch::{RecursiveMode, Watcher, notify};
use kotodama_protocol::Manifest;
use regex::Regex;

use crate::daemon::{Event, OWN};

/// How long the Extension directories must stay still before they are read
/// again, so a package install that writes many files is read once, whole.
const SETTLE: Duration = Duration::from_millis(200);

/// An Extension found on disk, with its Live query triggers compiled.
#[derive(Clone, Debug)]
pub struct Found {
    pub manifest: Manifest,
    /// The directory holding `manifest.toml`; the command runs from here.
    pub dir: PathBuf,
    pub triggers: Vec<Regex>,
}

impl Found {
    /// Refuses a Manifest whose Live query trigger doesn't compile, or that
    /// takes the name the Daemon's own Results go by.
    pub fn new(manifest: Manifest, dir: PathBuf) -> Result<Self, String> {
        if manifest.name == OWN {
            return Err(format!("the name `{OWN}` is reserved for the Daemon"));
        }
        let triggers = manifest
            .live_query
            .iter()
            .flat_map(|live| &live.triggers)
            .map(|trigger| Regex::new(trigger).map_err(|err| format!("trigger `{trigger}`: {err}")))
            .collect::<Result<_, _>>()?;
        Ok(Found {
            manifest,
            dir,
            triggers,
        })
    }

    /// The program to run and its arguments. A relative program path is
    /// resolved against the Extension's directory.
    pub fn program(&self) -> PathBuf {
        self.dir.join(&self.manifest.command[0])
    }
}

/// Every Extension in `dirs`, least important directory first. An Extension in
/// a later directory replaces one of the same name in an earlier one, so the
/// user's own copy wins over the distro's. Sorted by name. Broken Manifests
/// are logged and skipped.
pub fn discover(dirs: &[PathBuf]) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect();
        paths.sort();
        for path in paths {
            let manifest_path = path.join("manifest.toml");
            if !manifest_path.is_file() {
                continue;
            }
            match load(&path, &manifest_path) {
                Ok(extension) => {
                    found.retain(|other| other.manifest.name != extension.manifest.name);
                    found.push(extension);
                }
                Err(err) => log::error!("{}: {err}", manifest_path.display()),
            }
        }
    }
    found.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    found
}

fn load(dir: &Path, manifest_path: &Path) -> Result<Found, String> {
    let text = fs::read_to_string(manifest_path).map_err(|err| err.to_string())?;
    let manifest = Manifest::from_toml(&text).map_err(|err| err.to_string())?;
    Found::new(manifest, dir.to_owned())
}

/// Watches the Extension directories and sends [`Event::Extensions`] after each
/// change: an Extension directory added, removed or renamed, or a
/// `manifest.toml` written. Files an Extension writes in its own directory
/// are ignored. Watching stops when the watcher is dropped.
pub fn watch(dirs: Vec<PathBuf>, events: Sender<Event>) -> notify::Result<Watcher> {
    let filter = dirs.clone();
    let first = watched(&dirs);
    let watcher = Watcher::start(
        "extensions",
        RecursiveMode::NonRecursive,
        SETTLE,
        move |event, _| event.paths.iter().any(|path| is_relevant(&filter, path)),
        move |watcher| {
            // New Extension directories need watching for their Manifests.
            watcher.watch(watched(&dirs));
            let _ = events.send(Event::Extensions(discover(&dirs)));
        },
    )?;
    watcher.watch(first);
    Ok(watcher)
}

/// Each Extension directory and each Extension in it, to watch without
/// recursing: deeper changes are an Extension's own business.
fn watched(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut watched = dirs.to_vec();
    for dir in dirs {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                watched.push(path);
            }
        }
    }
    watched
}

/// An Extension directory itself, or a Manifest in one.
fn is_relevant(dirs: &[PathBuf], path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    dirs.iter().any(|dir| dir == parent)
        || (path.file_name().is_some_and(|name| name == "manifest.toml")
            && parent
                .parent()
                .is_some_and(|grandparent| dirs.iter().any(|dir| dir == grandparent)))
}
