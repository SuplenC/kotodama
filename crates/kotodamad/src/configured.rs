//! The Extensions as the config has them: those installed, switched off,
//! changed or added by their `[extensions.<name>]` tables.
//!
//! ```toml
//! # Switched off, still installed.
//! [extensions.windows]
//! enable = false
//!
//! # Run from a development build, asked sooner.
//! [extensions.calculator]
//! command = ["/home/me/src/calculator/target/debug/kotodama-calculator"]
//! env = { RUST_LOG = "debug" }
//! live_query = { debounce_ms = 0 }
//!
//! # An app that speaks the protocol itself, with no Manifest on disk.
//! [extensions.notes]
//! command = ["notes", "--kotodama"]
//! title = "Notes"
//! word = "note"
//! live_query = { prefixes = ["note "], debounce_ms = 100 }
//! ```
//!
//! A table with `command` for an Extension that isn't installed defines one,
//! from the Manifest's keys in it. Read when the config or the Extension
//! directories change, never on a keystroke.
//!
//! # Owns
//!
//! - [`configure`].
//!
//! # Not here
//!
//! - `word`, `prefix` and `in_search`: `apps.rs`.
//! - Which keys the Extension gets of its table: `config.rs`, `extension_settings`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use kotodama_extension::Chain;
use kotodama_protocol::LiveQueries;
use kotodama_protocol::Manifest;
use kotodama_protocol::Table;
use kotodama_protocol::Value;
use regex::Regex;
use serde::Deserialize;

use crate::config::MANIFEST_KEYS;
use crate::config::to_toml;
use crate::discovery::Found;
use crate::problem::Report;

/// `installed` as the config's `[extensions.<name>]` tables have them, then
/// the Extensions the config defines, by name. What can't be read goes in
/// `report`: a bad override is left out, and an Extension defined badly isn't
/// run.
pub fn configure(installed: &[Found], config: &Table, report: &mut Report) -> Vec<Found> {
    let empty = Table::new();
    let tables = match config.get("extensions") {
        Some(Value::Table(tables)) => tables,
        _ => &empty,
    };
    let table = |name: &str| match tables.get(name) {
        Some(Value::Table(table)) => Some(table),
        _ => None,
    };
    let mut configured = Vec::with_capacity(installed.len());
    for found in installed {
        let name = found.manifest.name.as_str();
        match table(name) {
            Some(table) if enabled(table, name, report) => configured.push(overridden(found, table, report)),
            Some(_) => {}
            None => configured.push(found.clone()),
        }
    }
    for (name, value) in tables {
        let Value::Table(table) = value else {
            continue;
        };
        if installed.iter().any(|found| found.manifest.name == *name)
            || !table.contains_key("command")
            || !enabled(table, name, report)
        {
            continue;
        }
        match inline(name, table) {
            Ok(found) => configured.push(found),
            Err(err) => report.add(format!("extensions.{name}"), err),
        }
    }
    configured
}

/// Whether `enable` leaves the Extension on: true unless it says false.
fn enabled(table: &Table, name: &str, report: &mut Report) -> bool {
    match table.get("enable") {
        None | Some(Value::Bool(true)) => true,
        Some(Value::Bool(false)) => false,
        Some(_) => {
            report.add(format!("extensions.{name}.enable"), "should be true or false");
            true
        }
    }
}

/// `found` with what its table overrides: `command`, `env` and the Live
/// query's `prefixes`, `triggers` and `debounce_ms`.
fn overridden(found: &Found, table: &Table, report: &mut Report) -> Found {
    let name = &found.manifest.name;
    let mut manifest = found.manifest.clone();
    let mut command_from_config = found.command_from_config;
    if let Some(value) = table.get("command") {
        match command(value) {
            Ok(command) => {
                manifest.command = command;
                command_from_config = true;
            }
            Err(err) => report.add(format!("extensions.{name}.command"), err),
        }
    }
    let env = match table.get("env") {
        Some(value) => env(value).unwrap_or_else(|err| {
            report.add(format!("extensions.{name}.env"), err);
            BTreeMap::new()
        }),
        None => BTreeMap::new(),
    };
    if let Some(value) = table.get("live_query") {
        match live_query(manifest.live_query.take(), value) {
            Ok(live) => manifest.live_query = Some(live),
            Err(err) => {
                report.add(format!("extensions.{name}.live_query"), err);
                manifest.live_query.clone_from(&found.manifest.live_query);
            }
        }
    }
    let mut configured = match Found::new(manifest, found.dir.clone()) {
        Ok(configured) => configured,
        Err(err) => {
            report.add(format!("extensions.{name}.live_query"), Chain(&err).to_string());
            found.clone()
        }
    };
    configured.env = env;
    configured.command_from_config = command_from_config;
    configured
}

