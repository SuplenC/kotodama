//! Arguments: what is typed after an Item's keyword, as `1.23` in `go 1.23`,
//! goes to the Action that runs.

use std::collections::BTreeMap;

use kotodama_protocol::Argument;
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
        .find(|argument| !argument.optional && given.get(&argument.name).is_none_or(|value| value.is_empty()))
}

/// The form that asks for `wanted`, with what `given` has already filled in.
pub fn form(id: String, title: &str, wanted: &[Argument], given: &BTreeMap<String, String>) -> View {
    View {
        id,
        title: title.to_owned(),
        content: ViewContent::Form {
            fields: wanted
                .iter()
                .map(|argument| FormField {
                    id: argument.name.clone(),
                    label: argument.name.clone(),
                    input: FormInput::Text {
                        placeholder: argument.placeholder.clone(),
                        default: given.get(&argument.name).cloned(),
                        secret: false,
                    },
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
