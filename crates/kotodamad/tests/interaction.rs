//! Raycast-style interaction as the Daemon sees it: Arguments typed after an
//! Item's keyword, the form that asks for the ones left out, the Views
//! Extensions push, and the jobs they report.

#[path = "support/harness.rs"]
#[expect(dead_code, reason = "each test uses part of the harness")]
mod harness;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc;

use harness::*;
use kotodama_protocol::*;
use kotodamad::daemon::Event;
use kotodamad::daemon::FrontEndId;
use kotodamad::usage::Used;

fn argument(name: &str, optional: bool) -> Argument {
    Argument {
        optional,
        ..Argument::new(name).placeholder(format!("the {name}"))
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

#[test]
fn a_file_argument_reaches_the_action_as_an_absolute_path() {
    let mut h = Harness::new(vec![plain("tools")]);
    h.daemon.set_home("/home/kuba".into());
    let tools = h.up(0);
    h.add(
        &tools,
        vec![go(vec![
            Argument::new("into").folder(),
            Argument::new("config").file(["conf"]),
        ])],
    );
    let ui = h.front_end();
    tools.drain();

    h.query(&ui, "go vpn ~/Downloads/work.conf");
    run(&mut h, &ui, "tools", "go", "tools", "open");

    assert_eq!(
        arguments_run(&tools),
        [strings(&[
            ("into", "/home/kuba/vpn"),
            ("config", "/home/kuba/Downloads/work.conf")
        ])]
    );
}

#[test]
fn a_file_argument_left_out_is_asked_for_in_a_file_field_and_runs_with_an_absolute_path() {
    let mut h = Harness::new(vec![plain("tools")]);
    h.daemon.set_home("/home/kuba".into());
    let tools = h.up(0);
    h.add(&tools, vec![go(vec![Argument::new("config").file(["conf"])])]);
    let ui = h.front_end();
    tools.drain();

    h.query(&ui, "go");
    run(&mut h, &ui, "tools", "go", "tools", "open");
    let views = pushed(&ui);
    let (_, form) = &views[0];
    let ViewContent::Form { fields, .. } = &form.content else {
        panic!("expected a form, got {form:?}")
    };
    assert_eq!(
        fields[0].input,
        FormInput::File {
            accept: vec!["conf".into()],
            folder: false,
            default: None,
            placeholder: None,
        }
    );
    h.front_end_sends(
        &ui,
        FromFrontEnd::SubmitForm {
            extension: "kotodama".into(),
            view: form.id.clone(),
            values: BTreeMap::from([("config".into(), Value::String("~/work.conf".into()))]),
        },
    );

    assert_eq!(arguments_run(&tools), [strings(&[("config", "/home/kuba/work.conf")])]);
}

/// A home directory of its own, in the build's target directory rather than
/// the real home or `/tmp`, holding `paths` (a folder where one ends in `/`).
fn home_with(paths: &[&str]) -> tempfile::TempDir {
    let scratch = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let home = tempfile::Builder::new().prefix("home-").tempdir_in(scratch).unwrap();
    for path in paths {
        let full = home.path().join(path.trim_end_matches('/'));
        if path.ends_with('/') {
            std::fs::create_dir_all(full).unwrap();
        } else {
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, "").unwrap();
        }
    }
    home
}

/// The paths each `path_completions` sent to `ui` offered, by title.
fn completed(ui: &FrontEnd) -> Vec<(String, Vec<String>)> {
    ui.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToFrontEnd::PathCompletions { text, entries, .. } => {
                Some((text, entries.into_iter().map(|entry| entry.title).collect()))
            }
            _ => None,
        })
        .collect()
}

fn complete(extension: &str, view: &str, field: &str, text: &str) -> FromFrontEnd {
    FromFrontEnd::CompletePath {
        extension: extension.into(),
        view: view.into(),
        field: field.into(),
        text: text.into(),
    }
}

