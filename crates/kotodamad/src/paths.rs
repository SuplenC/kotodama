//! Where the Daemon finds its socket, config and Extensions, from the XDG base
//! directory variables.

use std::env;
use std::path::PathBuf;

pub struct Paths {
    pub socket: PathBuf,
    /// Least important first: the distro's, then the user's.
    pub config_dirs: Vec<PathBuf>,
    /// Least important first: the packages', then the user's.
    pub extension_dirs: Vec<PathBuf>,
    /// `$XDG_STATE_HOME/kotodama/history.json`.
    pub history: PathBuf,
    /// `$XDG_STATE_HOME/kotodama/usage.jsonl`.
    pub usage_log: PathBuf,
    /// `$XDG_CACHE_HOME/kotodama`, where the Items wait that aren't shown.
    pub cache: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Self, String> {
        let socket = kotodama_protocol::socket_path().ok_or("XDG_RUNTIME_DIR isn't set")?;
        let config_home = xdg_home("XDG_CONFIG_HOME", ".config")?;
        let user = config_home.join("kotodama");
        let state_home = xdg_home("XDG_STATE_HOME", ".local/state")?;
        let cache_home = xdg_home("XDG_CACHE_HOME", ".cache")?;

        let mut config_dirs = search_path("XDG_CONFIG_DIRS", "/etc/xdg");
        config_dirs.push(user.clone());
        let mut extension_dirs: Vec<PathBuf> =
            search_path("XDG_DATA_DIRS", "/usr/local/share:/usr/share")
                .into_iter()
                .map(|dir| dir.join("extensions"))
                .collect();
        extension_dirs.push(user.join("extensions"));

        Ok(Paths {
            socket,
            config_dirs,
            extension_dirs,
            history: state_home.join("kotodama/history.json"),
            usage_log: state_home.join("kotodama/usage.jsonl"),
            cache: cache_home.join("kotodama"),
        })
    }
}

/// `$var`, or `fallback` in the home directory when it is unset or empty.
fn xdg_home(var: &str, fallback: &str) -> Result<PathBuf, String> {
    match env::var_os(var).filter(|dir| !dir.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Ok(PathBuf::from(env::var_os("HOME").ok_or("HOME isn't set")?).join(fallback)),
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
        .map(|dir| PathBuf::from(dir).join("kotodama"))
        .collect();
    dirs.reverse();
    dirs
}
