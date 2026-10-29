use std::path::PathBuf;

use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::ItemRef;
use kotodama_protocol::WireFormat;

fn decode(json: &str) -> Result<Item, String> {
    WireFormat::Json.decode::<Item>(json.as_bytes()).map_err(|err| {
        let source = std::error::Error::source(&err)
            .map(ToString::to_string)
            .unwrap_or_default();
        format!("{err}: {source}")
    })
}

/// An Item with the given kind and kind fields, and everything else valid.
fn item_json(kind_and_fields: &str) -> String {
    format!(r#"{{"id": "x", {kind_and_fields}, "title": "X", "actions": [{{"id": "open", "title": "Open"}}]}}"#)
}

#[test]
fn a_shared_kind_item_carries_its_field() {
    let cases = [
        (
            r#""kind": "app", "desktop_id": "firefox.desktop""#,
            ItemKind::App {
                desktop_id: "firefox.desktop".into(),
            },
        ),
        (
            r#""kind": "file", "path": "/etc/hosts""#,
            ItemKind::File {
                path: PathBuf::from("/etc/hosts"),
            },
        ),
        (
            r#""kind": "folder", "path": "/etc""#,
            ItemKind::Folder {
                path: PathBuf::from("/etc"),
            },
        ),
        (
            r#""kind": "project", "path": "/src/kotodama""#,
            ItemKind::Project {
                path: PathBuf::from("/src/kotodama"),
            },
        ),
        (
            r#""kind": "window", "address": "0x5f2a""#,
            ItemKind::Window {
                address: "0x5f2a".into(),
            },
        ),
        (
            r#""kind": "url", "url": "https://example.org""#,
            ItemKind::Url {
                url: "https://example.org".into(),
            },
        ),
    ];
    for (json, kind) in cases {
        assert_eq!(decode(&item_json(json)).unwrap().kind, kind, "{json}");
    }
}

#[test]
fn a_shared_kind_item_without_its_field_is_rejected() {
    for (kind, field) in [
        ("app", "desktop_id"),
        ("file", "path"),
        ("folder", "path"),
        ("project", "path"),
        ("window", "address"),
        ("url", "url"),
    ] {
        let err = decode(&item_json(&format!(r#""kind": "{kind}""#))).unwrap_err();
        assert!(err.contains(&format!("`{kind}` Item needs `{field}`")), "{kind}: {err}");
    }
}

#[test]
fn a_field_that_belongs_to_another_kind_is_rejected() {
    let err = decode(&item_json(
        r#""kind": "file", "path": "/etc/hosts", "url": "file:///etc/hosts""#,
    ))
    .unwrap_err();
    assert!(err.contains("`file` Item can't have `url`"), "{err}");
}

#[test]
fn a_namespaced_kind_belongs_to_the_extension_named_before_the_dot() {
    let item = decode(&item_json(r#""kind": "kubaos.suite""#)).unwrap();
    assert_eq!(item.kind.name(), "kubaos.suite");
    assert_eq!(item.kind.extension(), Some("kubaos"));
}

#[test]
fn an_unknown_kind_without_a_namespace_is_rejected() {
    let err = decode(&item_json(r#""kind": "suite""#)).unwrap_err();
    assert!(err.contains("unknown Item kind `suite`"), "{err}");
}

#[test]
fn a_malformed_namespaced_kind_is_rejected() {
    for kind in [
        ".suite",
        "kubaos.",
        "kubaos.suite.extra",
        "KubaOS.suite",
        "kuba os.suite",
        "kubaos.app!",
    ] {
        let err = decode(&item_json(&format!(r#""kind": "{kind}""#))).unwrap_err();
        assert!(err.contains("Item kind"), "{kind}: {err}");
    }
}

#[test]
fn an_item_without_an_action_is_rejected() {
    let err =
        decode(r#"{"id": "x", "kind": "url", "url": "https://example.org", "title": "X", "actions": []}"#).unwrap_err();
    assert!(err.contains("primary Action"), "{err}");
}

#[test]
fn an_item_may_name_the_thing_it_stands_for() {
    let item = decode(
        r#"{"id": "x", "kind": "url", "url": "https://example.org", "identity": "url:https://example.org",
            "title": "X", "actions": [{"id": "open", "title": "Open"}]}"#,
    )
    .unwrap();
    assert_eq!(item.identity.as_deref(), Some("url:https://example.org"));

    let item = decode(&item_json(r#""kind": "url", "url": "https://example.org""#)).unwrap();
    assert_eq!(item.identity, None, "an identity is optional");

    let err = decode(
        r#"{"id": "x", "kind": "url", "url": "https://example.org", "identity": "",
            "title": "X", "actions": [{"id": "open", "title": "Open"}]}"#,
    )
    .unwrap_err();
    assert!(err.contains("identity"), "{err}");
}

#[test]
fn an_item_that_yields_needs_an_identity_to_yield_to() {
    let item = decode(
        r#"{"id": "x", "kind": "url", "url": "https://example.org", "identity": "url:https://example.org",
            "yields": true, "title": "X", "actions": [{"id": "open", "title": "Open"}]}"#,
    )
    .unwrap();
    assert!(item.yields);

    let item = decode(&item_json(r#""kind": "url", "url": "https://example.org""#)).unwrap();
    assert!(!item.yields, "an Item doesn't yield unless it says so");

    let err = decode(
        r#"{"id": "x", "kind": "url", "url": "https://example.org", "yields": true,
            "title": "X", "actions": [{"id": "open", "title": "Open"}]}"#,
    )
    .unwrap_err();
    assert!(err.contains("identity"), "{err}");
}

#[test]
fn an_item_reference_carries_the_kind_fields_but_not_the_title() {
    let reference: ItemRef = WireFormat::Json
        .decode(br#"{"id": "kotodama", "kind": "project", "path": "/src/kotodama"}"#)
        .unwrap();
    assert_eq!(
        reference.kind,
        ItemKind::Project {
            path: PathBuf::from("/src/kotodama")
        }
    );

    let err = WireFormat::Json
        .decode::<ItemRef>(br#"{"id": "kotodama", "kind": "project"}"#)
        .unwrap_err();
    assert!(
        std::error::Error::source(&err)
            .unwrap()
            .to_string()
            .contains("`project` Item needs `path`")
    );
}