#[test]
fn a_file_arguments_form_field_completes_what_is_typed_from_its_directory() {
    let home = home_with(&["vpn/work.conf", "vpn/notes.txt", "vpn/old/"]);
    let mut h = Harness::new(vec![plain("tools")]);
    h.daemon.set_home(home.path().to_path_buf());
    let tools = h.up(0);
    h.add(&tools, vec![go(vec![Argument::new("config").file(["conf"])])]);
    let ui = h.front_end();

    h.query(&ui, "go");
    run(&mut h, &ui, "tools", "go", "tools", "open");
    let views = pushed(&ui);
    let (_, form) = &views[0];
    h.front_end_sends(&ui, complete("kotodama", &form.id, "config", "~/vpn/"));

    assert_eq!(
        completed(&ui),
        [("~/vpn/".to_owned(), vec!["old".to_owned(), "work.conf".to_owned()])]
    );
}

#[test]
fn only_the_front_end_showing_an_argument_form_gets_its_completions() {
    let home = home_with(&["vpn/work.conf"]);
    let mut h = Harness::new(vec![plain("tools")]);
    h.daemon.set_home(home.path().to_path_buf());
    let tools = h.up(0);
    h.add(&tools, vec![go(vec![Argument::new("config").file(["conf"])])]);
    let ui = h.front_end();
    let other = h.front_end();

    h.query(&ui, "go");
    run(&mut h, &ui, "tools", "go", "tools", "open");
    let views = pushed(&ui);
    let (_, form) = &views[0];
    other.drain();
    h.front_end_sends(&other, complete("kotodama", &form.id, "config", "~/vpn/"));

    assert_eq!(completed(&other), []);
}

#[test]
fn an_extensions_file_field_completes_and_another_field_does_not() {
    let home = home_with(&["Downloads/"]);
    let mut h = Harness::new(vec![plain("tools")]);
    h.daemon.set_home(home.path().to_path_buf());
    let tools = h.up(0);
    let ui = h.front_end();
    let form = View {
        id: "import".into(),
        title: "Import".into(),
        content: ViewContent::Form {
            fields: vec![
                FormField::new(
                    "into",
                    "Into",
                    FormInput::File {
                        accept: Vec::new(),
                        folder: true,
                        default: None,
                        placeholder: None,
                    },
                ),
                FormField::new(
                    "name",
                    "Name",
                    FormInput::Text {
                        placeholder: None,
                        default: None,
                        secret: false,
                    },
                ),
            ],
            submit: "Import".into(),
        },
    };
    h.send(&tools, FromExtension::PushView(form));
    ui.drain();

    h.front_end_sends(&ui, complete("tools", "import", "into", "Dow"));
    h.front_end_sends(&ui, complete("tools", "import", "name", "Dow"));
    h.front_end_sends(&ui, complete("tools", "gone", "into", "Dow"));

    assert_eq!(completed(&ui), [("Dow".to_owned(), vec!["Downloads".to_owned()])]);
}

#[test]
fn a_file_field_of_an_extensions_form_is_submitted_as_an_absolute_path() {
    let mut h = Harness::new(vec![plain("tools")]);
    h.daemon.set_home("/home/kuba".into());
    let tools = h.up(0);
    let ui = h.front_end();
    let form = View {
        id: "import".into(),
        title: "Import".into(),
        content: ViewContent::Form {
            fields: vec![FormField::new(
                "config",
                "Config",
                FormInput::File {
                    accept: Vec::new(),
                    folder: false,
                    default: None,
                    placeholder: None,
                },
            )],
            submit: "Import".into(),
        },
    };
    h.send(&tools, FromExtension::PushView(form));
    tools.drain();

    h.front_end_sends(
        &ui,
        FromFrontEnd::SubmitForm {
            extension: "tools".into(),
            view: "import".into(),
            values: BTreeMap::from([("config".into(), Value::String("work.conf".into()))]),
        },
    );

    let submitted: Vec<Table> = tools
        .drain()
        .into_iter()
        .filter_map(|message| match message {
            ToExtension::SubmitForm { values, .. } => Some(values),
            _ => None,
        })
        .collect();
    assert_eq!(
        submitted,
        [Table::from([(
            "config".into(),
            Value::String("/home/kuba/work.conf".into())
        )])]
    );
}