/// An Extension defined by its table alone.
fn inline(name: &str, table: &Table) -> Result<Found, String> {
    let mut manifest = toml::Table::new();
    manifest.insert("name".into(), toml::Value::String(name.to_owned()));
    for key in MANIFEST_KEYS
        .iter()
        .chain(&["command", "live_query", "word", "prefix", "in_search"])
    {
        if let Some(value) = table.get(*key) {
            manifest.insert((*key).to_owned(), to_toml(value));
        }
    }
    let manifest = Manifest::deserialize(toml::Value::Table(manifest)).map_err(|err| err.to_string())?;
    let program = PathBuf::from(&manifest.command[0]);
    if !program.is_absolute() && program.components().count() > 1 {
        return Err("`command` needs an absolute path, or a program on PATH".into());
    }
    let env = match table.get("env") {
        Some(value) => env(value)?,
        None => BTreeMap::new(),
    };
    let mut found = Found::new(manifest, PathBuf::new()).map_err(|err| Chain(&err).to_string())?;
    found.env = env;
    found.inline = true;
    found.command_from_config = true;
    Ok(found)
}

fn command(value: &Value) -> Result<Vec<String>, &'static str> {
    const WANTED: &str = "should be a list of the program and its arguments";
    let Value::Array(words) = value else {
        return Err(WANTED);
    };
    let command: Vec<String> = words
        .iter()
        .map(|word| match word {
            Value::String(word) => Some(word.clone()),
            _ => None,
        })
        .collect::<Option<_>>()
        .ok_or(WANTED)?;
    if command.is_empty() {
        return Err(WANTED);
    }
    Ok(command)
}

fn env(value: &Value) -> Result<BTreeMap<String, String>, String> {
    const WANTED: &str = "should be a table of variables and their values";
    let Value::Table(vars) = value else {
        return Err(WANTED.into());
    };
    vars.iter()
        .map(|(var, value)| match value {
            Value::String(value) => Ok((var.clone(), value.clone())),
            _ => Err(format!("`{var}` should be a string")),
        })
        .collect()
}

/// `live` with what `value`, a `live_query` table, overrides of it; one
/// with no Live queries gets them.
fn live_query(live: Option<LiveQueries>, value: &Value) -> Result<LiveQueries, String> {
    let Value::Table(table) = value else {
        return Err("should be a table".into());
    };
    let mut live = live.unwrap_or(LiveQueries {
        prefixes: Vec::new(),
        triggers: Vec::new(),
        debounce_ms: 0,
        fallback: false,
    });
    let texts = |value: &Value| match value {
        Value::Array(texts) => texts
            .iter()
            .map(|text| match text {
                Value::String(text) => Some(text.clone()),
                _ => None,
            })
            .collect::<Option<Vec<String>>>(),
        _ => None,
    };
    for (key, value) in table {
        match key.as_str() {
            "prefixes" => live.prefixes = texts(value).ok_or("`prefixes` should be a list of strings")?,
            "triggers" => live.triggers = texts(value).ok_or("`triggers` should be a list of strings")?,
            "debounce_ms" => {
                live.debounce_ms = match value {
                    Value::Integer(ms) => u32::try_from(*ms).ok(),
                    _ => None,
                }
                .ok_or("`debounce_ms` should be a number of milliseconds")?;
            }
            _ => {
                return Err(format!(
                    "`{key}` can't be overridden; only `prefixes`, `triggers` and `debounce_ms`"
                ));
            }
        }
    }
    for trigger in &live.triggers {
        Regex::new(trigger).map_err(|err| format!("trigger `{trigger}`: {err}"))?;
    }
    Ok(live)
}

#[cfg(test)]
mod tests;
