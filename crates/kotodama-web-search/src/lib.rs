//! Web search: for a query, a Result searching each configured engine for
//! it and, if the query looks like an address, one opening it, marked as the
//! answer. Templates are filled and opened as Quicklinks' are
//! ([`kotodama_extension::url`]).

#![forbid(unsafe_code)]

use kotodama_extension::{config, url};
use kotodama_protocol::{Action, Fraction, Item, ItemKind, ItemRef, LiveResult, Table, Value};

/// The one Action of every Item: opens its URL.
pub const OPEN: &str = "open";
const ICON: &str = "web-browser";

/// A search engine: what it is called and the URL template a query goes in,
/// with `{query}` where the query goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Engine {
    pub name: String,
    pub url: String,
}

impl Engine {
    fn new(name: &str, url: &str) -> Self {
        Engine {
            name: name.into(),
            url: url.into(),
        }
    }
}

/// The engines searched when the config names none.
pub fn default_engines() -> Vec<Engine> {
    vec![
        Engine::new("DuckDuckGo", "https://duckduckgo.com/?q={query}"),
        Engine::new("Google", "https://www.google.com/search?q={query}"),
        Engine::new(
            "Wikipedia",
            "https://en.wikipedia.org/w/index.php?search={query}",
        ),
    ]
}

/// The `engines` setting: a list of `{ name, url }` tables, in the order
/// their Results show. An entry of the wrong shape is logged and skipped;
/// without the setting, or with no entry right, the defaults apply. An empty
/// list turns searching off, leaving addresses.
pub fn engines(table: &Table) -> Vec<Engine> {
    let Some(value) = table.get("engines") else {
        return default_engines();
    };
    let Value::Array(entries) = value else {
        config::warn("`engines` should be a list of { name, url } tables");
        return default_engines();
    };
    if entries.is_empty() {
        return Vec::new();
    }
    let engines: Vec<Engine> = entries
        .iter()
        .enumerate()
        .filter_map(|(n, entry)| match engine(entry) {
            Ok(engine) => Some(engine),
            Err(err) => {
                config::warn(format_args!("engines[{n}]: {err}"));
                None
            }
        })
        .collect();
    if engines.is_empty() {
        default_engines()
    } else {
        engines
    }
}

fn engine(value: &Value) -> Result<Engine, &'static str> {
    let Value::Table(table) = value else {
        return Err("should be a { name, url } table");
    };
    let string = |key| match table.get(key) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        _ => Err("needs `name` and `url` strings"),
    };
    let (name, url) = (string("name")?, string("url")?);
    if !url.contains(url::PLACEHOLDER) {
        return Err("`url` has no {query} for the query");
    }
    Ok(Engine { name, url })
}

/// The address `query` names, ready to open, if it looks like one:
///
/// - a URL with an `http` or `https` scheme and a host, taken as it is;
/// - a domain name such as `example.com`, opened over `https`, whose last
///   label is a common generic top-level domain ([`GENERIC`]), a two-letter
///   country code that isn't also a common file extension ([`FILES`]), so
///   `main.rs` and `notes.md` stay searches, or not ASCII, such as `рф`;
/// - `localhost` with a port, or an IPv4 address, opened over `http`.
///
/// A port and a path may follow. Anything with whitespace inside is a
/// search, not an address.
pub fn address(query: &str) -> Option<String> {
    let query = query.trim();
    if query.is_empty() || query.contains(char::is_whitespace) {
        return None;
    }
    for scheme in ["https://", "http://"] {
        if let Some(start) = query.get(..scheme.len())
            && start.eq_ignore_ascii_case(scheme)
        {
            let host = authority(&query[scheme.len()..]);
            return (!host.is_empty()).then(|| query.to_owned());
        }
    }
    let scheme = scheme_for(authority(query))?;
    Some(format!("{scheme}{query}"))
}

/// Generic top-level domains common enough to take without a scheme. Others
/// open with one typed.
pub const GENERIC: &[&str] = &[
    "app", "art", "biz", "blog", "cloud", "club", "com", "design", "dev", "edu", "email", "gov",
    "info", "int", "link", "live", "mil", "name", "net", "news", "online", "org", "page", "pro",
    "shop", "site", "space", "store", "tech", "today", "top", "website", "wiki", "xyz",
];