/// The Views pushed to a Front end, with the Extension each came from.
fn pushed(ui: &FrontEnd) -> Vec<(String, View)> {
    ui.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToFrontEnd::PushView { extension, view, .. } => Some((extension, view)),
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
        vec![go(vec![argument("signal", false), argument("process", false)])],
    );
    let ui = h.front_end();
    tools.drain();

    h.query(&ui, "go 9");
    run(&mut h, &ui, "tools", "go", "tools", "open");
    assert_eq!(arguments_run(&tools), [], "nothing runs until the form is in");
    let views = pushed(&ui);
    assert_eq!(views.len(), 1);
    let (extension, form) = &views[0];
    assert_eq!(extension, "kotodama", "the Daemon's own View");
    let ViewContent::Form { fields, .. } = &form.content else {
        panic!("expected a form, got {form:?}")
    };
    let fields: Vec<_> = fields.iter().map(|field| (field.id.as_str(), &field.input)).collect();
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
            filters_itself: false,
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
    assert_eq!(pushed(&ui), [("git".to_owned(), branches())], "passed on unchanged");
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
    assert!(!git.drain().iter().any(|m| matches!(m, ToExtension::RunAction { .. })));
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

/// A Front end that connects, with what the Daemon sent it on connecting
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
        closed: Arc::default(),
        shown: Cell::default(),
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
        [("Installing Go".to_owned(), "Go 1.23 is installed".to_owned())]
    );
    assert_eq!(
        jobs_shown(&ui).last().map(|(_, job)| job.state),
        Some(JobState::Succeeded),
        "the open Front end sees it finish"
    );

    assert_eq!(jobs_shown(&reopened(&mut h)), []);
}

#[test]
fn a_job_whose_extension_stops_fails_and_says_so() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    let ui = h.front_end();
    h.send(
        &tools,
        FromExtension::JobProgress(job(JobState::Running, 0.5, "Downloading")),
    );

    h.down(&tools);
    let failed = jobs_shown(&ui).pop().map(|(extension, job)| (extension, job.state));
    assert_eq!(failed, Some(("tools".to_owned(), JobState::Failed)));
    assert_eq!(
        *h.outputs.2.borrow(),
        [("Installing Go".to_owned(), "tools stopped".to_owned())]
    );
    assert_eq!(jobs_shown(&reopened(&mut h)), []);
}

#[test]
fn each_argument_takes_a_word_and_the_last_takes_the_rest() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(
        &tools,
        vec![go(vec![argument("signal", false), argument("process", false)])],
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
        vec![go(vec![argument("signal", false), argument("process", false)])],
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
            shown: ui.shown.get(),
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

#[test]
fn an_empty_argument_from_the_front_end_is_taken_from_the_query() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(&tools, vec![go(vec![argument("version", false)])]);
    let ui = h.front_end();
    tools.drain();

    h.query(&ui, "go 1.23");
    h.front_end_sends(
        &ui,
        FromFrontEnd::RunAction {
            result: ResultRef {
                extension: "tools".into(),
                id: "go".into(),
            },
            shown: ui.shown.get(),
            action: ActionRef {
                extension: "tools".into(),
                id: "open".into(),
            },
            arguments: strings(&[("version", "")]),
        },
    );
    assert_eq!(arguments_run(&tools), [strings(&[("version", "1.23")])]);
}

// Every run of an Action is recorded, however it was run: the Action panel,
// a key binding (which the Front end sends as the same RunAction), an Item of
// a View, or a form that asked for its Arguments.

/// The picks the Daemon put in the usage log: Result id and Action.
fn picks(h: &Harness) -> Vec<(String, String)> {
    h.saved
        .1
        .borrow()
        .iter()
        .filter_map(|entry| match &entry.used {
            Used::Pick { id, action, .. } => Some((id.clone(), action.clone())),
            Used::Query { .. } => None,
        })
        .collect()
}

fn pick(id: &str, action: &str) -> (String, String) {
    (id.to_owned(), action.to_owned())
}

fn logging(extensions: Vec<kotodamad::discovery::Found>) -> Harness {
    Harness::with_config(extensions, config("[history]\nusage_log = true"))
}

