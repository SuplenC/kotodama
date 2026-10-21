//! Where an Extension keeps its files, by the XDG Base Directory spec.
//!
//! # Owns
//!
//! The state and cache directories of an Extension, from the XDG variables
//! or `$HOME`.
//!
//! # Not here
//!
//! Making the directories or anything in them, which the Extension does;
//! `$XDG_RUNTIME_DIR` (`crate::runtime`).

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

/// The directory for what `extension` keeps between runs, such as a history:
/// `$XDG_STATE_HOME/kotodama/<extension>`, or `~/.local/state/kotodama/<extension>`.
/// `None` if neither is set.
pub fn state(extension: &str) -> Option<PathBuf> {
    let base = base(env::var_os("XDG_STATE_HOME"), env::var_os("HOME"), ".local/state")?;
    Some(base.join("kotodama").join(extension))
}

/// The directory for what `extension` can fetch again if it's lost:
/// `$XDG_CACHE_HOME/kotodama/<extension>`, or `~/.cache/kotodama/<extension>`.
/// `None` if neither is set.
pub fn cache(extension: &str) -> Option<PathBuf> {
    let base = base(env::var_os("XDG_CACHE_HOME"), env::var_os("HOME"), ".cache")?;
    Some(base.join("kotodama").join(extension))
}

/// The base directory a variable names, or `home`'s `fallback` if it names
/// none. The spec has a relative path ignored, like an empty one.
fn base(var: Option<OsString>, home: Option<OsString>, fallback: &str) -> Option<PathBuf> {
    match var.map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => Some(dir),
        _ => Some(PathBuf::from(home.filter(|home| !home.is_empty())?).join(fallback)),
    }
}

#[cfg(test)]
mod tests;
