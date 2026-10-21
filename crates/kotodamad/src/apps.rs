//! Apps: every Extension is one. The search shows an entry Item for it,
//! found by its title and its word, such as `projects`; running that Item,
//! or Tab on it, enters the app, where the query searches the Extension's
//! Items alone, those it keeps out of the search included.
//!
//! # Owns
//!
//! - [`Apps`]: each Extension's app, from its Manifest and the user's `[extensions.<name>]` `word`
//!   and `in_search`, and the app a query is in.
//! - [`App::entry`]: the entry Item. [`App::scope`]: what a Front end shows inside, the intro card
//!   included.
//!
//! # Not here
//!
//! - Which Segments the search skips: `index.rs`, `Reach`.
//! - Entering, searching inside and the empty app: `daemon/front_ends.rs`, `daemon/search.rs`,
//!   `daemon/empty.rs`.

use std::collections::HashMap;

use kotodama_protocol::APP_ENTRY_KIND;
use kotodama_protocol::Action;
use kotodama_protocol::Intro;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::Manifest;
use kotodama_protocol::NamespacedKind;
use kotodama_protocol::Scope;
use kotodama_protocol::Table;
use kotodama_protocol::Value;
use kotodama_protocol::is_valid_word;

use crate::config::extension_table;
use crate::conflict::Conflict;
use crate::conflict::Holder;
use crate::daemon::ExtensionId;
use crate::index::SIDES;
use crate::keywords::Keywords;
use crate::problem::Report;

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
/// which is still read and reported.
fn configured_word<'t>(table: &'t Table, name: &str, report: &mut Report) -> Option<&'t Value> {
    if let Some(word) = table.get("word") {
        return Some(word);
    }
    let word = table.get("prefix")?;
    report.add(
        format!("extensions.{name}.prefix"),
        "is the old name of `word`; call it `word`",
    );
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
    /// later keystrokes nothing, with `note`, what the Extension last said
    /// the user should know.
    pub fn scope(&self, typed: bool, note: Option<&str>) -> Scope {
        Scope {
            word: self.word.clone(),
            title: self.title.clone(),
            icon: self.icon.clone(),
            intro: (!typed).then(|| Intro {
                note: note.map(str::to_owned),
                ..self.intro.clone()
            }),
        }
    }
}

/// Where an app's word comes from, first the one that wins a word two apps
/// ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Asked {
    /// The user's `[extensions.<name>] word`.
    Config,
    /// The Manifest's `word`.
    Manifest,
    /// The Extension's name, without a word.
    Name,
}

/// Every Extension's app, by the Extension's name. Two apps never share a
/// word, and no app has an Alias's or a Quicklink's keyword.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Apps {
    apps: Vec<App>,
    /// The words apps asked for and didn't get, by the order they were
    /// given out in.
    conflicts: Vec<Conflict>,
}

impl Apps {
    /// The apps of `extensions`, by their Manifests, unless the config's
    /// `[extensions.<name>]` says otherwise: `word` is the word, and
    /// `in_search` puts every Item in the search, or none. Each keyword
    /// goes to one holder, by the precedence `conflict.rs` describes; an app
    /// that loses its word goes by another, and every app gets one. A
    /// setting that can't be read goes in `report`.
    pub fn new<'a>(
        extensions: impl IntoIterator<Item = (ExtensionId, &'a Manifest)>,
        config: &Table,
        keywords: &Keywords,
        report: &mut Report,
    ) -> Self {
        let mut asked: Vec<(Asked, &str, App)> = extensions
            .into_iter()
            .map(|(extension, manifest)| {
                let (from, app) = App::new(extension, manifest, config, report);
                (from, manifest.name.as_str(), app)
            })
            .collect();
        asked.sort_by_key(|&(from, name, _)| (from, name));
        let mut held: HashMap<String, Holder> = HashMap::new();
        let holder_of =
            |held: &HashMap<String, Holder>, word: &str| keywords.holder(word).or_else(|| held.get(word).cloned());
        let mut lost = Vec::new();
        for (n, (_, name, app)) in asked.iter().enumerate() {
            match holder_of(&held, &app.word) {
                Some(holder) => lost.push((n, holder)),
                None => {
                    held.insert(app.word.clone(), Holder::App((*name).to_owned()));
                }
            }
        }
        lost.sort_by_key(|&(n, _)| asked[n].1);
        let mut conflicts = Vec::new();
        for (n, holder) in lost {
            let (_, name, app) = &mut asked[n];
            #[expect(
                clippy::maybe_infinite_iter,
                reason = "the keywords and the apps hold finitely many words, so one of endless candidates is free"
            )]
            let instead = (1..)
                .map(|n| match n {
                    1 => (*name).to_owned(),
                    n => format!("{name}-{n}"),
                })
                .find(|word| holder_of(&held, word).is_none())
                .expect("a free word among endless ones");
            held.insert(instead.clone(), Holder::App((*name).to_owned()));
            conflicts.push(Conflict::WordTaken {
                extension: (*name).to_owned(),
                word: std::mem::replace(&mut app.word, instead.clone()),
                holder,
                instead,
            });
        }
        asked.sort_by_key(|&(_, name, _)| name);
        Apps {
            apps: asked.into_iter().map(|(_, _, app)| app).collect(),
            conflicts,
        }
    }

    /// The words apps asked for and didn't get.
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }
}

impl App {
    /// The app of the Extension of `manifest`, with the word it asks for,
    /// and where that word comes from.
    fn new(extension: ExtensionId, manifest: &Manifest, config: &Table, report: &mut Report) -> (Asked, Self) {
        let name = &manifest.name;
        let table = extension_table(config, name);
        let configured = match configured_word(&table, name, report) {
            Some(Value::String(word)) if is_valid_word(word) => Some(word.as_str()),
            Some(other) => {
                report.add(
                    format!("extensions.{name}.word"),
                    format!("{other:?} isn't one word; ignoring it"),
                );
                None
            }
            None => None,
        };
        let (from, word) = match (configured, &manifest.word) {
            (Some(word), _) => (Asked::Config, word),
            (None, Some(word)) => (Asked::Manifest, word.as_str()),
            (None, None) => (Asked::Name, name.as_str()),
        };
        let word = word.to_lowercase();
        let in_search = match table.get("in_search") {
            Some(Value::Bool(in_search)) => Some(*in_search),
            Some(other) => {
                report.add(
                    format!("extensions.{name}.in_search"),
                    format!("{other:?} isn't true or false"),
                );
                None
            }
            None => None,
        };
        let searched = match in_search {
            Some(true) => [true, true],
            None if manifest.in_search => [true, false],
            Some(false) | None => [false, false],
        };
        let title = manifest.title.clone().unwrap_or_else(|| name.clone());
        let about = manifest
            .about
            .clone()
            .or_else(|| manifest.description.clone())
            .unwrap_or_else(|| format!("Search {title}"));
        let app = App {
            word,
            extension,
            title,
            icon: manifest.icon.clone(),
            description: manifest.description.clone(),
            intro: Intro {
                about,
                examples: manifest.examples.clone(),
                note: None,
            },
            commands: manifest.commands.clone(),
            searched,
        };
        (from, app)
    }
}

impl Apps {
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
        let n = self
            .apps
            .iter()
            .position(|app| word.chars().flat_map(char::to_lowercase).eq(app.word.chars()))?;
        let space = trimmed[end..].chars().next().map_or(0, char::len_utf8);
        Some((n, start + end + space))
    }
}

#[cfg(test)]
mod tests;
