use super::*;

fn prefixes(manifests: &[&str], config: Table) -> Prefixes {
    let manifests: Vec<Manifest> = manifests
        .iter()
        .map(|toml| Manifest::from_toml(toml).unwrap())
        .collect();
    Prefixes::new(
        manifests
            .iter()
            .enumerate()
            .map(|(n, manifest)| (ExtensionId(n), manifest)),
        &config,
    )
}

fn emoji() -> Prefixes {
    prefixes(
        &[
            "name = \"apps\"\ncommand = [\"x\"]",
            "name = \"emoji\"\ncommand = [\"x\"]\nprefix = \"emoji\"",
        ],
        Table::new(),
    )
}

#[test]
fn a_query_is_behind_the_prefix_once_whitespace_follows_it() {
    let prefixes = emoji();

    assert_eq!(prefixes.scope_of("emoji fire"), Some((0, 6)));
    assert_eq!(prefixes.scope_of("emoji "), Some((0, 6)));
    assert_eq!(prefixes.scope_of("  EMOJI\tfire"), Some((0, 8)));
    assert_eq!(prefixes.scope_of("emoji"), None);
    assert_eq!(prefixes.scope_of("emojis fire"), None);
    assert_eq!(prefixes.scope_of("fire emoji "), None);
}

#[test]
fn only_the_extensions_with_a_prefix_are_scoped() {
    assert_eq!(emoji().scoped(3), [false, true, false]);
}

#[test]
fn two_extensions_never_share_a_prefix() {
    let prefixes = prefixes(
        &[
            "name = \"emoji\"\ncommand = [\"x\"]\nprefix = \"e\"",
            "name = \"editors\"\ncommand = [\"x\"]\nprefix = \"E\"",
        ],
        Table::new(),
    );

    assert_eq!(prefixes.scoped(2), [true, false]);
}

#[test]
fn a_prefix_in_the_config_that_isnt_one_word_is_left_out() {
    let table = Table::from([("prefix".to_owned(), Value::String("two words".into()))]);
    let extensions = Table::from([("emoji".to_owned(), Value::Table(table))]);
    let config = Table::from([("extensions".to_owned(), Value::Table(extensions))]);

    let prefixes = prefixes(
        &["name = \"emoji\"\ncommand = [\"x\"]\nprefix = \"emoji\""],
        config,
    );

    assert_eq!(prefixes.scoped(1), [false]);
}
