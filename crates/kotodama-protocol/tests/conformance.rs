//! Runs every message through every Wire format and checks it comes back the
//! same, then pins down the JSON shape that Extension authors write by hand.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::PathBuf;

use kotodama_protocol::*;
use serde::Serialize;
use serde::de::DeserializeOwned;

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(message: &T) {
    for format in WireFormat::ALL {
        let bytes = format.encode(message).unwrap();
        let back: T = format.decode(&bytes).unwrap_or_else(|err| {
            panic!(
                "{format:?}: {err}: {:?}\n{message:?}",
                std::error::Error::source(&err)
            )
        });
        assert_eq!(&back, message, "{format:?}");
    }
}

fn action(id: &str) -> Action {
    Action {
        id: id.into(),
        title: id.into(),
        shortcut: None,
        arguments: vec![],
    }
}

fn items() -> Vec<Item> {
    vec![
        Item {
            id: "firefox".into(),
            kind: ItemKind::App {
                desktop_id: "firefox.desktop".into(),
            },
            title: "Firefox".into(),
            subtitle: Some("Web browser".into()),
            icon: Some("firefox".into()),
            keyword: None,
            keywords: vec!["browser".into(), "web".into()],
            actions: vec![
                action("open"),
                Action {
                    shortcut: Some("ctrl+n".into()),
                    ..action("new-window")
                },
            ],
        },
        Item {
            id: "go".into(),
            kind: ItemKind::Namespaced(NamespacedKind::new("kubaos.toolchain").unwrap()),
            title: "Go toolchain".into(),
            subtitle: None,
            icon: None,
            keyword: Some("go".into()),
            keywords: vec![],
            actions: vec![Action {
                arguments: vec![
                    Argument {
                        name: "version".into(),
                        placeholder: Some("1.23".into()),
                        optional: false,
                    },
                    Argument {
                        name: "note".into(),
                        placeholder: None,
                        optional: true,
                    },
                ],
                ..action("install")
            }],
        },
        Item {
            id: "kotodama".into(),
            kind: ItemKind::Project {
                path: PathBuf::from("/src/kotodama"),
            },
            title: "kotodama".into(),
            subtitle: None,
            icon: None,
            keyword: None,
            keywords: vec![],
            actions: vec![action("open")],
        },
    ]
}

fn table() -> Table {
    let mut nested = Table::new();
    nested.insert("depth".into(), Value::Integer(2));
    BTreeMap::from([
        ("greeting".into(), Value::String("hi".into())),
        ("enabled".into(), Value::Bool(true)),
        ("limit".into(), Value::Integer(-40)),
        ("ratio".into(), Value::Float(0.5)),
        (
            "roots".into(),
            Value::Array(vec![Value::String("~/src".into()), Value::Integer(1)]),
        ),
        ("nested".into(), Value::Table(nested)),
    ])
}

fn view(content: ViewContent) -> View {
    View {
        id: "v".into(),
        title: "A View".into(),
        content,
    }
}

fn to_extension_messages() -> Vec<ToExtension> {
    vec![
        ToExtension::Config(table()),
        ToExtension::Config(Table::new()),
        ToExtension::LiveQuery {
            id: 7,
            query: "2+2".into(),
            prefix: None,
        },
        ToExtension::LiveQuery {
            id: 8,
            query: "rust".into(),
            prefix: Some("g ".into()),
        },
        ToExtension::CancelLiveQuery { id: 7 },
        ToExtension::RunAction {
            invocation: 1,
            action: "pin-version".into(),
            item: items()[2].to_ref(),
            arguments: BTreeMap::from([("version".into(), "1.23".into())]),
        },
        ToExtension::SubmitForm {
            view: "new".into(),
            values: table(),
        },
        ToExtension::ViewPopped { view: "new".into() },
        ToExtension::ActionCatalog {
            entries: vec![CatalogEntry {
                extension: "kubaos".into(),
                kind: KindName::new("project").unwrap(),
                action: Action {
                    arguments: vec![Argument {
                        name: "version".into(),
                        placeholder: None,
                        optional: false,
                    }],
                    ..action("pin-version")
                },
            }],
        },
    ]
}

