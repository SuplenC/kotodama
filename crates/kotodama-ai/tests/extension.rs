//! Drives the AI Extension the way the Daemon does, over its stdio, with a
//! fake `curl` first on its `PATH`: it records what it was given and answers
//! with a canned model response. Nothing here reaches the network.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use kotodama_ai::ASK;
use kotodama_ai::RUN;
use kotodama_ai::Settings;
use kotodama_ai::request_body;
use kotodama_extension::testing::Spawned;
use kotodama_protocol::*;

const WAIT: Duration = Duration::from_secs(10);
const KEY: &str = "sk-ant-test-do-not-log-me";
const QUERY: &str = "set up a go project";

/// Records its arguments and stdin, then answers as `response` says: slowly
/// if `slow` is there, refusing the key if `unauthorized` is.
const FAKE_CURL: &str = r#"#!/bin/sh
dir=$(dirname "$0")
printf '%s\n' "$@" >"$dir/args"
cat >"$dir/request"
[ -f "$dir/slow" ] && exec sleep 5
if [ -f "$dir/unauthorized" ]; then
    echo "curl: (22) The requested URL returned error: 401" >&2
    exit 22
fi
cat "$dir/response"
"#;

fn fake_curl(dir: &Path) {
    let curl = dir.join("curl");
    fs::write(&curl, FAKE_CURL).expect("write the fake curl");
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).expect("make it executable");
}

fn start(dir: &Path, config: Table) -> Spawned {
    fake_curl(dir);
    let path = std::env::var("PATH").unwrap_or_default();
    let mut ai = Spawned::start(
        Command::new(env!("CARGO_BIN_EXE_kotodama-ai"))
            .env("PATH", format!("{}:{path}", dir.display()))
            .stderr(Stdio::piped()),
        WAIT,
    );
    ai.send(ToExtension::Config(config));
    ai
}

/// A key from `api_key_command`, and no debounce.
fn configured() -> Table {
    Table::from([
        (
            "api_key_command".into(),
            Value::Array(vec![
                Value::String("echo".into()),
                Value::String(KEY.into()),
            ]),
        ),
        ("debounce_ms".into(), Value::Integer(0)),
    ])
}

fn catalog() -> Vec<CatalogEntry> {
    let name = Argument {
        name: "name".into(),
        placeholder: Some("hello".into()),
        optional: false,
    };
    vec![
        CatalogEntry {
            extension: "apps".into(),
            kind: KindName::new("app").expect("a valid kind"),
            action: Action::new("open", "Open"),
        },
        CatalogEntry {
            extension: "kubaos".into(),
            kind: KindName::new("kubaos.suite").expect("a valid kind"),
            action: Action::new("install-go", "Install Go Suite"),
        },
        CatalogEntry {
            extension: "kubaos".into(),
            kind: KindName::new("kubaos.suite").expect("a valid kind"),
            action: Action::new("new-go-project", "New Go project").arguments(vec![name]),
        },
    ]
}

/// The model proposes installing the Go Suite, then a new project "demo".
fn canned_answer(dir: &Path) {
    let answer = r#"{
        "id": "msg_1", "type": "message", "role": "assistant", "stop_reason": "tool_use",
        "content": [
            {"type": "thinking", "thinking": ""},
            {"type": "tool_use", "id": "toolu_1", "name": "propose_actions", "input": {
                "proposals": [
                    {"action": 1, "arguments": []},
                    {"action": 2, "arguments": [{"name": "name", "value": "demo"}]}
                ]
            }}
        ]
    }"#;
    fs::write(dir.join("response"), answer).expect("write the answer");
}

fn live(ai: &mut Spawned, id: u64, query: &str, prefix: Option<&str>) {
    ai.send(ToExtension::LiveQuery {
        id,
        query: query.into(),
        prefix: prefix.map(Into::into),
    });
}

fn results(ai: &Spawned, id: u64) -> Vec<LiveResult> {
    match ai.recv() {
        FromExtension::LiveResults {
            live_query,
            results,
            done: true,
        } if live_query == id => results,
        other => panic!("expected live_results for {id}, got {other:?}"),
    }
}

fn titles(results: &[LiveResult]) -> Vec<&str> {
    results.iter().map(|r| r.item.title.as_str()).collect()
}

/// What the Extension logged, once it has exited.
fn log(mut ai: Spawned) -> String {
    ai.hang_up();
    let mut stderr = String::new();
    ai.child
        .stderr
        .take()
        .expect("stderr is piped")
        .read_to_string(&mut stderr)
        .expect("read the log");
    stderr
}

