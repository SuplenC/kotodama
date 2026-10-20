//! The Items projects and scripts become.

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
        hidden: false,
        actions: vec![
            Action::new(OPEN, "Open"),
            Action::new(SHELL, "Open Terminal Here"),
            Action::new(LAZYGIT, "Open in Lazygit"),
            Action::new(FILE_MANAGER, "Open in File Manager"),
            Action::new(COPY_PATH, "Copy Path"),
        ],
    }
}

pub fn script(script: &Script, home: Option<&Path>) -> Item {
    // A `.desktop` file is a file here too: the app the apps Extension offers
    // is a different thing (ADR 0008).
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
