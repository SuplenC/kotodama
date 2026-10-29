//! A submitted form checked against its fields before the Extension gets it,
//! so an Extension never sees a required field left blank, a number out of
//! range or a date that isn't one. Runs on submit only, never per keystroke.
//!
//! # Owns
//!
//! - [`check`]: each field's value against its input and its `optional` and `pattern`, in words for
//!   the user, and the values put in the form the Extension gets them.
//! - [`number`]: what counts as a number, for a number field and a number Argument.
//! - [`drop_bad_patterns`]: a pattern that isn't a regular expression, dropped as the View arrives.
//!
//! # Not here
//!
//! - Sending the errors to the Front end, and the form's submit: `daemon/front_ends.rs`.
//! - A number Argument typed after a keyword: `arguments.rs:invalid`.

use kotodama_protocol::FieldError;
use kotodama_protocol::FormField;
use kotodama_protocol::FormInput;
use kotodama_protocol::SelectOption;
use kotodama_protocol::Table;
use kotodama_protocol::Value;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;
use regex::Regex;

const REQUIRED: &str = "Required";
const NOT_A_NUMBER: &str = "Not a number";
const NO_PATTERN_MATCH: &str = "Not in the form this field takes";

/// The largest integer an `f64` holds exactly, 2^53: a whole number past it
/// stays a float.
const EXACT_INTEGERS: f64 = 9_007_199_254_740_992.0;

/// How far from a whole number of steps a value may be and still be on a
/// step, relative to the number of steps: `0.1 * 3` is
/// `0.30000000000000004`, and the error grows with the value.
const STEP_SLACK: f64 = 1e-9;

/// Checks `values`, a submitted form's values by field id, against `fields`,
/// and returns what is wrong, a [`FieldError`] per field in the form's order;
/// none when the form may go to its Extension. On the way it puts each value
/// in the form the Extension gets it: a number field's as an integer when it
/// is whole and a float otherwise, typed as text or not, and an optional one
/// left blank left out; a description's dropped. A value for an id no field
/// has is left as it is.
pub fn check(fields: &[FormField], values: &mut Table) -> Vec<FieldError> {
    let mut errors = Vec::new();
    for field in fields {
        if let FormInput::Description { .. } = field.input {
            values.remove(&field.id);
            continue;
        }
        let blank = values.get(&field.id).is_none_or(is_blank);
        let checked = if blank {
            if matches!(field.input, FormInput::Number { .. }) {
                values.remove(&field.id);
            }
            missing(field)
        } else {
            values
                .get_mut(&field.id)
                .map_or(Ok(()), |value| check_value(field, value))
        };
        if let Err(message) = checked {
            errors.push(FieldError {
                field: field.id.clone(),
                message,
            });
        }
    }
    errors
}

/// Whether `value` counts as left blank: whitespace, or no option chosen.
fn is_blank(value: &Value) -> bool {
    match value {
        Value::String(text) => text.trim().is_empty(),
        Value::Array(values) => values.is_empty(),
        _ => false,
    }
}

/// Whether `field` may be left blank. A checkbox may always: unticked is a
/// value.
fn missing(field: &FormField) -> Result<(), String> {
    if field.optional || matches!(field.input, FormInput::Checkbox { .. }) {
        Ok(())
    } else {
        Err(REQUIRED.to_owned())
    }
}

/// Checks a value that isn't blank, and puts a number in the form the
/// Extension gets it.
fn check_value(field: &FormField, value: &mut Value) -> Result<(), String> {
    match &field.input {
        FormInput::Text { .. } | FormInput::TextArea { .. } => {
            let text = as_text(value)?;
            match &field.pattern {
                Some(pattern) if !matches_whole(pattern, text) => Err(NO_PATTERN_MATCH.to_owned()),
                _ => Ok(()),
            }
        }
        FormInput::File { .. } => as_text(value).map(|_| ()),
        FormInput::Checkbox { .. } => match value {
            Value::Bool(_) => Ok(()),
            _ => Err("Expects true or false".to_owned()),
        },
        FormInput::Select { options, .. } => among(options, as_text(value)?),
        FormInput::MultiSelect { options, .. } => {
            let Value::Array(chosen) = value else {
                return Err("Expects a list of options".to_owned());
            };
            chosen.iter().try_for_each(|chosen| match chosen {
                Value::String(chosen) => among(options, chosen),
                _ => Err("Expects a list of options".to_owned()),
            })
        }
        FormInput::Number { min, max, step, .. } => {
            let Some(n) = number(value) else {
                return Err(NOT_A_NUMBER.to_owned());
            };
            in_range(n, *min, *max, *step)?;
            *value = whole_or_float(n);
            Ok(())
        }
        FormInput::Date { time, .. } => {
            let text = as_text(value)?.trim();
            match (time, is_date(text, *time)) {
                (_, true) => {
                    *value = Value::String(text.to_owned());
                    Ok(())
                }
                (false, false) => Err("Not a date: write it as YYYY-MM-DD".to_owned()),
                (true, false) => Err("Not a date and time: write it as YYYY-MM-DDTHH:MM".to_owned()),
            }
        }
        FormInput::Description { .. } => Ok(()),
    }
}

