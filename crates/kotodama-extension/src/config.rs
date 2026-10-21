//! Reading settings from the Extension's config table, the
//! `[extensions.<name>]` table of the user's config. A setting of the wrong
//! shape is logged and read as missing, so the Extension keeps its default
//! and the other settings still apply.

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
    let value = config.get(key)?;
    match strings(value) {
        Some(command) if !command.is_empty() => Some(command),
        _ => {
            warn(format_args!("`{key}` should be a list of strings, the program first"));
            None
        }
    }
}

/// The string `key` names.
pub fn string<'a>(config: &'a Table, key: &str) -> Option<&'a str> {
    match config.get(key)? {
        Value::String(value) => Some(value),
        _ => {
            warn(format_args!("`{key}` should be a string"));
            None
        }
    }
}

/// The boolean `key` names.
pub fn boolean(config: &Table, key: &str) -> Option<bool> {
    match config.get(key)? {
        Value::Bool(value) => Some(*value),
        _ => {
            warn(format_args!("`{key}` should be true or false"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(pairs: &[(&str, Value)]) -> Table {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    fn string_value(text: &str) -> Value {
        Value::String(text.into())
    }

    #[test]
    fn a_command_is_a_list_of_strings_that_isnt_empty() {
        let config = table(&[
            ("good", Value::Array(vec![string_value("foot"), string_value("-e")])),
            ("empty", Value::Array(Vec::new())),
            ("mixed", Value::Array(vec![string_value("foot"), Value::Integer(1)])),
            ("word", string_value("foot")),
        ]);
        assert_eq!(command(&config, "good"), Some(vec!["foot".into(), "-e".into()]));
        assert_eq!(command(&config, "empty"), None);
        assert_eq!(command(&config, "mixed"), None);
        assert_eq!(command(&config, "word"), None);
        assert_eq!(command(&config, "missing"), None);
    }

    #[test]
    fn a_setting_of_the_wrong_shape_reads_as_missing() {
        let config = table(&[
            ("url", string_value("https://example.com")),
            ("on", Value::Bool(true)),
            ("number", Value::Integer(3)),
        ]);
        assert_eq!(string(&config, "url"), Some("https://example.com"));
        assert_eq!(string(&config, "number"), None);
        assert_eq!(boolean(&config, "on"), Some(true));
        assert_eq!(boolean(&config, "url"), None);
        assert_eq!(boolean(&config, "missing"), None);
    }
}
