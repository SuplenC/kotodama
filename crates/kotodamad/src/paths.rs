//! Where the Daemon finds its config, Extensions, socket, state and cache.
//!
//! The config directories come first, since the rest may be set in the
//! config: `kotodamad --config <dir>`, else `KOTODAMA_CONFIG_DIR`, else the
//! XDG ones. Then `[paths]`:
//!
//! ```toml
//! [paths]
//! extension_dirs = ["~/src/my-extension/dist"]  # read after the default ones
//! default_extension_dirs = true  # false reads only `extension_dirs`
//! state_dir = "~/.local/state/kotodama"   # the History and the Usage log
//! cache_dir = "~/.cache/kotodama"         # the Items not shown
//! socket = "/run/user/1000/kotodama.sock" # `KOTODAMA_SOCKET` wins
//! ```
//!
//! The Extension directories follow the config as it reloads; the state and
//! cache directories and the socket are read once, at start.
//!
//! # Owns
//!
//! - [`config_dirs`], [`Paths`] and [`Overrides`] (`[paths]`).
//!
//! # Not here
//!
//! - Re-reading the Extension directories when `[paths]` changes: `lib.rs`.

use std::env;
use std::path::Path;
use std::path::PathBuf;

use kotodama_protocol::Table;
use kotodama_protocol::Value;

use crate::problem::Report;

/// The environment variable that names the one config directory, as
/// `--config` does.
pub const CONFIG_DIR_VAR: &str = "KOTODAMA_CONFIG_DIR";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub socket: PathBuf,
    /// Least important first: the distro's, then the user's.
    pub config_dirs: Vec<PathBuf>,
    /// Least important first: the packages', then the user's, then those
    /// `[paths] extension_dirs` adds.
    pub extension_dirs: Vec<PathBuf>,
    /// `<state>/history.json`.
    pub history: PathBuf,
    /// `<state>/usage.jsonl`.
    pub usage_log: PathBuf,
    /// Where the Items wait that aren't shown.
    pub cache: PathBuf,
}

/// What the config's `[paths]` table says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overrides {
    /// Read after the default Extension directories, so an Extension here
    /// replaces one of the same name there.
    pub extension_dirs: Vec<PathBuf>,
    /// Whether the XDG Extension directories are read at all: without them
    /// a distro or a test pins exactly what runs.
    pub default_extension_dirs: bool,
    pub state_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub socket: Option<PathBuf>,
}

impl Default for Overrides {
    fn default() -> Self {
        Overrides {
            extension_dirs: Vec::new(),
            default_extension_dirs: true,
            state_dir: None,
            cache_dir: None,
            socket: None,
        }
    }
}

impl Overrides {
    /// The `[paths]` table of the merged config. A path that can't be read
    /// goes in `report`, and is left out.
    pub fn from_config(config: &Table, report: &mut Report) -> Self {
        let mut overrides = Overrides::default();
        let table = match config.get("paths") {
            Some(Value::Table(table)) => table,
            Some(_) => {
                report.add("paths", "should be a table");
                return overrides;
            }
            None => return overrides,
        };
        for (key, value) in table {
            let set = match key.as_str() {
                "extension_dirs" => match value {
                    Value::Array(dirs) => {
                        for (n, dir) in dirs.iter().enumerate() {
                            match path(dir) {
                                Ok(dir) => overrides.extension_dirs.push(dir),
                                Err(err) => report.add(format!("paths.extension_dirs[{n}]"), err),
                            }
                        }
                        Ok(())
                    }
                    _ => Err("should be a list of directories"),
                },
                "default_extension_dirs" => match value {
                    Value::Bool(on) => {
                        overrides.default_extension_dirs = *on;
                        Ok(())
                    }
                    _ => Err("should be true or false"),
                },
                "state_dir" => path(value).map(|dir| overrides.state_dir = Some(dir)),
                "cache_dir" => path(value).map(|dir| overrides.cache_dir = Some(dir)),
                "socket" => path(value).map(|socket| overrides.socket = Some(socket)),
                _ => Err("isn't a setting"),
            };
            if let Err(err) = set {
                report.add(format!("paths.{key}"), err);
            }
        }
        overrides
    }
}

/// Why a path the Daemon needs has no place.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Nothing names the socket.
    #[error("XDG_RUNTIME_DIR isn't set, nor `[paths] socket`")]
    NoSocket,
    /// A path falls back on the home directory, and there is none.
    #[error("HOME isn't set")]
    NoHome,
}

