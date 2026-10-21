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
            report.add(format!("extensions.{name}.live_query"), err);
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
    let mut found = Found::new(manifest, PathBuf::new())?;
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
mod tests {
    use super::*;

    fn installed(toml: &str) -> Found {
        Found::new(Manifest::from_toml(toml).unwrap(), PathBuf::from("/ext/calc")).unwrap()
    }

    fn configure_with(installed: &[Found], toml: &str) -> (Vec<Found>, Vec<String>) {
        let config = crate::config::parse(toml).unwrap();
        let mut report = Report::new();
        let configured = configure(installed, &config, &mut report);
        let abouts = report
            .into_problems()
            .into_iter()
            .map(|problem| problem.about)
            .collect();
        (configured, abouts)
    }

    fn calculator() -> Found {
        installed(
            "name = \"calculator\"\ncommand = [\"bin/calc\"]\n[live_query]\ntriggers = ['^\\d']\ndebounce_ms = 60\n",
        )
    }

    #[test]
    fn a_bare_program_is_looked_up_on_path_only_when_the_config_names_it() {
        let shell = installed("name = \"calculator\"\ncommand = [\"sh\"]\n");
        assert_eq!(shell.program(), PathBuf::from("/ext/calc/sh"));

        let (configured, _) = configure_with(
            &[shell],
            "[extensions.calculator]\ncommand = [\"sh\", \"-c\", \"true\"]\n",
        );

        let program = configured[0].program();
        assert!(program.is_absolute(), "{}", program.display());
        assert!(program.is_file(), "{}", program.display());
    }

    #[test]
    fn an_extension_without_a_table_runs_as_installed() {
        let (configured, problems) = configure_with(&[calculator()], "");

        assert_eq!(configured[0].manifest, calculator().manifest);
        assert!(problems.is_empty());
    }

    #[test]
    fn enable_false_switches_an_installed_extension_off() {
        let (configured, problems) = configure_with(&[calculator()], "[extensions.calculator]\nenable = false\n");

        assert!(configured.is_empty());
        assert!(problems.is_empty());
    }

    #[test]
    fn command_env_and_live_queries_override_the_manifest() {
        let (configured, problems) = configure_with(
            &[calculator()],
            "[extensions.calculator]\ncommand = [\"/dev/calc\", \"-v\"]\nenv = { RUST_LOG = \"debug\" }\n\
             live_query = { debounce_ms = 0, prefixes = [\"= \"] }\n",
        );

        assert!(problems.is_empty(), "{problems:?}");
        let calc = &configured[0];
        assert_eq!(calc.manifest.command, ["/dev/calc", "-v"]);
        assert_eq!(calc.env, BTreeMap::from([("RUST_LOG".to_owned(), "debug".to_owned())]));
        let live = calc.manifest.live_query.as_ref().unwrap();
        assert_eq!(
            (live.debounce_ms, live.prefixes.as_slice()),
            (0, ["= ".to_owned()].as_slice())
        );
        assert_eq!(live.triggers, ["^\\d"]);
        assert_eq!(calc.dir, PathBuf::from("/ext/calc"));
        assert!(!calc.inline);
    }

    #[test]
    fn a_bad_override_is_a_problem_and_the_manifest_stands() {
        let (configured, problems) = configure_with(
            &[calculator()],
            "[extensions.calculator]\ncommand = []\nlive_query = { triggers = [\"(\"] }\n",
        );

        assert_eq!(configured[0].manifest, calculator().manifest);
        assert_eq!(
            problems,
            ["extensions.calculator.command", "extensions.calculator.live_query"]
        );
    }

    #[test]
    fn a_table_with_a_command_defines_an_extension_from_its_manifest_keys() {
        let (configured, problems) = configure_with(
            &[calculator()],
            "[extensions.notes]\ncommand = [\"notes\", \"--kotodama\"]\ntitle = \"Notes\"\nword = \"note\"\n\
             env = { NOTES_DIR = \"/tmp\" }\nlive_query = { prefixes = [\"note \"], debounce_ms = 100 }\n\
             folder = \"~/notes\"\n",
        );

        assert!(problems.is_empty(), "{problems:?}");
        let notes = &configured[1];
        assert!(notes.inline);
        assert_eq!(notes.manifest.name, "notes");
        assert_eq!(notes.manifest.command, ["notes", "--kotodama"]);
        assert_eq!(notes.manifest.title.as_deref(), Some("Notes"));
        assert_eq!(notes.manifest.word.as_deref(), Some("note"));
        assert_eq!(notes.manifest.live_query.as_ref().unwrap().prefixes, ["note "]);
        assert_eq!(notes.env["NOTES_DIR"], "/tmp");
    }

    #[test]
    fn a_table_without_a_command_defines_nothing() {
        let (configured, problems) = configure_with(&[], "[extensions.emoji]\nword = \"e\"\n");

        assert!(configured.is_empty());
        assert!(problems.is_empty());
    }

    #[test]
    fn an_extension_defined_badly_is_a_problem_and_isnt_run() {
        let (configured, problems) = configure_with(
            &[],
            "[extensions.notes]\ncommand = [\"bin/notes\"]\n[extensions.Bad]\ncommand = [\"x\"]\n",
        );

        assert!(configured.is_empty());
        assert_eq!(problems, ["extensions.Bad", "extensions.notes"]);
    }
}
