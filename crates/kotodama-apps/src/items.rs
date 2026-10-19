//! The Extension's settings, from its `[extensions.apps]` config table, and the
//! Items the apps become under them.
//!
//! ```toml
//! [extensions.apps]
//! # What starts apps with `Terminal=true`; the app's command follows it.
//! terminal = ["ghostty", "-e"]
//! # Apps to leave out, by desktop id, with or without `.desktop`.
//! hidden = ["qt5ct", "qt6ct"]
//! # Names to show instead of the app's own, by desktop id.
//! names = { nvim = "Neovim" }
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

use kotodama_protocol::{Action, Item, ItemKind, Table, Value};

use crate::scan::{App, app_id};

/// The primary Action, which starts the app.
pub const OPEN: &str = "open";
/// What a desktop action's Action id starts with; its own id follows.
pub const DESKTOP_ACTION: &str = "desktop-action:";

/// The terminal used when the config names none: the freedesktop proposal's
/// launcher, which starts the user's preferred terminal.
const DEFAULT_TERMINAL: &str = "xdg-terminal-exec";

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub terminal: Vec<String>,
    /// App ids, without `.desktop`.
    pub hidden: HashSet<String>,
    /// Names by app id, without `.desktop`.
    pub names: HashMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            terminal: vec![DEFAULT_TERMINAL.to_owned()],
            hidden: HashSet::new(),
            names: HashMap::new(),
        }
    }
}

impl Settings {
    /// Reads the config table. A setting of the wrong shape is logged and left
    /// at its default; the others still apply.
    pub fn from_config(config: &Table) -> Self {
        let mut settings = Settings::default();
        if let Some(value) = config.get("terminal") {
            match strings(value) {
                Some(terminal) if !terminal.is_empty() => settings.terminal = terminal,
                _ => warn("`terminal` should be a list of strings, the program first"),
            }
        }
        if let Some(value) = config.get("hidden") {
            match strings(value) {
                Some(hidden) => settings.hidden = hidden.iter().map(|id| app_id(id)).collect(),
                None => warn("`hidden` should be a list of desktop ids"),
            }
        }
        match config.get("names") {
            Some(Value::Table(names)) => {
                for (id, name) in names {
                    match name {
                        Value::String(name) => {
                            settings.names.insert(app_id(id), name.clone());
                        }
                        _ => warn(&format!("`names.{id}` should be a string")),
                    }
                }
            }
            Some(_) => warn("`names` should be a table of names by desktop id"),
            None => {}
        }
        settings
    }

    /// The Items for `apps`, without the hidden ones.
    pub fn items<'a>(&'a self, apps: &'a BTreeMap<String, App>) -> impl Iterator<Item = Item> + 'a {
        apps.values()
            .filter(|app| !self.hidden.contains(&app.id))
            .map(|app| self.item(app))
    }

    fn item(&self, app: &App) -> Item {
        let renamed = self.names.get(&app.id);
        let title = renamed.unwrap_or(&app.name).clone();
        // Found by its generic name, by its own name when renamed, and by its
        // desktop id, besides its keywords.
        let mut keywords = app.keywords.clone();
        keywords.extend(app.generic_name.iter().cloned());
        if renamed.is_some() {
            keywords.push(app.name.clone());
        }
        if !app.id.eq_ignore_ascii_case(&title) {
            keywords.push(app.id.clone());
        }
        let mut actions = Vec::with_capacity(1 + app.actions.len());
        actions.push(action(OPEN.to_owned(), "Open".to_owned()));
        actions.extend(app.actions.iter().map(|desktop| {
            action(
                format!("{DESKTOP_ACTION}{}", desktop.id),
                desktop.name.clone(),
            )
        }));
        Item {
            id: app.id.clone(),
            kind: ItemKind::App {
                desktop_id: app.desktop_id.clone(),
            },
            // The same app from another Extension, such as a Folder source,
            // shares its History (ADR 0008).
            identity: Some(format!("app:{}", app.desktop_id)),
            title,
            subtitle: app.generic_name.clone().or_else(|| app.comment.clone()),
            icon: app.icon.clone(),
            keyword: None,
            keywords,
            actions,
        }
    }
}

/// The command an Action of `app` runs: [`OPEN`] or one of its desktop
/// actions. `None` for an Action the app doesn't have.
pub fn command<'a>(app: &'a App, action: &str) -> Option<&'a [String]> {
    if action == OPEN {
        return Some(&app.command);
    }
    let own = action.strip_prefix(DESKTOP_ACTION)?;
    app.actions
        .iter()
        .find(|desktop| desktop.id == own)
        .map(|desktop| desktop.command.as_slice())
}

fn action(id: String, title: String) -> Action {
    Action {
        id,
        title,
        shortcut: None,
        arguments: Vec::new(),
    }
}

fn strings(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Array(values) => values
            .iter()
            .map(|value| match value {
                Value::String(value) => Some(value.clone()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn warn(message: &str) {
    eprintln!("kotodama-apps: config: {message}");
}

/// What the Daemon has of each Item, by id, so only what changed is sent
/// again. A fingerprint stands in for each Item, so the Items aren't kept
/// twice.
#[derive(Default)]
pub struct SentItems(HashMap<String, u64>);

impl SentItems {
    /// Makes `items` what the Daemon has. Returns the ids to remove and the
    /// Items to add or replace.
    pub fn update(&mut self, items: impl Iterator<Item = Item>) -> (Vec<String>, Vec<Item>) {
        let mut next = HashMap::with_capacity(self.0.len());
        let mut added = Vec::new();
        for item in items {
            let print = fingerprint(&item);
            if self.0.remove(&item.id) != Some(print) {
                next.insert(item.id.clone(), print);
                added.push(item);
            } else {
                next.insert(item.id, print);
            }
        }
        let mut removed: Vec<String> = std::mem::replace(&mut self.0, next).into_keys().collect();
        removed.sort();
        (removed, added)
    }
}

/// A hash of everything an Item sends.
fn fingerprint(item: &Item) -> u64 {
    let mut hasher = DefaultHasher::new();
    let Item {
        id,
        kind,
        identity,
        title,
        subtitle,
        icon,
        keyword,
        keywords,
        actions,
    } = item;
    (id, kind, identity, title, subtitle, icon, keyword, keywords).hash(&mut hasher);
    for Action {
        id,
        title,
        shortcut,
        arguments,
    } in actions
    {
        (id, title, shortcut, arguments.len()).hash(&mut hasher);
    }
    hasher.finish()
}
