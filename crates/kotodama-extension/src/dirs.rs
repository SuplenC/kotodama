//! Where an Extension keeps its files, by the XDG Base Directory spec.

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
mod tests {
    use super::*;

    fn os(value: &str) -> Option<OsString> {
        Some(value.into())
    }

    #[test]
    fn the_variable_is_used_when_it_names_a_directory() {
        assert_eq!(
            base(os("/x/state"), os("/home/me"), ".local/state"),
            Some(PathBuf::from("/x/state"))
        );
    }

    #[test]
    fn home_is_used_when_the_variable_is_unset_empty_or_relative() {
        for var in [None, os(""), os("relative/state")] {
            assert_eq!(
                base(var, os("/home/me"), ".local/state"),
                Some(PathBuf::from("/home/me/.local/state"))
            );
        }
    }

    #[test]
    fn neither_set_is_none() {
        assert_eq!(base(None, None, ".cache"), None);
        assert_eq!(base(os(""), os(""), ".cache"), None);
    }
}