fn as_text(value: &Value) -> Result<&str, String> {
    match value {
        Value::String(text) => Ok(text),
        _ => Err("Expects text".to_owned()),
    }
}

fn among(options: &[SelectOption], chosen: &str) -> Result<(), String> {
    if options.iter().any(|option| option.value == chosen) {
        Ok(())
    } else {
        Err(format!("“{chosen}” isn't one of the options"))
    }
}

/// Whether the whole of `text` matches `pattern`.
fn matches_whole(pattern: &str, text: &str) -> bool {
    // HAZARD: a pattern that doesn't compile matches nothing. Matching
    // anything would let a form through unchecked; `drop_bad_patterns` takes
    // such a pattern out of an Extension's View as it arrives.
    Regex::new(&anchored(pattern)).is_ok_and(|whole| whole.is_match(text))
}

/// `pattern` made to match a whole text, as HTML's `pattern` does. A
/// pattern that compiles alone may not compile wrapped, as `(?x)a # note`,
/// whose comment swallows the closing `)`, so this is what is compiled
/// whenever a pattern is checked.
pub fn anchored(pattern: &str) -> String {
    format!("^(?:{pattern})$")
}

/// `value` as a finite number: an integer, a float, or text Rust's `f64`
/// reads (`25`, ` 2.5 `, `1e3`), whitespace around it aside.
pub fn number(value: &Value) -> Option<f64> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "an integer past 2^53 is as near as a float field gets to it"
    )]
    let n = match value {
        Value::Integer(n) => *n as f64,
        Value::Float(n) => *n,
        Value::String(text) => text.trim().parse().ok()?,
        _ => return None,
    };
    n.is_finite().then_some(n)
}

/// Whether `n` is at least `min`, at most `max`, and a whole number of
/// `step`s from `min`, or from 0 without one.
fn in_range(n: f64, min: Option<f64>, max: Option<f64>, step: Option<f64>) -> Result<(), String> {
    if let Some(min) = min
        && n < min
    {
        return Err(format!("At least {min}"));
    }
    if let Some(max) = max
        && n > max
    {
        return Err(format!("At most {max}"));
    }
    if let Some(step) = step.filter(|step| step.is_finite() && *step > 0.0) {
        let from = min.unwrap_or(0.0);
        let steps = (n - from) / step;
        if (steps - steps.round()).abs() > STEP_SLACK * steps.abs().max(1.0) {
            return Err(if from == 0.0 {
                format!("In steps of {step}")
            } else {
                format!("In steps of {step} from {from}")
            });
        }
    }
    Ok(())
}

/// `value` as a number Value, when it is a number: [`whole_or_float`].
pub fn number_value(value: &Value) -> Option<Value> {
    number(value).map(whole_or_float)
}

/// `n` as the Extension gets it: an integer when it is whole, a float
/// otherwise.
fn whole_or_float(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < EXACT_INTEGERS {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a whole number below 2^53 is an i64 exactly"
        )]
        Value::Integer(n as i64)
    } else {
        Value::Float(n)
    }
}

/// Whether `text` is an ISO 8601 date, `2026-10-30`, or with `time` a local
/// date and time, `2026-10-30T09:30` or `2026-10-30T09:30:15`, with no
/// offset.
fn is_date(text: &str, time: bool) -> bool {
    match (text.split_once('T'), time) {
        (None, false) => is_day(text),
        (Some((day, clock)), true) => is_day(day) && is_clock(clock),
        _ => false,
    }
}

fn is_day(text: &str) -> bool {
    let b = text.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let (Some(year), Some(month), Some(day)) = (digits(&b[..4]), digits(&b[5..7]), digits(&b[8..])) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

fn is_clock(text: &str) -> bool {
    let b = text.as_bytes();
    let seconds = match b.len() {
        5 => Some(0),
        8 if b[5] == b':' => digits(&b[6..]),
        _ => return false,
    };
    b[2] == b':'
        && matches!(
            (digits(&b[..2]), digits(&b[3..5]), seconds),
            (Some(0..24), Some(0..60), Some(0..60))
        )
}

/// The number `bytes` write in decimal digits, and nothing else.
fn digits(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0, |n, &byte| {
        byte.is_ascii_digit().then(|| n * 10 + u32::from(byte - b'0'))
    })
}

/// Drops the pattern of each of a form View's fields that isn't a regular
/// expression, so the field takes any text, and says which, in words for
/// the Extension's author.
pub fn drop_bad_patterns(view: &mut View) -> Vec<String> {
    let ViewContent::Form { fields, .. } = &mut view.content else {
        return Vec::new();
    };
    let mut dropped = Vec::new();
    for field in fields {
        let Some(pattern) = &field.pattern else {
            continue;
        };
        if let Err(error) = Regex::new(&anchored(pattern)) {
            dropped.push(format!(
                "form field `{}`'s pattern `{pattern}` isn't a regular expression, so the field takes any text: \
                 {error}",
                field.id
            ));
            field.pattern = None;
        }
    }
    dropped
}

#[cfg(test)]
mod tests;
