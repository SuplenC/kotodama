//! Arguments: what is typed after an Item's keyword, as `1.23` in `go 1.23`,
//! goes to the Action that runs.
//!
//! # Owns
//!
//! - [`fill`]: the Arguments an Action takes, filled from what follows its keyword.
//! - [`missing`], [`form`] and [`submitted`]: the form that asks for the rest, and its answers.
//! - [`invalid`]: a number Argument that isn't one; [`written_numbers`]: one that is, written out
//!   the one way.
//! - [`absolute`]: a file or folder Argument, or a form's file field, as the absolute path it
//!   reaches the Extension as.
//!
//! # Not here
//!
//! - Matching the keyword: `keywords.rs`, and the search in `daemon/search.rs`.
//! - Running the Action: `daemon/picks.rs`.
//! - Checking a submitted form, the argument form too: `validation.rs`.

use std::collections::BTreeMap;
use std::path::Path;

use kotodama_protocol::Argument;
use kotodama_protocol::ArgumentKind;
use kotodama_protocol::FieldError;
use kotodama_protocol::FormField;
use kotodama_protocol::FormInput;
use kotodama_protocol::Table;
use kotodama_protocol::Value;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;

use crate::validation;

/// Fills the Arguments `wanted` that `given` lacks, or has empty, from
/// `typed`, the text after the keyword: one word each, in order, except the
/// last, which takes the rest.
pub fn fill(wanted: &[Argument], typed: &str, given: &mut BTreeMap<String, String>) {
    let mut rest = typed.trim();
    let missing: Vec<&Argument> = wanted
        .iter()
        .filter(|argument| given.get(&argument.name).is_none_or(String::is_empty))
        .collect();
    for (n, argument) in missing.iter().enumerate() {
        if rest.is_empty() {
            break;
        }
        let value = if n + 1 == missing.len() {
            std::mem::take(&mut rest)
        } else {
            let (word, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            rest = after.trim_start();
            word
        };
        given.insert(argument.name.clone(), value.to_owned());
    }
}

/// The first required Argument in `wanted` that `given` has no value for.
pub fn missing<'a>(wanted: &'a [Argument], given: &BTreeMap<String, String>) -> Option<&'a Argument> {
    wanted
        .iter()
        .find(|argument| !argument.optional && given.get(&argument.name).is_none_or(String::is_empty))
}

/// A [`FieldError`] for each number Argument in `wanted` that `given` holds
/// something other than a number for, in `wanted`'s order.
pub fn invalid(wanted: &[Argument], given: &BTreeMap<String, String>) -> Vec<FieldError> {
    wanted
        .iter()
        .filter(|argument| argument.kind == ArgumentKind::Number)
        .filter_map(|argument| {
            let typed = given.get(&argument.name).filter(|typed| !typed.is_empty())?;
            validation::number(&Value::String(typed.clone()))
                .is_none()
                .then(|| FieldError {
                    field: argument.name.clone(),
                    message: format!("“{}” isn't a number", typed.trim()),
                })
        })
        .collect()
}

/// Writes each number Argument in `wanted` that `given` holds as Rust writes
/// the number, `1e3` as `1000` and ` 2.50` as `2.5`, however it was filled
/// in, so an Action gets one form of it. One that isn't a number stays as it
/// is: [`invalid`] names it.
pub fn written_numbers(wanted: &[Argument], given: &mut BTreeMap<String, String>) {
    for argument in wanted.iter().filter(|argument| argument.kind == ArgumentKind::Number) {
        let Some(typed) = given.get_mut(&argument.name) else {
            continue;
        };
        match validation::number_value(&Value::String(typed.clone())) {
            Some(Value::Integer(n)) => *typed = n.to_string(),
            Some(Value::Float(n)) => *typed = n.to_string(),
            _ => {}
        }
    }
}

