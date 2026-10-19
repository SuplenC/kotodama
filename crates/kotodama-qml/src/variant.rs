//! The open View, the jobs and a Result's Action panel as the plain maps and
//! lists QML reads them as. They are small and change only on a View, job or
//! Ctrl+K, so unlike Results they may pass through JavaScript.

use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};
use kotodama_protocol::{FormInput, Job, JobState, Value, ViewContent};

use crate::rows::Row;
use crate::views::Views;

pub type Map = QMap<QMapPair_QString_QVariant>;
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

/// The jobs still running, oldest first.
pub fn jobs(views: &Views) -> List {
    list(
        views
            .running()
            .map(|(extension, running)| job(extension, running)),
    )
}

/// The top View, or an empty map at the search. `kind` is `list`, `form`,
/// `detail` or `progress`; the other keys depend on it:
///
/// - a form has `fields`, each `{label, kind, placeholder, secret, options,
///   value}` with `kind` `text`, `checkbox` or `select`, and `submit`;
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
        ViewContent::List { .. } => put("kind", text("list")),
        ViewContent::Form { fields, submit } => {
            put("kind", text("form"));
            put("submit", text(submit));
            let fields = fields.iter().zip(&open.values).map(|(field, value)| {
                let value = match value {
                    Value::Bool(checked) => QVariant::from(checked),
                    Value::String(value) => text(value),
                    _ => QVariant::default(),
                };
                let (kind, placeholder, secret, options) = match &field.input {
                    FormInput::Text {
                        placeholder,
                        secret,
                        ..
                    } => ("text", placeholder.as_deref(), *secret, List::default()),
                    FormInput::Checkbox { .. } => ("checkbox", None, false, List::default()),
                    FormInput::Select { options, .. } => (
                        "select",
                        None,
                        false,
                        list(options.iter().map(|option| {
                            map([
                                ("value", text(&option.value)),
                                ("label", text(&option.label)),
                            ])
                        })),
                    ),
                };
                map([
                    ("id", text(&field.id)),
                    ("label", text(&field.label)),
                    ("kind", text(kind)),
                    ("placeholder", text(placeholder.unwrap_or_default())),
                    ("secret", QVariant::from(&secret)),
                    ("options", QVariant::from(&options)),
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
            (
                "shortcut",
                text(entry.action.shortcut.as_deref().unwrap_or_default()),
            ),
        ])
    }))
}
