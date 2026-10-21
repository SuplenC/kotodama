//! The stack of Views on top of the search, the values typed into a form, and
//! the jobs Extensions report. Nothing here touches Qt, so it is tested
//! without it.

use std::collections::BTreeMap;
use std::sync::Arc;

use kotodama_protocol::FormInput;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::Item;
use kotodama_protocol::Job;
use kotodama_protocol::JobState;
use kotodama_protocol::ListMatch;
use kotodama_protocol::OfferedAction;
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
                    FormInput::Text { default, .. } => Value::String(default.clone().unwrap_or_default()),
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
        let contributed = self.contributed(item).clone();
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
    /// into a field the new form still has is kept, and so is a list's
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
        open.filter = text.to_owned();
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
        self.jobs
            .retain(|(by, job)| job.state == JobState::Running || stack.iter().any(|open| open.follows(by, &job.id)));
    }
}

#[cfg(test)]
mod tests {
    use kotodama_protocol::Action;
    use kotodama_protocol::FormField;
    use kotodama_protocol::Fraction;
    use kotodama_protocol::Item;
    use kotodama_protocol::ItemKind;
    use kotodama_protocol::SelectOption;

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

    fn none() -> Contributed {
        Contributed::new()
    }

    fn item(id: &str, kind: ItemKind) -> Item {
        Item {
            id: id.into(),
            kind,
            identity: None,
            title: id.into(),
            subtitle: None,
            icon: None,
            keyword: None,
            keywords: Vec::new(),
            hidden: false,
            app_only: false,
            actions: Vec::new(),
        }
    }

    fn url(id: &str) -> Item {
        item(
            id,
            ItemKind::Url {
                url: format!("https://{id}"),
            },
        )
    }

    fn list(id: &str, items: Vec<Item>, filters_itself: bool) -> View {
        View {
            id: id.into(),
            title: id.into(),
            content: ViewContent::List { items, filters_itself },
        }
    }

    fn found(item: u32, highlights: &[u32]) -> ListMatch {
        ListMatch {
            item,
            highlights: highlights.to_vec(),
        }
    }

    /// The top View's rows, by title.
    fn titles(views: &Views) -> Vec<String> {
        views
            .top()
            .and_then(Open::rows)
            .unwrap()
            .into_iter()
            .map(|row| row.title)
            .collect()
    }

    fn filter(extension: &str, view: &str, text: &str) -> Option<FromFrontEnd> {
        Some(FromFrontEnd::FilterView {
            extension: extension.into(),
            view: view.into(),
            text: text.into(),
        })
    }

    fn ids(views: &Views) -> Vec<&str> {
        views.stack.iter().map(|open| open.view.id.as_str()).collect()
    }

    #[test]
    fn esc_pops_the_top_view_and_tells_the_daemon() {
        let mut views = Views::default();
        views.push("git".into(), detail("a"), none());
        views.push("git".into(), detail("b"), none());
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
        views.push("git".into(), detail("a"), none());
        views.push("other".into(), detail("a"), none());
        views.push("git".into(), detail("b"), none());
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
        views.push("git".into(), detail("a"), none());
        views.push("git".into(), detail("b"), none());
        views.push("git".into(), detail("a"), none());
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
            none(),
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
            none(),
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
        views.push("git".into(), detail("a"), none());
        assert_eq!(views.submit(), None);
    }

    #[test]
    fn an_updated_form_keeps_what_was_typed_into_the_fields_it_still_has() {
        let mut views = Views::default();
        views.push(
            "timer".into(),
            form(vec![field("label", text(None)), field("seconds", text(None))]),
            none(),
        );
        views.set_value(0, Value::String("Tea".into()));
        views.set_value(1, Value::String("3".into()));

        assert!(views.update(
            "timer".into(),
            form(vec![field("seconds", text(None)), field("note", text(Some("hi")))]),
            none(),
        ));
        assert_eq!(
            views.top().unwrap().values,
            [Value::String("3".into()), Value::String("hi".into())]
        );
        assert!(!views.update("timer".into(), detail("not-open"), none()));
    }