#[test]
fn a_contributed_action_run_from_the_action_panel_is_recorded() {
    let pinner = manifest(
        "name = \"pinner\"\ncommand = [\"x\"]\n\
         [[contributed_actions]]\nkind = \"project\"\nid = \"pin\"\ntitle = \"Pin a version here\"",
    );
    let mut h = logging(vec![plain("projects"), pinner]);
    let projects = h.up(0);
    let pinner = h.up(1);
    h.add(&projects, vec![project("kotodama")]);
    let ui = h.front_end();
    h.query(&ui, "koto");

    run(&mut h, &ui, "projects", "kotodama", "pinner", "pin");
    assert!(matches!(&pinner.drain()[..], [.., ToExtension::RunAction { action, .. }] if action == "pin"));
    assert_eq!(picks(&h), [pick("kotodama", "pin")]);
}

#[test]
fn an_action_run_on_an_item_of_a_list_view_is_recorded() {
    let mut h = logging(vec![plain("git")]);
    let git = h.up(0);
    let ui = h.front_end();
    h.send(&git, FromExtension::PushView(branches()));

    run(&mut h, &ui, "git", "main", "git", "open");
    assert_eq!(picks(&h), [pick("main", "open")]);
}

#[test]
fn an_action_waiting_on_its_arguments_is_recorded_once_it_runs() {
    let mut h = logging(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(&tools, vec![go(vec![argument("version", false)])]);
    let ui = h.front_end();
    h.query(&ui, "go");
    run(&mut h, &ui, "tools", "go", "tools", "open");
    assert_eq!(picks(&h), [], "nothing has run yet");

    let (_, form) = pushed(&ui).pop().expect("the argument form");
    h.front_end_sends(
        &ui,
        FromFrontEnd::SubmitForm {
            extension: "kotodama".into(),
            view: form.id,
            values: BTreeMap::from([("version".into(), Value::String("1.23".into()))]),
        },
    );
    assert_eq!(arguments_run(&tools), [strings(&[("version", "1.23")])]);
    assert_eq!(picks(&h), [pick("go", "open")]);
}

/// A form View `id` of `fields`, as an Extension pushes it.
fn form_of(id: &str, fields: Vec<FormField>) -> View {
    View {
        id: id.into(),
        title: "Timer".into(),
        content: ViewContent::Form {
            fields,
            submit: "Start".into(),
        },
    }
}

fn submit(extension: &str, view: &str, values: &[(&str, Value)]) -> FromFrontEnd {
    FromFrontEnd::SubmitForm {
        extension: extension.into(),
        view: view.into(),
        values: values
            .iter()
            .map(|(id, value)| ((*id).to_owned(), value.clone()))
            .collect(),
    }
}

/// The values of each form submitted to the Extension.
fn submitted(ext: &Ext) -> Vec<Table> {
    ext.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToExtension::SubmitForm { values, .. } => Some(values),
            _ => None,
        })
        .collect()
}

/// The errors each `form_errors` sent to `ui` named, as `(field, message)`.
fn form_errors(ui: &FrontEnd) -> Vec<Vec<(String, String)>> {
    ui.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToFrontEnd::FormErrors { errors, .. } => {
                Some(errors.into_iter().map(|error| (error.field, error.message)).collect())
            }
            _ => None,
        })
        .collect()
}

fn errors(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(field, message)| ((*field).to_owned(), (*message).to_owned()))
        .collect()
}

fn timer_fields() -> Vec<FormField> {
    vec![
        FormField::new(
            "label",
            "Label",
            FormInput::Text {
                placeholder: None,
                default: None,
                secret: false,
            },
        )
        .optional(),
        FormField::new(
            "minutes",
            "Minutes",
            FormInput::Number {
                min: Some(1.0),
                max: Some(120.0),
                step: Some(1.0),
                default: None,
            },
        ),
        FormField::new(
            "about",
            "",
            FormInput::Description {
                text: "Rings once.".into(),
            },
        ),
    ]
}