/// The config directories, least important first: `dir`, as `--config`
/// names it, else `KOTODAMA_CONFIG_DIR`, else `$XDG_CONFIG_DIRS/kotodama`
/// and then `$XDG_CONFIG_HOME/kotodama`. One named directory is the whole
/// config, so a test or a distro reads nothing else.
///
/// # Errors
///
/// When neither `XDG_CONFIG_HOME` nor `HOME` is set.
pub fn config_dirs(dir: Option<PathBuf>) -> Result<Vec<PathBuf>, Error> {
    let named = dir.or_else(|| {
        env::var_os(CONFIG_DIR_VAR)
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
    });
    if let Some(dir) = named {
        return Ok(vec![dir]);
    }
    let mut dirs = search_path("XDG_CONFIG_DIRS", "/etc/xdg");
    dirs.push(xdg_home("XDG_CONFIG_HOME", ".config")?.join("kotodama"));
    Ok(dirs)
}

impl Paths {
    /// Every path, from the config directories and `[paths]`.
    ///
    /// # Errors
    ///
    /// When the socket has no place: neither `KOTODAMA_SOCKET`, `[paths]
    /// socket` nor `XDG_RUNTIME_DIR` is set; or when `HOME` is needed and
    /// isn't set.
    pub fn new(config_dirs: Vec<PathBuf>, overrides: &Overrides) -> Result<Self, Error> {
        let socket = match env::var_os("KOTODAMA_SOCKET").filter(|socket| !socket.is_empty()) {
            Some(socket) => PathBuf::from(socket),
            None => match &overrides.socket {
                Some(socket) => socket.clone(),
                None => kotodama_protocol::socket_path().ok_or(Error::NoSocket)?,
            },
        };
        let state = match &overrides.state_dir {
            Some(dir) => dir.clone(),
            None => xdg_home("XDG_STATE_HOME", ".local/state")?.join("kotodama"),
        };
        let cache = match &overrides.cache_dir {
            Some(dir) => dir.clone(),
            None => xdg_home("XDG_CACHE_HOME", ".cache")?.join("kotodama"),
        };
        Ok(Paths {
            socket,
            extension_dirs: extension_dirs(&config_dirs, overrides),
            config_dirs,
            history: state.join("history.json"),
            usage_log: state.join("usage.jsonl"),
            cache,
        })
    }
}

/// The Extension directories, least important first: unless `[paths]`
/// turns them off, `$XDG_DATA_DIRS/kotodama/extensions` and `extensions/`
/// in the user's config directory; then `[paths] extension_dirs`.
pub fn extension_dirs(config_dirs: &[PathBuf], overrides: &Overrides) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if overrides.default_extension_dirs {
        dirs.extend(
            search_path("XDG_DATA_DIRS", "/usr/local/share:/usr/share")
                .into_iter()
                .map(|dir| dir.join("extensions")),
        );
        dirs.extend(config_dirs.last().map(|dir| dir.join("extensions")));
    }
    dirs.extend(overrides.extension_dirs.iter().cloned());
    dirs
}

/// A path from the config: absolute, or starting with `~/` for the home
/// directory.
fn path(value: &Value) -> Result<PathBuf, &'static str> {
    const WANTED: &str = "should be an absolute path, or one starting with `~/`";
    let Value::String(text) = value else {
        return Err(WANTED);
    };
    let path = match text.strip_prefix("~/") {
        Some(rest) => PathBuf::from(env::var_os("HOME").ok_or("starts with `~/`, but HOME isn't set")?).join(rest),
        None => PathBuf::from(text),
    };
    if path.is_absolute() { Ok(path) } else { Err(WANTED) }
}

/// `$var`, or `fallback` in the home directory when it is unset or empty.
fn xdg_home(var: &str, fallback: &str) -> Result<PathBuf, Error> {
    match env::var_os(var).filter(|dir| !dir.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Ok(PathBuf::from(env::var_os("HOME").ok_or(Error::NoHome)?).join(fallback)),
    }
}

/// `<dir>/kotodama` for each directory in a colon-separated XDG search path,
/// reversed so the most important comes last and wins.
fn search_path(var: &str, default: &str) -> Vec<PathBuf> {
    let value = env::var(var).ok().filter(|value| !value.is_empty());
    let mut dirs: Vec<PathBuf> = value
        .as_deref()
        .unwrap_or(default)
        .split(':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(dir).join("kotodama"))
        .collect();
    dirs.reverse();
    dirs
}

#[cfg(test)]
mod tests;