fn from_extension_messages() -> Vec<FromExtension> {
    vec![
        FromExtension::AddItems { items: items() },
        FromExtension::RemoveItems {
            ids: vec!["firefox".into(), "go".into()],
        },
        FromExtension::LiveResults {
            live_query: 7,
            results: vec![
                LiveResult {
                    item: items()[0].clone(),
                    score: Fraction::new(1.0).unwrap(),
                    answer: true,
                },
                LiveResult {
                    item: items()[1].clone(),
                    score: Fraction::new(0.25).unwrap(),
                    answer: false,
                },
            ],
            done: true,
        },
        FromExtension::ActionDone {
            invocation: 1,
            outcome: Outcome::Close,
        },
        FromExtension::ActionDone {
            invocation: 2,
            outcome: Outcome::KeepOpen,
        },
        FromExtension::ActionDone {
            invocation: 3,
            outcome: Outcome::Type {
                text: "ÿ ✓ 言霊".into(),
            },
        },
        FromExtension::ActionDone {
            invocation: 4,
            outcome: Outcome::Paste(Clipboard::Text("hello".into())),
        },
        FromExtension::ActionDone {
            invocation: 5,
            outcome: Outcome::Paste(Clipboard::Image {
                mime: "image/png".into(),
                data: Bytes(vec![0x89, b'P', 0, 255]),
            }),
        },
        FromExtension::ActionDone {
            invocation: 6,
            outcome: Outcome::Failed {
                message: "no network".into(),
            },
        },
        FromExtension::PushView(view(ViewContent::List { items: items() })),
        FromExtension::PushView(view(ViewContent::Form {
            fields: vec![
                FormField {
                    id: "name".into(),
                    label: "Name".into(),
                    input: FormInput::Text {
                        placeholder: Some("suite".into()),
                        default: None,
                        secret: false,
                    },
                },
                FormField {
                    id: "token".into(),
                    label: "Token".into(),
                    input: FormInput::Text {
                        placeholder: None,
                        default: None,
                        secret: true,
                    },
                },
                FormField {
                    id: "force".into(),
                    label: "Force".into(),
                    input: FormInput::Checkbox { default: true },
                },
                FormField {
                    id: "channel".into(),
                    label: "Channel".into(),
                    input: FormInput::Select {
                        options: vec![
                            SelectOption {
                                value: "stable".into(),
                                label: "Stable".into(),
                            },
                            SelectOption {
                                value: "beta".into(),
                                label: "Beta".into(),
                            },
                        ],
                        default: Some("stable".into()),
                    },
                },
            ],
            submit: "Create".into(),
        })),
        FromExtension::UpdateView(view(ViewContent::Detail {
            markdown: "# Title\n\nBody".into(),
            metadata: vec![MetadataEntry {
                label: "Size".into(),
                value: "4 KiB".into(),
            }],
        })),
        FromExtension::UpdateView(view(ViewContent::Progress {
            job: "install".into(),
        })),
        FromExtension::PopView { view: "v".into() },
        FromExtension::JobProgress(Job {
            id: "install".into(),
            title: "Installing Go 1.23".into(),
            state: JobState::Running,
            progress: Some(Fraction::new(0.5).unwrap()),
            message: Some("Downloading".into()),
        }),
        FromExtension::JobProgress(Job {
            id: "install".into(),
            title: "Installing Go 1.23".into(),
            state: JobState::Succeeded,
            progress: None,
            message: None,
        }),
        FromExtension::JobProgress(Job {
            id: "install".into(),
            title: "Installing Go 1.23".into(),
            state: JobState::Failed,
            progress: None,
            message: Some("disk full".into()),
        }),
        FromExtension::RequestActionCatalog,
    ]
}

/// Maps each variant to an index. The matches are exhaustive, so a new variant
/// doesn't compile until it is added here, and the coverage tests then fail
/// until it has a sample above.
fn to_extension_variant(message: &ToExtension) -> usize {
    match message {
        ToExtension::Config(_) => 0,
        ToExtension::LiveQuery { .. } => 1,
        ToExtension::CancelLiveQuery { .. } => 2,
        ToExtension::RunAction { .. } => 3,
        ToExtension::SubmitForm { .. } => 4,
        ToExtension::ViewPopped { .. } => 5,
        ToExtension::ActionCatalog { .. } => 6,
    }
}
const TO_EXTENSION_VARIANTS: usize = 7;

fn from_extension_variant(message: &FromExtension) -> usize {
    match message {
        FromExtension::AddItems { .. } => 0,
        FromExtension::RemoveItems { .. } => 1,
        FromExtension::LiveResults { .. } => 2,
        FromExtension::ActionDone { .. } => 3,
        FromExtension::PushView(_) => 4,
        FromExtension::UpdateView(_) => 5,
        FromExtension::PopView { .. } => 6,
        FromExtension::JobProgress(_) => 7,
        FromExtension::RequestActionCatalog => 8,
    }
}
const FROM_EXTENSION_VARIANTS: usize = 9;

