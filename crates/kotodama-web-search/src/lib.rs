//! Web search: for a query, a Result searching each configured engine for it and, if the query
//! looks like an address, one opening it, marked as the answer.
//!
//! # Owns
//!
//! - The `engines` setting and the default engines.
//! - What counts as an address, and the scheme it opens with.
//! - The Live query results for a query, and the command an Action on one runs.
//!
//! # Not here
//!
//! - The protocol loop: `main.rs`.
//! - Filling a URL template and the opener it runs: [`kotodama_extension::url`], shared with
//!   Quicklinks.
//! - Which queries the Extension is asked and where its Results rank: the Daemon's
//!   `Daemon::add_fallbacks`.

#![forbid(unsafe_code)]

use kotodama_extension::config;
use kotodama_extension::url;
use kotodama_protocol::Action;
use kotodama_protocol::Fraction;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::ItemRef;
use kotodama_protocol::LiveResult;
use kotodama_protocol::Table;
use kotodama_protocol::Value;

/// The one Action of every Item: opens its URL.
pub const OPEN: &str = "open";
const ICON: &str = "web-browser";
/// The longest label a domain name may have (RFC 1035).
const MAX_LABEL: usize = 63;

/// A search engine: what it is called and the URL template a query goes in,
/// with `{query}` where the query goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Engine {
    pub name: String,
    pub url: String,
}

impl Engine {
    #[must_use]
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
        Engine::new("Wikipedia", "https://en.wikipedia.org/w/index.php?search={query}"),
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
    if engines.is_empty() { default_engines() } else { engines }
}

/// The engine one entry of the `engines` setting names, or why the entry is not one.
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
/// - a domain name such as `example.com`, opened over `https`, whose last label is a common generic
///   top-level domain ([`GENERIC`]), a two-letter country code that isn't also a common file
///   extension ([`FILES`]), so `main.rs` and `notes.md` stay searches, or not ASCII, such as `рф`;
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
    "app", "art", "biz", "blog", "cloud", "club", "com", "design", "dev", "edu", "email", "gov", "info", "int", "link",
    "live", "mil", "name", "net", "news", "online", "org", "page", "pro", "shop", "site", "space", "store", "tech",
    "today", "top", "website", "wiki", "xyz",
];

/// Two-letter file extensions, country codes or not, that a file name ends
/// in more often than an address does.
pub const FILES: &[&str] = &[
    "cc", "cs", "db", "el", "ex", "go", "gz", "hs", "js", "jl", "kt", "md", "mk", "ml", "nb", "pm", "ps", "py", "rb",
    "rc", "rs", "sh", "so", "sv", "ts", "vh", "xz",
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
    if is_ipv4(host) {
        return Some("http://");
    }
    let (rest, tld) = host.rsplit_once('.')?;
    (is_known_tld(tld) && rest.split('.').all(is_label)).then_some("https://")
}

/// The host and port at the start of an address, before its path.
fn authority(address: &str) -> &str {
    address.split(['/', '?', '#']).next().unwrap_or_default()
}

fn is_port(port: &str) -> bool {
    (1..=5).contains(&port.len())
        && port.bytes().all(|b| b.is_ascii_digit())
        && port.parse::<u32>().is_ok_and(|port| (1..=65535).contains(&port))
}

fn is_ipv4(host: &str) -> bool {
    host.split('.').count() == 4
        && host
            .split('.')
            .all(|octet| (1..=3).contains(&octet.len()) && octet.parse::<u8>().is_ok())
}

/// Whether `tld` is a top-level domain taken without a scheme: one in [`GENERIC`], a two-letter
/// country code not in [`FILES`], or any alphabetic one outside ASCII.
fn is_known_tld(tld: &str) -> bool {
    if !tld.is_ascii() {
        return tld.chars().count() >= 2 && tld.chars().all(char::is_alphabetic);
    }
    let tld_lower = tld.to_lowercase();
    match tld.len() {
        2 => tld.bytes().all(|b| b.is_ascii_alphabetic()) && !FILES.contains(&&*tld_lower),
        _ => GENERIC.contains(&&*tld_lower),
    }
}

/// Whether `label`, one dot-separated part of a host below its top-level domain, is a valid one.
fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| c.is_alphanumeric() || c == '-')
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
        score: score(0),
        answer: true,
    });
    let searches = engines.iter().enumerate().map(|(n, engine)| LiveResult {
        item: item(
            format!("search:{}", engine.name),
            format!("Search {} for {query}", engine.name),
            url::fill(&engine.url, query),
        ),
        score: score(n),
        answer: false,
    });
    opening.into_iter().chain(searches).collect()
}

/// The score of the Result `rank` places after the first, which scores 1: earlier engines come
/// first, everything else being equal.
fn score(rank: usize) -> Fraction {
    // 1 / (rank + 1) is in (0, 1] for every rank.
    Fraction::new(1.0 / (rank as f32 + 1.0)).expect("a web search score is in range")
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
        app_only: false,
        weak_keywords: Vec::new(),
        yields: false,
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
mod tests;
