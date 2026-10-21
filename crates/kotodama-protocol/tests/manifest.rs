use kotodama_protocol::{Action, Argument, KindName, Manifest};

fn parse(toml: &str) -> Result<Manifest, String> {
    Manifest::from_toml(toml).map_err(|err| err.to_string())
}

#[test]
fn a_manifest_declares_everything_an_extension_contributes() {
    let manifest = parse(
        r#"
        name = "kubaos"
        description = "KubaOS suites and toolchains"
        command = ["bin/kubaos-kotodama", "--stdio"]
        action_catalog = true

        [live_query]
        prefixes = ["suite "]
        triggers = ['^\d+\.\d+$']
        debounce_ms = 120

        [[contributed_actions]]
        kind = "project"
        id = "pin-version"
        title = "Pin a version here"
        shortcut = "ctrl+p"
        arguments = [{ name = "version", placeholder = "1.23" }]

        [[contributed_actions]]
        kind = "folder"
        id = "open-suite"
        title = "Open as a suite"
        "#,
    )
    .unwrap();

    assert_eq!(manifest.name, "kubaos");
    assert_eq!(
        manifest.description.as_deref(),
        Some("KubaOS suites and toolchains")
    );
    assert_eq!(manifest.command, ["bin/kubaos-kotodama", "--stdio"]);
    assert!(manifest.action_catalog);

    let live_query = manifest.live_query.unwrap();
    assert_eq!(live_query.prefixes, ["suite "]);
    assert_eq!(live_query.triggers, [r"^\d+\.\d+$"]);
    assert_eq!(live_query.debounce_ms, 120);
    assert!(!live_query.fallback);

    assert_eq!(manifest.contributed_actions.len(), 2);
    let pin = &manifest.contributed_actions[0];
    assert_eq!(pin.kind, KindName::new("project").unwrap());
    assert_eq!(
        pin.action,
        Action {
            id: "pin-version".into(),
            title: "Pin a version here".into(),
            shortcut: Some("ctrl+p".into()),
            arguments: vec![Argument {
                name: "version".into(),
                placeholder: Some("1.23".into()),
                optional: false
            }],
        }
    );
    assert_eq!(manifest.contributed_actions[1].kind.as_str(), "folder");
}

#[test]
fn an_extension_that_only_indexes_items_needs_only_a_name_and_a_command() {
    let manifest = parse(
        r#"
        name = "emoji"
        command = ["kotodama-emoji"]
        "#,
    )
    .unwrap();
    assert_eq!(manifest.live_query, None);
    assert!(manifest.contributed_actions.is_empty());
    assert!(!manifest.action_catalog);
}

#[test]
fn a_live_query_debounce_of_zero_is_allowed() {
    let manifest = parse(
        r#"
        name = "calculator"
        command = ["kotodama-calculator"]
        [live_query]
        triggers = ['^[\d(.]']
        debounce_ms = 0
        "#,
    )
    .unwrap();
    assert_eq!(manifest.live_query.unwrap().debounce_ms, 0);
}

#[test]
fn a_fallback_needs_no_prefix_or_trigger() {
    let manifest = parse(
        r#"
        name = "web-search"
        command = ["kotodama-web-search"]
        [live_query]
        fallback = true
        debounce_ms = 0
        "#,
    )
    .unwrap();
    let live_query = manifest.live_query.unwrap();
    assert!(live_query.fallback);
    assert!(live_query.prefixes.is_empty() && live_query.triggers.is_empty());
}

#[test]
fn invalid_manifests_are_rejected_with_the_reason() {
    let cases = [
        (
            r#"name = "Kuba OS"
            command = ["x"]"#,
            "isn't a valid Extension name",
        ),
        (
            r#"name = "x"
            command = []"#,
            "`command` is empty",
        ),
        (
            r#"name = "x"
            command = ["x"]
            comand = ["x"]"#,
            "unknown field `comand`",
        ),
        (
            r#"name = "x"
            command = ["x"]
            [live_query]
            debounce_ms = 100"#,
            "needs at least one prefix or trigger",
        ),
        (
            r#"name = "x"
            command = ["x"]
            [live_query]
            prefixes = ["="]"#,
            "missing field `debounce_ms`",
        ),
        (
            r#"name = "x"
            command = ["x"]
            [[contributed_actions]]
            kind = "suite"
            id = "a"
            title = "A""#,
            "unknown Item kind `suite`",
        ),
        (
            r#"name = "x"
            command = ["x"]
            [[contributed_actions]]
            kind = "project"
            id = "a"
            titel = "A""#,
            "unknown field `titel`",
        ),
    ];
    for (toml, reason) in cases {
        let err = parse(toml).unwrap_err();
        assert!(err.contains(reason), "expected {reason:?} in:\n{err}");
    }
}

#[test]
fn a_manifest_may_put_its_items_behind_a_prefix_with_an_entry_item() {
    let manifest = parse(
        r#"
        name = "emoji"
        command = ["kotodama-emoji"]
        prefix = "emoji"
        title = "Emoji"
        icon = "face-smile"
        "#,
    )
    .unwrap();

    assert_eq!(manifest.prefix.as_deref(), Some("emoji"));
    assert_eq!(manifest.title.as_deref(), Some("Emoji"));
    assert_eq!(manifest.icon.as_deref(), Some("face-smile"));
}

#[test]
fn an_extension_without_a_prefix_shows_its_items_in_the_search() {
    let manifest = parse("name = \"apps\"\ncommand = [\"x\"]").unwrap();

    assert_eq!(manifest.prefix, None);
    assert_eq!(manifest.title, None);
    assert_eq!(manifest.icon, None);
}

#[test]
fn a_prefix_is_one_word() {
    for prefix in ["", "two words", "tab\tbed", " emoji"] {
        let toml = format!("name = \"emoji\"\ncommand = [\"x\"]\nprefix = {prefix:?}");

        let err = parse(&toml).unwrap_err();

        assert!(err.contains("prefix"), "{prefix:?}: {err}");
    }
}
