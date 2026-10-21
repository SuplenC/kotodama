//! Pins the encoding of every message, so a change to it can't ship without a
//! new `PROTOCOL_VERSION` (ADR 0012).
//!
//! `layout.txt` holds the postcard and JSON encodings of a sample of every
//! message, every variant of every enum in one, and every field set to a value
//! that isn't its default. Its first line names the protocol version it belongs
//! to. When a message changes:
//!
//! 1. bump `PROTOCOL_VERSION` in `src/handshake.rs`;
//! 2. give anything new a sample here;
//! 3. rerun this test with `KOTODAMA_BLESS_LAYOUT=1`, which rewrites
//!    `layout.txt`. It refuses while the version is still the old one.
//!
//! The samples are written out in full, never with `..`, so a new field stops
//! this file compiling until it has a value. A variant without a sample fails
//! the test.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Write as _};
use std::path::{Path, PathBuf};

use kotodama_protocol::*;
use serde::Serialize;
use serde::de::DeserializeOwned;

const BLESS: &str = "KOTODAMA_BLESS_LAYOUT";

fn snapshot_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/layout.txt")
}

#[test]
fn the_layout_of_every_message_is_the_one_pinned_for_this_protocol_version() {
    let actual = layout();
    let path = snapshot_path();
    let pinned = std::fs::read_to_string(&path).unwrap_or_default();
    let bless = std::env::var_os(BLESS).is_some();
    match check(&pinned, &actual, PROTOCOL_VERSION, bless) {
        Verdict::Same => {}
        Verdict::Write => std::fs::write(&path, &actual).unwrap(),
        Verdict::Fail(message) => panic!("{message}"),
    }
}

#[derive(Debug, PartialEq)]
enum Verdict {
    Same,
    Write,
    Fail(String),
}

/// Compares the pinned layout with this build's. A changed layout may only be
/// written over the pinned one under a new protocol version.
fn check(pinned: &str, actual: &str, version: u32, bless: bool) -> Verdict {
    if pinned == actual {
        return Verdict::Same;
    }
    let pinned_version = pinned
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("protocol version "))
        .and_then(|version| version.parse::<u32>().ok());
    let same_version = pinned_version == Some(version);
    let changed: Vec<&str> = {
        let before: BTreeSet<&str> = pinned.lines().collect();
        actual
            .lines()
            .filter(|line| !before.contains(line))
            .take(5)
            .collect()
    };
    if same_version {
        return Verdict::Fail(format!(
            "The protocol's encoding changed, but PROTOCOL_VERSION is still {version}. \
             A peer built before this change would pass the handshake and then misread \
             messages. Bump PROTOCOL_VERSION in crates/kotodama-protocol/src/handshake.rs \
             (ADR 0012), then rerun with {BLESS}=1 to pin the new layout.\n\
             First changed lines:\n{}",
            changed.join("\n")
        ));
    }
    if bless {
        return Verdict::Write;
    }
    Verdict::Fail(format!(
        "PROTOCOL_VERSION is {version}, but layout.txt pins {}. Rerun this test with \
         {BLESS}=1 to pin this version's layout.",
        pinned_version.map_or("nothing".into(), |version| format!("version {version}"))
    ))
}

#[test]
fn a_changed_layout_under_the_same_version_is_refused_even_when_blessing() {
    let pinned = "protocol version 2\nItem postcard 00\n";
    let changed = "protocol version 2\nItem postcard 00 01\n";
    for bless in [false, true] {
        let Verdict::Fail(message) = check(pinned, changed, 2, bless) else {
            panic!("a changed layout under version 2 should fail");
        };
        assert!(message.contains("Bump PROTOCOL_VERSION"), "{message}");
        assert!(message.contains("Item postcard 00 01"), "{message}");
    }
}

