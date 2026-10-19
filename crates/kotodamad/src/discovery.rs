//! Finding Extensions: every `<dir>/<anything>/manifest.toml` in the Extension
//! directories.

use std::fs;
use std::path::{Path, PathBuf};

use kotodama_protocol::Manifest;
use regex::Regex;

/// An Extension found on disk, with its Live query triggers compiled.
#[derive(Clone, Debug)]
pub struct Found {
    pub manifest: Manifest,
    /// The directory holding `manifest.toml`; the command runs from here.
    pub dir: PathBuf,
    pub triggers: Vec<Regex>,
}

impl Found {
    /// Refuses a Manifest whose Live query trigger doesn't compile.
    pub fn new(manifest: Manifest, dir: PathBuf) -> Result<Self, String> {
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
