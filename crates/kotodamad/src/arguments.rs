//! Arguments: what is typed after an Item's keyword, as `1.23` in `go 1.23`,
//! goes to the Action that runs.
//!
//! # Owns
//!
//! - [`fill`]: the Arguments an Action takes, filled from what follows its keyword.
//! - [`missing`], [`form`] and [`submitted`]: the form that asks for the rest, and its answers.
//! - [`absolute`]: a file or folder Argument, or a form's file field, as the absolute path it
//!   reaches the Extension as.
//!
//! # Not here
//!
//! - Matching the keyword: `keywords.rs`, and the search in `daemon/search.rs`.
//! - Running the Action: `daemon/picks.rs`.

use std::collections::BTreeMap;
use std::path::Path;

use kotodama_protocol::Argument;
use kotodama_protocol::ArgumentKind;
use kotodama_protocol::FormField;
use kotodama_protocol::FormInput;
use kotodama_protocol::Table;
use kotodama_protocol::Value;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;

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
                    };
                    FormField {
                        id: argument.name.clone(),
                        label: argument.name.clone(),
                        input,
                    }
                })
                .collect(),
            submit: title.to_owned(),
        },
    }
}

/// The Arguments a submitted form holds: its text fields that aren't empty.
pub fn submitted(values: Table) -> BTreeMap<String, String> {
    values
        .into_iter()
        .filter_map(|(name, value)| match value {
            Value::String(value) if !value.is_empty() => Some((name, value)),
            _ => None,
        })
        .collect()
}

/// Makes the file and folder Arguments among `given` absolute paths, as
/// [`absolute`] does, however they were filled in: typed after the
/// keyword, in a form, or by the Front end.
pub fn absolute_paths(wanted: &[Argument], given: &mut BTreeMap<String, String>, home: &Path) {
    for argument in wanted.iter().filter(|argument| argument.kind != ArgumentKind::Text) {
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
/// directory is wherever systemd started it. An empty path stays empty: it
/// was left out.
pub fn absolute(path: &str, home: &Path) -> String {
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

#[cfg(test)]
mod tests;