#[test]
fn a_changed_layout_is_pinned_only_once_the_version_is_bumped() {
    let pinned = "protocol version 2\nItem postcard 00\n";
    let changed = "protocol version 3\nItem postcard 00 01\n";
    assert_eq!(check(pinned, changed, 3, true), Verdict::Write);
    assert!(matches!(check(pinned, changed, 3, false), Verdict::Fail(_)));
    assert_eq!(check(changed, changed, 3, false), Verdict::Same);
}

/// Every sample's encodings, one line each.
fn layout() -> String {
    let mut out = format!("protocol version {PROTOCOL_VERSION}\n");
    record_enum(&mut out, "ToExtension", &to_extension());
    record_enum(&mut out, "FromExtension", &from_extension());
    record_enum(&mut out, "FromFrontEnd", &from_front_end());
    record_enum(&mut out, "ToFrontEnd", &to_front_end());
    record_enum(&mut out, "Outcome", &outcomes());
    record_enum(&mut out, "Clipboard", &clipboards());
    record_enum(&mut out, "ViewContent", &view_contents());
    record_enum(&mut out, "FormInput", &form_inputs());
    record_enum(&mut out, "JobState", &job_states());
    for value in values() {
        record(
            &mut out,
            &format!("Value::{}", value_variant(&value)),
            &value,
        );
    }
    for item in items() {
        record(
            &mut out,
            &format!("Item::{}", kind_variant(&item.kind)),
            &item,
        );
    }
    out
}

/// Records a sample's encodings, after checking each decodes back to it: some
/// types decode through a different struct than they encode with, and the
/// pinned bytes are only worth pinning if this build also reads them.
fn record<T: Serialize + DeserializeOwned + PartialEq + Debug>(
    out: &mut String,
    label: &str,
    sample: &T,
) {
    let postcard = WireFormat::Postcard.encode(sample).unwrap();
    let json = String::from_utf8(WireFormat::Json.encode(sample).unwrap()).unwrap();
    for (format, bytes) in [
        (WireFormat::Postcard, postcard.as_slice()),
        (WireFormat::Json, json.as_bytes()),
    ] {
        let back: T = format
            .decode(bytes)
            .unwrap_or_else(|err| panic!("{label} doesn't decode from {format:?}: {err:?}"));
        assert_eq!(&back, sample, "{label} decodes differently from {format:?}");
    }
    write!(out, "{label} postcard").unwrap();
    for byte in postcard {
        write!(out, " {byte:02x}").unwrap();
    }
    writeln!(out, "\n{label} json {json}").unwrap();
}

/// Records each sample under its variant's name, and fails if a variant of
/// `T` has no sample.
fn record_enum<T: Serialize + DeserializeOwned + PartialEq + Debug>(
    out: &mut String,
    name: &str,
    samples: &[T],
) {
    let mut covered = BTreeSet::new();
    for sample in samples {
        let variant = match serde_json::to_value(sample).unwrap() {
            serde_json::Value::String(variant) => variant,
            serde_json::Value::Object(map) if map.len() == 1 => map.into_iter().next().unwrap().0,
            other => panic!("{name} isn't an externally tagged enum: {other}"),
        };
        record(out, &format!("{name}::{variant}"), sample);
        covered.insert(variant);
    }
    let all = variants::<T>();
    assert!(!all.is_empty(), "couldn't list the variants of {name}");
    for variant in all {
        assert!(
            covered.contains(&variant),
            "{name}::{variant} has no sample in tests/layout.rs; add one, \
             and bump PROTOCOL_VERSION if the variant is new (ADR 0012)"
        );
    }
}

