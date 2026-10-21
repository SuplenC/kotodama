use kotodama_protocol::{Action, Argument, Example, KindName, Manifest};

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
        confirm = true
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
            confirm: false,
        }
    );
    assert_eq!(manifest.contributed_actions[1].kind.as_str(), "folder");
    assert!(manifest.contributed_actions[1].action.confirm);
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
fn a_manifest_describes_its_app() {
    let manifest = parse(
        r#"
        name = "emoji"
        command = ["kotodama-emoji"]
        word = "emoji"
        title = "Emoji"
        icon = "face-smile"
        about = "Emoji and symbols, typed into the window you came from."
        examples = [
            { query = "fire", result = "🔥 Fire" },
            { query = "arrow", result = "→ Rightwards Arrow" },
        ]
        commands = ["settings"]
        in_search = false
        "#,
    )
    .unwrap();

    assert_eq!(manifest.word.as_deref(), Some("emoji"));
    assert_eq!(manifest.title.as_deref(), Some("Emoji"));
    assert_eq!(manifest.icon.as_deref(), Some("face-smile"));
    assert_eq!(
        manifest.about.as_deref(),
        Some("Emoji and symbols, typed into the window you came from.")
    );
    assert_eq!(
        manifest.examples,
        [
            Example {
                query: "fire".into(),
                result: "🔥 Fire".into()
            },
            Example {
                query: "arrow".into(),
                result: "→ Rightwards Arrow".into()
            },
        ]
    );
    assert_eq!(manifest.commands, ["settings"]);
    assert!(!manifest.in_search);
}

#[test]
fn an_extension_without_app_keys_keeps_its_items_in_the_search() {
    let manifest = parse("name = \"apps\"\ncommand = [\"x\"]").unwrap();

    assert_eq!(manifest.word, None);
    assert_eq!(manifest.title, None);
    assert_eq!(manifest.icon, None);
    assert_eq!(manifest.about, None);
    assert!(manifest.examples.is_empty());
    assert!(manifest.commands.is_empty());
    assert!(manifest.in_search);
}

#[test]
fn a_word_is_one_word() {
    for word in ["", "two words", "tab\tbed", " emoji"] {
        let toml = format!("name = \"emoji\"\ncommand = [\"x\"]\nword = {word:?}");

        let err = parse(&toml).unwrap_err();

        assert!(err.contains("word"), "{word:?}: {err}");
    }
}

#[test]
fn prefix_is_still_read_as_the_word() {
    let manifest = parse("name = \"emoji\"\ncommand = [\"x\"]\nprefix = \"emo\"").unwrap();

    assert_eq!(manifest.word.as_deref(), Some("emo"));
}

#[test]
fn a_manifest_with_both_word_and_prefix_is_refused() {
    let err =
        parse("name = \"emoji\"\ncommand = [\"x\"]\nword = \"emo\"\nprefix = \"em\"").unwrap_err();

    assert!(err.contains("word"), "{err}");
}