/// The form that asks for `wanted`, with what `given` has already filled in.
pub fn form(id: String, title: &str, wanted: &[Argument], given: &BTreeMap<String, String>) -> View {
    View {
        id,
        title: title.to_owned(),
        content: ViewContent::Form {
            fields: wanted
                .iter()
                .map(|argument| {
                    let placeholder = argument.placeholder.clone();
                    let default = given.get(&argument.name).cloned();
                    let input = match argument.kind {
                        ArgumentKind::Text => FormInput::Text {
                            placeholder,
                            default,
                            secret: false,
                        },
                        ArgumentKind::File | ArgumentKind::Folder => FormInput::File {
                            accept: argument.accept.clone(),
                            folder: argument.kind == ArgumentKind::Folder,
                            default,
                            placeholder,
                        },
                        ArgumentKind::Number => FormInput::Number {
                            min: None,
                            max: None,
                            step: None,
                            default: default.and_then(|typed| validation::number(&Value::String(typed))),
                        },
                    };
                    let field = FormField::new(argument.name.clone(), argument.name.clone(), input);
                    if argument.optional { field.optional() } else { field }
                })
                .collect(),
            submit: title.to_owned(),
        },
    }
}

/// The Arguments a submitted form holds: its text fields that aren't blank,
/// and its numbers, written out.
pub fn submitted(values: Table) -> BTreeMap<String, String> {
    values
        .into_iter()
        .filter_map(|(name, value)| match value {
            Value::String(value) if !value.trim().is_empty() => Some((name, value)),
            Value::Integer(n) => Some((name, n.to_string())),
            Value::Float(n) => Some((name, n.to_string())),
            _ => None,
        })
        .collect()
}

/// Makes the file and folder Arguments among `given` absolute paths, as
/// [`absolute`] does, however they were filled in: typed after the
/// keyword, in a form, or by the Front end.
pub fn absolute_paths(wanted: &[Argument], given: &mut BTreeMap<String, String>, home: &Path) {
    for argument in wanted
        .iter()
        .filter(|argument| matches!(argument.kind, ArgumentKind::File | ArgumentKind::Folder))
    {
        if let Some(path) = given.get_mut(&argument.name) {
            *path = absolute(path, home);
        }
    }
}

/// Makes the values of a form's file fields in `values` absolute paths, as
/// [`absolute`] does.
pub fn absolute_fields(fields: &[FormField], values: &mut Table, home: &Path) {
    for field in fields
        .iter()
        .filter(|field| matches!(field.input, FormInput::File { .. }))
    {
        if let Some(Value::String(path)) = values.get_mut(&field.id) {
            *path = absolute(path, home);
        }
    }
}

/// `path` as an absolute path: `~` and `~/…` expand to `home`, and a
/// relative path is taken from `home`, the directory a person's paths start
/// from in a file manager and a new terminal; the Daemon's own working
/// directory is wherever systemd started it. Only `~` alone and `~/` expand,
/// so `~bob/x` is a relative path too. A `file://` URI, as a file picker or a
/// drop gives, is its path. Whitespace around the path goes, and an empty
/// path stays empty: it was left out.
pub fn absolute(path: &str, home: &Path) -> String {
    let path = path.trim();
    if let Some(uri) = path.strip_prefix("file://") {
        // A local file's URI has an empty host: `file:///home/…`.
        return percent_decoded(uri.trim_start_matches("localhost"));
    }
    if path.is_empty() || path.starts_with('/') {
        return path.to_owned();
    }
    let rest = match path.strip_prefix('~') {
        Some("") => "",
        Some(rest) if rest.starts_with('/') => rest.trim_start_matches('/'),
        _ => path,
    };
    let joined = if rest.is_empty() {
        home.to_path_buf()
    } else {
        home.join(rest)
    };
    joined.to_string_lossy().into_owned()
}

/// A URI's path with its `%XX` escapes decoded; a `%` not followed by two
/// hex digits stays as it is.
fn percent_decoded(uri: &str) -> String {
    let bytes = uri.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = (bytes[at] == b'%')
            .then(|| bytes.get(at + 1..at + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(byte) = escaped {
            decoded.push(byte);
            at += 3;
        } else {
            decoded.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests;
