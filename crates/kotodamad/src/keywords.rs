//! Aliases and Quicklinks: the user's keywords, read from the merged config
//! and read again whenever it reloads.
//!
//! ```toml
//! [aliases]
//! ff = { extension = "apps", item = "firefox" }
//! # An Alias for one of the Item's own Actions makes it the primary one.
//! code = { extension = "projects", item = "kotodama", action = "open-editor" }
//!
//! [quicklinks.gh]
//! title = "GitHub"
//! url = "https://github.com"
//!
//! # `{query}` is what is typed after the keyword, percent-encoded in a URL.
//! [quicklinks.yt]
//! title = "Search YouTube"
//! url = "https://www.youtube.com/results?search_query={query}"
//!
//! [quicklinks.logs]
//! command = ["foot", "journalctl", "--user", "-fu", "{query}"]
//! ```
//!
//! Keywords are matched without regard to case. An entry that doesn't parse is
//! logged and skipped; the rest still apply.

use std::collections::HashMap;

use kotodama_protocol::{Action, Argument, Item, ItemKind, NamespacedKind, Table, Value};

/// The placeholder a Quicklink's template takes the typed query in.
pub const PLACEHOLDER: &str = "{query}";

/// A keyword that stands for an Item, or for one of its own Actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alias {
    pub extension: String,
    pub item: String,
    /// The Action that becomes primary; the Item's own primary if `None`.
    pub action: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Url(String),
    /// A program and its arguments, run without a shell.
    Command(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quicklink {
    /// Lowercase.
    pub keyword: String,
    pub title: String,
    pub icon: Option<String>,
    pub target: Target,
}

impl Quicklink {
    /// Whether the template has a place for a query.
    pub fn takes_query(&self) -> bool {
        match &self.target {
            Target::Url(url) => url.contains(PLACEHOLDER),
            Target::Command(args) => args.iter().any(|arg| arg.contains(PLACEHOLDER)),
        }
    }

    /// The command that opens the Quicklink for `query`. A URL opens with
    /// `xdg-open`.
    pub fn command(&self, query: &str) -> Vec<String> {
        match &self.target {
            Target::Url(url) => vec![
                "xdg-open".into(),
                url.replace(PLACEHOLDER, &percent_encode(query)),
            ],
            Target::Command(args) => args
                .iter()
                .map(|arg| arg.replace(PLACEHOLDER, query))
                .collect(),
        }
    }

    /// The Item the Daemon shows for this Quicklink.
    pub fn item(&self) -> Item {
        let subtitle = match &self.target {
            Target::Url(url) => url.clone(),
            Target::Command(args) => args.join(" "),
        };
        Item {
            id: format!("quicklink:{}", self.keyword),
            kind: ItemKind::Namespaced(
                NamespacedKind::new("kotodama.quicklink").expect("a valid kind"),
            ),
            title: self.title.clone(),
            subtitle: Some(subtitle),
            icon: self.icon.clone(),
            keyword: Some(self.keyword.clone()),
            identity: None,
            keywords: Vec::new(),
            actions: vec![Action {
                id: "open".into(),
                title: "Open".into(),
                shortcut: None,
                arguments: if self.takes_query() {
                    vec![Argument {
                        name: "query".into(),
                        placeholder: None,
                        // The keyword alone opens it with an empty query.
                        optional: true,
                    }]
                } else {
                    Vec::new()
                },
            }],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keywords {
    /// By keyword, lowercase.
    pub aliases: HashMap<String, Alias>,
    /// In keyword order.
    pub quicklinks: Vec<Quicklink>,
}

/// The `[aliases]` and `[quicklinks]` tables of the merged config.
pub fn from_config(config: &Table) -> Keywords {
    let mut keywords = Keywords::default();
    for (keyword, value) in tables(config, "aliases") {
        match alias(value) {
            Ok(alias) => {
                keywords.aliases.insert(keyword.to_lowercase(), alias);
            }
            Err(err) => log::error!("alias `{keyword}`: {err}"),
        }
    }
    for (keyword, value) in tables(config, "quicklinks") {
        match quicklink(keyword, value) {
            Ok(quicklink) => keywords.quicklinks.push(quicklink),
            Err(err) => log::error!("quicklink `{keyword}`: {err}"),
        }
    }
    keywords
}

fn tables<'a>(config: &'a Table, name: &str) -> impl Iterator<Item = (&'a String, &'a Value)> {
    match config.get(name) {
        Some(Value::Table(table)) => Some(table.iter()),
        Some(_) => {
            log::error!("`{name}` should be a table");
            None
        }
        None => None,
    }
    .into_iter()
    .flatten()
}

fn alias(value: &Value) -> Result<Alias, String> {
    let Value::Table(table) = value else {
        return Err("should be a table with `extension` and `item`".into());
    };
    let string = |key: &str| string(table, key);
    Ok(Alias {
        extension: string("extension")?.ok_or("needs `extension`")?,
        item: string("item")?.ok_or("needs `item`")?,
        action: string("action")?,
    })
}

fn quicklink(keyword: &str, value: &Value) -> Result<Quicklink, String> {
    if keyword.is_empty() || keyword.contains(char::is_whitespace) {
        return Err("a keyword is one word".into());
    }
    let Value::Table(table) = value else {
        return Err("should be a table with `url` or `command`".into());
    };
    let string = |key: &str| string(table, key);
    let target = match (string("url")?, table.get("command")) {
        (Some(url), None) => Target::Url(url),
        (None, Some(Value::Array(args))) if !args.is_empty() => Target::Command(
            args.iter()
                .map(|arg| match arg {
                    Value::String(arg) => Ok(arg.clone()),
                    _ => Err("`command` should be a list of strings".to_owned()),
                })
                .collect::<Result<_, _>>()?,
        ),
        (None, Some(_)) => return Err("`command` should be a list of strings".into()),
        (Some(_), Some(_)) => return Err("has both `url` and `command`".into()),
        (None, None) => return Err("needs `url` or `command`".into()),
    };
    Ok(Quicklink {
        keyword: keyword.to_lowercase(),
        title: string("title")?.unwrap_or_else(|| keyword.to_owned()),
        icon: string("icon")?,
        target,
    })
}

/// An optional string in a config table.
fn string(table: &Table, key: &str) -> Result<Option<String>, String> {
    match table.get(key) {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        None => Ok(None),
        Some(_) => Err(format!("`{key}` should be a string")),
    }
}

/// Percent-encodes everything but RFC 3986's unreserved characters, so any
/// query fits anywhere in a URL.
pub fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
