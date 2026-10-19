//! The stack of Views on top of the search, the values typed into a form, and
//! the jobs Extensions report. Nothing here touches Qt, so it is tested
//! without it.

use kotodama_protocol::{FormInput, FromFrontEnd, Job, JobState, Table, Value, View, ViewContent};

/// One open View.
#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    /// The Extension that pushed it; `kotodama` for the Daemon's own.
    pub extension: String,
    pub view: View,
    /// A form's values, one per field, in field order. Empty for other Views.
    pub values: Vec<Value>,
}

impl Open {
    fn new(extension: String, view: View) -> Self {
        let values = match &view.content {
            ViewContent::Form { fields, .. } => fields
                .iter()
                .map(|field| match &field.input {
                    FormInput::Text { default, .. } => {
                        Value::String(default.clone().unwrap_or_default())
                    }
                    FormInput::Checkbox { default } => Value::Bool(*default),
                    FormInput::Select { options, default } => Value::String(
                        default
                            .clone()
                            .or_else(|| options.first().map(|option| option.value.clone()))
                            .unwrap_or_default(),
                    ),
                })
                .collect(),
            _ => Vec::new(),
        };
        Open {
            extension,
            view,
            values,
        }
    }

    fn is(&self, extension: &str, id: &str) -> bool {
        self.extension == extension && self.view.id == id
    }

    /// Whether this is a progress View following `extension`'s job `job`.
    pub fn follows(&self, extension: &str, job: &str) -> bool {
        self.extension == extension
            && matches!(&self.view.content, ViewContent::Progress { job: followed } if followed == job)
    }

    fn popped(&self) -> FromFrontEnd {
        FromFrontEnd::ViewPopped {
            extension: self.extension.clone(),
            view: self.view.id.clone(),
        }
    }
}

#[derive(Default)]
pub struct Views {
    /// The top View last.
    stack: Vec<Open>,
    /// By the Extension that runs them, in the order they started.
    jobs: Vec<(String, Job)>,
}

impl Views {
    pub fn top(&self) -> Option<&Open> {
        self.stack.last()
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Puts a View on top. One already open with the same id moves to the top
    /// in its new form.
    pub fn push(&mut self, extension: String, view: View) {
        self.stack.retain(|open| !open.is(&extension, &view.id));
        self.stack.push(Open::new(extension, view));
    }

    /// Replaces the open View with the same id where it is. What was typed
    /// into a field the new form still has is kept. False if no such View is
    /// open.
    pub fn update(&mut self, extension: String, view: View) -> bool {
        let Some(open) = self
            .stack
            .iter_mut()
            .find(|open| open.is(&extension, &view.id))
        else {
            return false;
        };
        let mut new = Open::new(extension, view);
        if let (ViewContent::Form { fields: old, .. }, ViewContent::Form { fields: fresh, .. }) =
            (&open.view.content, &new.view.content)
        {
            for (n, field) in fresh.iter().enumerate() {
                if let Some(at) = old
                    .iter()
                    .position(|old| old.id == field.id && old.input == field.input)
                {
                    new.values[n] = open.values[at].clone();
                }
            }
        }
        *open = new;
        true
    }

    /// The Extension popped one of its Views.
    pub fn pop(&mut self, extension: &str, id: &str) {
        self.stack.retain(|open| !open.is(extension, id));
        self.forget_finished();
    }

    /// The user popped the top View, as Esc does: what to tell the Daemon.
    pub fn pop_top(&mut self) -> Option<FromFrontEnd> {
        let open = self.stack.pop()?;
        self.forget_finished();
        Some(open.popped())
    }

    /// Sets the value of the top form's `field`th field. False if there is no
    /// such field or the value doesn't suit it: a string for a text field or
    /// one of a select's option values, a boolean for a checkbox.
    pub fn set_value(&mut self, field: usize, value: Value) -> bool {
        let Some(open) = self.stack.last_mut() else {
            return false;
        };
        let ViewContent::Form { fields, .. } = &open.view.content else {
            return false;
        };
        let Some(input) = fields.get(field).map(|field| &field.input) else {
            return false;
        };
        let fits = match (input, &value) {
            (FormInput::Text { .. }, Value::String(_)) => true,
            (FormInput::Checkbox { .. }, Value::Bool(_)) => true,
            (FormInput::Select { options, .. }, Value::String(chosen)) => {
                options.iter().any(|option| option.value == *chosen)
            }
            _ => false,
        };
        if fits {
            open.values[field] = value;
        }
        fits
    }

    /// Submits the top form: what to send the Daemon.
    pub fn submit(&self) -> Option<FromFrontEnd> {
        let open = self.stack.last()?;
        let ViewContent::Form { fields, .. } = &open.view.content else {
            return None;
        };
        let values: Table = fields
            .iter()
            .zip(&open.values)
            .map(|(field, value)| (field.id.clone(), value.clone()))
            .collect();
        Some(FromFrontEnd::SubmitForm {
            extension: open.extension.clone(),
            view: open.view.id.clone(),
            values,
        })
    }

    /// A job's latest state.
    pub fn progress(&mut self, extension: String, job: Job) {
        match self
            .jobs
            .iter_mut()
            .find(|(by, known)| *by == extension && known.id == job.id)
        {
            Some((_, known)) => *known = job,
            None => self.jobs.push((extension, job)),
        }
        self.forget_finished();
    }

    pub fn job(&self, extension: &str, id: &str) -> Option<&Job> {
        self.jobs
            .iter()
            .find(|(by, job)| by == extension && job.id == id)
            .map(|(_, job)| job)
    }

    /// The jobs still running, oldest first, with the Extension of each.
    pub fn running(&self) -> impl Iterator<Item = (&str, &Job)> {
        self.jobs
            .iter()
            .filter(|(_, job)| job.state == JobState::Running)
            .map(|(by, job)| (by.as_str(), job))
    }

    /// The Daemon has gone, and every job and View with it.
    pub fn reset(&mut self) {
        self.stack.clear();
        self.jobs.clear();
    }

    /// Drops finished jobs no progress View follows.
    fn forget_finished(&mut self) {
        let stack = &self.stack;
        self.jobs.retain(|(by, job)| {
            job.state == JobState::Running || stack.iter().any(|open| open.follows(by, &job.id))
        });
    }
}

#[cfg(test)]
mod tests {
    use kotodama_protocol::{FormField, Fraction, SelectOption};

