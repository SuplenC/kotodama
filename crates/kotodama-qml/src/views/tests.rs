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
        weak_keywords: Vec::new(),
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

fn filter(extension: &str, view: &str, text: &str) -> FromFrontEnd {
    FromFrontEnd::FilterView {
        extension: extension.into(),
        view: view.into(),
        text: text.into(),
    }
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
    assert_eq!(views.set_filter("a"), Some(filter("bookmarks", "all", "a")));
    assert_eq!(views.set_filter("a"), None, "the same text again");
    assert_eq!(titles(&views), ["alpha", "beta", "gamma"], "until it answers");

    assert_eq!(
        views.filtered("bookmarks", "all", "a", vec![found(2, &[1]), found(0, &[0])]),
        Shown::Rows
    );
    assert_eq!(titles(&views), ["gamma", "alpha"]);
    let rows = views.top().unwrap().rows().unwrap();
    assert_eq!(rows[0].highlights, [1]);

    assert_eq!(views.set_filter("am"), Some(filter("bookmarks", "all", "am")));
    assert_eq!(
        views.filtered("bookmarks", "all", "a", vec![found(1, &[])]),
        Shown::Same,
        "matches for what was typed before are dropped"
    );
    assert_eq!(titles(&views), ["gamma", "alpha"]);

    assert_eq!(views.set_filter(""), Some(filter("bookmarks", "all", "")));
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
    assert_eq!(views.set_filter("be"), Some(filter("search", "web", "be")));
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
