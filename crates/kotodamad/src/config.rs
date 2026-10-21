//! The config directory: every top-level `*.toml` in each config directory,
//! merged into one table. Extensions get their `[extensions.<name>]` table from
//! it, and every other part of the Daemon reads its settings from it.
//!
//! Directories are read in the order given, and the files in each in filename
//! order. Later values win: tables merge key by key, and anything else,
//! including a list, replaces the earlier value whole. File names carry no
//! meaning, and subdirectories (such as `extensions/`) are ignored.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

use kotodama_extension::watch::RecursiveMode;
use kotodama_extension::watch::Watcher;
use kotodama_extension::watch::notify;
use kotodama_protocol::Table;
use kotodama_protocol::Value;

use crate::daemon::Event;

/// How long the files must stay still before a reload, so an editor's save,
/// often several writes and a rename, is read once and whole.
const SETTLE: Duration = Duration::from_millis(50);

/// Why the config couldn't be loaded. The caller keeps the last good config.
#[derive(Debug)]
pub struct Error {
    pub path: PathBuf,
    pub reason: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.reason)
    }
}

impl std::error::Error for Error {}

/// The file each value of the merged config came from, by its dotted key,
/// such as `history.half_life_days`. A list is one value.
pub type Sources = BTreeMap<String, PathBuf>;

/// Reads and merges every config file in `dirs`, least important directory
/// first. A directory that doesn't exist counts as empty. Fails on the first
/// file that can't be read or parsed.
///
/// # Errors
///
/// When a directory or a file in it can't be read, or a file isn't TOML.
pub fn load(dirs: &[PathBuf]) -> Result<Table, Error> {
    load_from(dirs, None)
}

/// [`load`], and where each value came from.
///
/// # Errors
///
/// As [`load`].
pub fn load_with_sources(dirs: &[PathBuf]) -> Result<(Table, Sources), Error> {
    let mut sources = Sources::new();
    let config = load_from(dirs, Some(&mut sources))?;
    Ok((config, sources))
}

fn load_from(dirs: &[PathBuf], mut sources: Option<&mut Sources>) -> Result<Table, Error> {
    let mut config = Table::new();
    for dir in dirs {
        for path in files(dir)? {
            let error = |reason: String| Error {
                path: path.clone(),
                reason,
            };
            let text = fs::read_to_string(&path).map_err(|err| error(err.to_string()))?;
            let table = parse(&text).map_err(error)?;
            if let Some(sources) = sources.as_deref_mut() {
                note_sources(&table, "", &path, sources);
            }
            merge(&mut config, table);
        }
    }
    Ok(config)
}

/// One config file's text as a table.
///
/// # Errors
///
/// When `text` isn't TOML.
pub fn parse(text: &str) -> Result<Table, String> {
    let table: toml::Table = text.parse().map_err(|err: toml::de::Error| err.message().to_owned())?;
    Ok(from_toml_table(table))
}

/// Notes `path` as the source of every value in `table`, under `prefix`.
fn note_sources(table: &Table, prefix: &str, path: &Path, sources: &mut Sources) {
    for (key, value) in table {
        let key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Value::Table(table) => note_sources(table, &key, path, sources),
            _ => {
                sources.insert(key, path.to_owned());
            }
        }
    }
}

/// Watches `dirs` and sends [`Event::Config`] with the reloaded config after
/// each change, or [`Event::ConfigFailed`] when it can't be loaded, and the
/// Daemon keeps the last good config. Watching stops when the watcher is
/// dropped.
///
/// # Errors
///
/// When the watcher can't start.
pub fn watch(dirs: Vec<PathBuf>, events: Sender<Event>) -> notify::Result<Watcher> {
    let filter = dirs.clone();
    let watcher = Watcher::start(
        "config",
        RecursiveMode::NonRecursive,
        SETTLE,
        move |event, _| event.paths.iter().any(|path| is_config_file(&filter, path)),
        {
            let dirs = dirs.clone();
            move |_| match load(&dirs) {
                Ok(config) => {
                    let _ = events.send(Event::Config(config));
                }
                Err(err) => {
                    let _ = events.send(Event::ConfigFailed(err));
                }
            }
        },
    )?;
    watcher.watch(dirs);
    Ok(watcher)
}

