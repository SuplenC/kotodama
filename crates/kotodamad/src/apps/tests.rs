use super::*;

fn apps(manifests: &[&str], config: Table) -> Apps {
    let manifests: Vec<Manifest> = manifests
        .iter()
        .map(|toml| Manifest::from_toml(toml).unwrap())
        .collect();
    Apps::new(
        manifests
            .iter()
            .enumerate()
            .map(|(n, manifest)| (ExtensionId(n), manifest)),
        &config,
        &crate::keywords::from_config(&config),
    )
}

fn emoji() -> Apps {
    apps(
        &[
            "name = \"apps\"\ncommand = [\"x\"]",
            "name = \"emoji\"\ncommand = [\"x\"]\nword = \"emo\"\nin_search = false",
        ],
        Table::new(),
    )
}

/// The config `[extensions.<name>]` with `key = value`.
fn config(name: &str, key: &str, value: Value) -> Table {
    let table = Table::from([(key.to_owned(), value)]);
    let extensions = Table::from([(name.to_owned(), Value::Table(table))]);
    Table::from([("extensions".to_owned(), Value::Table(extensions))])
}

fn words(apps: &Apps) -> Vec<&str> {
    apps.iter().map(|app| app.word.as_str()).collect()
}

#[test]
fn a_query_is_in_an_app_once_whitespace_follows_its_word() {
    let apps = emoji();

    assert_eq!(apps.scope_of("emo fire"), Some((1, 4)));
    assert_eq!(apps.scope_of("emo "), Some((1, 4)));
    assert_eq!(apps.scope_of("  EMO\tfire"), Some((1, 6)));
    assert_eq!(apps.scope_of("emo"), None);
    assert_eq!(apps.scope_of("emos fire"), None);
    assert_eq!(apps.scope_of("fire emo "), None);
}

#[test]
fn every_extension_has_an_app_named_by_its_name_unless_its_manifest_chooses_a_word() {
    assert_eq!(words(&emoji()), ["apps", "emo"]);
}

#[test]
fn the_search_reads_the_items_an_extension_means_for_it() {
    assert_eq!(
        emoji().searched(3),
        [true, false, false, false, true, false]
    );
}

fn words_by_name(apps: &Apps, manifests: &[&str]) -> Vec<(String, String)> {
    let names: Vec<String> = manifests
        .iter()
        .map(|toml| Manifest::from_toml(toml).unwrap().name)
        .collect();
    apps.iter()
        .map(|app| (names[app.extension.0].clone(), app.word.clone()))
        .collect()
}

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|&(name, word)| (name.to_owned(), word.to_owned()))
        .collect()
}

const EMOJI_E: &str = "name = \"emoji\"\ncommand = [\"x\"]\nword = \"e\"";
const EDITORS_E: &str = "name = \"editors\"\ncommand = [\"x\"]\nword = \"E\"";
const E: &str = "name = \"e\"\ncommand = [\"x\"]";

#[test]
fn two_apps_never_share_a_word_the_name_sorting_first_keeps_it() {
    let manifests = [EMOJI_E, EDITORS_E, E];
    let apps = apps(&manifests, Table::new());

    // A Manifest's word goes before an Extension's name, so `e` the
    // Extension, whose word is its name, gets `e-2`.
    assert_eq!(
        words_by_name(&apps, &manifests),
        pairs(&[("e", "e-2"), ("editors", "e"), ("emoji", "emoji")])
    );
    assert_eq!(apps.conflicts().len(), 2);
}

#[test]
fn the_words_dont_depend_on_the_order_extensions_are_found_in() {
    let one = [EMOJI_E, EDITORS_E, E];
    let other = [E, EDITORS_E, EMOJI_E];

    assert_eq!(
        words_by_name(&apps(&one, Table::new()), &one),
        words_by_name(&apps(&other, Table::new()), &other)
    );
}

#[test]
fn a_word_the_config_sets_goes_before_one_a_manifest_sets() {
    let manifests = [EDITORS_E, "name = \"zed\"\ncommand = [\"x\"]"];
    let apps = apps(&manifests, config("zed", "word", Value::String("e".into())));

    assert_eq!(
        words_by_name(&apps, &manifests),
        pairs(&[("editors", "editors"), ("zed", "e")])
    );
    assert_eq!(
        apps.conflicts(),
        [Conflict::WordTaken {
            extension: "editors".into(),
            word: "e".into(),
            holder: Holder::App("zed".into()),
            instead: "editors".into(),
        }]
    );
}

