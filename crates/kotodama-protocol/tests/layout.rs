//! Pins the encoding of every message under each protocol version, so a
//! change can't ship without a new version, and a new minor version can't
//! break a peer on an older one.
//!
//! `layouts/<major>.<minor>.txt` holds, for one version, the postcard,
//! protobuf, MessagePack and JSON encodings of a sample of every message:
//! every variant of every enum in one, and every field set to a value that
//! isn't its default. A file is written once, when its version is new, and
//! never again. Against the current version's file, the test checks that this
//! build encodes every sample exactly as pinned. Against an older minor
//! version's, it checks that this build still decodes what that version sent,
//! and that what it sends a peer on that version, in the formats such a peer
//! may be on, is byte for byte what that version encoded: so the older peer
//! reads it. When a message changes:
//!
//! 1. raise `PROTOCOL_VERSION` in `src/version.rs`: the minor version when the
//!    change only adds, and mark what is new with [`Since`] or `unread`;
//! 2. give anything new a sample here;
//! 3. rerun this test with `KOTODAMA_BLESS_LAYOUT=1`, which writes the new
//!    version's file. It never writes over one that exists.
//!
//! The samples are written out in full, never with `..`, so a new field stops
//! this file compiling until it has a value. A variant without a sample fails
//! the test.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Write as _};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use kotodama_protocol::*;
use serde::Serialize;
use serde::de::DeserializeOwned;

#[path = "support/schema.rs"]
mod schema;

const BLESS: &str = "KOTODAMA_BLESS_LAYOUT";

const FORMATS: [(WireFormat, &str); 4] = [
    (WireFormat::Postcard, "postcard"),
    (WireFormat::Protobuf, "protobuf"),
    (WireFormat::MessagePack, "msgpack"),
    (WireFormat::Json, "json"),
];

fn layouts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/layouts")
}

/// The versions with a pinned layout, and where each one's file is.
fn pinned_versions() -> Vec<(Version, PathBuf)> {
    let mut versions: Vec<_> = std::fs::read_dir(layouts_dir())
        .map(|dir| {
            dir.filter_map(|entry| {
                let path = entry.ok()?.path();
                let (major, minor) = path.file_stem()?.to_str()?.split_once('.')?;
                Some((Version::new(major.parse().ok()?, minor.parse().ok()?), path))
            })
            .collect()
        })
        .unwrap_or_default();
    versions.sort();
    versions
}

#[test]
fn the_layout_of_every_message_is_the_one_pinned_for_this_protocol_version() {
    let actual = layout(&samples(), PROTOCOL_VERSION);
    let path = layouts_dir().join(format!("{PROTOCOL_VERSION}.txt"));
    let pinned = std::fs::read_to_string(&path).ok();
    let bless = std::env::var_os(BLESS).is_some();
    match check(pinned.as_deref(), &actual, PROTOCOL_VERSION, bless) {
        Verdict::Same => {}
        Verdict::Write => {
            std::fs::create_dir_all(layouts_dir()).unwrap();
            std::fs::write(&path, &actual).unwrap();
        }
        Verdict::Fail(message) => panic!("{message}"),
    }
}

