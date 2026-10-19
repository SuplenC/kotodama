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
}

impl Paths {
    pub fn from_env() -> Result<Self, String> {
        let runtime = env::var_os("XDG_RUNTIME_DIR")
            .filter(|dir| !dir.is_empty())
            .ok_or("XDG_RUNTIME_DIR isn't set")?;
        let config_home = match env::var_os("XDG_CONFIG_HOME").filter(|dir| !dir.is_empty()) {
            Some(dir) => PathBuf::from(dir),
            None => PathBuf::from(env::var_os("HOME").ok_or("HOME isn't set")?).join(".config"),
        };
        let user = config_home.join("kotodama");

        let mut config_dirs = search_path("XDG_CONFIG_DIRS", "/etc/xdg");
        config_dirs.push(user.clone());
        let mut extension_dirs: Vec<PathBuf> =
            search_path("XDG_DATA_DIRS", "/usr/local/share:/usr/share")
                .into_iter()
                .map(|dir| dir.join("extensions"))
                .collect();
        extension_dirs.push(user.join("extensions"));

        Ok(Paths {
            socket: PathBuf::from(runtime).join("kotodama.sock"),
            config_dirs,
            extension_dirs,
        })
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
