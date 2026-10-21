//! Drives the TypeScript example Extension the way the Daemon will: over its
//! stdio in the protobuf Wire format, through the handshake, a Live query and
//! its Actions. Needs Node 23.6 or later, which the dev environment provides.

use std::collections::BTreeMap;

use kotodama_protocol::*;

#[path = "support/example.rs"]
mod example;

use example::Example;

const EXAMPLE: &str = "typescript-extension";

fn started() -> Example {
    let mut example = Example::start(EXAMPLE);
    example.send(ToExtension::Config(BTreeMap::new()));
    example
}

fn titles(example: &mut Example, id: u64, query: &str) -> Vec<String> {
    example.send(ToExtension::LiveQuery {
        id,
        query: query.into(),
        prefix: Some("ts ".into()),
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
    assert!(done);
    results
        .into_iter()
        .map(|result| result.item.title)
        .collect()
}

#[test]
fn the_typescript_example_settles_on_protobuf() {
    let example = Example::start(EXAMPLE);
    assert_eq!(example.format(), WireFormat::Protobuf);
}

#[test]
fn the_typescript_example_shows_a_timestamp_as_a_date_and_a_date_as_a_timestamp() {
    let mut example = started();

    assert_eq!(
        titles(&mut example, 1, "1700000000"),
        [
            "2023-11-14T22:13:20.000Z",
            "1700000000",
            "Tue, 14 Nov 2023 22:13:20 GMT"
        ]
    );
    assert_eq!(titles(&mut example, 2, "2024-02-29")[1], "1709164800");
    assert_eq!(titles(&mut example, 3, "not a date"), Vec::<String>::new());
}

#[test]
fn the_typescript_example_copies_or_types_the_form_picked() {
    let mut example = started();
    example.send(ToExtension::LiveQuery {
        id: 9,
        query: "0".into(),
        prefix: Some("ts ".into()),
    });
    let FromExtension::LiveResults { results, .. } = example.recv() else {
        panic!("expected live_results")
    };
    let iso = &results[0].item;
    assert_eq!(iso.kind.name(), "timestamp.time");
    assert!(results[0].score.get() > results[1].score.get());

    for (action, outcome) in [
        (
            "copy",
            Outcome::Copy(Clipboard::Text("1970-01-01T00:00:00.000Z".into())),
        ),
        (
            "type",
            Outcome::Type {
                text: "1970-01-01T00:00:00.000Z".into(),
            },
        ),
    ] {
        example.send(ToExtension::RunAction {
            invocation: 1 << 40,
            action: action.into(),
            item: iso.to_ref(),
            arguments: BTreeMap::new(),
        });
        assert_eq!(
            example.recv(),
            FromExtension::ActionDone {
                invocation: 1 << 40,
                outcome,
            }
        );
    }
}

#[test]
fn the_typescript_example_exits_when_the_daemon_hangs_up() {
    let example = started();
    assert!(example.hang_up());
}
