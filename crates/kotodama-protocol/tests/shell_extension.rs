//! Drives the shell + jq example Extension the way the Daemon will: over its
//! stdio, through the handshake, config, Items, a Contributed action, and the
//! Timer's Argument, form, progress View and job. Needs bash 4+ and jq, which
//! the dev environment provides.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use kotodama_protocol::*;

fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/shell-extension")
}

fn manifest() -> Manifest {
    Manifest::from_toml(&std::fs::read_to_string(example_dir().join("manifest.toml")).unwrap())
        .unwrap()
}

struct Extension {
    child: Child,
    channel: Channel<ChildStdout, ChildStdin>,
}

/// Starts the example as the Daemon would and does the handshake.
fn spawn() -> (Child, Channel<ChildStdout, ChildStdin>) {
    let manifest = manifest();
    let mut child = Command::new(example_dir().join(&manifest.command[0]))
        .args(&manifest.command[1..])
        .current_dir(example_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let (stdout, stdin) = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let channel = Channel::handshake(stdout, stdin, &Hello::new(&WireFormat::ALL)).unwrap();
    (child, channel)
}

impl Extension {
    fn start() -> Self {
        let (child, channel) = spawn();
        Extension { child, channel }
    }

    fn send(&mut self, message: ToExtension) {
        self.channel.send(&message).unwrap();
    }

    fn recv(&mut self) -> FromExtension {
        self.channel
            .recv()
            .unwrap()
            .expect("the Extension closed the connection")
    }
}

impl Drop for Extension {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn bookmarks(bookmarks: &[(&str, &str)]) -> ToExtension {
    let list = bookmarks
        .iter()
        .map(|(title, url)| {
            Value::Table(BTreeMap::from([
                ("title".into(), Value::String((*title).into())),
                ("url".into(), Value::String((*url).into())),
            ]))
        })
        .collect();
    ToExtension::Config(BTreeMap::from([("bookmarks".into(), Value::Array(list))]))
}

#[test]
fn the_shell_example_settles_on_json() {
    let extension = Extension::start();
    assert_eq!(extension.channel.format(), WireFormat::Json);
}

#[test]
fn the_shell_example_turns_its_config_table_into_url_items() {
    let mut extension = Extension::start();
    extension.send(bookmarks(&[
        ("Kotodama", "https://github.com/SuplenC/kotodama"),
        ("Wayland", "https://wayland.freedesktop.org"),
    ]));

    let FromExtension::AddItems { items } = extension.recv() else {
        panic!("expected add_items")
    };
    let summary: Vec<_> = items
        .iter()
        .filter(|item| matches!(item.kind, ItemKind::Url { .. }))
        .map(|item| (item.title.as_str(), item.kind.clone()))
        .collect();
    assert_eq!(
        summary,
        [
            (
                "Kotodama",
                ItemKind::Url {
                    url: "https://github.com/SuplenC/kotodama".into()
                }
            ),
            (
                "Wayland",
                ItemKind::Url {
                    url: "https://wayland.freedesktop.org".into()
                }
            ),
        ]
    );
}

#[test]
fn the_shell_example_replaces_its_items_when_the_config_reloads() {
    let mut extension = Extension::start();
    extension.send(bookmarks(&[
        ("A", "https://a.example"),
        ("B", "https://b.example"),
    ]));
    let FromExtension::AddItems { items: first } = extension.recv() else {
        panic!("expected add_items")
    };

    extension.send(bookmarks(&[("C", "https://c.example")]));
    let FromExtension::RemoveItems { ids } = extension.recv() else {
        panic!("expected remove_items")
    };
    // The timer is added again, which replaces it, so it is never removed.
    assert_eq!(first.len(), 3, "the bookmarks and the timer");
    assert_eq!(ids, ["https://a.example", "https://b.example"]);
    let FromExtension::AddItems { items } = extension.recv() else {
        panic!("expected add_items")
    };
    assert_eq!(
        items
            .iter()
            .map(|item| item.title.as_str())
            .collect::<Vec<_>>(),
        ["C", "Timer"]
    );
}

#[test]
fn the_shell_example_contributes_an_action_for_projects() {
    let contributed = manifest().contributed_actions;
    assert_eq!(contributed.len(), 1);
    assert_eq!(contributed[0].kind.as_str(), "project");

    let mut extension = Extension::start();
    extension.send(bookmarks(&[]));
    assert!(matches!(extension.recv(), FromExtension::AddItems { .. }));

    extension.send(ToExtension::RunAction {
        invocation: 42,
        action: contributed[0].action.id.clone(),
        item: ItemRef {
            id: "kotodama".into(),
            kind: ItemKind::Project {
                path: "/src/my \"quoted\" project".into(),
            },
        },
        arguments: BTreeMap::new(),
    });
    assert_eq!(
        extension.recv(),
        FromExtension::ActionDone {
            invocation: 42,
            outcome: Outcome::Paste(Clipboard::Text("/src/my \"quoted\" project".into())),
        }
    );
}

#[test]
fn the_shell_example_exits_when_the_daemon_hangs_up() {
    let (mut child, channel) = spawn();
    drop(channel);
    assert!(child.wait().unwrap().success());
}

/// Starts the example with no bookmarks and returns its timer Item.
fn timer() -> (Extension, Item) {
    let mut extension = Extension::start();
    extension.send(bookmarks(&[]));
    let FromExtension::AddItems { items } = extension.recv() else {
        panic!("expected add_items")
    };
    let timer = items
        .into_iter()
        .find(|item| item.id == "timer")
        .expect("the timer Item");
    (extension, timer)
}

/// Runs the timer's `action`th Action with `arguments`, as invocation 7.
fn run(extension: &mut Extension, timer: &Item, action: usize, arguments: &[(&str, &str)]) {
    extension.send(ToExtension::RunAction {
        invocation: 7,
        action: timer.actions[action].id.clone(),
        item: ItemRef {
            id: timer.id.clone(),
            kind: timer.kind.clone(),
        },
        arguments: arguments
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
    });
}

/// Receives job progress until the job finishes: each report's state and
/// progress, in order.
fn follow(extension: &mut Extension, id: &str, title: &str) -> Vec<(JobState, Option<f32>)> {
    let mut reports = Vec::new();
    loop {
        let FromExtension::JobProgress(job) = extension.recv() else {
            panic!("expected job_progress")
        };
        assert_eq!((job.id.as_str(), job.title.as_str()), (id, title));
        reports.push((job.state, job.progress.map(Fraction::get)));
        if job.state != JobState::Running {
            return reports;
        }
    }
}

#[test]
fn the_shell_examples_timer_takes_its_seconds_as_a_required_argument_after_its_keyword() {
    let (_extension, timer) = timer();
    assert_eq!(timer.keyword.as_deref(), Some("timer"));
    let start = &timer.actions[0];
    assert_eq!(start.id, "start");
    assert_eq!(start.arguments.len(), 1);
    assert_eq!(start.arguments[0].name, "seconds");
    assert!(!start.arguments[0].optional);
    assert_eq!(timer.actions[1].shortcut.as_deref(), Some("ctrl+l"));
}

#[test]
fn the_shell_examples_timer_shows_a_progress_view_and_reports_until_done() {
    let (mut extension, timer) = timer();
    run(&mut extension, &timer, 0, &[("seconds", "2")]);

    let FromExtension::PushView(view) = extension.recv() else {
        panic!("expected push_view")
    };
    assert_eq!(view.title, "2 second timer");
    let ViewContent::Progress { job } = view.content else {
        panic!("expected a progress View")
    };
    assert_eq!(
        extension.recv(),
        FromExtension::ActionDone {
            invocation: 7,
            outcome: Outcome::KeepOpen,
        }
    );
    assert_eq!(
        follow(&mut extension, &job, "2 second timer"),
        [
            (JobState::Running, Some(0.0)),
            (JobState::Running, Some(0.5)),
            (JobState::Succeeded, Some(1.0)),
        ]
    );
}

#[test]
fn the_shell_examples_timer_refuses_what_is_not_a_number_of_seconds() {
    let (mut extension, timer) = timer();
    run(&mut extension, &timer, 0, &[("seconds", "soon")]);
    assert!(matches!(
        extension.recv(),
        FromExtension::ActionDone {
            invocation: 7,
            outcome: Outcome::Failed { .. },
        }
    ));
}

#[test]
fn the_shell_examples_labelled_timer_asks_in_a_form_then_times_what_was_submitted() {
    let (mut extension, timer) = timer();
    run(&mut extension, &timer, 1, &[]);

    let FromExtension::PushView(form) = extension.recv() else {
        panic!("expected push_view")
    };
    let ViewContent::Form { fields, .. } = &form.content else {
        panic!("expected a form")
    };
    let ids: Vec<_> = fields.iter().map(|field| field.id.as_str()).collect();
    assert_eq!(ids, ["label", "seconds"]);
    assert_eq!(
        extension.recv(),
        FromExtension::ActionDone {
            invocation: 7,
            outcome: Outcome::KeepOpen,
        }
    );

    extension.send(ToExtension::SubmitForm {
        view: form.id.clone(),
        values: BTreeMap::from([
            ("label".into(), Value::String("Tea".into())),
            ("seconds".into(), Value::String("1".into())),
        ]),
    });
    assert_eq!(
        extension.recv(),
        FromExtension::PopView {
            view: form.id.clone()
        }
    );
    let FromExtension::PushView(progress) = extension.recv() else {
        panic!("expected push_view")
    };
    assert_eq!(progress.title, "Tea");
    let ViewContent::Progress { job } = progress.content else {
        panic!("expected a progress View")
    };
    assert_eq!(
        follow(&mut extension, &job, "Tea"),
        [
            (JobState::Running, Some(0.0)),
            (JobState::Succeeded, Some(1.0)),
        ]
    );
}
