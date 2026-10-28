//! The Items projects and scripts become, and the ids of their Actions.
//!
//! # Owns
//!
//! - Each Item's id, identity, title, subtitle, keywords and Actions.
//!
//! # Not here
//!
//! - What an Action runs: `main.rs`, with the commands from `settings.rs`.
//! - Finding the projects and scripts: `projects.rs` and `scripts.rs`.

use std::path::Path;

use kotodama_protocol::Action;
use kotodama_protocol::Argument;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;

use crate::projects::Project;
use crate::scripts::Script;
use crate::settings::Settings;

/// A project's primary Action: a terminal in its folder, or what the config opens instead.
pub const OPEN: &str = "open";
/// A terminal in a project's folder.
pub const SHELL: &str = "shell";
/// Lazygit on a project, offered when lazygit is there to run.
pub const LAZYGIT: &str = "lazygit";
/// The file manager on a project's folder.
pub const FILE_MANAGER: &str = "file-manager";
/// A project's or a script's path to the clipboard.
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

/// The Item of a project, `home` shown as `~` in its subtitle, with the Actions `settings` offer.
pub fn project(project: &Project, home: Option<&Path>, settings: &Settings) -> Item {
    let mut actions = vec![Action::new(OPEN, "Open"), Action::new(SHELL, "Open Terminal Here")];
    if settings.lazygit.is_some() {
        actions.push(Action::new(LAZYGIT, "Open in Lazygit"));
    }
    actions.push(Action::new(FILE_MANAGER, "Open in File Manager"));
    actions.push(Action::new(COPY_PATH, "Copy Path"));
    Item {
        id: id(&project.path),
        kind: ItemKind::Project {
            path: project.path.clone(),
        },
        // The folder, as any Extension that lists it names it.
        identity: Some(format!("folder:{}", project.path.to_string_lossy())),
        title: project.name.clone(),
        subtitle: Some(shown(&project.path, home)),
        icon: Some("folder".to_owned()),
        keyword: None,
        keywords: Vec::new(),
        hidden: false,
        app_only: false,
        weak_keywords: Vec::new(),
        actions,
    }
}

/// The Item of a script, `home` shown as `~` in its subtitle. It asks for arguments when its
/// header says so.
pub fn script(script: &Script, home: Option<&Path>) -> Item {
    // A `.desktop` file is a file here too: the app the apps Extension offers is a different
    // thing.
    let identity = format!("file:{}", script.path.to_string_lossy());
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
        hidden: false,
        app_only: false,
        weak_keywords: Vec::new(),
        // Found by its file's name when its header renames it.
        keywords: if script.header.name.is_some() {
            vec![script.stem.clone()]
        } else {
            Vec::new()
        },
        actions: vec![
            Action::new(RUN, run).arguments(arguments),
            Action::new(COPY_PATH, "Copy Path"),
        ],
    }
}
