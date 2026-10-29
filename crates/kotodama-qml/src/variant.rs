//! The open View, the jobs and a Result's Action panel as the plain maps and
//! lists QML reads them as. They are small and change only on a View, job or
//! Ctrl+K, so unlike Results they may pass through JavaScript.
//!
//! # Owns
//!
//! - The keys of each map QML reads: [`view`], [`jobs`], [`actions`], [`examples`] and
//!   [`completions`].
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
use kotodama_protocol::FormInput;
use kotodama_protocol::Job;
use kotodama_protocol::JobState;
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

/// The top View, or an empty map at the search. `kind` is `list`, `form`,
/// `detail` or `progress`; the other keys depend on it:
///
/// - a list has `filter`, what is typed to filter it;
/// - a form has `fields`, each `{label, kind, placeholder, secret, options, accept, value}` with
///   `kind` `text`, `checkbox`, `select`, `file` or `folder` (a path, `accept` listing the file
///   extensions or MIME types a `file` takes, none for any), and `submit`;
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
            let fields = fields.iter().zip(&open.values).map(|(field, value)| {
                let value = match value {
                    Value::Bool(checked) => QVariant::from(checked),
                    Value::String(value) => text(value),
                    _ => QVariant::default(),
                };
                let mut accepted = List::default();
                let (kind, placeholder, secret, options) = match &field.input {
                    FormInput::Text {
                        placeholder, secret, ..
                    } => ("text", placeholder.as_deref(), *secret, List::default()),
                    FormInput::Checkbox { .. } => ("checkbox", None, false, List::default()),
                    FormInput::Select { options, .. } => (
                        "select",
                        None,
                        false,
                        list(
                            options
                                .iter()
                                .map(|option| map([("value", text(&option.value)), ("label", text(&option.label))])),
                        ),
                    ),
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
                map([
                    ("id", text(&field.id)),
                    ("label", text(&field.label)),
                    ("kind", text(kind)),
                    ("placeholder", text(placeholder.unwrap_or_default())),
                    ("secret", QVariant::from(&secret)),
                    ("options", QVariant::from(&options)),
                    ("accept", QVariant::from(&accepted)),
                    ("value", value),
                ])
            });
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

/// A Result's Action panel: `{title, group, shortcut}` for each Action, in
/// order. `group` is the Extension offering a Contributed action, empty for
/// the Item's own.
pub fn actions(row: &Row) -> List {
    list(row.actions().map(|entry| {
        map([
            ("title", text(&entry.action.title)),
            ("group", text(entry.group.unwrap_or_default())),
            ("shortcut", text(entry.shortcut.unwrap_or_default())),
        ])
    }))
}
