use kotodama_protocol::Argument;
use kotodama_protocol::KindName;
use serde_json::json;

use super::*;

fn catalog() -> Vec<CatalogEntry> {
    let name = Argument {
        name: "name".into(),
        placeholder: None,
        optional: false,
    };
    vec![
        CatalogEntry {
            extension: "apps".into(),
            kind: KindName::new("app").expect("a shared kind"),
            action: Action::new("open", "Open"),
        },
        CatalogEntry {
            extension: "suites".into(),
            kind: KindName::new("suites.suite").expect("a namespaced kind"),
            action: Action::new("new-go-project", "New Go project").arguments(vec![name]),
        },
    ]
}

fn answer(input: serde_json::Value) -> Vec<u8> {
    json!({
        "stop_reason": "tool_use",
        "content": [
            { "type": "thinking", "thinking": "" },
            { "type": "tool_use", "id": "t", "name": TOOL, "input": input },
        ],
    })
    .to_string()
    .into_bytes()
}

#[test]
fn proposals_outside_the_catalog_and_arguments_an_action_does_not_take_are_dropped() {
    let answer = answer(json!({ "proposals": [
        { "action": 1, "arguments": [
            { "name": "name", "value": " demo " },
            { "name": "path", "value": "/etc" },
        ] },
        { "action": 9, "arguments": [] },
        { "action": 1, "arguments": [{ "name": "name", "value": "demo" }] },
    ] }));

    let proposals = parse_response(&answer, &catalog()).expect("read the answer");

    let expected = Proposal {
        entry: catalog()[1].clone(),
        arguments: BTreeMap::from([("name".into(), "demo".into())]),
    };
    assert_eq!(proposals, [expected]);
    assert_eq!(
        proposals[0].item("1-0".into()).title,
        "New Go project (name: demo)"
    );
}

#[test]
fn a_refusal_proposes_nothing() {
    let refused = json!({ "stop_reason": "refusal", "content": [] }).to_string();

    let proposals = parse_response(refused.as_bytes(), &catalog()).expect("read the answer");

    assert_eq!(proposals, []);
}

#[test]
fn an_answer_that_is_not_json_is_an_error() {
    assert!(matches!(
        parse_response(b"<html>", &catalog()),
        Err(ParseError::NotJson(_))
    ));
}

#[test]
fn a_proposal_id_names_its_request_and_place() {
    assert_eq!(parse_proposal_id(&proposal_id(12, 3)), Some((12, 3)));
    assert_eq!(parse_proposal_id("nope"), None);
}