/// Every variant of a derived enum, read off serde's error for a variant it
/// doesn't know: "unknown variant `x`, expected one of `a`, `b`".
fn variants<T: DeserializeOwned>() -> Vec<String> {
    let error = serde_json::from_str::<T>(r#""?""#)
        .err()
        .expect("`?` isn't a variant")
        .to_string();
    let expected = error
        .split_once("expected")
        .unwrap_or_else(|| panic!("unexpected error: {error}"))
        .1;
    expected
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

// Value and ItemKind aren't plain derived enums, so their variants are listed
// by these matches instead. A new variant stops them compiling: give it a
// sample in `values` or `items` too.

fn value_variant(value: &Value) -> &'static str {
    match value {
        Value::Bool(_) => "Bool",
        Value::Integer(_) => "Integer",
        Value::Float(_) => "Float",
        Value::String(_) => "String",
        Value::Array(_) => "Array",
        Value::Table(_) => "Table",
    }
}

fn kind_variant(kind: &ItemKind) -> &'static str {
    match kind {
        ItemKind::App { .. } => "App",
        ItemKind::File { .. } => "File",
        ItemKind::Folder { .. } => "Folder",
        ItemKind::Project { .. } => "Project",
        ItemKind::Window { .. } => "Window",
        ItemKind::Url { .. } => "Url",
        ItemKind::Namespaced(_) => "Namespaced",
    }
}

#[test]
fn every_value_and_item_kind_has_a_sample() {
    let values: BTreeSet<_> = values().iter().map(value_variant).collect();
    assert_eq!(values.len(), 6, "{values:?}");
    let kinds: BTreeSet<_> = items()
        .iter()
        .map(|item| kind_variant(&item.kind))
        .collect();
    assert_eq!(kinds.len(), 7, "{kinds:?}");
}

// The samples. Every field holds a value that isn't its default, and no two
// neighbouring fields of one type hold the same value, so dropping, adding or
// swapping a field shows in the bytes.

fn values() -> Vec<Value> {
    vec![
        Value::Bool(true),
        Value::Integer(-40),
        Value::Float(0.5),
        Value::String("hi".into()),
        Value::Array(vec![Value::Integer(1), Value::String("two".into())]),
        Value::Table(table()),
    ]
}

fn table() -> Table {
    BTreeMap::from([
        ("enabled".into(), Value::Bool(true)),
        ("limit".into(), Value::Integer(7)),
    ])
}

fn action() -> Action {
    Action {
        id: "open".into(),
        title: "Open".into(),
        shortcut: Some("ctrl+o".into()),
        arguments: vec![Argument {
            name: "version".into(),
            placeholder: Some("1.23".into()),
            optional: true,
        }],
    }
}

fn item(kind: ItemKind) -> Item {
    Item {
        id: "id".into(),
        kind,
        identity: Some("app:firefox.desktop".into()),
        title: "Title".into(),
        subtitle: Some("Subtitle".into()),
        icon: Some("icon".into()),
        keyword: Some("go".into()),
        keywords: vec!["browser".into()],
        actions: vec![action()],
        hidden: true,
    }
}

fn items() -> Vec<Item> {
    vec![
        item(app()),
        item(file()),
        item(ItemKind::Folder {
            path: PathBuf::from("/a/folder"),
        }),
        item(project()),
        item(ItemKind::Window {
            address: "0x1".into(),
        }),
        item(url()),
        item(ItemKind::Namespaced(
            NamespacedKind::new("kubaos.suite").unwrap(),
        )),
    ]
}

fn item_ref() -> ItemRef {
    ItemRef {
        id: "id".into(),
        kind: project(),
    }
}

fn app() -> ItemKind {
    ItemKind::App {
        desktop_id: "firefox.desktop".into(),
    }
}

fn file() -> ItemKind {
    ItemKind::File {
        path: PathBuf::from("/a/file"),
    }
}

fn project() -> ItemKind {
    ItemKind::Project {
        path: PathBuf::from("/a/project"),
    }
}

fn url() -> ItemKind {
    ItemKind::Url {
        url: "https://example.org".into(),
    }
}

fn fraction() -> Fraction {
    Fraction::new(0.75).unwrap()
}

