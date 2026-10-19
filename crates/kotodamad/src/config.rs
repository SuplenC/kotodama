//! The config directory: every top-level `*.toml` in each config directory,
//! merged into one table. Extensions get their `[extensions.<name>]` table from
//! it, and every other part of the Daemon reads its settings from it.
//!
//! Directories are read in the order given, and the files in each in filename
//! order. Later values win: tables merge key by key, and anything else,
//! including a list, replaces the earlier value whole. File names carry no
//! meaning, and subdirectories (such as `extensions/`) are ignored.

use std::path::{Path, PathBuf};
use std::{fmt, fs, io};

use kotodama_protocol::{Table, Value};

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

/// Reads and merges every config file in `dirs`, least important directory
/// first. A directory that doesn't exist counts as empty. Fails on the first
/// file that can't be read or parsed.
pub fn load(dirs: &[PathBuf]) -> Result<Table, Error> {
    let mut config = Table::new();
    for dir in dirs {
        for path in files(dir)? {
            let error = |reason: String| Error {
                path: path.clone(),
                reason,
            };
            let text = fs::read_to_string(&path).map_err(|err| error(err.to_string()))?;
            let table: toml::Table = text
                .parse()
                .map_err(|err: toml::de::Error| error(err.message().to_owned()))?;
            merge(&mut config, from_toml_table(table));
        }
    }
    Ok(config)
}

/// Whether `path` is a file `load` reads from one of `dirs`, so a watcher can
/// ignore everything else.
pub fn is_config_file(dirs: &[PathBuf], path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "toml")
        && path
            .parent()
            .is_some_and(|parent| dirs.iter().any(|dir| dir == parent))
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
        .filter_map(|entry| entry.ok())
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

fn from_toml_table(table: toml::Table) -> Table {
    table
        .into_iter()
        .map(|(key, value)| (key, from_toml(value)))
        .collect()
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
