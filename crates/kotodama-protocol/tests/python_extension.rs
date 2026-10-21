//! Drives the Python example Extension the way the Daemon will: over its
//! stdio, through the handshake, its config and status, a Live query and its
//! Actions. Needs python3, which the dev environment provides.

use std::collections::BTreeMap;

use kotodama_protocol::*;

#[path = "support/example.rs"]
mod example;

use example::Example;

const EXAMPLE: &str = "python-extension";

fn config(cases: &[&str]) -> ToExtension {
    let cases = cases.iter().map(|case| Value::String((*case).into())).collect();
    ToExtension::Config(BTreeMap::from([("cases".into(), Value::Array(cases))]))
}

fn started() -> Example {
    let mut example = Example::start(EXAMPLE);
    example.send(ToExtension::Config(BTreeMap::new()));
    assert_eq!(
        example.recv(),
        FromExtension::Status {
            usable: true,
            note: None
        }
    );
    example
}

fn live_query(example: &mut Example, id: u64, query: &str) -> (Vec<LiveResult>, bool) {
    example.send(ToExtension::LiveQuery {
        id,
        query: query.into(),
        prefix: Some("case ".into()),
    });
    let FromExtension::LiveResults {
        live_query,
        results,
        done,
    } = example.recv()
    else {
        panic!("expected live_results")
    };
    assert_eq!(live_query, id);
    (results, done)
}

fn run(example: &mut Example, item: &Item, action: &str) -> Outcome {
    example.send(ToExtension::RunAction {
        invocation: 7,
        action: action.into(),
        item: item.to_ref(),
        arguments: BTreeMap::new(),
    });
    let FromExtension::ActionDone { invocation, outcome } = example.recv() else {
        panic!("expected action_done")
    };
    assert_eq!(invocation, 7);
    outcome
}

#[test]
fn the_python_example_settles_on_json_and_asks_for_queries_after_its_prefix() {
    let example = Example::start(EXAMPLE);
    assert_eq!(example.format(), WireFormat::Json);

    let manifest = example::manifest(EXAMPLE);
    let live = manifest.live_query.expect("Live queries");
    assert_eq!(live.prefixes, ["case "]);
    assert_eq!(manifest.word.as_deref(), Some("case"));
}

#[test]
fn the_python_example_answers_a_live_query_with_the_text_in_each_case_best_first() {
    let mut example = started();

    let (results, done) = live_query(&mut example, 3, "hello world");

    assert!(done);
    let titles: Vec<_> = results.iter().map(|result| result.item.title.as_str()).collect();
    assert_eq!(
        titles,
        ["HELLO WORLD", "hello world", "Hello World", "hello_world", "hello-world", "helloWorld"]
    );
    assert!(results.windows(2).all(|pair| pair[0].score.get() > pair[1].score.get()));
    assert!(
        results
            .iter()
            .all(|result| result.item.kind.name() == "text-case.text")
    );
}

#[test]
fn the_python_example_copies_or_types_the_result_picked() {
    let mut example = started();
    let (results, _) = live_query(&mut example, 4, "Kotodama menu");
    let snake = &results[3].item;
    assert_eq!(snake.actions[0].id, "copy");
    assert_eq!(snake.actions[1].shortcut.as_deref(), Some("ctrl+t"));

    assert_eq!(
        run(&mut example, snake, "copy"),
        Outcome::Copy(Clipboard::Text("kotodama_menu".into()))
    );
    assert_eq!(
        run(&mut example, snake, "type"),
        Outcome::Type {
            text: "kotodama_menu".into()
        }
    );
}

#[test]
fn the_python_example_offers_only_the_configured_cases_and_says_when_none_is_known() {
    let mut example = started();

    example.send(config(&["camel", "nope"]));
    assert!(matches!(example.recv(), FromExtension::Status { usable: true, .. }));
    let (results, _) = live_query(&mut example, 5, "hello world");
    let titles: Vec<_> = results.iter().map(|result| result.item.title.as_str()).collect();
    assert_eq!(titles, ["helloWorld"]);

    example.send(config(&["nope"]));
    let FromExtension::Status { usable, note } = example.recv() else {
        panic!("expected status")
    };
    assert!(!usable);
    assert!(note.expect("a note").contains("[extensions.text-case] cases"));
}

#[test]
fn the_python_example_exits_when_the_daemon_hangs_up() {
    let example = started();
    assert!(example.hang_up());
}