fn arguments() -> BTreeMap<String, String> {
    BTreeMap::from([("version".into(), "1.23".into())])
}

fn view() -> View {
    View {
        id: "view".into(),
        title: "A View".into(),
        content: ViewContent::Progress { job: "job".into() },
    }
}

fn contributed() -> BTreeMap<String, Vec<OfferedAction>> {
    BTreeMap::from([(
        "file".into(),
        vec![OfferedAction {
            extension: "pinner".into(),
            action: action(),
        }],
    )])
}

fn job() -> Job {
    Job {
        id: "job".into(),
        title: "Installing".into(),
        state: JobState::Failed,
        progress: Some(fraction()),
        message: Some("out of space".into()),
    }
}

fn to_extension() -> Vec<ToExtension> {
    vec![
        ToExtension::Config(table()),
        ToExtension::LiveQuery {
            id: 3,
            query: "1+1".into(),
            prefix: Some("=".into()),
        },
        ToExtension::CancelLiveQuery { id: 4 },
        ToExtension::RunAction {
            invocation: 5,
            action: "open".into(),
            item: item_ref(),
            arguments: arguments(),
        },
        ToExtension::SubmitForm {
            view: "view".into(),
            values: table(),
        },
        ToExtension::ViewPopped {
            view: "view".into(),
        },
        ToExtension::FilterView {
            view: "view".into(),
            text: "fir".into(),
        },
        ToExtension::ActionCatalog {
            entries: vec![CatalogEntry {
                extension: "shell".into(),
                kind: KindName::new("project").unwrap(),
                action: action(),
            }],
        },
    ]
}

fn from_extension() -> Vec<FromExtension> {
    vec![
        FromExtension::AddItems {
            items: vec![item(app())],
        },
        FromExtension::RemoveItems {
            ids: vec!["a".into(), "b".into()],
        },
        FromExtension::LiveResults {
            live_query: 3,
            results: vec![LiveResult {
                item: item(url()),
                score: fraction(),
                answer: true,
            }],
            done: true,
        },
        FromExtension::ActionDone {
            invocation: 5,
            outcome: Outcome::KeepOpen,
        },
        FromExtension::PushView(view()),
        FromExtension::UpdateView(view()),
        FromExtension::PopView {
            view: "view".into(),
        },
        FromExtension::ReplaceView {
            view: "form".into(),
            with: view(),
        },
        FromExtension::JobProgress(job()),
        FromExtension::RequestActionCatalog,
        FromExtension::WithdrawPick { invocation: 5 },
    ]
}

fn from_front_end() -> Vec<FromFrontEnd> {
    vec![
        FromFrontEnd::Query { text: "fir".into() },
        FromFrontEnd::Viewport { top: 2, rows: 9 },
        FromFrontEnd::SelectionMoved,
        FromFrontEnd::RunAction {
            result: ResultRef {
                extension: "apps".into(),
                id: "firefox".into(),
            },
            shown: 7,
            action: ActionRef {
                extension: "shell".into(),
                id: "open".into(),
            },
            arguments: arguments(),
        },
        FromFrontEnd::SubmitForm {
            extension: "shell".into(),
            view: "view".into(),
            values: table(),
        },
        FromFrontEnd::ViewPopped {
            extension: "shell".into(),
            view: "view".into(),
        },
        FromFrontEnd::FilterView {
            extension: "shell".into(),
            view: "view".into(),
            text: "fir".into(),
        },
        FromFrontEnd::Closed,
        FromFrontEnd::Open {
            query: "fir".into(),
            view: Some("view".into()),
        },
        FromFrontEnd::Toggle,
        FromFrontEnd::Undo { change: 7 },
        FromFrontEnd::Shown,
    ]
}

