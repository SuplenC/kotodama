use super::*;

const ADDRESSES: &[(&str, &str)] = &[
    ("example.com", "https://example.com"),
    ("  example.com ", "https://example.com"),
    ("github.com/SuplenC/kotodama", "https://github.com/SuplenC/kotodama"),
    ("sub.example.co.uk?q=1#top", "https://sub.example.co.uk?q=1#top"),
    ("example.com:8443/x", "https://example.com:8443/x"),
    ("https://example.com/a?b=c", "https://example.com/a?b=c"),
    ("HTTP://Example.com", "HTTP://Example.com"),
    ("localhost:8080", "http://localhost:8080"),
    ("localhost:3000/api", "http://localhost:3000/api"),
    ("192.168.1.10", "http://192.168.1.10"),
    ("127.0.0.1:8000/", "http://127.0.0.1:8000/"),
    ("żółw.pl", "https://żółw.pl"),
    ("пример.рф", "https://пример.рф"),
    ("kotodama.dev", "https://kotodama.dev"),
    ("http://localhost/", "http://localhost/"),
    ("https://intranet/wiki", "https://intranet/wiki"),
];

#[test]
fn domains_addresses_with_a_scheme_and_localhost_with_a_port_are_addresses() {
    for &(query, url) in ADDRESSES {
        assert_eq!(address(query).as_deref(), Some(url), "{query:?}");
    }
}

/// The Daemon sends what may be an address whatever else matches, by the
/// Manifest's triggers, so they must let every address through.
#[test]
fn the_manifest_triggers_send_every_address() {
    let manifest = kotodama_protocol::Manifest::from_toml(include_str!("../manifest.toml"));
    let triggers = manifest.unwrap().live_query.unwrap().triggers;
    let triggers: Vec<regex::Regex> = triggers
        .iter()
        .map(|trigger| regex::Regex::new(trigger).unwrap())
        .collect();
    for &(query, _) in ADDRESSES {
        assert!(triggers.iter().any(|t| t.is_match(query)), "{query:?}");
    }
}

#[test]
fn words_numbers_and_text_with_spaces_are_not_addresses() {
    let cases = [
        "",
        "weather",
        "foo.bar baz",
        "go to example.com",
        "3.14",
        "1.2.3",
        "256.1.1.1",
        "localhost",
        "localhost:",
        "localhost:99999",
        "example.com:port",
        "example.",
        ".com",
        "a..com",
        "-a.com",
        "main.c",
        "main.rs",
        "notes.md",
        "Cargo.toml",
        "index.js",
        "node.js",
        "setup.py",
        "example.unknowntld",
        "v1.2",
        "https://",
        "https:// example.com",
        "mailto:me@example.com",
        "ftp://example.com",
    ];
    for query in cases {
        assert_eq!(address(query), None, "{query:?}");
    }
}

fn titles(results: &[LiveResult]) -> Vec<&str> {
    results.iter().map(|r| r.item.title.as_str()).collect()
}

fn url_of(result: &LiveResult) -> &str {
    match &result.item.kind {
        ItemKind::Url { url } => url,
        kind => panic!("{kind:?}"),
    }
}

#[test]
fn a_query_gives_a_search_of_each_engine_in_order() {
    let results = results("rust book", &default_engines());
    assert_eq!(
        titles(&results),
        [
            "Search DuckDuckGo for rust book",
            "Search Google for rust book",
            "Search Wikipedia for rust book"
        ]
    );
    assert!(results.iter().all(|r| !r.answer));
    assert!(results.is_sorted_by(|a, b| a.score.get() > b.score.get()));
    assert_eq!(url_of(&results[0]), "https://duckduckgo.com/?q=rust%20book");
}

#[test]
fn queries_are_encoded_into_the_template() {
    let engines = [Engine::new("X", "https://x.example/s?q={query}&lang=pl")];
    let results = results("tom & jerry żółw", &engines);
    assert_eq!(
        url_of(&results[0]),
        "https://x.example/s?q=tom%20%26%20jerry%20%C5%BC%C3%B3%C5%82w&lang=pl"
    );
}

#[test]
fn an_address_opens_first_as_the_answer_and_is_searched_too() {
    let results = results("example.com", &default_engines()[..1]);
    assert_eq!(
        titles(&results),
        ["Open example.com", "Search DuckDuckGo for example.com"]
    );
    assert!(results[0].answer);
    assert_eq!(url_of(&results[0]), "https://example.com");
}

#[test]
fn a_blank_query_gives_nothing() {
    assert!(results("  ", &default_engines()).is_empty());
}

#[test]
fn open_runs_the_opener_on_the_items_url() {
    let item = results("x", &default_engines()).remove(0).item.to_ref();
    assert_eq!(
        command(OPEN, &item),
        Some(vec!["xdg-open".to_owned(), "https://duckduckgo.com/?q=x".to_owned()])
    );
    assert_eq!(command("copy", &item), None);
}

fn table(text: &str) -> Table {
    toml::from_str(text).unwrap()
}

#[test]
fn engines_come_from_the_config_in_order() {
    let engines = engines(&table(
        r#"engines = [
            { name = "Kagi", url = "https://kagi.com/search?q={query}" },
            { name = "Broken" },
            { name = "No query", url = "https://example.com" },
            { name = "Crates", url = "https://crates.io/search?q={query}" },
        ]"#,
    ));
    assert_eq!(
        engines,
        [
            Engine::new("Kagi", "https://kagi.com/search?q={query}"),
            Engine::new("Crates", "https://crates.io/search?q={query}"),
        ]
    );
}

#[test]
fn no_setting_gives_the_defaults_and_an_empty_list_none() {
    assert_eq!(engines(&Table::new()), default_engines());
    assert_eq!(engines(&table("engines = []")), []);
    assert_eq!(engines(&table("engines = 3")), default_engines());
}