    use super::*;

    fn detail(id: &str) -> View {
        View {
            id: id.into(),
            title: id.into(),
            content: ViewContent::Detail {
                markdown: String::new(),
                metadata: Vec::new(),
            },
        }
    }

    fn field(id: &str, input: FormInput) -> FormField {
        FormField {
            id: id.into(),
            label: id.into(),
            input,
        }
    }

    fn text(default: Option<&str>) -> FormInput {
        FormInput::Text {
            placeholder: None,
            default: default.map(Into::into),
            secret: false,
        }
    }

    fn form(fields: Vec<FormField>) -> View {
        View {
            id: "form".into(),
            title: "Form".into(),
            content: ViewContent::Form {
                fields,
                submit: "Go".into(),
            },
        }
    }

    fn progress(id: &str, job: &str) -> View {
        View {
            id: id.into(),
            title: id.into(),
            content: ViewContent::Progress { job: job.into() },
        }
    }

    fn job(id: &str, state: JobState) -> Job {
        Job {
            id: id.into(),
            title: id.into(),
            state,
            progress: Some(Fraction::new(0.5).unwrap()),
            message: None,
        }
    }

    fn popped(extension: &str, view: &str) -> FromFrontEnd {
        FromFrontEnd::ViewPopped {
            extension: extension.into(),
            view: view.into(),
        }
    }

    fn ids(views: &Views) -> Vec<&str> {
        views
            .stack
            .iter()
            .map(|open| open.view.id.as_str())
            .collect()
    }

    #[test]
    fn esc_pops_the_top_view_and_tells_the_daemon() {
        let mut views = Views::default();
        views.push("git".into(), detail("a"));
        views.push("git".into(), detail("b"));
        assert_eq!(views.depth(), 2);
        assert_eq!(views.top().unwrap().view.id, "b");

        assert_eq!(views.pop_top(), Some(popped("git", "b")));
        assert_eq!(ids(&views), ["a"]);
        assert_eq!(views.pop_top(), Some(popped("git", "a")));
        assert_eq!(views.pop_top(), None);
    }

    #[test]
    fn an_extension_pops_its_own_view_wherever_it_is_in_the_stack() {
        let mut views = Views::default();
        views.push("git".into(), detail("a"));
        views.push("other".into(), detail("a"));
        views.push("git".into(), detail("b"));
        views.pop("git", "a");
        assert_eq!(
            views
                .stack
                .iter()
                .map(|open| (open.extension.as_str(), open.view.id.as_str()))
                .collect::<Vec<_>>(),
            [("other", "a"), ("git", "b")]
        );
    }

