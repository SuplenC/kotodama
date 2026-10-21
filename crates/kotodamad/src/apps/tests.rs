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

#[test]
fn two_apps_never_share_a_word_the_second_falls_back_to_its_name() {
    let apps = apps(
        &[
            "name = \"emoji\"\ncommand = [\"x\"]\nword = \"e\"",
            "name = \"editors\"\ncommand = [\"x\"]\nword = \"E\"",
            "name = \"e\"\ncommand = [\"x\"]",
        ],
        Table::new(),
    );

    assert_eq!(words(&apps), ["e", "editors"]);
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

    let intro = app.scope(false).intro.unwrap();

    assert_eq!(intro.about, "Emoji and symbols");
    assert!(intro.examples.is_empty());
    assert_eq!(app.scope(true).intro, None);
}
