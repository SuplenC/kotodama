//! Apps: every Extension is one. The search shows an entry Item for it,
//! found by its title and its word, such as `projects`; running that Item,
//! or Tab on it, enters the app, where the query searches the Extension's
//! Items alone, those it keeps out of the search included.
//!
//! # Owns
//!
//! - [`Apps`]: each Extension's app, from its Manifest and the user's
//!   `[extensions.<name>]` `word` and `in_search`, and the app a query is
//!   in.
//! - [`App::entry`]: the entry Item. [`App::scope`]: what a Front end shows
//!   inside, the intro card included.
//!
//! # Not here
//!
//! - Which Segments the search skips: `index.rs`, `Reach`.
//! - Entering, searching inside and the empty app: `daemon.rs`.

use kotodama_protocol::{
    APP_ENTRY_KIND, Action, Intro, Item, ItemKind, Manifest, NamespacedKind, Scope, Table, Value,
    is_valid_word,
};

use crate::config::extension_table;
use crate::daemon::ExtensionId;
use crate::index::SIDES;

/// One Extension's app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    /// What the query starts with inside it, in lowercase.
    pub word: String,
    pub extension: ExtensionId,
    /// The Extension's name as the user reads it.
    pub title: String,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub intro: Intro,
    /// The ids of the Extension's Items the app lists first.
    pub commands: Vec<String>,
    /// Whether the whole search reads each of the Extension's Segments: the
    /// Items it means for the search, then those it means for the app alone.
    pub searched: [bool; SIDES],
}

/// The id of an Extension's entry Item, among the Daemon's own Items: by the
/// Extension's name, so its History outlives a change of word.
fn entry_id(extension: &str) -> String {
    format!("app:{extension}")
}

/// The word `[extensions.<name>]` sets: `word`, or `prefix`, its old name,
/// which is still read and warned about.
fn configured_word<'t>(table: &'t Table, name: &str) -> Option<&'t Value> {
    if let Some(word) = table.get("word") {
        return Some(word);
    }
    let word = table.get("prefix")?;
    log::warn!("[extensions.{name}] prefix is deprecated; call it `word`");
    Some(word)
}

impl App {
    /// The Item the search shows for the Extension, found by its title and
    /// its word. Running it enters the app.
    pub fn entry(&self, extension: &str) -> Item {
        Item {
            id: entry_id(extension),
            kind: ItemKind::Namespaced(NamespacedKind::new(APP_ENTRY_KIND).expect("a valid kind")),
            title: self.title.clone(),
            subtitle: Some(match &self.description {
                Some(description) => description.clone(),
                None => format!("Search {}", self.title),
            }),
            icon: self.icon.clone(),
            keyword: Some(self.word.clone()),
            identity: None,
            keywords: vec![self.word.clone()],
            hidden: false,
            app_only: false,
            actions: vec![Action::new("open", "Open")],
        }
    }

    /// What a Front end shows for a query inside it: the intro card too
    /// while nothing is typed, so it comes with the first frame and costs
    /// later keystrokes nothing.
    pub fn scope(&self, typed: bool) -> Scope {
        Scope {
            word: self.word.clone(),
            title: self.title.clone(),
            icon: self.icon.clone(),
            intro: (!typed).then(|| self.intro.clone()),
        }
    }
}

/// Every Extension's app, in Extension order. Two apps never share a word.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Apps {
    apps: Vec<App>,
}

impl Apps {
    /// The apps of `extensions`, by their Manifests, unless the config's
    /// `[extensions.<name>]` says otherwise: `word` is the word, and
    /// `in_search` puts every Item in the search, or none.
    pub fn new<'a>(
        extensions: impl IntoIterator<Item = (ExtensionId, &'a Manifest)>,
        config: &Table,
    ) -> Self {
        let mut apps: Vec<App> = Vec::new();
        for (extension, manifest) in extensions {
            let name = &manifest.name;
            let table = extension_table(config, name);
            let chosen = match configured_word(&table, name) {
                Some(Value::String(word)) if is_valid_word(word) => Some(word.as_str()),
                Some(other) => {
                    log::warn!("[extensions.{name}] word: {other:?} isn't one word; ignoring it");
                    manifest.word.as_deref()
                }
                None => manifest.word.as_deref(),
            };
            let taken = |word: &str| apps.iter().any(|app| app.word == word);
            let chosen = chosen.unwrap_or(name).to_lowercase();
            let word = if !taken(&chosen) {
                chosen
            } else if !taken(name) {
                log::warn!("{name}: another app has the word `{chosen}`; using `{name}`");
                name.clone()
            } else {
                log::warn!("{name}: another app has the word `{chosen}`, and `{name}`; no app");
                continue;
            };
            let in_search = match table.get("in_search") {
                Some(Value::Bool(in_search)) => Some(*in_search),
                Some(other) => {
                    log::warn!("[extensions.{name}] in_search: {other:?} isn't true or false");
                    None
                }
                None => None,
            };
            let searched = match in_search {
                Some(true) => [true, true],
                Some(false) => [false, false],
                None if manifest.in_search => [true, false],
                None => [false, false],
            };
            let title = manifest.title.clone().unwrap_or_else(|| name.clone());
            let about = manifest
                .about
                .clone()
                .or_else(|| manifest.description.clone())
                .unwrap_or_else(|| format!("Search {title}"));
            apps.push(App {
                word,
                extension,
                title,
                icon: manifest.icon.clone(),
                description: manifest.description.clone(),
                intro: Intro {
                    about,
                    examples: manifest.examples.clone(),
                },
                commands: manifest.commands.clone(),
                searched,
            });
        }
        Apps { apps }
    }

    pub fn iter(&self) -> impl Iterator<Item = &App> {
        self.apps.iter()
    }

    pub fn get(&self, n: usize) -> Option<&App> {
        self.apps.get(n)
    }

    /// The place here of the app of `extension`.
    pub fn of(&self, extension: ExtensionId) -> Option<usize> {
        self.apps.iter().position(|app| app.extension == extension)
    }

    /// By Segment, of `extensions` Extensions, whether the whole search reads
    /// it. An Extension without an app keeps the Items it means for the
    /// search there, as there would be no other way to them.
    pub fn searched(&self, extensions: usize) -> Vec<bool> {
        let mut searched = vec![false; extensions * SIDES];
        for side in searched.chunks_mut(SIDES) {
            side[0] = true;
        }
        for app in &self.apps {
            let at = app.extension.0 * SIDES;
            if let Some(sides) = searched.get_mut(at..at + SIDES) {
                sides.copy_from_slice(&app.searched);
            }
        }
        searched
    }

    /// The app whose word `query` starts with, by its place here, and where
    /// the rest of the query starts: the word in any case, then whitespace.
    /// The word alone, with nothing after it, searches as any word does.
    pub fn scope_of(&self, query: &str) -> Option<(usize, usize)> {
        if self.apps.is_empty() {
            return None;
        }
        let trimmed = query.trim_start();
        let start = query.len() - trimmed.len();
        let end = trimmed.find(char::is_whitespace)?;
        let word = &trimmed[..end];
        let n = self.apps.iter().position(|app| {
            word.chars()
                .flat_map(char::to_lowercase)
                .eq(app.word.chars())
        })?;
        let space = trimmed[end..].chars().next().map_or(0, char::len_utf8);
        Some((n, start + end + space))
    }
}

#[cfg(test)]
mod tests;
