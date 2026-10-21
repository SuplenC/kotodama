//! Reading settings from the Extension's config table, the
//! `[extensions.<name>]` table of the user's config. A setting of the wrong
//! shape is logged and read as missing, so the Extension keeps its default
//! and the other settings still apply.
//!
//! # Owns
//!
//! The shapes a setting can take (a string, a boolean, a list of strings, a
//! command) and the warning for one of the wrong shape.
//!
//! # Not here
//!
//! What any setting means, which is the Extension's own; delivering the
//! table (`ToExtension::Config`, from the Daemon).

use std::fmt::Display;

use kotodama_protocol::Table;
use kotodama_protocol::Value;

/// Logs a problem with the config, as `<name>: config: <message>`.
pub fn warn(message: impl Display) {
    crate::log!("config: {message}");
}

/// `value` as a list of strings, or `None` if it isn't one.
pub fn strings(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Array(values) => values
            .iter()
            .map(|value| match value {
                Value::String(value) => Some(value.clone()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// The command `key` names: a list of strings, the program first.
pub fn command(config: &Table, key: &str) -> Option<Vec<String>> {
    let command = strings(config.get(key)?).filter(|command| !command.is_empty());
    if command.is_none() {
        warn(format_args!("`{key}` should be a list of strings, the program first"));
    }
    command
}

/// The string `key` names.
pub fn string<'a>(config: &'a Table, key: &str) -> Option<&'a str> {
    if let Value::String(value) = config.get(key)? {
        Some(value)
    } else {
        warn(format_args!("`{key}` should be a string"));
        None
    }
}

/// The boolean `key` names.
pub fn boolean(config: &Table, key: &str) -> Option<bool> {
    if let Value::Bool(value) = config.get(key)? {
        Some(*value)
    } else {
        warn(format_args!("`{key}` should be true or false"));
        None
    }
}

#[cfg(test)]
mod tests;