fn to_front_end() -> Vec<ToFrontEnd> {
    vec![
        ToFrontEnd::Results {
            version: 7,
            results: vec![ShownResult {
                extension: "apps".into(),
                item: item(app()),
                contributed: vec![OfferedAction {
                    extension: "shell".into(),
                    action: action(),
                }],
                highlights: vec![0, 2],
                alias: Some("ff".into()),
                unavailable: Some("apps isn't running".into()),
            }],
            scope: Some(Scope {
                prefix: "emoji".into(),
                title: "Emoji".into(),
                icon: Some("face-smile".into()),
            }),
        },
        ToFrontEnd::Status {
            down: vec!["shell".into()],
        },
        ToFrontEnd::Close,
        ToFrontEnd::ActionFailed {
            message: "no".into(),
        },
        ToFrontEnd::PushView {
            extension: "shell".into(),
            view: view(),
            contributed: contributed(),
        },
        ToFrontEnd::UpdateView {
            extension: "shell".into(),
            view: view(),
            contributed: contributed(),
        },
        ToFrontEnd::ReplaceView {
            extension: "shell".into(),
            view: "form".into(),
            with: view(),
            contributed: contributed(),
        },
        ToFrontEnd::FilteredView {
            extension: "shell".into(),
            view: "view".into(),
            text: "fir".into(),
            matches: vec![ListMatch {
                item: 3,
                highlights: vec![0, 2],
            }],
        },
        ToFrontEnd::PopView {
            extension: "shell".into(),
            view: "view".into(),
        },
        ToFrontEnd::JobProgress {
            extension: "shell".into(),
            job: job(),
        },
        ToFrontEnd::Open {
            query: "fir".into(),
            view: Some("view".into()),
        },
        ToFrontEnd::Toggle,
        ToFrontEnd::OfferUndo {
            change: 7,
            title: "Forgot “Firefox”".into(),
        },
        ToFrontEnd::WithdrawUndo { change: 7 },
        ToFrontEnd::SetQuery {
            text: "emoji ".into(),
        },
    ]
}

fn outcomes() -> Vec<Outcome> {
    vec![
        Outcome::Close,
        Outcome::KeepOpen,
        Outcome::Type {
            text: "typed".into(),
        },
        Outcome::Paste(Clipboard::Text("pasted".into())),
        Outcome::Copy(Clipboard::Text("copied".into())),
        Outcome::Failed {
            message: "no".into(),
        },
        Outcome::RunCatalogAction {
            extension: "kubaos".into(),
            kind: KindName::new("kubaos.suite").expect("a valid kind"),
            action: "new-project".into(),
            arguments: BTreeMap::from([("name".into(), "demo".into())]),
        },
    ]
}

fn clipboards() -> Vec<Clipboard> {
    vec![
        Clipboard::Text("text".into()),
        Clipboard::Image {
            mime: "image/png".into(),
            data: Bytes(vec![0x89, 0x50]),
        },
    ]
}

fn view_contents() -> Vec<ViewContent> {
    vec![
        ViewContent::List {
            items: vec![item(file())],
            filters_itself: true,
        },
        ViewContent::Form {
            fields: vec![FormField {
                id: "name".into(),
                label: "Name".into(),
                input: FormInput::Checkbox { default: true },
            }],
            submit: "Go".into(),
        },
        ViewContent::Detail {
            markdown: "# Hi".into(),
            metadata: vec![MetadataEntry {
                label: "Size".into(),
                value: "3 MB".into(),
            }],
        },
        ViewContent::Progress { job: "job".into() },
    ]
}

fn form_inputs() -> Vec<FormInput> {
    vec![
        FormInput::Text {
            placeholder: Some("name".into()),
            default: Some("kuba".into()),
            secret: true,
        },
        FormInput::Checkbox { default: true },
        FormInput::Select {
            options: vec![SelectOption {
                value: "a".into(),
                label: "A".into(),
            }],
            default: Some("a".into()),
        },
    ]
}

fn job_states() -> Vec<JobState> {
    vec![JobState::Running, JobState::Succeeded, JobState::Failed]
}
