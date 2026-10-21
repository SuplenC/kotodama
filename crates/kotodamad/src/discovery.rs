//! Finding Extensions: every `<dir>/<anything>/manifest.toml` in the Extension
//! directories.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime};

use kotodama_extension::watch::{RecursiveMode, Watcher, notify};
use kotodama_protocol::{Manifest, Problem};
use regex::Regex;

use crate::daemon::{Event, OWN};
use crate::problem::Report;

/// How long the Extension directories must stay still before they are read
/// again, so a package install that writes many files is read once, whole.
const SETTLE: Duration = Duration::from_millis(200);

/// An Extension found on disk or defined in the config, with its Live query
/// triggers compiled.
#[derive(Clone, Debug)]
pub struct Found {
    pub manifest: Manifest,
    /// The directory holding `manifest.toml`; the command runs from here.
    /// Empty for an Extension defined in the config, which runs from the
    /// Daemon's directory.
    pub dir: PathBuf,
    pub triggers: Vec<Regex>,
    /// Set in its process's environment, from `[extensions.<name>] env`.
    pub env: BTreeMap<String, String>,
    /// Defined in the config rather than by a `manifest.toml`.
    pub inline: bool,
    /// Whether `command` came from the config, where a bare program name is
    /// looked up on `PATH`.
    pub command_from_config: bool,
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
            env: BTreeMap::new(),
            inline: false,
            command_from_config: false,
        })
    }

    /// The program to run. A Manifest's relative path is resolved against
    /// the Extension's directory and never looked up on `PATH`, so what runs
    /// doesn't depend on the Daemon's environment. A bare name from the
    /// config is looked up on `PATH`, so a packaged program needs no path.
    pub fn program(&self) -> PathBuf {
        let program = Path::new(&self.manifest.command[0]);
        let bare = program.components().count() == 1 && !program.is_absolute();
        if bare && self.command_from_config {
            return on_path(program).unwrap_or_else(|| program.to_owned());
        }
        self.dir.join(program)
    }

    /// What the program file is like now, to tell when it changes; `None`
    /// while it doesn't exist.
    pub fn stamp(&self) -> Option<Stamp> {
        let metadata = fs::metadata(self.program()).ok()?;
        Some(Stamp {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

/// The first file named `program` in a `PATH` directory.
fn on_path(program: &Path) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// When a file was last written, and how long it is: enough to tell that a
/// program was installed, rebuilt or replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

/// The Extensions found, and what is wrong with the Manifests that were
/// refused or are out of date.
#[derive(Clone, Debug, Default)]
pub struct Discovered {
    pub found: Vec<Found>,
    pub problems: Vec<Problem>,
}

impl From<Vec<Found>> for Discovered {
    fn from(found: Vec<Found>) -> Self {
        Discovered {
            found,
            problems: Vec::new(),
        }
    }
}

/// Every Extension in `dirs`, least important directory first. An Extension in
/// a later directory replaces one of the same name in an earlier one, so the
/// user's own copy wins over the distro's. Sorted by name. Broken Manifests
/// are problems, and skipped.
pub fn discover(dirs: &[PathBuf]) -> Discovered {
    let mut found: Vec<Found> = Vec::new();
    let mut report = Report::new();
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
            match load(&path, &manifest_path, &mut report) {
                Ok(extension) => {
                    found.retain(|other| other.manifest.name != extension.manifest.name);
                    found.push(extension);
                }
                Err(err) => report.add(manifest_path.display().to_string(), err),
            }
        }
    }
    found.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    Discovered {
        found,
        problems: report.into_problems(),
    }
}

fn load(dir: &Path, manifest_path: &Path, report: &mut Report) -> Result<Found, String> {
    let text = fs::read_to_string(manifest_path).map_err(|err| err.to_string())?;
    let manifest = Manifest::from_toml(&text).map_err(|err| err.to_string())?;
    if text
        .parse::<toml::Table>()
        .is_ok_and(|table| table.contains_key("prefix"))
    {
        report.add(
            manifest_path.display().to_string(),
            "`prefix` is the old name of `word`; call it `word`",
        );
    }
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