fn to_front_end_messages() -> Vec<ToFrontEnd> {
    vec![
        ToFrontEnd::Results {
            results: vec![ResultRow {
                extension: "projects".into(),
                item: items()[2].clone(),
                contributed: vec![ExtensionAction {
                    extension: "kubaos".into(),
                    action: action("pin-version"),
                }],
                highlights: vec![0, 1, 4],
            }],
        },
        ToFrontEnd::Results { results: vec![] },
        ToFrontEnd::Status {
            down: vec!["calculator".into()],
        },
        ToFrontEnd::Close,
        ToFrontEnd::ActionFailed {
            message: "no network".into(),
        },
        ToFrontEnd::PushView {
            extension: "kubaos".into(),
            view: view(ViewContent::Detail {
                markdown: "# Hi".into(),
                metadata: vec![],
            }),
        },
        ToFrontEnd::UpdateView {
            extension: "kubaos".into(),
            view: view(ViewContent::Progress { job: "j".into() }),
        },
        ToFrontEnd::PopView {
            extension: "kubaos".into(),
            view: "v".into(),
        },
        ToFrontEnd::JobProgress {
            extension: "kubaos".into(),
            job: Job {
                id: "j".into(),
                title: "Installing".into(),
                state: JobState::Running,
                progress: Some(Fraction::new(0.5).unwrap()),
                message: None,
            },
        },
    ]
}

fn from_front_end_messages() -> Vec<FromFrontEnd> {
    vec![
        FromFrontEnd::Query { text: "fir".into() },
        FromFrontEnd::Viewport { rows: 8 },
        FromFrontEnd::RunAction {
            result: ResultRef {
                extension: "projects".into(),
                id: "kotodama".into(),
            },
            action: ActionRef {
                extension: "kubaos".into(),
                id: "pin-version".into(),
            },
            arguments: BTreeMap::from([("version".into(), "1.23".into())]),
        },
        FromFrontEnd::SubmitForm {
            extension: "kubaos".into(),
            view: "new".into(),
            values: table(),
        },
        FromFrontEnd::ViewPopped {
            extension: "kubaos".into(),
            view: "new".into(),
        },
        FromFrontEnd::Closed,
    ]
}

fn to_front_end_variant(message: &ToFrontEnd) -> usize {
    match message {
        ToFrontEnd::Results { .. } => 0,
        ToFrontEnd::Status { .. } => 1,
        ToFrontEnd::Close => 2,
        ToFrontEnd::ActionFailed { .. } => 3,
        ToFrontEnd::PushView { .. } => 4,
        ToFrontEnd::UpdateView { .. } => 5,
        ToFrontEnd::PopView { .. } => 6,
        ToFrontEnd::JobProgress { .. } => 7,
    }
}
const TO_FRONT_END_VARIANTS: usize = 8;

fn from_front_end_variant(message: &FromFrontEnd) -> usize {
    match message {
        FromFrontEnd::Query { .. } => 0,
        FromFrontEnd::Viewport { .. } => 1,
        FromFrontEnd::RunAction { .. } => 2,
        FromFrontEnd::SubmitForm { .. } => 3,
        FromFrontEnd::ViewPopped { .. } => 4,
        FromFrontEnd::Closed => 5,
    }
}
const FROM_FRONT_END_VARIANTS: usize = 6;

#[test]
fn every_message_to_a_front_end_survives_every_wire_format() {
    let messages = to_front_end_messages();
    let mut covered: Vec<usize> = messages.iter().map(to_front_end_variant).collect();
    covered.dedup();
    assert_eq!(
        covered,
        (0..TO_FRONT_END_VARIANTS).collect::<Vec<_>>(),
        "a variant has no sample"
    );
    messages.iter().for_each(round_trip);
}

#[test]
fn every_message_from_a_front_end_survives_every_wire_format() {
    let messages = from_front_end_messages();
    let mut covered: Vec<usize> = messages.iter().map(from_front_end_variant).collect();
    covered.dedup();
    assert_eq!(
        covered,
        (0..FROM_FRONT_END_VARIANTS).collect::<Vec<_>>(),
        "a variant has no sample"
    );
    messages.iter().for_each(round_trip);
}

#[test]
fn every_message_to_an_extension_survives_every_wire_format() {
    let messages = to_extension_messages();
    let mut covered: Vec<usize> = messages.iter().map(to_extension_variant).collect();
    covered.dedup();
    assert_eq!(
        covered,
        (0..TO_EXTENSION_VARIANTS).collect::<Vec<_>>(),
        "a variant has no sample"
    );
    messages.iter().for_each(round_trip);
}

#[test]
fn every_message_from_an_extension_survives_every_wire_format() {
    let messages = from_extension_messages();
    let mut covered: Vec<usize> = messages.iter().map(from_extension_variant).collect();
    covered.dedup();
    assert_eq!(
        covered,
        (0..FROM_EXTENSION_VARIANTS).collect::<Vec<_>>(),
        "a variant has no sample"
    );
    messages.iter().for_each(round_trip);
}

