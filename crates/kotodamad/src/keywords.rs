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
//!
//! # What opens a URL, the URL its last argument: the Quicklink's own
//! # `opener`, else the default here, else `xdg-open`.
//! [quicklinks.work]
//! url = "https://mail.example.com"
//! opener = ["firefox", "-P", "work"]
//!
//! [quicklink_defaults]
//! opener = ["firefox", "--new-tab"]
//! ```
//!
//! Keywords are matched without regard to case. An entry that doesn't parse is
//! logged and skipped; the rest still apply.

use std::collections::HashMap;

use kotodama_extension::url::{self, PLACEHOLDER};
use kotodama_protocol::{Action, Argument, Item, ItemKind, NamespacedKind, Table, Value};

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
    Url {
        template: String,
        /// The program and its first arguments that open the URL, which
        /// follows them; `xdg-open` if `None`.
        opener: Option<Vec<String>>,
    },
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
            Target::Url { template, .. } => template.contains(PLACEHOLDER),
            Target::Command(args) => args.iter().any(|arg| arg.contains(PLACEHOLDER)),
        }
    }

    /// The command that opens the Quicklink for `query`. A URL opens with its
    /// opener, or [`url::opener`] without one.
    pub fn command(&self, query: &str) -> Vec<String> {
        match &self.target {
            Target::Url { template, opener } => {
                let url = url::fill(template, query);
                match opener {
                    Some(opener) => {
                        let mut command = opener.clone();
                        command.push(url);
                        command
                    }
                    None => url::opener(&url),
                }
            }
            Target::Command(args) => args
                .iter()
                .map(|arg| arg.replace(PLACEHOLDER, query))
                .collect(),
        }
    }

    /// The Item the Daemon shows for this Quicklink.
    pub fn item(&self) -> Item {
        let subtitle = match &self.target {
            Target::Url { template, .. } => template.clone(),
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
            hidden: false,
            actions: vec![Action {
                id: "open".into(),
                title: "Open".into(),
                shortcut: None,
                arguments: if self.takes_query() {
                    vec![Argument {
                        name: "query".into(),
                        placeholder: None,
                        // The keyword alone opens it with an empty Argument.
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

/// The `[aliases]`, `[quicklinks]` and `[quicklink_defaults]` tables of the
/// merged config.
pub fn from_config(config: &Table) -> Keywords {
    let mut keywords = Keywords::default();
    let default_opener = match default_opener(config) {
        Ok(opener) => opener,
        Err(err) => {
            log::error!("quicklink_defaults: {err}");
            None
        }
    };
    for (keyword, value) in tables(config, "aliases") {
        match alias(value) {
            Ok(alias) => {
                keywords.aliases.insert(keyword.to_lowercase(), alias);
            }
            Err(err) => log::error!("alias `{keyword}`: {err}"),
        }
    }
    for (keyword, value) in tables(config, "quicklinks") {
        match quicklink(keyword, value, default_opener.as_deref()) {
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

/// The `opener` of `[quicklink_defaults]`, if set.
fn default_opener(config: &Table) -> Result<Option<Vec<String>>, String> {
    match config.get("quicklink_defaults") {
        Some(Value::Table(table)) => command(table, "opener"),
        Some(_) => Err("should be a table".into()),
        None => Ok(None),
    }
}

fn quicklink(
    keyword: &str,
    value: &Value,
    default_opener: Option<&[String]>,
) -> Result<Quicklink, String> {
    if keyword.is_empty() || keyword.contains(char::is_whitespace) {
        return Err("a keyword is one word".into());
    }
    let Value::Table(table) = value else {
        return Err("should be a table with `url` or `command`".into());
    };
    let string = |key: &str| string(table, key);
    let opener = command(table, "opener")?;
    let target = match (string("url")?, command(table, "command")?) {
        (Some(template), None) => Target::Url {
            template,
            opener: opener.or_else(|| default_opener.map(<[String]>::to_vec)),
        },
        (None, Some(_)) if opener.is_some() => {
            return Err("has both `command` and `opener`".into());
        }
        (None, Some(args)) => Target::Command(args),
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

/// An optional program and its arguments in a config table: a list of
/// strings, not empty.
fn command(table: &Table, key: &str) -> Result<Option<Vec<String>>, String> {
    let wrong = || format!("`{key}` should be a list of strings");
    let Some(value) = table.get(key) else {
        return Ok(None);
    };
    let Value::Array(args) = value else {
        return Err(wrong());
    };
    if args.is_empty() {
        return Err(wrong());
    }
    let mut command = Vec::with_capacity(args.len());
    for arg in args {
        let Value::String(arg) = arg else {
            return Err(wrong());
        };
        command.push(arg.clone());
    }
    Ok(Some(command))
}

/// An optional string in a config table.
pub(crate) fn string(table: &Table, key: &str) -> Result<Option<String>, String> {
    match table.get(key) {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        None => Ok(None),
        Some(_) => Err(format!("`{key}` should be a string")),
    }
}
