//! The stack of Views on top of the search, the values typed into a form, and
//! the jobs Extensions report. Nothing here touches Qt, so it is tested
//! without it.
//!
//! # Owns
//!
//! - [`Views`]: the open Views, top last, and the jobs.
//! - A list View's filter, the Daemon's matches for it, and the form of the View held back until
//!   they come.
//! - A list View's rows as [`rows::diff`](crate::rows::diff) takes them ([`Listed`]), and its
//!   Contributed actions by Item kind.
//! - The file field being completed, and the Daemon's paths for it.
//! - The Daemon's errors for the fields of a form submitted.
//!
//! # Not here
//!
//! - The rows themselves and the diff: `rows`.
//! - The Views and jobs as QML reads them: `variant`.
//! - Showing a change: `updates`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use kotodama_protocol::FieldError;
use kotodama_protocol::FormInput;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::Item;
use kotodama_protocol::Job;
use kotodama_protocol::JobState;
use kotodama_protocol::ListMatch;
use kotodama_protocol::OfferedAction;
use kotodama_protocol::PathCompletion;
use kotodama_protocol::SelectOption;
use kotodama_protocol::Table;
use kotodama_protocol::Value;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;

use crate::rows::Fresh;
use crate::rows::Row;

/// The Actions other Extensions contribute to the Items of a list View, by
/// Item kind, as the Daemon sends them.
pub type Contributed = BTreeMap<String, Vec<OfferedAction>>;

/// [`Contributed`] as the rows hold it: each kind's list shared by every row.
type Shared = BTreeMap<String, Arc<[OfferedAction]>>;

/// What a change to the Views needs the menu to show again: the least
/// first, so a batch of changes shows the most any of them needs, once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Shown {
    /// Nothing more than the change itself showed.
    Same,
    /// The rows of the list View on top.
    Rows,
    /// The View stack, and so the rows too.
    View,
}

/// One open View.
#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    /// The Extension that pushed it; `kotodama` for the Daemon's own.
    pub extension: String,
    pub view: View,
    /// A form's values, one per field, in field order. Empty for other Views.
    pub values: Vec<Value>,
    /// What is wrong with each of a form's fields, in field order, empty
    /// for a field that is right, as the Daemon last said; empty for other
    /// Views.
    errors: Vec<String>,
    /// A list View's Contributed actions by kind, each list shared by the
    /// rows of that kind.
    contributed: Shared,
    /// What is typed to filter a list View; empty when it shows every Item.
    pub filter: String,
    /// The Daemon's matches for `filter`, best first; `None` until they come,
    /// and for an empty filter or a list that filters itself.
    matches: Option<Vec<ListMatch>>,
    /// A new form of a list the Daemon filters, held back while `filter`
    /// isn't empty: `matches` count Items of the form shown, and the
    /// Daemon's matches for the new one follow it.
    pending: Option<(View, Shared)>,
}

impl Open {
    fn new(extension: String, view: View, contributed: Contributed) -> Self {
        let values = match &view.content {
            ViewContent::Form { fields, .. } => fields
                .iter()
                .map(|field| match &field.input {
                    FormInput::Text { default, .. }
                    | FormInput::File { default, .. }
                    | FormInput::TextArea { default, .. }
                    | FormInput::Date { default, .. } => Value::String(default.clone().unwrap_or_default()),
                    FormInput::Checkbox { default } => Value::Bool(*default),
                    FormInput::Select { options, default } => Value::String(
                        default
                            .clone()
                            .or_else(|| options.first().map(|option| option.value.clone()))
                            .unwrap_or_default(),
                    ),
                    FormInput::Number { default, .. } => default.map_or_else(|| Value::String(String::new()), number),
                    FormInput::MultiSelect { options, default } => {
                        chosen(options, |value| default.iter().any(|chosen| chosen == value))
                    }
                    FormInput::Description { .. } => Value::String(String::new()),
                })
                .collect(),
            _ => Vec::new(),
        };
        let errors = vec![String::new(); values.len()];
        Open {
            extension,
            view,
            values,
            errors,
            contributed: shared(contributed),
            filter: String::new(),
            matches: None,
            pending: None,
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

    pub fn is_list(&self) -> bool {
        matches!(self.view.content, ViewContent::List { .. })
    }

    /// Whether the Daemon filters this list, rather than its Extension.
    fn filtered_by_daemon(&self) -> bool {
        matches!(
            self.view.content,
            ViewContent::List {
                filters_itself: false,
                ..
            }
        )
    }

    /// Takes the form held back for the Daemon's matches, if there is one.
    fn take_pending(&mut self) -> bool {
        let Some((view, contributed)) = self.pending.take() else {
            return false;
        };
        self.view = view;
        self.contributed = contributed;
        true
    }

    /// A list View's rows: the Items that match the filter, best first, or
    /// every Item in order. `None` for other Views. Each row is made only
    /// when [`crate::rows::diff`] needs it.
    pub fn listed(&self) -> Option<Listed<'_>> {
        let ViewContent::List { items, .. } = &self.view.content else {
            return None;
        };
        let shown = match &self.matches {
            Some(matches) => matches
                .iter()
                .filter_map(|found| {
                    let item = items.get(found.item as usize)?;
                    Some((item, found.highlights.as_slice()))
                })
                .collect(),
            None => items.iter().map(|item| (item, &[][..])).collect(),
        };
        Some(Listed {
            open: self,
            shown,
            none: Arc::new([]),
        })
    }

    /// The rows [`Open::listed`] gives, all made.
    #[cfg(test)]
    pub fn rows(&self) -> Option<Vec<Row>> {
        let listed = self.listed()?;
        Some((0..listed.len()).map(|n| listed.row(n)).collect())
    }
}

/// A list View's rows, as [`Open::listed`] gives them.
pub struct Listed<'a> {
    open: &'a Open,
    /// The Items shown, in order, each with its matched characters.
    shown: Vec<(&'a Item, &'a [u32])>,
    none: Arc<[OfferedAction]>,
}