#[test]
fn a_form_with_a_wrong_field_stays_open_and_the_front_end_is_told_which_and_why() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    let ui = h.front_end();
    h.send(&tools, FromExtension::PushView(form_of("timer", timer_fields())));
    tools.drain();
    ui.drain();

    h.front_end_sends(&ui, submit("tools", "timer", &[("minutes", Value::Integer(300))]));

    assert_eq!(submitted(&tools), Vec::<Table>::new(), "the Extension gets nothing");
    let sent = ui.drain();
    assert_eq!(
        sent,
        [ToFrontEnd::FormErrors {
            extension: "tools".into(),
            view: "timer".into(),
            errors: vec![FieldError {
                field: "minutes".into(),
                message: "At most 120".into(),
            }],
        }],
        "nothing pops the form"
    );
}

#[test]
fn only_the_front_end_that_submitted_a_form_is_told_what_is_wrong() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    let ui = h.front_end();
    let other = h.front_end();
    h.send(&tools, FromExtension::PushView(form_of("timer", timer_fields())));
    other.drain();

    h.front_end_sends(&ui, submit("tools", "timer", &[]));

    assert_eq!(form_errors(&ui), [errors(&[("minutes", "Required")])]);
    assert_eq!(form_errors(&other), Vec::<Vec<(String, String)>>::new());
}

#[test]
fn a_right_form_reaches_its_extension_with_its_numbers_as_numbers_and_no_description() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    let ui = h.front_end();
    h.send(&tools, FromExtension::PushView(form_of("timer", timer_fields())));
    tools.drain();
    ui.drain();

    h.front_end_sends(
        &ui,
        submit(
            "tools",
            "timer",
            &[
                ("label", Value::String(String::new())),
                ("minutes", Value::String("25".into())),
                ("about", Value::String("Rings once.".into())),
            ],
        ),
    );

    assert_eq!(
        submitted(&tools),
        [Table::from([
            ("label".into(), Value::String(String::new())),
            ("minutes".into(), Value::Integer(25)),
        ])]
    );
    assert_eq!(form_errors(&ui), Vec::<Vec<(String, String)>>::new());
}

#[test]
fn a_form_field_whose_pattern_isnt_a_regular_expression_takes_any_text_and_the_extension_is_told() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    let ui = h.front_end();
    let code = FormField::new(
        "code",
        "Code",
        FormInput::Text {
            placeholder: None,
            default: None,
            secret: false,
        },
    )
    .pattern("[0-9");
    tools.drain();

    h.send(&tools, FromExtension::PushView(form_of("pair", vec![code])));

    let rejected: Vec<String> = tools
        .drain()
        .into_iter()
        .filter_map(|message| match message {
            ToExtension::Rejected { reason } => Some(reason),
            _ => None,
        })
        .collect();
    assert_eq!(rejected.len(), 1);
    assert!(
        rejected[0].contains("`code`'s pattern `[0-9` isn't a regular expression"),
        "{rejected:?}"
    );
    let views = pushed(&ui);
    let ViewContent::Form { fields, .. } = &views[0].1.content else {
        panic!("expected a form, got {views:?}")
    };
    assert_eq!(fields[0].pattern, None);
}

#[test]
fn a_number_argument_runs_with_the_number_typed_after_the_keyword() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(&tools, vec![go(vec![Argument::new("minutes").number()])]);
    let ui = h.front_end();
    tools.drain();

    h.query(&ui, "go 25");
    run(&mut h, &ui, "tools", "go", "tools", "open");

    assert_eq!(arguments_run(&tools), [strings(&[("minutes", "25")])]);
}

