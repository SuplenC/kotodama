//! Raycast-style interaction as the Daemon sees it: Arguments typed after an
//! Item's keyword, the form that asks for the ones left out, the Views
//! Extensions push, and the jobs they report.

mod common;

use std::collections::BTreeMap;
use std::sync::mpsc;

use common::*;
use kotodama_protocol::*;
use kotodamad::daemon::{Event, FrontEndId};

fn argument(name: &str, optional: bool) -> Argument {
    Argument {
        name: name.into(),
        placeholder: Some(format!("the {name}")),
        optional,
    }
}

/// An Item named by the keyword `go`, whose primary Action takes `arguments`.
fn go(arguments: Vec<Argument>) -> Item {
    let mut item = item(
        "go",
        "Go toolchain",
        ItemKind::Url {
            url: "https://go.dev".into(),
        },
    );
    item.keyword = Some("go".into());
    item.actions[0].arguments = arguments;
    item
}

/// The Arguments of every Action the Extension was asked to run.
fn arguments_run(ext: &Ext) -> Vec<BTreeMap<String, String>> {
    ext.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToExtension::RunAction { arguments, .. } => Some(arguments),
            _ => None,
        })
        .collect()
}

fn strings(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

#[test]
fn an_items_keyword_takes_the_rest_of_the_query_as_its_arguments() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(&tools, vec![go(vec![argument("version", false)])]);
    let ui = h.front_end();
    tools.drain();

    assert_eq!(titles(&h.query(&ui, "go 1.23")), ["Go toolchain"]);
    run(&mut h, &ui, "tools", "go", "tools", "open");
    assert_eq!(arguments_run(&tools), [strings(&[("version", "1.23")])]);
}

/// The Views pushed to a Front end, with the Extension each came from.
fn pushed(ui: &FrontEnd) -> Vec<(String, View)> {
    ui.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToFrontEnd::PushView { extension, view } => Some((extension, view)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_required_argument_left_out_opens_a_form_that_runs_the_action_once_submitted() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(
        &tools,
        vec![go(vec![
            argument("signal", false),
            argument("process", false),
        ])],
    );
    let ui = h.front_end();
    tools.drain();

    h.query(&ui, "go 9");
    run(&mut h, &ui, "tools", "go", "tools", "open");
    assert_eq!(
        arguments_run(&tools),
        [],
        "nothing runs until the form is in"
    );
    let views = pushed(&ui);
    assert_eq!(views.len(), 1);
    let (extension, form) = &views[0];
    assert_eq!(extension, "kotodama", "the Daemon's own View");
    let ViewContent::Form { fields, .. } = &form.content else {
        panic!("expected a form, got {form:?}")
    };
    let fields: Vec<_> = fields
        .iter()
        .map(|field| (field.id.as_str(), &field.input))
        .collect();
    assert_eq!(
        fields,
        [
            (
                "signal",
                &FormInput::Text {
                    placeholder: Some("the signal".into()),
                    default: Some("9".into()),
                    secret: false,
                }
            ),
            (
                "process",
                &FormInput::Text {
                    placeholder: Some("the process".into()),
                    default: None,
                    secret: false,
                }
            ),
        ]
    );

    h.front_end_sends(
        &ui,
        FromFrontEnd::SubmitForm {
            extension: "kotodama".into(),
            view: form.id.clone(),
            values: BTreeMap::from([
                ("signal".into(), Value::String("9".into())),
                ("process".into(), Value::String("firefox".into())),
            ]),
        },
    );
    assert_eq!(
        arguments_run(&tools),
        [strings(&[("signal", "9"), ("process", "firefox")])]
    );
    assert!(ui.drain().contains(&ToFrontEnd::PopView {
        extension: "kotodama".into(),
        view: form.id.clone(),
    }));
}

#[test]
fn an_optional_argument_left_out_runs_the_action_without_it() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(&tools, vec![go(vec![argument("version", true)])]);
    let ui = h.front_end();
    tools.drain();

    h.query(&ui, "go");
    run(&mut h, &ui, "tools", "go", "tools", "open");
    assert_eq!(arguments_run(&tools), [BTreeMap::new()]);
    assert_eq!(pushed(&ui), []);
}

#[test]
fn a_popped_argument_form_runs_nothing() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(&tools, vec![go(vec![argument("version", false)])]);
    let ui = h.front_end();
    tools.drain();

    h.query(&ui, "go");
    run(&mut h, &ui, "tools", "go", "tools", "open");
    let form = pushed(&ui).remove(0).1;
    h.front_end_sends(
        &ui,
        FromFrontEnd::ViewPopped {
            extension: "kotodama".into(),
            view: form.id.clone(),
        },
    );
    h.front_end_sends(
        &ui,
        FromFrontEnd::SubmitForm {
            extension: "kotodama".into(),
            view: form.id,
            values: BTreeMap::from([("version".into(), Value::String("1".into()))]),
        },
    );
    assert_eq!(arguments_run(&tools), []);
}

fn branches() -> View {
    View {
        id: "branches".into(),
        title: "Branches".into(),
        content: ViewContent::List {
            items: vec![item(
                "main",
                "main",
                ItemKind::Url {
                    url: "https://git/main".into(),
                },
            )],
        },
    }
}

fn failures(ui: &FrontEnd) -> Vec<String> {
    ui.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToFrontEnd::ActionFailed { message } => Some(message),
            _ => None,
        })
        .collect()
}

