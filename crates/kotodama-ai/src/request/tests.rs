use kotodama_protocol::Action;
use kotodama_protocol::Argument;
use kotodama_protocol::CatalogEntry;
use kotodama_protocol::KindName;
use serde_json::json;

use super::*;
use crate::DEFAULT_MODEL;

fn catalog() -> Vec<CatalogEntry> {
    let name = Argument {
        name: "name".into(),
        placeholder: Some("hello".into()),
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
        CatalogEntry {
            extension: NAME.into(),
            kind: KindName::new("ai.proposal").expect("a namespaced kind"),
            action: Action::new("run", "Run"),
        },
    ]
}

#[test]
fn the_request_holds_the_query_and_the_catalog_and_nothing_else() {
    let body = request_body(&Settings::default(), "go", &catalog());

    let body: serde_json::Value = serde_json::from_str(&body).expect("parse the body");
    assert_eq!(body["model"], DEFAULT_MODEL);
    assert_eq!(body["output_config"]["effort"], "low");
    let messages = body["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 1);
    let user: serde_json::Value =
        serde_json::from_str(messages[0]["content"].as_str().expect("text")).expect("parse it");
    assert_eq!(
        user,
        json!({
            "query": "go",
            "catalog": [
                { "n": 0, "title": "Open", "extension": "apps", "kind": "app", "arguments": [] },
                {
                    "n": 1, "title": "New Go project", "extension": "suites",
                    "kind": "suites.suite",
                    "arguments": [{ "name": "name", "placeholder": "hello", "optional": false }],
                },
            ],
        })
    );
}

#[test]
fn a_request_without_effort_leaves_output_config_out() {
    let settings = Settings {
        effort: None,
        ..Settings::default()
    };

    let body: serde_json::Value = serde_json::from_str(&request_body(&settings, "go", &[])).expect("parse the body");

    assert!(body.get("output_config").is_none());
}

#[test]
fn the_curl_config_quotes_the_key_and_the_body() {
    let config = curl_config(Some(&Secret::new("k\"ey")), r#"{"a":"b\n"}"#);

    assert!(config.contains(r#"header = "x-api-key: k\"ey""#));
    assert!(config.contains(r#"data-binary = "{\"a\":\"b\\n\"}""#));
}

#[test]
fn a_request_cancelled_before_it_starts_never_runs_curl() {
    let in_flight = Mutex::new(InFlight::default());
    in_flight.lock().expect("lock").cancel();

    let posted = post("/nonexistent/curl", "http://localhost", None, "{}", &in_flight);

    assert!(matches!(posted, Err(Failure::Cancelled)));
}