impl Listed<'_> {
    fn contributed(&self, item: &Item) -> &Arc<[OfferedAction]> {
        self.open.contributed.get(item.kind.name()).unwrap_or(&self.none)
    }
}

impl Fresh for Listed<'_> {
    fn len(&self) -> usize {
        self.shown.len()
    }

    fn key(&self, n: usize) -> (&str, &str) {
        (&self.open.extension, &self.shown[n].0.id)
    }

    fn row(&self, n: usize) -> Row {
        let (item, highlights) = self.shown[n];
        let contributed = Arc::clone(self.contributed(item));
        Row::listed(&self.open.extension, item, contributed, highlights.to_vec())
    }

    fn refresh(&self, n: usize, old: &mut Row) -> bool {
        let (item, highlights) = self.shown[n];
        old.relist(&self.open.extension, item, self.contributed(item), highlights)
    }
}

fn shared(contributed: Contributed) -> Shared {
    contributed
        .into_iter()
        .map(|(kind, offered)| (kind, offered.into()))
        .collect()
}

/// The Views open on top of the search, and the jobs Extensions report.
#[derive(Debug, Default)]
pub struct Views {
    /// The top View last.
    stack: Vec<Open>,
    /// By the Extension that runs them, in the order they started.
    jobs: Vec<(String, Job)>,
    /// The file field last asked to be completed, and what was typed in it.
    completing: Option<Completing>,
    /// The Daemon's answer for `completing`; empty until it comes.
    completions: Vec<PathCompletion>,
}

/// A request for the paths that complete a file field.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Completing {
    extension: String,
    view: String,
    /// The field's place in its form, and its id.
    at: usize,
    field: String,
    text: String,
}

