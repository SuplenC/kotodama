//! Prefixes: the keywords that put one Extension's Items behind them, such
//! as `emoji`. The search leaves those Items out and shows one entry Item
//! for the Extension; `emoji fire` searches them alone.
//!
//! # Owns
//!
//! - [`Prefixes`]: each Extension's Prefix, from its Manifest and the
//!   user's `[extensions.<name>] prefix`, and the Prefix a query starts with.
//! - [`Prefix::entry`]: the entry Item.
//!
//! # Not here
//!
//! - Skipping the Segments behind a Prefix: `index.rs`, `Reach`.
//! - Searching behind one, and running the entry Item: `daemon.rs`.

use kotodama_protocol::{
    Action, Item, ItemKind, Manifest, NamespacedKind, Scope, Table, Value, is_valid_prefix,
};

use crate::config::extension_table;
use crate::daemon::ExtensionId;

/// One Extension's Prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefix {
    /// In lowercase.
    pub keyword: String,
    pub extension: ExtensionId,
    /// The Extension's name as the user reads it.
    pub title: String,
    pub icon: Option<String>,
    pub description: Option<String>,
}

/// The id of an Extension's entry Item, among the Daemon's own Items: by the
/// Extension's name, so its History outlives a change of Prefix.
fn entry_id(extension: &str) -> String {
    format!("prefix:{extension}")
}

impl Prefix {
    /// The Item the search shows for the Extension, found by its title and
    /// its Prefix. Running it enters the Prefix.
    pub fn entry(&self, extension: &str) -> Item {
        Item {
            id: entry_id(extension),
            kind: ItemKind::Namespaced(
                NamespacedKind::new("kotodama.prefix").expect("a valid kind"),
            ),
            title: self.title.clone(),
            subtitle: Some(match &self.description {
                Some(description) => description.clone(),
                None => format!("Search {}", self.title),
            }),
            icon: self.icon.clone(),
            keyword: Some(self.keyword.clone()),
            identity: None,
            keywords: vec![self.keyword.clone()],
            hidden: false,
            actions: vec![Action::new("open", "Search here")],
        }
    }

    /// What a Front end shows for a query behind it.
    pub fn scope(&self) -> Scope {
        Scope {
            prefix: self.keyword.clone(),
            title: self.title.clone(),
            icon: self.icon.clone(),
        }
    }
}

/// Every Extension's Prefix, in Extension order. Two Extensions never share
/// one: the first keeps it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Prefixes {
    prefixes: Vec<Prefix>,
}

impl Prefixes {
    /// The Prefixes of `extensions`, by their Manifests, unless the config's
    /// `[extensions.<name>] prefix` says otherwise; `prefix = ""` leaves an
    /// Extension's Items in the search.
    pub fn new<'a>(
        extensions: impl IntoIterator<Item = (ExtensionId, &'a Manifest)>,
        config: &Table,
    ) -> Self {
        let mut prefixes: Vec<Prefix> = Vec::new();
        for (extension, manifest) in extensions {
            let table = extension_table(config, &manifest.name);
            let keyword = match table.get("prefix") {
                Some(Value::String(prefix)) if prefix.is_empty() => continue,
                Some(Value::String(prefix)) if is_valid_prefix(prefix) => prefix.as_str(),
                Some(other) => {
                    log::warn!(
                        "[extensions.{}] prefix: {other:?} isn't one word; leaving it out",
                        manifest.name
                    );
                    continue;
                }
                None => match &manifest.prefix {
                    Some(prefix) => prefix.as_str(),
                    None => continue,
                },
            };
            let keyword = keyword.to_lowercase();
            if prefixes.iter().any(|prefix| prefix.keyword == keyword) {
                log::warn!(
                    "{}: another Extension has the Prefix `{keyword}`; leaving its Items in the search",
                    manifest.name
                );
                continue;
            }
            prefixes.push(Prefix {
                keyword,
                extension,
                title: manifest
                    .title
                    .clone()
                    .unwrap_or_else(|| manifest.name.clone()),
                icon: manifest.icon.clone(),
                description: manifest.description.clone(),
            });
        }
        Prefixes { prefixes }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Prefix> {
        self.prefixes.iter()
    }

    pub fn get(&self, n: usize) -> Option<&Prefix> {
        self.prefixes.get(n)
    }

    /// By Extension, of `extensions`, whether its Items are behind a Prefix.
    pub fn scoped(&self, extensions: usize) -> Vec<bool> {
        let mut scoped = vec![false; extensions];
        for prefix in &self.prefixes {
            if let Some(scoped) = scoped.get_mut(prefix.extension.0) {
                *scoped = true;
            }
        }
        scoped
    }

    /// The Prefix `query` starts with, by its place here, and where the rest
    /// of the query starts: the Prefix in any case, then whitespace. The
    /// Prefix alone, with nothing after it, searches as any word does.
    pub fn scope_of(&self, query: &str) -> Option<(usize, usize)> {
        if self.prefixes.is_empty() {
            return None;
        }
        let trimmed = query.trim_start();
        let start = query.len() - trimmed.len();
        let end = trimmed.find(char::is_whitespace)?;
        let word = &trimmed[..end];
        let n = self.prefixes.iter().position(|prefix| {
            word.chars()
                .flat_map(char::to_lowercase)
                .eq(prefix.keyword.chars())
        })?;
        let space = trimmed[end..].chars().next().map_or(0, char::len_utf8);
        Some((n, start + end + space))
    }
}

#[cfg(test)]
mod tests;