#[test]
fn a_number_argument_typed_as_something_else_is_asked_for_again_in_a_number_field() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(&tools, vec![go(vec![Argument::new("minutes").number()])]);
    let ui = h.front_end();
    tools.drain();

    h.query(&ui, "go soon");
    run(&mut h, &ui, "tools", "go", "tools", "open");

    assert_eq!(arguments_run(&tools), [], "nothing runs until it is a number");
    let sent = ui.drain();
    let form = sent
        .iter()
        .find_map(|message| match message {
            ToFrontEnd::PushView { view, .. } => Some(view.clone()),
            _ => None,
        })
        .expect("the argument form");
    let ViewContent::Form { fields, .. } = &form.content else {
        panic!("expected a form, got {form:?}")
    };
    assert_eq!(
        fields[0].input,
        FormInput::Number {
            min: None,
            max: None,
            step: None,
            default: None,
        }
    );
    assert!(sent.contains(&ToFrontEnd::FormErrors {
        extension: "kotodama".into(),
        view: form.id.clone(),
        errors: vec![FieldError {
            field: "minutes".into(),
            message: "“soon” isn't a number".into(),
        }],
    }));

    h.front_end_sends(&ui, submit("kotodama", &form.id, &[("minutes", Value::Integer(5))]));

    assert_eq!(arguments_run(&tools), [strings(&[("minutes", "5")])]);
}

#[test]
fn an_argument_form_submitted_with_a_required_field_blank_stays_open_and_runs_nothing() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(
        &tools,
        vec![go(vec![argument("signal", false), argument("process", false)])],
    );
    let ui = h.front_end();
    tools.drain();
    h.query(&ui, "go 9");
    run(&mut h, &ui, "tools", "go", "tools", "open");
    let form = pushed(&ui).remove(0).1;

    h.front_end_sends(
        &ui,
        submit(
            "kotodama",
            &form.id,
            &[
                ("signal", Value::String("9".into())),
                ("process", Value::String(" ".into())),
            ],
        ),
    );

    assert_eq!(arguments_run(&tools), [], "nothing runs");
    assert_eq!(
        form_errors(&ui),
        [errors(&[("process", "Required")])],
        "and nothing pops the form"
    );

    h.front_end_sends(
        &ui,
        submit(
            "kotodama",
            &form.id,
            &[
                ("signal", Value::String("9".into())),
                ("process", Value::String("firefox".into())),
            ],
        ),
    );

    assert_eq!(
        arguments_run(&tools),
        [strings(&[("signal", "9"), ("process", "firefox")])]
    );
}

#[test]
fn an_optional_number_argument_typed_wrong_then_left_blank_in_its_form_runs_without_it() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(
        &tools,
        vec![go(vec![
            Argument::new("minutes").number(),
            Argument::new("seconds").number().optional(),
        ])],
    );
    let ui = h.front_end();
    tools.drain();
    h.query(&ui, "go 5 soon");
    run(&mut h, &ui, "tools", "go", "tools", "open");
    let form = pushed(&ui).remove(0).1;

    h.front_end_sends(
        &ui,
        submit(
            "kotodama",
            &form.id,
            &[
                ("minutes", Value::Integer(5)),
                ("seconds", Value::String(String::new())),
            ],
        ),
    );

    assert_eq!(arguments_run(&tools), [strings(&[("minutes", "5")])]);
}

#[test]
fn an_optional_argument_cleared_in_its_form_runs_without_what_was_typed() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(
        &tools,
        vec![go(vec![argument("note", true), argument("process", false)])],
    );
    let ui = h.front_end();
    tools.drain();
    h.query(&ui, "go draft");
    run(&mut h, &ui, "tools", "go", "tools", "open");
    let form = pushed(&ui).remove(0).1;

    h.front_end_sends(
        &ui,
        submit(
            "kotodama",
            &form.id,
            &[
                ("note", Value::String(String::new())),
                ("process", Value::String("firefox".into())),
            ],
        ),
    );

    assert_eq!(arguments_run(&tools), [strings(&[("process", "firefox")])]);
}

#[test]
fn a_number_argument_typed_in_another_way_reaches_the_action_written_one_way() {
    let mut h = Harness::new(vec![plain("tools")]);
    let tools = h.up(0);
    h.add(&tools, vec![go(vec![Argument::new("minutes").number()])]);
    let ui = h.front_end();
    tools.drain();

    h.query(&ui, "go 2.50");
    run(&mut h, &ui, "tools", "go", "tools", "open");

    assert_eq!(arguments_run(&tools), [strings(&[("minutes", "2.5")])]);
}