/// Reads a curl config's double-quoted value back.
fn unquote(value: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .expect("a quoted value");
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next().expect("an escaped character") {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                c => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn with_no_key_it_adds_no_results_and_makes_no_network_calls() {
    let dir = tempfile::tempdir().expect("make a temporary directory");
    canned_answer(dir.path());
    let mut ai = start(dir.path(), Table::new());

    live(&mut ai, 1, QUERY, Some("?"));
    assert_eq!(results(&ai, 1), []);
    live(&mut ai, 2, QUERY, None);
    assert_eq!(results(&ai, 2), []);
    // Not even the Action catalog is asked for.
    ai.quiet();
    assert!(!dir.path().join("request").exists(), "curl ran");
}

#[test]
fn a_question_proposes_catalog_actions_and_nothing_runs_until_the_user_confirms() {
    let dir = tempfile::tempdir().expect("make a temporary directory");
    canned_answer(dir.path());
    let mut ai = start(dir.path(), configured());
    assert_eq!(ai.recv(), FromExtension::RequestActionCatalog);
    ai.send(ToExtension::ActionCatalog { entries: catalog() });

    live(&mut ai, 1, QUERY, Some("?"));
    let proposed = results(&ai, 1);
    assert_eq!(
        titles(&proposed),
        ["Install Go Suite", "New Go project (name: demo)"]
    );
    assert!(proposed.iter().all(|r| r.item.actions[0].id == RUN));
    // Proposing ran nothing: no message but the results.
    ai.quiet();

    ai.start_run(7, RUN, proposed[1].item.to_ref(), BTreeMap::new());
    assert_eq!(
        ai.outcome(7, |_| false),
        Outcome::RunCatalogAction {
            extension: "kubaos".into(),
            kind: KindName::new("kubaos.suite").expect("a valid kind"),
            action: "new-go-project".into(),
            arguments: BTreeMap::from([("name".into(), "demo".into())]),
        }
    );
}

#[test]
fn the_request_holds_only_the_query_and_the_catalog_and_the_key_stays_off_the_command_line() {
    let dir = tempfile::tempdir().expect("make a temporary directory");
    canned_answer(dir.path());
    let mut ai = start(dir.path(), configured());
    assert_eq!(ai.recv(), FromExtension::RequestActionCatalog);
    ai.send(ToExtension::ActionCatalog { entries: catalog() });
    live(&mut ai, 1, QUERY, Some("?"));
    results(&ai, 1);

    let config = fs::read_to_string(dir.path().join("request")).expect("read what curl was given");
    let mut headers = Vec::new();
    let mut body = None;
    for line in config.lines() {
        let (key, value) = line.split_once(" = ").expect("an option");
        match key {
            "header" => headers.push(unquote(value)),
            "data-binary" => body = Some(unquote(value)),
            other => panic!("unexpected curl option {other}"),
        }
    }
    assert!(headers.contains(&format!("x-api-key: {KEY}")));
    // The body is made from the query and the catalog alone.
    let settings = Settings::from_config(&configured());
    assert_eq!(
        body.expect("a body"),
        request_body(&settings, QUERY, &catalog())
    );

    let args = fs::read_to_string(dir.path().join("args")).expect("read the arguments");
    assert!(
        !args.contains(KEY),
        "the key is in curl's arguments: {args}"
    );
    assert!(args.contains("https://api.anthropic.com/v1/messages"));
}

#[test]
fn the_key_never_shows_up_in_the_log() {
    let dir = tempfile::tempdir().expect("make a temporary directory");
    fs::write(dir.path().join("unauthorized"), "").expect("mark the fake curl");
    let mut ai = start(dir.path(), configured());
    assert_eq!(ai.recv(), FromExtension::RequestActionCatalog);
    ai.send(ToExtension::ActionCatalog { entries: catalog() });
    live(&mut ai, 1, QUERY, Some("?"));
    assert_eq!(results(&ai, 1), []);

    let log = log(ai);
    assert!(log.contains("refused the API key"), "{log}");
    assert!(!log.contains(KEY), "{log}");
}

#[test]
fn the_key_command_runs_once_and_again_after_a_new_config() {
    let dir = tempfile::tempdir().expect("make a temporary directory");
    canned_answer(dir.path());
    let reads = dir.path().join("key-reads");
    let command = format!("echo read >>'{}'; echo {KEY}", reads.display());
    let mut table = configured();
    table.insert(
        "api_key_command".into(),
        Value::Array(vec![
            Value::String("sh".into()),
            Value::String("-c".into()),
            Value::String(command),
        ]),
    );
    let mut ai = start(dir.path(), table.clone());
    assert_eq!(ai.recv(), FromExtension::RequestActionCatalog);
    ai.send(ToExtension::ActionCatalog { entries: catalog() });
    let count = || {
        fs::read_to_string(&reads)
            .expect("read the key reads")
            .lines()
            .count()
    };

    live(&mut ai, 1, QUERY, Some("?"));
    results(&ai, 1);
    live(&mut ai, 2, "new go project", Some("?"));
    results(&ai, 2);
    assert_eq!(count(), 1);

    ai.send(ToExtension::Config(table));
    live(&mut ai, 3, QUERY, Some("?"));
    results(&ai, 3);
    assert_eq!(count(), 2);
}

/// Opens the "Ask AI" View for `QUERY` with a slow curl, and waits until its
/// request is in flight.
fn view_waiting(dir: &Path) -> Spawned {
    canned_answer(dir);
    fs::write(dir.join("slow"), "").expect("mark the fake curl");
    let mut ai = start(dir, configured());
    assert_eq!(ai.recv(), FromExtension::RequestActionCatalog);
    ai.send(ToExtension::ActionCatalog { entries: catalog() });
    live(&mut ai, 1, QUERY, None);
    let ask = results(&ai, 1).remove(0).item;
    ai.start_run(1, ASK, ask.to_ref(), BTreeMap::new());
    assert!(matches!(ai.recv(), FromExtension::PushView(_)));
    assert_eq!(ai.outcome(1, |_| false), Outcome::KeepOpen);
    let deadline = Instant::now() + WAIT;
    while !dir.join("request").exists() {
        assert!(Instant::now() < deadline, "curl never ran");
        std::thread::sleep(Duration::from_millis(10));
    }
    ai
}

#[test]
fn a_new_config_while_the_view_waits_asks_again_under_it() {
    let dir = tempfile::tempdir().expect("make a temporary directory");
    let mut ai = view_waiting(dir.path());

    fs::remove_file(dir.path().join("slow")).expect("unmark the fake curl");
    ai.send(ToExtension::Config(configured()));

    match ai.recv() {
        FromExtension::UpdateView(View {
            content: ViewContent::List { items, .. },
            ..
        }) => assert_eq!(items.len(), 2),
        other => panic!("expected the Proposals, got {other:?}"),
    }
}

#[test]
fn a_config_that_turns_it_off_while_the_view_waits_says_so() {
    let dir = tempfile::tempdir().expect("make a temporary directory");
    let mut ai = view_waiting(dir.path());

    ai.send(ToExtension::Config(Table::new()));

    match ai.recv() {
        FromExtension::UpdateView(View {
            content: ViewContent::Detail { markdown, .. },
            ..
        }) => assert_eq!(markdown, "the AI isn't configured"),
        other => panic!("expected the View to say so, got {other:?}"),
    }
    ai.quiet();
}

#[test]
fn a_cancelled_question_never_answers() {
    let dir = tempfile::tempdir().expect("make a temporary directory");
    canned_answer(dir.path());
    fs::write(dir.path().join("slow"), "").expect("mark the fake curl");
    let mut ai = start(dir.path(), configured());
    assert_eq!(ai.recv(), FromExtension::RequestActionCatalog);
    ai.send(ToExtension::ActionCatalog { entries: catalog() });

    live(&mut ai, 1, QUERY, Some("?"));
    // The request is in flight once curl has its config.
    let deadline = Instant::now() + WAIT;
    while !dir.path().join("request").exists() {
        assert!(Instant::now() < deadline, "curl never ran");
        std::thread::sleep(Duration::from_millis(10));
    }
    ai.send(ToExtension::CancelLiveQuery { id: 1 });
    ai.quiet();
}

#[test]
fn typing_alone_never_reaches_the_model() {
    let dir = tempfile::tempdir().expect("make a temporary directory");
    canned_answer(dir.path());
    let mut table = configured();
    table.insert("debounce_ms".into(), Value::Integer(300));
    let mut ai = start(dir.path(), table);
    assert_eq!(ai.recv(), FromExtension::RequestActionCatalog);
    ai.send(ToExtension::ActionCatalog { entries: catalog() });

    // A query nothing matches gets the Result that asks, and asks nothing.
    live(&mut ai, 1, QUERY, None);
    assert_eq!(titles(&results(&ai, 1)), [format!("Ask AI: “{QUERY}”")]);
    // A `?` query changed inside the debounce is never sent.
    live(&mut ai, 2, "set up", Some("?"));
    ai.send(ToExtension::CancelLiveQuery { id: 2 });
    ai.quiet();
    assert!(!dir.path().join("request").exists(), "curl ran");
}

#[test]
fn the_ask_ai_result_shows_the_proposals_in_a_view_to_confirm_from() {
    let dir = tempfile::tempdir().expect("make a temporary directory");
    canned_answer(dir.path());
    let mut ai = start(dir.path(), configured());
    assert_eq!(ai.recv(), FromExtension::RequestActionCatalog);
    ai.send(ToExtension::ActionCatalog { entries: catalog() });
    live(&mut ai, 1, QUERY, None);
    let ask = results(&ai, 1).remove(0).item;

    ai.start_run(1, ASK, ask.to_ref(), BTreeMap::new());
    assert!(matches!(ai.recv(), FromExtension::PushView(_)));
    assert_eq!(ai.outcome(1, |_| false), Outcome::KeepOpen);
    let items = match ai.recv() {
        FromExtension::UpdateView(View {
            content: ViewContent::List { items, .. },
            ..
        }) => items,
        other => panic!("expected the proposals, got {other:?}"),
    };
    assert_eq!(items.len(), 2);

    ai.start_run(2, RUN, items[0].to_ref(), BTreeMap::new());
    assert!(matches!(ai.recv(), FromExtension::PopView { .. }));
    assert!(matches!(
        ai.outcome(2, |_| false),
        Outcome::RunCatalogAction { action, .. } if action == "install-go"
    ));
}