    #[test]
    fn pushing_a_view_that_is_already_open_moves_it_to_the_top() {
        let mut views = Views::default();
        views.push("git".into(), detail("a"));
        views.push("git".into(), detail("b"));
        views.push("git".into(), detail("a"));
        assert_eq!(ids(&views), ["b", "a"]);
    }

    #[test]
    fn a_submitted_form_sends_what_was_typed_and_the_defaults_of_the_rest() {
        let mut views = Views::default();
        views.push(
            "timer".into(),
            form(vec![
                field("label", text(None)),
                field("seconds", text(Some("3"))),
                field("loud", FormInput::Checkbox { default: false }),
                field(
                    "sound",
                    FormInput::Select {
                        options: vec![
                            SelectOption {
                                value: "bell".into(),
                                label: "Bell".into(),
                            },
                            SelectOption {
                                value: "gong".into(),
                                label: "Gong".into(),
                            },
                        ],
                        default: None,
                    },
                ),
            ]),
        );
        assert!(views.set_value(0, Value::String("Tea".into())));
        assert!(views.set_value(2, Value::Bool(true)));
        assert_eq!(
            views.submit(),
            Some(FromFrontEnd::SubmitForm {
                extension: "timer".into(),
                view: "form".into(),
                values: Table::from([
                    ("label".into(), Value::String("Tea".into())),
                    ("seconds".into(), Value::String("3".into())),
                    ("loud".into(), Value::Bool(true)),
                    ("sound".into(), Value::String("bell".into())),
                ]),
            })
        );
    }

    #[test]
    fn a_value_that_does_not_suit_its_field_is_refused() {
        let mut views = Views::default();
        views.push(
            "timer".into(),
            form(vec![
                field("label", text(None)),
                field("loud", FormInput::Checkbox { default: false }),
                field(
                    "sound",
                    FormInput::Select {
                        options: vec![SelectOption {
                            value: "bell".into(),
                            label: "Bell".into(),
                        }],
                        default: None,
                    },
                ),
            ]),
        );
        assert!(!views.set_value(0, Value::Bool(true)));
        assert!(!views.set_value(1, Value::String("yes".into())));
        assert!(!views.set_value(2, Value::String("gong".into())));
        assert!(!views.set_value(3, Value::String("x".into())));
        assert_eq!(views.top().unwrap().values[1], Value::Bool(false));
    }

    #[test]
    fn only_a_form_can_be_submitted() {
        let mut views = Views::default();
        assert_eq!(views.submit(), None);
        views.push("git".into(), detail("a"));
        assert_eq!(views.submit(), None);
    }

    #[test]
    fn an_updated_form_keeps_what_was_typed_into_the_fields_it_still_has() {
        let mut views = Views::default();
        views.push(
            "timer".into(),
            form(vec![
                field("label", text(None)),
                field("seconds", text(None)),
            ]),
        );
        views.set_value(0, Value::String("Tea".into()));
        views.set_value(1, Value::String("3".into()));

        assert!(views.update(
            "timer".into(),
            form(vec![
                field("seconds", text(None)),
                field("note", text(Some("hi")))
            ]),
        ));
        assert_eq!(
            views.top().unwrap().values,
            [Value::String("3".into()), Value::String("hi".into())]
        );
        assert!(!views.update("timer".into(), detail("not-open")));
    }

    #[test]
    fn running_jobs_are_listed_and_a_finished_one_stays_only_while_a_view_follows_it() {
        let mut views = Views::default();
        views.progress("tools".into(), job("a", JobState::Running));
        views.progress("tools".into(), job("b", JobState::Running));
        views.push("tools".into(), progress("p", "b"));
        assert_eq!(
            views
                .running()
                .map(|(_, job)| job.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );

        views.progress("tools".into(), job("a", JobState::Succeeded));
        views.progress("tools".into(), job("b", JobState::Failed));
        assert_eq!(views.running().count(), 0);
        assert_eq!(views.job("tools", "a"), None, "no View follows it");
        assert_eq!(
            views.job("tools", "b").map(|job| job.state),
            Some(JobState::Failed),
            "its progress View still shows how it ended"
        );

        views.pop_top();
        assert_eq!(views.job("tools", "b"), None);
    }
}
