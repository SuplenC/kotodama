//! Finding the apps: every `.desktop` file in the `applications` directory of
//! each XDG data directory, by desktop id.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use crate::entry;
use crate::entry::Entry;
use crate::entry::Locale;
use crate::exec;
use crate::exec::Fields;

/// How deep `applications` directories are searched, so a symlink loop can't
/// keep the scan going.
const MAX_DEPTH: usize = 8;

/// An app that can be shown and started.
#[derive(Clone, Debug, PartialEq)]
pub struct App {
    /// The desktop id without `.desktop`, as in `firefox` or
    /// `org.gnome.Nautilus`: the Item id, and what the config names apps by.
    pub id: String,
    /// The desktop id, as in `firefox.desktop`.
    pub desktop_id: String,
    pub name: String,
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub icon: Option<String>,
    pub keywords: Vec<String>,
    pub command: Vec<String>,
    pub terminal: bool,
    /// The directory to start it in.
    pub dir: Option<PathBuf>,
    pub actions: Vec<AppAction>,
}

/// A desktop action of an [`App`], ready to start.
#[derive(Clone, Debug, PartialEq)]
pub struct AppAction {
    pub id: String,
    pub name: String,
    pub command: Vec<String>,
}

/// What decides which entries are shown, read once from the environment.
pub struct Environment {
    pub locale: Locale,
    /// `XDG_CURRENT_DESKTOP`, split on `:`.
    pub desktops: Vec<String>,
    /// `PATH`, for `TryExec`.
    pub path: Option<OsString>,
}

impl Environment {
    pub fn from_env() -> Self {
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(|var| env::var(var).ok())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        Environment {
            locale: Locale::new(&locale),
            desktops: env::var("XDG_CURRENT_DESKTOP")
                .unwrap_or_default()
                .split(':')
                .filter(|desktop| !desktop.is_empty())
                .map(str::to_owned)
                .collect(),
            path: env::var_os("PATH"),
        }
    }
}

/// The `applications` directory of each XDG data directory, most important
/// first: `$XDG_DATA_HOME`, then each of `$XDG_DATA_DIRS`.
pub fn application_dirs() -> Vec<PathBuf> {
    let home = env::var_os("XDG_DATA_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let dirs = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in home
        .into_iter()
        .chain(dirs.split(':').filter(|dir| !dir.is_empty()).map(PathBuf::from))
    {
        let dir = dir.join("applications");
        if !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// Every app in `dirs`, most important directory first, by id. Of two files
/// with the same desktop id, the one in the more important directory wins,
/// even if it hides the app.
pub fn scan(dirs: &[PathBuf], env: &Environment) -> BTreeMap<String, App> {
    let mut seen = HashSet::new();
    let mut apps = BTreeMap::new();
    for dir in dirs {
        let mut files = Vec::new();
        collect(dir, "", 0, &mut files);
        files.sort();
        for (desktop_id, path) in files {
            if !seen.insert(desktop_id.clone()) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let Some(entry) = entry::parse(&text, &env.locale) else {
                continue;
            };
            match app(desktop_id, &path, entry, env) {
                Ok(Some(app)) => {
                    apps.insert(app.id.clone(), app);
                }
                Ok(None) => {}
                Err(err) => kotodama_extension::log!("{}: {err}", path.display()),
            }
        }
    }
    apps
}

/// The `.desktop` files under `dir` with their desktop ids: the path below
/// `applications`, with `/` as `-`.
fn collect(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // Follows symlinks, as packages and Nix profiles use them.
        let is_dir = if kind.is_symlink() {
            fs::metadata(&path).is_ok_and(|meta| meta.is_dir())
        } else {
            kind.is_dir()
        };
        if is_dir {
            if depth < MAX_DEPTH {
                collect(&path, &format!("{prefix}{name}-"), depth + 1, out);
            }
        } else if name.ends_with(".desktop") {
            out.push((format!("{prefix}{name}"), path));
        }
    }
}

/// A desktop id without `.desktop`: the app's Item id, and how the config
/// names it, with the suffix or without.
pub fn app_id(desktop_id: &str) -> String {
    desktop_id.strip_suffix(".desktop").unwrap_or(desktop_id).to_owned()
}

/// The app an entry describes, or `None` if it isn't one to show here.
fn app(desktop_id: String, path: &Path, entry: Entry, env: &Environment) -> Result<Option<App>, String> {
    if entry.kind != "Application" || entry.hidden || entry.no_display || !shown_in(&entry, env) {
        return Ok(None);
    }
    let Some(exec) = &entry.exec else {
        return Ok(None);
    };
    if let Some(try_exec) = &entry.try_exec
        && !installed(try_exec, env.path.as_deref())
    {
        return Ok(None);
    }
    let file = path.to_string_lossy();
    let fields = Fields {
        name: &entry.name,
        icon: entry.icon.as_deref(),
        file: &file,
    };
    let command = exec::command(exec, &fields)?;
    let actions = entry
        .actions
        .iter()
        .filter_map(|action| match exec::command(&action.exec, &fields) {
            Ok(command) => Some(AppAction {
                id: action.id.clone(),
                name: action.name.clone(),
                command,
            }),
            Err(err) => {
                kotodama_extension::log!("{}: action {}: {err}", path.display(), action.id);
                None
            }
        })
        .collect();
    Ok(Some(App {
        id: app_id(&desktop_id),
        desktop_id,
        name: entry.name,
        generic_name: entry.generic_name,
        comment: entry.comment,
        icon: entry.icon,
        keywords: entry.keywords,
        command,
        terminal: entry.terminal,
        dir: entry.path.filter(|dir| !dir.is_empty()).map(PathBuf::from),
        actions,
    }))
}

/// Whether `OnlyShowIn` and `NotShowIn` let the entry show on the current
/// desktop.
fn shown_in(entry: &Entry, env: &Environment) -> bool {
    let current = |list: &[String]| env.desktops.iter().any(|desktop| list.contains(desktop));
    (entry.only_show_in.is_empty() || current(&entry.only_show_in)) && !current(&entry.not_show_in)
}

/// Whether `program`, a path or a name looked up in `PATH`, is an executable
/// file.
fn installed(program: &str, path: Option<&std::ffi::OsStr>) -> bool {
    if program.contains('/') {
        return executable(Path::new(program));
    }
    path.is_some_and(|path| env::split_paths(path).any(|dir| executable(&dir.join(program))))
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(path: &Path) -> bool {
    path.is_file()
}