#[test]
fn the_handshake_survives_every_wire_format() {
    round_trip(&Hello::new(&WireFormat::ALL));
}

/// Other languages read MessagePack with a generic decoder, so it must have the
/// same shape as JSON: named fields, variant names as keys or strings.
#[test]
fn messagepack_has_the_same_shape_as_json() {
    fn same_shape<T: Serialize + Debug>(message: &T) {
        let bytes = WireFormat::MessagePack.encode(message).unwrap();
        let generic: serde_json::Value = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(generic, json(message), "{message:?}");
    }
    to_extension_messages().iter().for_each(same_shape);
    from_extension_messages().iter().for_each(same_shape);
    to_front_end_messages().iter().for_each(same_shape);
    from_front_end_messages().iter().for_each(same_shape);
}

fn json<T: Serialize>(message: &T) -> serde_json::Value {
    serde_json::from_slice(&WireFormat::Json.encode(message).unwrap()).unwrap()
}

#[test]
fn json_messages_are_tagged_by_a_snake_case_key() {
    assert_eq!(
        json(&FromExtension::RemoveItems {
            ids: vec!["a".into()]
        }),
        serde_json::json!({"remove_items": {"ids": ["a"]}})
    );
    assert_eq!(
        json(&FromExtension::RequestActionCatalog),
        serde_json::json!("request_action_catalog")
    );
    assert_eq!(
        json(&FromExtension::ActionDone {
            invocation: 1,
            outcome: Outcome::Close
        }),
        serde_json::json!({"action_done": {"invocation": 1, "outcome": "close"}})
    );
}

#[test]
fn config_reaches_a_json_extension_as_a_plain_object() {
    assert_eq!(
        json(&ToExtension::Config(table())),
        serde_json::json!({"config": {
            "greeting": "hi",
            "enabled": true,
            "limit": -40,
            "ratio": 0.5,
            "roots": ["~/src", 1],
            "nested": {"depth": 2},
        }})
    );
}

#[test]
fn an_extension_may_leave_out_optional_fields() {
    let message: FromExtension = WireFormat::Json
        .decode(
            br#"{"add_items": {"items": [{"id": "h", "kind": "folder", "path": "/home", "title": "Home",
                "actions": [{"id": "open", "title": "Open"}]}]}}"#,
        )
        .unwrap();
    let FromExtension::AddItems { items } = message else {
        panic!("{message:?}")
    };
    assert_eq!(items[0].subtitle, None);
    assert_eq!(items[0].keywords, Vec::<String>::new());
    assert_eq!(items[0].actions[0].arguments, vec![]);

    let message: FromExtension = WireFormat::Json
        .decode(br#"{"live_results": {"live_query": 3, "results": [{"item": {"id": "4", "kind": "calc.answer", "title": "4",
                "actions": [{"id": "copy", "title": "Copy"}]}, "score": 1}]}}"#)
        .unwrap();
    let FromExtension::LiveResults { results, done, .. } = message else {
        panic!("{message:?}")
    };
    assert!(!results[0].answer);
    assert!(!done);
}

#[test]
fn image_bytes_travel_as_base64_in_json() {
    let message = FromExtension::ActionDone {
        invocation: 9,
        outcome: Outcome::Paste(Clipboard::Image {
            mime: "image/png".into(),
            data: Bytes(b"PNG".to_vec()),
        }),
    };
    assert_eq!(
        json(&message),
        serde_json::json!({"action_done": {"invocation": 9, "outcome": {"paste": {"image": {"mime": "image/png", "data": "UE5H"}}}}})
    );
}

#[test]
fn a_score_outside_zero_to_one_is_rejected() {
    for score in ["1.5", "-0.1"] {
        let bytes = format!(
            r#"{{"live_results": {{"live_query": 3, "results": [{{"item": {{"id": "4", "kind": "calc.answer", "title": "4",
                "actions": [{{"id": "copy", "title": "Copy"}}]}}, "score": {score}}}], "done": true}}}}"#
        );
        assert!(
            WireFormat::Json
                .decode::<FromExtension>(bytes.as_bytes())
                .is_err(),
            "{score}"
        );
    }
    assert!(Fraction::new(f32::NAN).is_err());
}

#[test]
fn config_values_json_cannot_express_are_rejected() {
    assert!(
        WireFormat::Json
            .decode::<ToExtension>(br#"{"config": {"x": null}}"#)
            .is_err()
    );
    assert!(
        WireFormat::Json
            .decode::<ToExtension>(br#"{"config": {"x": 18446744073709551615}}"#)
            .is_err()
    );
}
