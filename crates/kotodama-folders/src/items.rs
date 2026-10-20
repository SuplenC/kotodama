//! The Items projects and scripts become, and sending only what changed.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use kotodama_protocol::{Action, Argument, Item, ItemKind};

use crate::projects::Project;
use crate::scripts::Script;

/// A project's primary Action: its tmux session, with the editor.
pub const OPEN: &str = "open";
pub const SHELL: &str = "shell";
pub const LAZYGIT: &str = "lazygit";
pub const FILE_MANAGER: &str = "file-manager";
pub const COPY_PATH: &str = "copy-path";
/// A script's primary Action.
pub const RUN: &str = "run";
/// The Argument a script with `args=prompt` asks for.
pub const ARGUMENTS: &str = "Arguments";

/// An Item's id: its path, which no other project or script has.
pub fn id(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `path` with the home folder shown as `~`.
fn shown(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.to_string_lossy()),
        None => path.to_string_lossy().into_owned(),
    }
}

pub fn project(project: &Project, home: Option<&Path>) -> Item {
    Item {
        id: id(&project.path),
        kind: ItemKind::Project {
            path: project.path.clone(),
        },
        // The folder, as any Extension that lists it names it (ADR 0008).
        identity: Some(format!("folder:{}", project.path.to_string_lossy())),
        title: project.name.clone(),
        subtitle: Some(shown(&project.path, home)),
        icon: Some("folder".to_owned()),
        keyword: None,
        keywords: Vec::new(),
        actions: vec![
            action(OPEN, "Open", Vec::new()),
            action(SHELL, "Open Terminal Here", Vec::new()),
            action(LAZYGIT, "Open in Lazygit", Vec::new()),
            action(FILE_MANAGER, "Open in File Manager", Vec::new()),
            action(COPY_PATH, "Copy Path", Vec::new()),
        ],
    }
}

pub fn script(script: &Script, home: Option<&Path>) -> Item {
    let file_name = script
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    // A desktop entry kept among the scripts is the app the apps Extension
    // offers under the same desktop id (ADR 0008).
    let identity = if file_name.ends_with(".desktop") {
        format!("app:{file_name}")
    } else {
        format!("file:{}", script.path.to_string_lossy())
    };
    let arguments = if script.header.prompt {
        vec![Argument {
            name: ARGUMENTS.to_owned(),
            placeholder: None,
            optional: false,
        }]
    } else {
        Vec::new()
    };
    let run = if script.header.terminal {
        "Run in Terminal"
    } else {
        "Run in Background"
    };
    Item {
        id: id(&script.path),
        kind: ItemKind::File {
            path: script.path.clone(),
        },
        identity: Some(identity),
        title: script.title(),
        subtitle: Some(shown(&script.path, home)),
        icon: Some("utilities-terminal".to_owned()),
        keyword: None,
        // Found by its file's name when its header renames it.
        keywords: if script.header.name.is_some() {
            vec![script.stem.clone()]
        } else {
            Vec::new()
        },
        actions: vec![
            action(RUN, run, arguments),
            action(COPY_PATH, "Copy Path", Vec::new()),
        ],
    }
}

fn action(id: &str, title: &str, arguments: Vec<Argument>) -> Action {
    Action {
        id: id.to_owned(),
        title: title.to_owned(),
        shortcut: None,
        arguments,
    }
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
        (id, title, shortcut).hash(&mut hasher);
        for Argument {
            name,
            placeholder,
            optional,
        } in arguments
        {
            (name, placeholder, optional).hash(&mut hasher);
        }
    }
    hasher.finish()
}
