use kotodama_protocol::Action;
use kotodama_protocol::FieldError;
use kotodama_protocol::FormField;
use kotodama_protocol::Fraction;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::PathCompletion;
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
    FormField::new(id, id, input)
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
        yields: false,
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
fn a_file_field_starts_with_its_default_and_takes_a_typed_path() {
    let mut views = Views::default();
    let file = |default: Option<&str>, folder| FormInput::File {
        accept: vec!["conf".into()],
        folder,
        default: default.map(Into::into),
        placeholder: None,
    };
    views.push(
        "wireguard".into(),
        form(vec![
            field("config", file(Some("~/Downloads/work.conf"), false)),
            field("into", file(None, true)),
        ]),
        none(),
    );

    assert!(views.set_value(1, Value::String("~/vpn".into())));
    assert!(!views.set_value(1, Value::Bool(true)));

    assert_eq!(
        views.submit(),
        Some(FromFrontEnd::SubmitForm {
            extension: "wireguard".into(),
            view: "form".into(),
            values: Table::from([
                ("config".into(), Value::String("~/Downloads/work.conf".into())),
                ("into".into(), Value::String("~/vpn".into())),
            ]),
        })
    );
}

fn import_form() -> View {
    form(vec![
        field("name", text(None)),
        field(
            "config",
            FormInput::File {
                accept: vec!["conf".into()],
                folder: false,
                default: None,
                placeholder: None,
            },
        ),
    ])
}

fn path(title: &str) -> PathCompletion {
    PathCompletion {
        path: format!("/home/kuba/{title}").into(),
        title: title.into(),
        folder: false,
        modified: None,
        highlights: Vec::new(),
    }
}

#[test]
fn a_file_field_asks_the_daemon_to_complete_what_is_typed() {
    let mut views = Views::default();
    views.push("wireguard".into(), import_form(), none());

    assert_eq!(
        views.complete(1, "~/wo"),
        Some(FromFrontEnd::CompletePath {
            extension: "wireguard".into(),
            view: "form".into(),
            field: "config".into(),
            text: "~/wo".into(),
        })
    );
    assert_eq!(views.complete(0, "~/wo"), None, "a text field isn't completed");
}

#[test]
fn only_the_answer_to_what_was_typed_last_is_offered() {
    let mut views = Views::default();
    views.push("wireguard".into(), import_form(), none());
    views.complete(1, "~/w");
    views.complete(1, "~/wo");

    assert!(!views.completed("wireguard", "form", "config", "~/w", vec![path("stale.conf")]));
    assert!(views.completed("wireguard", "form", "config", "~/wo", vec![path("work.conf")]));

    assert_eq!(views.completions(), Some((1, [path("work.conf")].as_slice())));
}

#[test]
fn completions_go_with_their_form() {
    let mut views = Views::default();
    views.push("wireguard".into(), import_form(), none());
    views.complete(1, "");
    views.completed("wireguard", "form", "config", "", vec![path("work.conf")]);

    views.push("git".into(), detail("a"), none());

    assert_eq!(views.completions(), None);
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

fn options(values: &[&str]) -> Vec<SelectOption> {
    values
        .iter()
        .map(|value| SelectOption {
            value: (*value).into(),
            label: value.to_uppercase(),
        })
        .collect()
}

/// A form of every kind a field may be, as a timer's.
fn every_kind() -> View {
    form(vec![
        field(
            "notes",
            FormInput::TextArea {
                placeholder: None,
                default: Some("Tea".into()),
            },
        ),
        field(
            "minutes",
            FormInput::Number {
                min: Some(1.0),
                max: None,
                step: Some(1.0),
                default: Some(25.0),
            },
        ),
        field(
            "at",
            FormInput::Date {
                time: true,
                default: None,
            },
        ),
        field(
            "days",
            FormInput::MultiSelect {
                options: options(&["mon", "tue", "wed"]),
                default: vec!["tue".into(), "sun".into()],
            },
        ),
        field(
            "about",
            FormInput::Description {
                text: "Rings once.".into(),
            },
        ),
    ])
}

fn submitted_values(views: &mut Views) -> Table {
    match views.submit() {
        Some(FromFrontEnd::SubmitForm { values, .. }) => values,
        other => panic!("expected a form submitted, got {other:?}"),
    }
}

#[test]
fn every_kind_of_field_starts_with_its_default_and_a_description_sends_nothing() {
    let mut views = Views::default();
    views.push("timer".into(), every_kind(), none());

    assert_eq!(
        submitted_values(&mut views),
        Table::from([
            ("notes".into(), Value::String("Tea".into())),
            ("minutes".into(), Value::Integer(25)),
            ("at".into(), Value::String(String::new())),
            ("days".into(), Value::Array(vec![Value::String("tue".into())])),
        ])
    );
}

#[test]
fn a_number_typed_is_sent_as_a_number_and_anything_else_as_typed_for_the_daemon_to_refuse() {
    let mut views = Views::default();
    views.push("timer".into(), every_kind(), none());

    assert!(views.set_value(1, Value::String("2.5".into())));
    assert_eq!(submitted_values(&mut views)["minutes"], Value::Float(2.5));
    assert!(views.set_value(1, Value::String("soon".into())));
    assert_eq!(submitted_values(&mut views)["minutes"], Value::String("soon".into()));
}

#[test]
fn an_option_of_a_multi_select_is_toggled_and_the_chosen_ones_are_sent_in_the_options_order() {
    let mut views = Views::default();
    views.push("timer".into(), every_kind(), none());

    assert!(views.toggle_option(3, "wed"));
    assert!(views.toggle_option(3, "mon"));
    assert!(views.toggle_option(3, "tue"));
    assert!(!views.toggle_option(3, "sun"), "not an option");
    assert!(!views.toggle_option(0, "mon"), "not a multi-select");

    assert_eq!(
        submitted_values(&mut views)["days"],
        Value::Array(vec![Value::String("mon".into()), Value::String("wed".into())])
    );
}

#[test]
fn a_description_takes_no_value() {
    let mut views = Views::default();
    views.push("timer".into(), every_kind(), none());

    assert!(!views.set_value(4, Value::String("x".into())));
}

#[test]
fn the_daemons_errors_show_under_their_fields_until_the_form_is_submitted_again() {
    let mut views = Views::default();
    views.push("timer".into(), every_kind(), none());
    let errors = vec![
        FieldError {
            field: "at".into(),
            message: "Required".into(),
        },
        FieldError {
            field: "gone".into(),
            message: "Required".into(),
        },
    ];

    assert!(!views.set_errors("timer", "other", errors.clone()), "another form's");
    assert!(views.set_errors("timer", "form", errors));
    assert_eq!(views.errors(), ["", "", "Required", "", ""]);

    views.submit();
    assert_eq!(views.errors(), ["", "", "", "", ""]);
}

#[test]
fn a_forms_errors_stay_with_their_fields_when_its_extension_updates_it() {
    let mut views = Views::default();
    views.push("timer".into(), every_kind(), none());
    views.set_errors(
        "timer",
        "form",
        vec![FieldError {
            field: "at".into(),
            message: "Required".into(),
        }],
    );
    let ViewContent::Form { mut fields, submit } = every_kind().content else {
        unreachable!()
    };
    fields.remove(0);

    views.update(
        "timer".into(),
        View {
            content: ViewContent::Form { fields, submit },
            ..every_kind()
        },
        none(),
    );

    assert_eq!(views.errors(), ["", "Required", "", ""]);
}