/// Two-letter file extensions, country codes or not, that a file name ends
/// in more often than an address does.
pub const FILES: &[&str] = &[
    "cc", "cs", "db", "el", "ex", "go", "gz", "hs", "js", "jl", "kt", "md", "mk", "ml", "nb", "pm",
    "ps", "py", "rb", "rc", "rs", "sh", "so", "sv", "ts", "vh", "xz",
];

/// The scheme an address at `authority`, a host and an optional port, opens
/// with, if it is one.
fn scheme_for(authority: &str) -> Option<&'static str> {
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if port.is_some_and(|port| !is_port(port)) {
        return None;
    }
    if host.eq_ignore_ascii_case("localhost") {
        return port.map(|_| "http://");
    }
    if host.split('.').count() == 4
        && host
            .split('.')
            .all(|octet| (1..=3).contains(&octet.len()) && octet.parse::<u8>().is_ok())
    {
        return Some("http://");
    }
    let (rest, tld) = host.rsplit_once('.')?;
    let label = |label: &str| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_alphanumeric() || c == '-')
    };
    let tld_lower = tld.to_lowercase();
    let known = if tld.is_ascii() {
        match tld.len() {
            2 => tld.bytes().all(|b| b.is_ascii_alphabetic()) && !FILES.contains(&&*tld_lower),
            _ => GENERIC.contains(&&*tld_lower),
        }
    } else {
        tld.chars().count() >= 2 && tld.chars().all(char::is_alphabetic)
    };
    (known && rest.split('.').all(label)).then_some("https://")
}

/// The host and port at the start of an address, before its path.
fn authority(address: &str) -> &str {
    address.split(['/', '?', '#']).next().unwrap_or_default()
}

fn is_port(port: &str) -> bool {
    (1..=5).contains(&port.len())
        && port.bytes().all(|b| b.is_ascii_digit())
        && port
            .parse::<u32>()
            .is_ok_and(|port| (1..=65535).contains(&port))
}

/// The Live query results for `query`: the address it names first, as the answer, then a
/// search of each engine, in order.
pub fn results(query: &str, engines: &[Engine]) -> Vec<LiveResult> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let opening = address(query).map(|address| LiveResult {
        item: item(format!("open:{address}"), format!("Open {query}"), address),
        score: Fraction::new(1.0).expect("in range"),
        answer: true,
    });
    let searches = engines.iter().enumerate().map(|(n, engine)| LiveResult {
        item: item(
            format!("search:{}", engine.name),
            format!("Search {} for {query}", engine.name),
            url::fill(&engine.url, query),
        ),
        // Earlier engines first, everything else being equal.
        score: Fraction::new(1.0 / (n as f32 + 1.0)).expect("in range"),
        answer: false,
    });
    opening.into_iter().chain(searches).collect()
}

fn item(id: String, title: String, url: String) -> Item {
    Item {
        id,
        kind: ItemKind::Url { url: url.clone() },
        identity: None,
        title,
        subtitle: Some(url),
        icon: Some(ICON.into()),
        keyword: None,
        keywords: Vec::new(),
        actions: vec![Action::new(OPEN, "Open")],
        hidden: false,
    }
}

/// The command an Action on `item` runs, if it is one of ours.
pub fn command(action: &str, item: &ItemRef) -> Option<Vec<String>> {
    match (&item.kind, action) {
        (ItemKind::Url { url }, OPEN) => Some(url::opener(url)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDRESSES: &[(&str, &str)] = &[
        ("example.com", "https://example.com"),
        ("  example.com ", "https://example.com"),
        (
            "github.com/SuplenC/kotodama",
            "https://github.com/SuplenC/kotodama",
        ),
        (
            "sub.example.co.uk?q=1#top",
            "https://sub.example.co.uk?q=1#top",
        ),
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
            Some(vec![
                "xdg-open".to_owned(),
                "https://duckduckgo.com/?q=x".to_owned()
            ])
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
}