/// The config with `[quicklinks.<keyword>]` and `[aliases.<keyword>]` for
/// each of the given keywords.
fn keywords(quicklinks: &[&str], aliases: &[&str]) -> Table {
    let quicklink = || {
        Value::Table(Table::from([(
            "url".to_owned(),
            Value::String("https://example.com".into()),
        )]))
    };
    let alias = || {
        Value::Table(Table::from([
            ("extension".to_owned(), Value::String("apps".into())),
            ("item".to_owned(), Value::String("firefox".into())),
        ]))
    };
    let table = |keywords: &[&str], value: &dyn Fn() -> Value| {
        Value::Table(
            keywords
                .iter()
                .map(|keyword| ((*keyword).to_owned(), value()))
                .collect(),
        )
    };
    Table::from([
        ("quicklinks".to_owned(), table(quicklinks, &quicklink)),
        ("aliases".to_owned(), table(aliases, &alias)),
    ])
}

#[test]
fn a_quicklink_keeps_its_keyword_and_the_app_goes_by_its_name() {
    let manifests = ["name = \"calculator\"\ncommand = [\"x\"]\nword = \"calc\""];
    let apps = apps(&manifests, keywords(&["calc"], &[]));

    assert_eq!(words(&apps), ["calculator"]);
    assert_eq!(
        apps.conflicts(),
        [Conflict::WordTaken {
            extension: "calculator".into(),
            word: "calc".into(),
            holder: Holder::Quicklink,
            instead: "calculator".into(),
        }]
    );
}

#[test]
fn an_alias_keeps_its_keyword() {
    let apps = apps(&[EMOJI_E], keywords(&[], &["e"]));

    assert_eq!(words(&apps), ["emoji"]);
    assert!(matches!(
        apps.conflicts(),
        [Conflict::WordTaken {
            holder: Holder::Alias,
            ..
        }]
    ));
}

#[test]
fn an_app_whose_word_and_name_are_both_held_still_gets_a_word() {
    let apps = apps(&[EMOJI_E], keywords(&["e", "emoji"], &[]));

    assert_eq!(words(&apps), ["emoji-2"]);
    assert_eq!(apps.scope_of("emoji-2 fire").map(|(n, _)| n), Some(0));
}

#[test]
fn an_alias_over_a_quicklink_is_a_conflict() {
    let keywords = crate::keywords::from_config(&keywords(&["gh"], &["gh"]));

    assert_eq!(
        keywords.conflicts(),
        [Conflict::QuicklinkUnderAlias {
            keyword: "gh".into()
        }]
    );
}

#[test]
fn the_config_chooses_the_word() {
    let apps = apps(
        &["name = \"emoji\"\ncommand = [\"x\"]\nword = \"emo\""],
        config("emoji", "word", Value::String("Smile".into())),
    );

    assert_eq!(words(&apps), ["smile"]);
}

#[test]
fn the_configs_prefix_is_still_read_as_the_word() {
    let apps = apps(
        &["name = \"emoji\"\ncommand = [\"x\"]\nword = \"emo\""],
        config("emoji", "prefix", Value::String("smile".into())),
    );

    assert_eq!(words(&apps), ["smile"]);
}

#[test]
fn a_word_in_the_config_that_isnt_one_word_is_ignored() {
    for word in ["two words", ""] {
        let apps = apps(
            &["name = \"emoji\"\ncommand = [\"x\"]\nword = \"emo\""],
            config("emoji", "word", Value::String(word.into())),
        );

        assert_eq!(words(&apps), ["emo"], "{word:?}");
    }
}

#[test]
fn in_search_false_in_the_config_puts_the_whole_extension_behind_its_app() {
    let apps = apps(
        &["name = \"apps\"\ncommand = [\"x\"]"],
        config("apps", "in_search", Value::Bool(false)),
    );

    assert_eq!(apps.searched(1), [false, false]);
}

#[test]
fn in_search_true_in_the_config_puts_every_item_in_the_search() {
    let apps = apps(
        &["name = \"emoji\"\ncommand = [\"x\"]\nin_search = false"],
        config("emoji", "in_search", Value::Bool(true)),
    );

    assert_eq!(apps.searched(1), [true, true]);
}

#[test]
fn the_intro_falls_back_to_the_description_and_shows_only_before_typing() {
    let apps = apps(
        &["name = \"emoji\"\ncommand = [\"x\"]\ndescription = \"Emoji and symbols\""],
        Table::new(),
    );
    let app = apps.get(0).unwrap();

    let intro = app.scope(false, None).intro.unwrap();

    assert_eq!(intro.about, "Emoji and symbols");
    assert!(intro.examples.is_empty());
    assert_eq!(intro.note, None);
    assert_eq!(app.scope(true, None).intro, None);
}

#[test]
fn the_intro_carries_the_extensions_note() {
    let apps = apps(&["name = \"clipboard\"\ncommand = [\"x\"]"], Table::new());
    let app = apps.get(0).unwrap();

    let intro = app.scope(false, Some("Paused")).intro.unwrap();

    assert_eq!(intro.note.as_deref(), Some("Paused"));
    assert_eq!(app.scope(true, Some("Paused")).intro, None);
}