#[test]
fn an_item_in_a_list_view_runs_its_actions_like_a_search_result() {
    let mut h = Harness::new(vec![plain("git")]);
    let git = h.up(0);
    let ui = h.front_end();
    h.send(&git, FromExtension::PushView(branches()));
    assert_eq!(
        pushed(&ui),
        [("git".to_owned(), branches())],
        "passed on unchanged"
    );
    git.drain();

    run(&mut h, &ui, "git", "main", "git", "open");
    let sent = git.drain();
    assert!(
        matches!(&sent[..], [ToExtension::RunAction { action, item, .. }] if action == "open" && item.id == "main"),
        "{sent:?}"
    );
}

#[test]
fn the_items_of_a_popped_list_view_no_longer_run() {
    let mut h = Harness::new(vec![plain("git")]);
    let git = h.up(0);
    let ui = h.front_end();
    h.send(&git, FromExtension::PushView(branches()));
    h.front_end_sends(
        &ui,
        FromFrontEnd::ViewPopped {
            extension: "git".into(),
            view: "branches".into(),
        },
    );
    ui.drain();
    git.drain();

    run(&mut h, &ui, "git", "main", "git", "open");
    assert!(
        !git.drain()
            .iter()
            .any(|m| matches!(m, ToExtension::RunAction { .. }))
    );
    assert_eq!(failures(&ui).len(), 1);
}

#[test]
fn an_extensions_views_are_popped_when_it_stops() {
    let mut h = Harness::new(vec![plain("git")]);
    let git = h.up(0);
    let ui = h.front_end();
    h.send(&git, FromExtension::PushView(branches()));
    ui.drain();

    h.down(&git);
    assert!(ui.drain().contains(&ToFrontEnd::PopView {
        extension: "git".into(),
        view: "branches".into(),
    }));
}

fn job(state: JobState, progress: f32, message: &str) -> Job {
    Job {
        id: "install".into(),
        title: "Installing Go".into(),
        state,
        progress: Some(Fraction::new(progress).unwrap()),
        message: Some(message.into()),
    }
}

/// A Front end that connects now, with what the Daemon sent it on connecting
/// still unread.
fn reopened(h: &mut Harness) -> FrontEnd {
    let id = FrontEndId(1000 + h.next_front_end);
    h.next_front_end += 1;
    let (tx, rx) = mpsc::sync_channel(OUTBOX);
    h.event(Event::FrontEndUp {
        front_end: id,
        sender: tx,
        disconnect: Box::new(|| {}),
    });
    FrontEnd {
        id,
        rx,
        closed: Default::default(),
    }
}

fn jobs_shown(ui: &FrontEnd) -> Vec<(String, Job)> {
    ui.rx
        .try_iter()
        .filter_map(|message| match message {
            ToFrontEnd::JobProgress { extension, job } => Some((extension, job)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_front_end_that_connects_while_a_job_runs_is_shown_its_latest_progress() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.send(
        &tools,
        FromExtension::JobProgress(job(JobState::Running, 0.2, "Downloading")),
    );
    h.send(
        &tools,
        FromExtension::JobProgress(job(JobState::Running, 0.6, "Unpacking")),
    );

    assert_eq!(
        jobs_shown(&reopened(&mut h)),
        [("tools".to_owned(), job(JobState::Running, 0.6, "Unpacking"))]
    );
}

#[test]
fn a_finished_job_sends_a_desktop_notification_and_is_no_longer_shown() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    let ui = h.front_end();
    h.send(
        &tools,
        FromExtension::JobProgress(job(JobState::Running, 0.5, "Downloading")),
    );
    assert_eq!(h.outputs.2.borrow().len(), 0);

    h.send(
        &tools,
        FromExtension::JobProgress(job(JobState::Succeeded, 1.0, "Go 1.23 is installed")),
    );
    assert_eq!(
        *h.outputs.2.borrow(),
        [(
            "Installing Go".to_owned(),
            "Go 1.23 is installed".to_owned()
        )]
    );
    assert_eq!(
        jobs_shown(&ui).last().map(|(_, job)| job.state),
        Some(JobState::Succeeded),
        "the open Front end sees it finish"
    );

    assert_eq!(jobs_shown(&reopened(&mut h)), []);
}

#[test]
fn each_argument_takes_a_word_and_the_last_takes_the_rest() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(
        &tools,
        vec![go(vec![
            argument("signal", false),
            argument("process", false),
        ])],
    );
    let ui = h.front_end();
    tools.drain();

    h.query(&ui, "GO  9   web  browser ");
    run(&mut h, &ui, "tools", "go", "tools", "open");
    assert_eq!(
        arguments_run(&tools),
        [strings(&[("signal", "9"), ("process", "web  browser")])]
    );
}

#[test]
fn arguments_the_front_end_gives_win_over_the_typed_ones() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(
        &tools,
        vec![go(vec![
            argument("signal", false),
            argument("process", false),
        ])],
    );
    let ui = h.front_end();
    tools.drain();

    h.query(&ui, "go firefox");
    h.front_end_sends(
        &ui,
        FromFrontEnd::RunAction {
            result: ResultRef {
                extension: "tools".into(),
                id: "go".into(),
            },
            action: ActionRef {
                extension: "tools".into(),
                id: "open".into(),
            },
            arguments: strings(&[("signal", "15")]),
        },
    );
    assert_eq!(
        arguments_run(&tools),
        [strings(&[("signal", "15"), ("process", "firefox")])]
    );
}
