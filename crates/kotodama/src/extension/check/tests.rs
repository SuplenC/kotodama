use super::*;

fn manifest(extra: &str) -> Manifest {
    Manifest::from_toml(&format!("name = \"notes\"\ncommand = [\"./notes\"]\n{extra}")).unwrap()
}

fn message(json: &str) -> FromExtension {
    WireFormat::Json.decode(json.as_bytes()).unwrap()
}

fn item(id: &str, kind: &str, shortcut: &str) -> String {
    format!(
        r#"{{"id":"{id}","kind":"{kind}","title":"{id}","actions":[{{"id":"open","title":"Open","shortcut":"{shortcut}"}}]}}"#
    )
}

#[test]
fn an_item_of_another_extensions_kind_is_rejected_and_its_own_kept() {
    let mut seen = Seen::default();
    let findings = seen.take(
        &manifest(""),
        message(&format!(
            r#"{{"add_items":{{"items":[{},{}]}}}}"#,
            item("a", "notes.note", "ctrl+o"),
            item("b", "todo.task", "ctrl+o")
        )),
    );
    assert_eq!(
        findings,
        [Finding::Rejected(
            "Item `b` has another Extension's kind `todo.task`; name your own kinds `notes.<kind>`".into()
        )]
    );
    assert_eq!(seen.items.keys().collect::<Vec<_>>(), ["a"]);
}

#[test]
fn a_reserved_shortcut_is_rejected() {
    let findings = Seen::default().take(
        &manifest(""),
        message(&format!(
            r#"{{"add_items":{{"items":[{}]}}}}"#,
            item("a", "notes.note", "Ctrl+K")
        )),
    );
    assert!(matches!(&findings[..], [Finding::Rejected(reason)] if reason.contains("`Ctrl+K`")));
}

#[test]
fn live_results_answer_a_live_query_until_done() {
    let manifest = manifest("[live_query]\nprefixes = [\"n \"]\ndebounce_ms = 100");
    let mut seen = Seen::default();
    let results = |done: bool| {
        message(&format!(
            r#"{{"live_results":{{"live_query":1,"done":{done},"results":[{{"item":{},"score":0.5,"answer":true}}]}}}}"#,
            item("a", "notes.note", "ctrl+o")
        ))
    };

    let unasked = seen.take(&manifest, results(true));
    assert!(matches!(&unasked[..], [Finding::Problem(problem)] if problem.contains("didn't send")));

    seen.live.insert(1, Vec::new());
    let answered = seen.take(&manifest, results(true));
    assert!(matches!(&answered[..], [Finding::Rejected(reason)] if reason.contains("debounce_ms")));
    assert!(seen.finished.contains(&1));
    assert_eq!(seen.live[&1].len(), 1);

    let after_done = seen.take(&manifest, results(false));
    assert!(matches!(&after_done[..], [Finding::Problem(_)]));
}

#[test]
fn an_action_is_answered_once() {
    let mut seen = Seen::default();
    seen.invocations.insert(7, None);
    let done = message(r#"{"action_done":{"invocation":7,"outcome":"close"}}"#);
    assert_eq!(seen.take(&manifest(""), done.clone()), []);
    assert_eq!(seen.invocations[&7], Some(Outcome::Close));
    assert!(matches!(&seen.take(&manifest(""), done)[..], [Finding::Problem(_)]));
}

#[test]
fn the_action_catalog_needs_the_manifest_to_ask_for_it() {
    let request = message(r#""request_action_catalog""#);
    assert!(matches!(
        &Seen::default().take(&manifest(""), request.clone())[..],
        [Finding::Rejected(_)]
    ));
    assert_eq!(Seen::default().take(&manifest("action_catalog = true"), request), []);
}

#[test]
fn an_action_is_targeted_only_when_the_manifest_declares_it_targeted() {
    let target = message(r#"{"target_action": {"action": "remove", "identities": ["app:gimp.desktop"]}}"#);
    let by_kind = manifest("[[contributed_actions]]\nkind = \"app\"\nid = \"remove\"\ntitle = \"Remove\"");
    let targeted = manifest("[[contributed_actions]]\ntargeted = true\nid = \"remove\"\ntitle = \"Remove\"");

    assert_eq!(
        Seen::default().take(&by_kind, target.clone()),
        [Finding::Rejected(untargeted("remove"))]
    );
    assert_eq!(Seen::default().take(&targeted, target), []);
}

#[test]
fn a_frame_that_isnt_one_is_explained() {
    assert!(plain(&Error::BadFrameHeader).contains("stderr"));
}

#[test]
fn a_directory_without_a_manifest_fails_the_check() {
    let dir = tempfile::tempdir().unwrap();
    let mut out = Vec::new();
    assert!(!check(&Options::new(dir.path()), &mut out));
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("manifest.toml"), "{out}");
    assert!(out.ends_with("1 problem, 0 warnings.\n"), "{out}");
}

#[test]
fn a_program_on_path_is_not_one_the_daemon_runs() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("manifest.toml"),
        "name = \"notes\"\ncommand = [\"python3\", \"notes.py\"]",
    )
    .unwrap();
    let mut out = Vec::new();
    assert!(!check(&Options::new(dir.path()), &mut out));
    assert!(
        String::from_utf8(out)
            .unwrap()
            .contains("never looks a program up on PATH")
    );
}

#[test]
fn a_few_names_are_listed() {
    assert_eq!(listed(&["a", "b"]), "`a`, `b`");
    assert_eq!(
        listed(&["a", "b", "c", "d", "e", "f", "g"]),
        "`a`, `b`, `c`, `d`, `e`, and 2 more"
    );
}