    #[test]
    fn running_jobs_are_listed_and_a_finished_one_stays_only_while_a_view_follows_it() {
        let mut views = Views::default();
        views.progress("tools".into(), job("a", JobState::Running));
        views.progress("tools".into(), job("b", JobState::Running));
        views.push("tools".into(), progress("p", "b"), none());
        assert_eq!(
            views.running().map(|(_, job)| job.id.as_str()).collect::<Vec<_>>(),
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

    #[test]
    fn a_list_views_items_list_the_actions_contributed_to_their_kind() {
        let mut views = Views::default();
        let mut url_actions = Contributed::new();
        url_actions.insert(
            "url".into(),
            vec![OfferedAction {
                extension: "links".into(),
                action: Action::new("copy", "Copy link"),
            }],
        );
        let project = item(
            "kotodama",
            ItemKind::Project {
                path: "/src/kotodama".into(),
            },
        );
        views.push(
            "bookmarks".into(),
            list("all", vec![url("a"), project], false),
            url_actions,
        );
        let rows = views.top().unwrap().rows().unwrap();
        let groups: Vec<Vec<_>> = rows
            .iter()
            .map(|row| row.actions().map(|entry| entry.group).collect())
            .collect();
        assert_eq!(groups, [vec![Some("links")], vec![]]);
    }

    #[test]
    fn typing_in_a_list_view_asks_the_daemon_and_shows_its_matches_best_first() {
        let mut views = Views::default();
        views.push(
            "bookmarks".into(),
            list("all", vec![url("alpha"), url("beta"), url("gamma")], false),
            none(),
        );
        assert_eq!(views.set_filter("a"), filter("bookmarks", "all", "a"));
        assert_eq!(views.set_filter("a"), None, "the same text again");
        assert_eq!(titles(&views), ["alpha", "beta", "gamma"], "until it answers");

        assert_eq!(
            views.filtered("bookmarks", "all", "a", vec![found(2, &[1]), found(0, &[0])]),
            Shown::Rows
        );
        assert_eq!(titles(&views), ["gamma", "alpha"]);
        let rows = views.top().unwrap().rows().unwrap();
        assert_eq!(rows[0].highlights, [1]);

        assert_eq!(views.set_filter("am"), filter("bookmarks", "all", "am"));
        assert_eq!(
            views.filtered("bookmarks", "all", "a", vec![found(1, &[])]),
            Shown::Same,
            "matches for what was typed before are dropped"
        );
        assert_eq!(titles(&views), ["gamma", "alpha"]);

        assert_eq!(views.set_filter(""), filter("bookmarks", "all", ""));
        assert_eq!(titles(&views), ["alpha", "beta", "gamma"], "at once");
        assert_eq!(views.top().unwrap().filter, "");
    }

    #[test]
    fn only_a_list_view_takes_a_filter() {
        let mut views = Views::default();
        assert_eq!(views.set_filter("a"), None);
        views.push("git".into(), detail("a"), none());
        assert_eq!(views.set_filter("a"), None);
        assert_eq!(views.top().unwrap().rows(), None);
    }

    #[test]
    fn a_list_that_filters_itself_shows_what_its_extension_sends() {
        let mut views = Views::default();
        views.push(
            "search".into(),
            list("web", vec![url("alpha"), url("beta")], true),
            none(),
        );
        assert_eq!(views.set_filter("be"), filter("search", "web", "be"));
        assert!(views.update("search".into(), list("web", vec![url("beta")], true), none()));
        assert_eq!(titles(&views), ["beta"]);
        assert_eq!(views.top().unwrap().filter, "be", "the filter stays");
    }

    #[test]
    fn a_filtered_list_updated_by_its_extension_shows_the_old_matches_until_the_new_ones_come() {
        let mut views = Views::default();
        views.push(
            "bookmarks".into(),
            list("all", vec![url("alpha"), url("beta")], false),
            none(),
        );
        views.set_filter("a");
        views.filtered("bookmarks", "all", "a", vec![found(0, &[0]), found(1, &[3])]);

        let updated = list("all", vec![url("delta"), url("alpha"), url("beta")], false);
        assert!(!views.update("bookmarks".into(), updated, none()));
        assert_eq!(titles(&views), ["alpha", "beta"]);

        assert_eq!(
            views.filtered(
                "bookmarks",
                "all",
                "a",
                vec![found(0, &[4]), found(1, &[0]), found(2, &[3])]
            ),
            Shown::View,
            "the View's new form shows with its matches"
        );
        assert_eq!(titles(&views), ["delta", "alpha", "beta"]);
        assert_eq!(views.top().unwrap().filter, "a");
    }

    #[test]
    fn a_filtered_list_updated_into_another_kind_of_view_shows_it_at_once() {
        let mut views = Views::default();
        views.push("bookmarks".into(), list("all", vec![url("alpha")], false), none());
        views.set_filter("a");
        assert!(views.update("bookmarks".into(), detail("all"), none()));
        assert_eq!(views.top().unwrap().rows(), None);
        assert_eq!(views.top().unwrap().filter, "", "only a list has a filter");
    }

    #[test]
    fn an_emptied_filter_shows_an_update_held_back_for_matches() {
        let mut views = Views::default();
        views.push("bookmarks".into(), list("all", vec![url("alpha")], false), none());
        views.set_filter("a");
        views.update(
            "bookmarks".into(),
            list("all", vec![url("alpha"), url("beta")], false),
            none(),
        );
        views.set_filter("");
        assert_eq!(titles(&views), ["alpha", "beta"]);
    }

    #[test]
    fn a_replaced_view_gives_its_place_to_the_new_one_in_one_step() {
        let mut views = Views::default();
        views.push("git".into(), detail("a"), none());
        views.push("timer".into(), form(Vec::new()), none());
        views.replace("timer".into(), "form", progress("p", "job"), none());
        assert_eq!(ids(&views), ["a", "p"]);

        views.replace("timer".into(), "not-open", detail("b"), none());
        assert_eq!(ids(&views), ["a", "p", "b"], "it is pushed all the same");
    }
}