impl Views {
    pub fn top(&self) -> Option<&Open> {
        self.stack.last()
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Puts a View on top. One already open with the same id moves to the top
    /// in its new form, its filter cleared.
    pub fn push(&mut self, extension: String, view: View, contributed: Contributed) {
        self.stack.retain(|open| !open.is(&extension, &view.id));
        self.stack.push(Open::new(extension, view, contributed));
    }

    /// Puts `with` on top in place of the Extension's View `id`, in one step,
    /// so the search never shows between them.
    pub fn replace(&mut self, extension: String, id: &str, with: View, contributed: Contributed) {
        self.stack
            .retain(|open| !open.is(&extension, id) && !open.is(&extension, &with.id));
        self.stack.push(Open::new(extension, with, contributed));
        self.forget_finished();
    }

    /// Replaces the open View with the same id where it is. What was typed
    /// into a field the new form still has is kept, with the Daemon's error
    /// for it, and so is a list's
    /// filter, while it is still a list. False if no such View is open, or if
    /// the new form waits for the Daemon's matches for the filter, so what
    /// shows stays as it is.
    pub fn update(&mut self, extension: String, view: View, contributed: Contributed) -> bool {
        let Some(open) = self.stack.iter_mut().find(|open| open.is(&extension, &view.id)) else {
            return false;
        };
        let mut new = Open::new(extension, view, contributed);
        if new.filtered_by_daemon() && open.filtered_by_daemon() && !open.filter.is_empty() {
            open.pending = Some((new.view, new.contributed));
            return false;
        }
        if matches!(new.view.content, ViewContent::List { .. }) {
            new.filter = std::mem::take(&mut open.filter);
        }
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
                if let Some(at) = old.iter().position(|old| old.id == field.id) {
                    new.errors[n] = std::mem::take(&mut open.errors[at]);
                }
            }
        }
        *open = new;
        true
    }

    /// Filters the top list View by `text`: what to tell the Daemon, which
    /// matches it or passes it on to the Extension. `None` if the top View
    /// isn't a list or `text` is its filter already. An emptied filter shows
    /// every Item at once.
    pub fn set_filter(&mut self, text: &str) -> Option<FromFrontEnd> {
        let open = self.stack.last_mut()?;
        if !matches!(open.view.content, ViewContent::List { .. }) || open.filter == text {
            return None;
        }
        text.clone_into(&mut open.filter);
        if text.is_empty() {
            open.matches = None;
            open.take_pending();
        }
        Some(FromFrontEnd::FilterView {
            extension: open.extension.clone(),
            view: open.view.id.clone(),
            text: text.to_owned(),
        })
    }

    /// The Daemon's matches for `text` in a list View, and the form of it
    /// held back for them. Matches for a filter since changed are dropped.
    pub fn filtered(&mut self, extension: &str, id: &str, text: &str, matches: Vec<ListMatch>) -> Shown {
        let top = self.stack.len().saturating_sub(1);
        let Some((at, open)) = self
            .stack
            .iter_mut()
            .enumerate()
            .find(|(_, open)| open.is(extension, id))
        else {
            return Shown::Same;
        };
        if open.filter != text || text.is_empty() {
            return Shown::Same;
        }
        let updated = open.take_pending();
        open.matches = Some(matches);
        match (at == top, updated) {
            (false, _) => Shown::Same,
            (true, false) => Shown::Rows,
            (true, true) => Shown::View,
        }
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
    /// such field or the value doesn't suit it: a string for a text, text
    /// area, date or file field or one of a select's option values, a
    /// boolean for a checkbox, a number or the text typed for a number field,
    /// which keeps text that isn't a number for the Daemon to refuse, and a
    /// list of option values for a multi-select. A description takes none.
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
        let value = match (input, value) {
            (FormInput::Number { .. }, Value::String(typed)) => match typed.trim().parse::<f64>() {
                Ok(n) if n.is_finite() => number(n),
                _ => Value::String(typed),
            },
            (FormInput::Number { .. }, Value::Integer(n)) => Value::Integer(n),
            (FormInput::Number { .. }, Value::Float(n)) if n.is_finite() => number(n),
            (FormInput::MultiSelect { options, .. }, Value::Array(values)) => {
                let wanted: Vec<&str> = values
                    .iter()
                    .filter_map(|value| match value {
                        Value::String(value) => Some(value.as_str()),
                        _ => None,
                    })
                    .collect();
                if wanted.len() != values.len() || !wanted.iter().all(|value| is_option(options, value)) {
                    return false;
                }
                chosen(options, |value| wanted.contains(&value))
            }
            (
                FormInput::Text { .. } | FormInput::File { .. } | FormInput::TextArea { .. } | FormInput::Date { .. },
                value @ Value::String(_),
            )
            | (FormInput::Checkbox { .. }, value @ Value::Bool(_)) => value,
            (FormInput::Select { options, .. }, Value::String(chosen)) if is_option(options, &chosen) => {
                Value::String(chosen)
            }
            _ => return false,
        };
        open.values[field] = value;
        true
    }

    /// Ticks the option `value` of the top form's `field`th field, a
    /// multi-select, or unticks it if it was ticked. False if there is no
    /// such field or option.
    pub fn toggle_option(&mut self, field: usize, value: &str) -> bool {
        let Some(open) = self.stack.last_mut() else {
            return false;
        };
        let ViewContent::Form { fields, .. } = &open.view.content else {
            return false;
        };
        let Some(FormInput::MultiSelect { options, .. }) = fields.get(field).map(|field| &field.input) else {
            return false;
        };
        if !is_option(options, value) {
            return false;
        }
        let ticked = |option: &str| match &open.values[field] {
            Value::Array(values) => values
                .iter()
                .any(|ticked| matches!(ticked, Value::String(ticked) if ticked == option)),
            _ => false,
        };
        let now_ticked = !ticked(value);
        let toggled = chosen(
            options,
            |option| if option == value { now_ticked } else { ticked(option) },
        );
        open.values[field] = toggled;
        true
    }

    /// The Daemon's `errors` for the form `view` of `extension`, which it
    /// kept from the Extension. False, and nothing kept, unless the form is
    /// open; true when it is the top View, whose errors show.
    pub fn set_errors(&mut self, extension: &str, view: &str, errors: Vec<FieldError>) -> bool {
        let Some(at) = self.stack.iter().position(|open| open.is(extension, view)) else {
            return false;
        };
        let open = &mut self.stack[at];
        let ViewContent::Form { fields, .. } = &open.view.content else {
            return false;
        };
        let mut shown = vec![String::new(); fields.len()];
        for error in errors {
            if let Some(n) = fields.iter().position(|field| field.id == error.field) {
                shown[n] = error.message;
            }
        }
        open.errors = shown;
        at + 1 == self.stack.len()
    }

    /// What is wrong with each field of the top form, in field order, empty
    /// for a field that is right; empty when the top View isn't a form.
    pub fn errors(&self) -> &[String] {
        self.stack.last().map_or(&[], |open| &open.errors)
    }

    /// Submits the top form: what to send the Daemon. Its errors go; the
    /// Daemon names what is still wrong.
    pub fn submit(&mut self) -> Option<FromFrontEnd> {
        let open = self.stack.last_mut()?;
        let ViewContent::Form { fields, .. } = &open.view.content else {
            return None;
        };
        let values: Table = fields
            .iter()
            .zip(&open.values)
            .filter(|(field, _)| !matches!(field.input, FormInput::Description { .. }))
            .map(|(field, value)| (field.id.clone(), value.clone()))
            .collect();
        open.errors.iter_mut().for_each(String::clear);
        Some(FromFrontEnd::SubmitForm {
            extension: open.extension.clone(),
            view: open.view.id.clone(),
            values,
        })
    }

    /// Asks for the paths that complete `text` in the top form's `field`th
    /// field: what to send the Daemon. `None` if that isn't a file field.
    /// The paths offered for what was typed before stay until the answer
    /// comes.
    pub fn complete(&mut self, field: usize, text: &str) -> Option<FromFrontEnd> {
        let open = self.stack.last()?;
        let ViewContent::Form { fields, .. } = &open.view.content else {
            return None;
        };
        let asked = fields
            .get(field)
            .filter(|asked| matches!(asked.input, FormInput::File { .. }))?;
        let completing = Completing {
            extension: open.extension.clone(),
            view: open.view.id.clone(),
            at: field,
            field: asked.id.clone(),
            text: text.to_owned(),
        };
        let message = FromFrontEnd::CompletePath {
            extension: completing.extension.clone(),
            view: completing.view.clone(),
            field: completing.field.clone(),
            text: completing.text.clone(),
        };
        if self.completing.as_ref().is_some_and(|last| {
            last.at != field || last.view != completing.view || last.extension != completing.extension
        }) {
            self.completions.clear();
        }
        self.completing = Some(completing);
        Some(message)
    }

    /// The Daemon's paths for `text` in a file field. False, and nothing
    /// kept, unless they answer the last request: one for text typed since
    /// is stale.
    pub fn completed(
        &mut self,
        extension: &str,
        view: &str,
        field: &str,
        text: &str,
        entries: Vec<PathCompletion>,
    ) -> bool {
        let answers = self.completing.as_ref().is_some_and(|last| {
            last.extension == extension && last.view == view && last.field == field && last.text == text
        });
        if answers {
            self.completions = entries;
        }
        answers
    }

    /// The paths offered for the field being completed, and its place in
    /// the top form; none once that form isn't on top.
    pub fn completions(&self) -> Option<(usize, &[PathCompletion])> {
        let completing = self.completing.as_ref()?;
        let top = self.stack.last()?;
        top.is(&completing.extension, &completing.view)
            .then_some((completing.at, self.completions.as_slice()))
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
        self.completing = None;
        self.completions.clear();
    }

    /// Drops finished jobs no progress View follows.
    fn forget_finished(&mut self) {
        let stack = &self.stack;
        self.jobs
            .retain(|(by, job)| job.state == JobState::Running || stack.iter().any(|open| open.follows(by, &job.id)));
    }
}

/// The largest integer an `f64` holds exactly, 2^53: a whole number past it
/// stays a float.
const EXACT_INTEGERS: f64 = 9_007_199_254_740_992.0;

/// `n` as a number field holds it: an integer when it is whole, as the
/// Daemon sends it on.
fn number(n: f64) -> Value {
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

fn is_option(options: &[SelectOption], value: &str) -> bool {
    options.iter().any(|option| option.value == value)
}

/// A multi-select's value: the `options` that are `ticked`, in their order.
fn chosen(options: &[SelectOption], ticked: impl Fn(&str) -> bool) -> Value {
    Value::Array(
        options
            .iter()
            .filter(|option| ticked(&option.value))
            .map(|option| Value::String(option.value.clone()))
            .collect(),
    )
}

// `benches/filter.rs` compiles this file through `#[path]`, which looks for
// child modules beside it rather than under `views/`; the path finds the tests
// from both.
#[cfg(test)]
#[path = "views/tests.rs"]
mod tests;