/// Whether `path` is a file `load` reads from one of `dirs`, so a watcher can
/// ignore everything else.
pub fn is_config_file(dirs: &[PathBuf], path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "toml")
        && path.parent().is_some_and(|parent| dirs.iter().any(|dir| dir == parent))
}

/// The top-level `*.toml` files in `dir`, in filename order.
fn files(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(Error {
                path: dir.to_owned(),
                reason: err.to_string(),
            });
        }
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml") && path.is_file())
        .collect();
    files.sort();
    Ok(files)
}

/// Merges `later` into `config`: tables key by key, everything else whole.
pub fn merge(config: &mut Table, later: Table) {
    for (key, value) in later {
        match (config.get_mut(&key), value) {
            (Some(Value::Table(earlier)), Value::Table(later)) => merge(earlier, later),
            (_, value) => {
                config.insert(key, value);
            }
        }
    }
}

/// The `[extensions.<name>]` table, or an empty one.
pub fn extension_table(config: &Table, name: &str) -> Table {
    match config.get("extensions") {
        Some(Value::Table(extensions)) => match extensions.get(name) {
            Some(Value::Table(table)) => table.clone(),
            _ => Table::new(),
        },
        _ => Table::new(),
    }
}

/// The keys of `[extensions.<name>]` the Daemon reads itself, for every
/// Extension.
pub const DAEMON_KEYS: [&str; 7] = ["word", "prefix", "in_search", "enable", "command", "env", "live_query"];
/// The Manifest's keys, which an Extension defined in the config takes from
/// its table too.
pub const MANIFEST_KEYS: [&str; 8] = [
    "description",
    "title",
    "icon",
    "about",
    "examples",
    "commands",
    "contributed_actions",
    "action_catalog",
];

/// The `[extensions.<name>]` table as the Extension gets it: without the
/// keys the Daemon reads itself, which the Extension would warn are no
/// setting of its own; for one `inline`, defined in the config, without its
/// Manifest's keys too.
pub fn extension_settings(config: &Table, name: &str, inline: bool) -> Table {
    let mut table = extension_table(config, name);
    table.retain(|key, _| !DAEMON_KEYS.contains(&key.as_str()) && (!inline || !MANIFEST_KEYS.contains(&key.as_str())));
    table
}

fn from_toml_table(table: toml::Table) -> Table {
    table.into_iter().map(|(key, value)| (key, from_toml(value))).collect()
}

/// `value` as TOML: what a Manifest is read from, and what `kotodamad config`
/// prints.
pub fn to_toml(value: &Value) -> toml::Value {
    match value {
        Value::String(value) => toml::Value::String(value.clone()),
        Value::Integer(value) => toml::Value::Integer(*value),
        Value::Float(value) => toml::Value::Float(*value),
        Value::Bool(value) => toml::Value::Boolean(*value),
        Value::Array(values) => toml::Value::Array(values.iter().map(to_toml).collect()),
        Value::Table(table) => {
            toml::Value::Table(table.iter().map(|(key, value)| (key.clone(), to_toml(value))).collect())
        }
    }
}

fn from_toml(value: toml::Value) -> Value {
    match value {
        toml::Value::String(value) => Value::String(value),
        toml::Value::Integer(value) => Value::Integer(value),
        toml::Value::Float(value) => Value::Float(value),
        toml::Value::Boolean(value) => Value::Bool(value),
        toml::Value::Datetime(value) => Value::String(value.to_string()),
        toml::Value::Array(values) => Value::Array(values.into_iter().map(from_toml).collect()),
        toml::Value::Table(table) => Value::Table(from_toml_table(table)),
    }
}
