//! The open View, the jobs and a Result's Action panel as the plain maps and
//! lists QML reads them as. They are small and change only on a View, job or
//! Ctrl+K, so unlike Results they may pass through JavaScript.
//!
//! # Owns
//!
//! - The keys of each map QML reads: [`view`], [`jobs`], [`actions`], [`examples`] and
//!   [`completions`], and the list [`field_errors`].
//!
//! # Not here
//!
//! - The Views and jobs themselves: `views`; the Action panel: `rows`.
//! - The Results, which reach QML through the model's roles: `model`.

#![forbid(unsafe_code)]

use cxx_qt_lib::QList;
use cxx_qt_lib::QMap;
use cxx_qt_lib::QMapPair_QString_QVariant;
use cxx_qt_lib::QString;
use cxx_qt_lib::QVariant;
use kotodama_protocol::Example;
use kotodama_protocol::FormField;
use kotodama_protocol::FormInput;
use kotodama_protocol::Job;
use kotodama_protocol::JobState;
use kotodama_protocol::SelectOption;
use kotodama_protocol::Value;
use kotodama_protocol::ViewContent;

use crate::rows::Row;
use crate::views::Views;

/// A JavaScript object, as QML reads a `QVariantMap`.
pub type Map = QMap<QMapPair_QString_QVariant>;
/// A JavaScript array, as QML reads a `QVariantList`.
pub type List = QList<QVariant>;