#[test]
fn what_the_daemon_sends_a_peer_on_an_older_minor_version_is_what_that_version_reads() {
    let samples = samples();
    let mut failures = Vec::new();
    for (version, path) in pinned_versions() {
        if version.major == PROTOCOL_VERSION.major && version < PROTOCOL_VERSION {
            failures.extend(check_older(
                version,
                &std::fs::read_to_string(path).unwrap(),
                &samples,
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_major_version_from_the_first_release_on_has_a_pinned_layout() {
    let pinned: Vec<Version> = pinned_versions()
        .into_iter()
        .map(|(version, _)| version)
        .collect();
    assert!(
        pinned.contains(&FIRST_VERSION),
        "no layout pinned for {FIRST_VERSION}: {pinned:?}"
    );
}

#[derive(Debug, PartialEq)]
enum Verdict {
    Same,
    Write,
    Fail(String),
}

/// Compares the layout pinned for `version`, if any, with this build's. A
/// layout is only written for a version that has none.
fn check(pinned: Option<&str>, actual: &str, version: Version, bless: bool) -> Verdict {
    match pinned {
        Some(pinned) if pinned == actual => Verdict::Same,
        Some(pinned) => {
            let before: BTreeSet<&str> = pinned.lines().collect();
            let changed: Vec<&str> = actual
                .lines()
                .filter(|line| !before.contains(line))
                .take(5)
                .collect();
            Verdict::Fail(format!(
                "The protocol's encoding changed, but PROTOCOL_VERSION is still {version}, whose \
                 layout is pinned. A peer built for {version} would pass the handshake and then \
                 misread messages. Raise PROTOCOL_VERSION in crates/kotodama-protocol/src/version.rs \
                 (the minor version if the change only adds), then rerun with {BLESS}=1 to pin the \
                 new version's layout.\nFirst changed lines:\n{}",
                changed.join("\n")
            ))
        }
        None if bless => Verdict::Write,
        None => Verdict::Fail(format!(
            "No layout is pinned for protocol {version}. Rerun this test with {BLESS}=1 to pin it."
        )),
    }
}

#[test]
fn a_changed_layout_under_a_pinned_version_is_refused_even_when_blessing() {
    let pinned = "protocol version 1.0\nItem postcard 00\n";
    let changed = "protocol version 1.0\nItem postcard 00 01\n";
    for bless in [false, true] {
        let Verdict::Fail(message) = check(Some(pinned), changed, Version::new(1, 0), bless) else {
            panic!("a changed layout under a pinned version should fail");
        };
        assert!(message.contains("Raise PROTOCOL_VERSION"), "{message}");
        assert!(message.contains("Item postcard 00 01"), "{message}");
    }
}

#[test]
fn a_layout_is_pinned_only_for_a_version_without_one() {
    let layout = "protocol version 1.1\nItem postcard 00 01\n";
    let version = Version::new(1, 1);
    assert_eq!(check(None, layout, version, true), Verdict::Write);
    assert!(matches!(
        check(None, layout, version, false),
        Verdict::Fail(_)
    ));
    assert_eq!(check(Some(layout), layout, version, false), Verdict::Same);
}

/// `kotodama.proto` describes this version's messages, for other languages'
/// protobuf code generators. Blessing the layout rewrites it.
#[test]
fn the_published_protobuf_schema_is_this_versions() {
    let mut schema = schema::Schema::default();
    for sample in samples() {
        (sample.add_to_schema)(&mut schema);
    }
    // The one ItemRef sample has one kind's field; the others are left out.
    for item in items() {
        schema.add(&ItemRef {
            id: item.id,
            kind: item.kind,
        });
    }
    let header = format!(
        "// The Kotodama protocol {PROTOCOL_VERSION} as Protocol Buffers, for the `protobuf` Wire format.\n\
         // Written by crates/kotodama-protocol/tests/layout.rs from the protocol's types; don't edit.\n\
         //\n\
         // Each frame (an ASCII length, a newline, then the payload) holds one message.\n\
         // An Extension reads ToExtension and writes FromExtension; a Front end reads\n\
         // ToFrontEnd and writes FromFrontEnd. The hello before them is always JSON.\n\
         // A field absent from a message holds its default; unknown fields are\n\
         // skipped, and a peer on an older minor version is never sent a field or a\n\
         // variant its version lacks.\n\
         \n\
         syntax = \"proto3\";\n\
         \n\
         package kotodama.v{};\n",
        PROTOCOL_VERSION.major
    );
    let actual = schema.render(&header).unwrap_or_else(|err| panic!("{err}"));
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("kotodama.proto");
    let pinned = std::fs::read_to_string(&path).unwrap_or_default();
    if pinned != actual {
        assert!(
            std::env::var_os(BLESS).is_some(),
            "kotodama.proto doesn't describe protocol {PROTOCOL_VERSION}. Rerun this test with {BLESS}=1 \
             to rewrite it."
        );
        std::fs::write(&path, &actual).unwrap();
    }
}

/// A sample message, with what the test does with it in each format.
struct Sample {
    label: String,
    /// For what the Daemon sends, the minor version that added it, which no
    /// older peer is sent. `None` for what the Daemon only receives: an older
    /// peer never sends what its version lacks.
    since: Option<u32>,
    /// Encodes the sample for a peer on a version.
    encode: Box<dyn Fn(WireFormat, Version) -> Vec<u8>>,
    decode: Box<Decode>,
    add_to_schema: Box<dyn Fn(&mut schema::Schema)>,
}

/// Decodes bytes, and tells whether they hold the sample.
type Decode = dyn Fn(WireFormat, &[u8]) -> Result<bool, String>;

fn sample<T>(label: String, since: Option<u32>, value: T) -> Sample
where
    T: Serialize + DeserializeOwned + PartialEq + Debug + 'static,
{
    let value = Rc::new(value);
    let decoded = Rc::clone(&value);
    let described = Rc::clone(&value);
    Sample {
        label,
        since,
        add_to_schema: Box::new(move |schema| schema.add(&*described)),
        encode: Box::new(move |format, version| format.encode_for(version, &*value).unwrap()),
        decode: Box::new(move |format, bytes| {
            format
                .decode::<T>(bytes)
                .map(|back| back == *decoded)
                .map_err(|err| err.to_string())
        }),
    }
}

/// Every sample's encodings for a peer on `version`, one line each.
///
/// For the current version, each encoding is checked to decode back to its
/// sample first: some types decode through a different struct than they
/// encode with, and the pinned bytes are only worth pinning if this build
/// also reads them.
fn layout(samples: &[Sample], version: Version) -> String {
    let mut out = format!("protocol version {version}\n");
    for sample in samples {
        for (format, name) in FORMATS {
            let bytes = (sample.encode)(format, version);
            if version == PROTOCOL_VERSION {
                match (sample.decode)(format, &bytes) {
                    Ok(true) => {}
                    Ok(false) => panic!("{} decodes differently from {name}", sample.label),
                    Err(err) => panic!("{} doesn't decode from {name}: {err}", sample.label),
                }
            }
            write!(out, "{} {name}", sample.label).unwrap();
            if format == WireFormat::Json {
                write!(out, " {}", String::from_utf8(bytes).unwrap()).unwrap();
            } else {
                for byte in bytes {
                    write!(out, " {byte:02x}").unwrap();
                }
            }
            out.push('\n');
        }
    }
    out
}

/// Every encoding in a pinned layout, by label.
fn parse(layout: &str) -> BTreeMap<&str, Vec<(WireFormat, Vec<u8>)>> {
    let mut encodings: BTreeMap<&str, Vec<_>> = BTreeMap::new();
    for line in layout.lines().skip(1) {
        let mut parts = line.splitn(3, ' ');
        let (Some(label), Some(name)) = (parts.next(), parts.next()) else {
            panic!("a line without a label and a format: {line}");
        };
        let data = parts.next().unwrap_or("");
        let (format, _) = FORMATS
            .into_iter()
            .find(|(_, known)| *known == name)
            .unwrap_or_else(|| panic!("an unknown format: {line}"));
        let bytes = if format == WireFormat::Json {
            data.as_bytes().to_vec()
        } else {
            data.split(' ')
                .filter(|byte| !byte.is_empty())
                .map(|byte| u8::from_str_radix(byte, 16).unwrap())
                .collect()
        };
        encodings.entry(label).or_default().push((format, bytes));
    }
    encodings
}

/// What is wrong in talking to a peer on `version`, an older minor version of
/// this major whose layout is `pinned`.
fn check_older(version: Version, pinned: &str, samples: &[Sample]) -> Vec<String> {
    let recorded = parse(pinned);
    let mut failures = Vec::new();
    for (label, encodings) in &recorded {
        let Some(sample) = samples.iter().find(|sample| sample.label == *label) else {
            failures.push(format!(
                "{label} was in protocol {version} and is gone: a minor version only adds"
            ));
            continue;
        };
        // Postcard is only spoken between peers on the same version.
        for (format, bytes) in encodings
            .iter()
            .filter(|(format, _)| *format != WireFormat::Postcard)
        {
            if let Err(err) = (sample.decode)(*format, bytes) {
                failures.push(format!(
                    "{label} as protocol {version} sent it in {format:?} no longer decodes: {err}"
                ));
            }
            if (sample.encode)(*format, version) != *bytes {
                failures.push(format!(
                    "{label} encoded in {format:?} for a peer on protocol {version} isn't what {version} \
                     encoded: a field added since needs #[serde(default, skip_serializing_if = \
                     \"crate::version::unread::<MINOR, _>\")]"
                ));
            }
        }
    }
    for sample in samples {
        if let Some(since) = sample.since
            && !recorded.contains_key(sample.label.as_str())
            && since <= version.minor
        {
            failures.push(format!(
                "{} is new since protocol {version}, but its since() is {since}, so the Daemon would \
                 send it to a peer on {version}, which can't read it. Make since() name the minor \
                 version that added it.",
                sample.label
            ));
        }
    }
    failures
}

#[test]
fn an_older_minor_versions_layout_that_this_build_matches_passes() {
    let samples = samples();
    let pinned = layout(&samples, Version::new(PROTOCOL_VERSION.major, 0));
    assert_eq!(
        check_older(Version::new(PROTOCOL_VERSION.major, 0), &pinned, &samples),
        Vec::<String>::new()
    );
}

#[test]
fn an_older_minor_version_that_reads_an_encoding_differently_fails() {
    let samples = samples();
    let version = Version::new(PROTOCOL_VERSION.major, 0);
    let pinned = layout(&samples, version).replace(
        r#"ToExtension::cancel_live_query json {"cancel_live_query":{"id":4}}"#,
        r#"ToExtension::cancel_live_query json {"cancel_live_query":{"id":5}}"#,
    );
    let failures = check_older(version, &pinned, &samples);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].contains("ToExtension::cancel_live_query encoded in Json"),
        "{failures:?}"
    );
}

#[test]
fn a_message_an_older_minor_version_had_may_not_go() {
    let samples = samples();
    let version = Version::new(PROTOCOL_VERSION.major, 0);
    let pinned = format!("{}Gone::message json \"gone\"\n", layout(&samples, version));
    let failures = check_older(version, &pinned, &samples);
    assert_eq!(
        failures,
        ["Gone::message was in protocol 1.0 and is gone: a minor version only adds"]
    );
}

#[test]
fn a_message_new_since_an_older_minor_version_must_say_it_is_newer() {
    let samples = samples();
    let version = Version::new(PROTOCOL_VERSION.major, 0);
    let pinned: String = layout(&samples, version)
        .lines()
        .filter(|line| !line.starts_with("ToFrontEnd::close "))
        .map(|line| format!("{line}\n"))
        .collect();
    let failures = check_older(version, &pinned, &samples);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].starts_with("ToFrontEnd::close is new since protocol 1.0"),
        "{failures:?}"
    );
}

/// Every sample, in the order the layout lists them.
fn samples() -> Vec<Sample> {
    let mut samples = Vec::new();
    enum_samples(&mut samples, "ToExtension", to_extension(), |m| {
        Some(m.since())
    });
    enum_samples(&mut samples, "FromExtension", from_extension(), |_| None);
    enum_samples(&mut samples, "FromFrontEnd", from_front_end(), |_| None);
    enum_samples(&mut samples, "ToFrontEnd", to_front_end(), |m| {
        Some(m.since())
    });
    enum_samples(&mut samples, "Outcome", outcomes(), |_| None);
    enum_samples(&mut samples, "Clipboard", clipboards(), |_| None);
    enum_samples(&mut samples, "ViewContent", view_contents(), |m| {
        Some(m.since())
    });
    enum_samples(&mut samples, "FormInput", form_inputs(), |m| {
        Some(m.since())
    });
    enum_samples(&mut samples, "JobState", job_states(), |m| Some(m.since()));
    // The Daemon sends Values in an Extension's config. No Value is newer than
    // the first version; one that is needs a since() to say so.
    for value in values() {
        samples.push(sample(
            format!("Value::{}", value_variant(&value)),
            Some(0),
            value,
        ));
    }
    for item in items() {
        let label = format!("Item::{}", kind_variant(&item.kind));
        samples.push(sample(label, Some(item.since()), item));
    }
    samples
}

/// A sample for each variant, under the variant's name; fails if a variant of
/// `T` has none.
fn enum_samples<T>(
    out: &mut Vec<Sample>,
    name: &str,
    values: Vec<T>,
    since: impl Fn(&T) -> Option<u32>,
) where
    T: Serialize + DeserializeOwned + PartialEq + Debug + 'static,
{
    let mut covered = BTreeSet::new();
    for value in values {
        let variant = match serde_json::to_value(&value).unwrap() {
            serde_json::Value::String(variant) => variant,
            serde_json::Value::Object(map) if map.len() == 1 => map.into_iter().next().unwrap().0,
            other => panic!("{name} isn't an externally tagged enum: {other}"),
        };
        let since = since(&value);
        out.push(sample(format!("{name}::{variant}"), since, value));
        covered.insert(variant);
    }
    let all = variants::<T>();
    assert!(!all.is_empty(), "couldn't list the variants of {name}");
    for variant in all {
        assert!(
            covered.contains(&variant),
            "{name}::{variant} has no sample in tests/layout.rs; add one, and raise \
             PROTOCOL_VERSION if the variant is new"
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
        confirm: true,
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
        app_only: true,
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
        ToExtension::Rejected {
            reason: "Item `a` has no Actions".into(),
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
        FromExtension::Status {
            usable: true,
            note: Some("Paused for an hour".into()),
        },
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
                word: "emoji".into(),
                title: "Emoji".into(),
                icon: Some("face-smile".into()),
                intro: Some(Intro {
                    about: "Emoji and symbols, typed where you were.".into(),
                    examples: vec![Example {
                        query: "fire".into(),
                        result: "🔥".into(),
                    }],
                    note: Some("Paused for an hour".into()),
                }),
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
        ToFrontEnd::Reached { menus: 1 },
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
