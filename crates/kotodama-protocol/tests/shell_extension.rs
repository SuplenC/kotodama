//! Drives the shell + jq example Extension the way the Daemon will: over its
//! stdio, through the handshake, config, Items and a Contributed action. Needs
//! bash 4+ and jq, which the dev environment provides.

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
    assert_eq!(
        ids,
        first.iter().map(|item| item.id.clone()).collect::<Vec<_>>()
    );
    let FromExtension::AddItems { items } = extension.recv() else {
        panic!("expected add_items")
    };
    assert_eq!(
        items
            .iter()
            .map(|item| item.title.as_str())
            .collect::<Vec<_>>(),
        ["C"]
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