fn text(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

fn map<const N: usize>(entries: [(&str, QVariant); N]) -> Map {
    let mut map = Map::default();
    for (key, value) in entries {
        map.insert_clone(&QString::from(key), &value);
    }
    map
}

fn list(items: impl IntoIterator<Item = Map>) -> List {
    let mut list = List::default();
    for item in items {
        list.append(QVariant::from(&item));
    }
    list
}

/// `{extension, id, title, state, progress, message}`: `state` is `running`,
/// `succeeded` or `failed`, and `progress` runs from 0 to 1, or is -1 when the
/// Extension can't tell.
fn job(extension: &str, job: &Job) -> Map {
    let state = match job.state {
        JobState::Running => "running",
        JobState::Succeeded => "succeeded",
        JobState::Failed => "failed",
    };
    map([
        ("extension", text(extension)),
        ("id", text(&job.id)),
        ("title", text(&job.title)),
        ("state", text(state)),
        (
            "progress",
            QVariant::from(&job.progress.map_or(-1.0, |p| f64::from(p.get()))),
        ),
        ("message", text(job.message.as_deref().unwrap_or_default())),
    ])
}

/// An intro card's examples, each `{query, result}`.
pub fn examples(examples: &[Example]) -> List {
    list(
        examples
            .iter()
            .map(|example| map([("query", text(&example.query)), ("result", text(&example.result))])),
    )
}

/// The paths offered to complete the file field being completed, best first,
/// each `{path, title, folder, modified, highlights}`: `modified` in seconds
/// since the Unix epoch, or -1 when unknown, and `highlights` the positions of
/// the title's characters that matched. Empty when no field of the top form
/// is being completed.
pub fn completions(views: &Views) -> List {
    let Some((_, entries)) = views.completions() else {
        return List::default();
    };
    list(entries.iter().map(|entry| {
        let mut highlights = List::default();
        for &at in &entry.highlights {
            highlights.append(QVariant::from(&i64::from(at)));
        }
        map([
            ("path", text(&entry.path.to_string_lossy())),
            ("title", text(&entry.title)),
            ("folder", QVariant::from(&entry.folder)),
            (
                "modified",
                QVariant::from(&entry.modified.and_then(|at| i64::try_from(at).ok()).unwrap_or(-1)),
            ),
            ("highlights", QVariant::from(&highlights)),
        ])
    }))
}

/// The jobs still running, oldest first.
pub fn jobs(views: &Views) -> List {
    list(views.running().map(|(extension, running)| job(extension, running)))
}

/// A form field's value as QML reads it: a boolean, a string, a number, or
/// a list of strings.
fn form_value(value: &Value) -> QVariant {
    match value {
        Value::Bool(checked) => QVariant::from(checked),
        Value::String(value) => text(value),
        Value::Integer(n) => QVariant::from(n),
        Value::Float(n) => QVariant::from(n),
        Value::Array(values) => QVariant::from(&variants(values.iter().map(form_value))),
        Value::Table(_) => QVariant::default(),
    }
}

/// `values` as a JavaScript array.
fn variants(values: impl IntoIterator<Item = QVariant>) -> List {
    let mut out = List::default();
    for value in values {
        out.append(value);
    }
    out
}

/// What is wrong with each field of the top form, in field order, as the
/// Daemon said when it was submitted: a string per field, empty for a field
/// that is right. Empty when the top View isn't a form.
pub fn field_errors(views: &Views) -> List {
    variants(views.errors().iter().map(|message| text(message)))
}

/// A form field as QML reads it; [`view`] lists the keys.
fn form_field(field: &FormField, value: &Value) -> Map {
    let value = form_value(value);
    let mut accepted = List::default();
    let (mut min, mut max, mut step, mut time, mut said) = (None, None, None, false, "");
    let select = |options: &[SelectOption]| {
        list(
            options
                .iter()
                .map(|option| map([("value", text(&option.value)), ("label", text(&option.label))])),
        )
    };
    let (kind, placeholder, secret, options) = match &field.input {
        FormInput::Text {
            placeholder, secret, ..
        } => ("text", placeholder.as_deref(), *secret, List::default()),
        FormInput::Checkbox { .. } => ("checkbox", None, false, List::default()),
        FormInput::Select { options, .. } => ("select", None, false, select(options)),
        FormInput::MultiSelect { options, .. } => ("multiselect", None, false, select(options)),
        FormInput::TextArea { placeholder, .. } => ("textarea", placeholder.as_deref(), false, List::default()),
        FormInput::Number {
            min: least,
            max: most,
            step: by,
            ..
        } => {
            (min, max, step) = (*least, *most, *by);
            ("number", None, false, List::default())
        }
        FormInput::Date { time: with_time, .. } => {
            time = *with_time;
            ("date", None, false, List::default())
        }
        FormInput::Description { text } => {
            said = text;
            ("description", None, false, List::default())
        }
        FormInput::File {
            accept,
            folder,
            placeholder,
            ..
        } => {
            for kind in accept {
                accepted.append(text(kind));
            }
            let kind = if *folder { "folder" } else { "file" };
            (kind, placeholder.as_deref(), false, List::default())
        }
    };
    let number = |n: Option<f64>| n.map_or_else(QVariant::default, |n| QVariant::from(&n));
    map([
        ("id", text(&field.id)),
        ("label", text(&field.label)),
        ("kind", text(kind)),
        ("optional", QVariant::from(&field.optional)),
        ("pattern", text(field.pattern.as_deref().unwrap_or_default())),
        ("min", number(min)),
        ("max", number(max)),
        ("step", number(step)),
        ("time", QVariant::from(&time)),
        ("text", text(said)),
        ("placeholder", text(placeholder.unwrap_or_default())),
        ("secret", QVariant::from(&secret)),
        ("options", QVariant::from(&options)),
        ("accept", QVariant::from(&accepted)),
        ("value", value),
    ])
}

/// The top View, or an empty map at the search. `kind` is `list`, `form`,
/// `detail` or `progress`; the other keys depend on it:
///
/// - a list has `filter`, what is typed to filter it;
/// - a form has `fields`, each `{id, label, kind, optional, pattern, placeholder, secret, options,
///   accept, min, max, step, time, text, value}`, and `submit`. `optional` is false for a required
///   field and `pattern` empty without one. `kind` is one of:
///   - `text`, a line of text, `secret` hiding it, and `textarea`, several lines; `value` a string;
///   - `checkbox`, `value` a boolean;
///   - `select`, one of `options`, each `{value, label}`, and `multiselect`, any number of them,
///     `value` the list of chosen `value`s;
///   - `file` or `folder`, a path, `accept` listing the file extensions or MIME types a `file`
///     takes, none for any;
///   - `number`, `min`, `max` and `step` each a number or `undefined` without one, `value` a
///     number, or the text typed when it isn't one, or empty;
///   - `date`, `time` true when it takes a time of day too, `value` an ISO 8601 string;
///   - `description`, `text` to read, with no value;
/// - a detail has `markdown` and `metadata`, each `{label, value}`;
/// - a progress View has `job`, as [`jobs`] lists them.
pub fn view(views: &Views) -> Map {
    let Some(open) = views.top() else {
        return Map::default();
    };
    let mut out = map([
        ("extension", text(&open.extension)),
        ("id", text(&open.view.id)),
        ("title", text(&open.view.title)),
    ]);
    let mut put = |key: &str, value: QVariant| out.insert_clone(&QString::from(key), &value);
    match &open.view.content {
        ViewContent::List { .. } => {
            put("kind", text("list"));
            put("filter", text(&open.filter));
        }
        ViewContent::Form { fields, submit } => {
            put("kind", text("form"));
            put("submit", text(submit));
            let fields = fields
                .iter()
                .zip(&open.values)
                .map(|(field, value)| form_field(field, value));
            put("fields", QVariant::from(&list(fields)));
        }
        ViewContent::Detail { markdown, metadata } => {
            put("kind", text("detail"));
            put("markdown", text(markdown));
            let metadata = metadata
                .iter()
                .map(|entry| map([("label", text(&entry.label)), ("value", text(&entry.value))]));
            put("metadata", QVariant::from(&list(metadata)));
        }
        ViewContent::Progress { job: id } => {
            put("kind", text("progress"));
            // Until the Extension reports on it, the job is just starting.
            let followed = views.job(&open.extension, id).cloned().unwrap_or(Job {
                id: id.clone(),
                title: open.view.title.clone(),
                state: JobState::Running,
                progress: None,
                message: None,
            });
            put("job", QVariant::from(&job(&open.extension, &followed)));
        }
    }
    out
}

/// A Result's Action panel: `{id, title, group, shortcut, role, keywords}`
/// for each Action, in order. `group` is the Extension offering a
/// Contributed action, empty for the Item's own; `role` is `normal`,
/// `destructive` or `form`; `keywords` are the verbs the Action is known by.
pub fn actions(row: &Row) -> List {
    list(row.actions().map(|entry| {
        map([
            ("id", text(&entry.action.id)),
            ("title", text(&entry.action.title)),
            ("group", text(entry.group.unwrap_or_default())),
            ("shortcut", text(entry.shortcut.unwrap_or_default())),
            ("role", text(entry.action.role.as_str())),
            ("keywords", QVariant::from(&strings(&entry.action.keywords))),
        ])
    }))
}

fn strings(words: &[String]) -> List {
    let mut list = List::default();
    for word in words {
        list.append(text(word));
    }
    list
}
