//! The Extension's settings, from its `[extensions.folders]` config table.
//!
//! ```toml
//! [extensions.folders]
//! # The folders, each with its kind. Projects are git repositories found down
//! # to `depth` folders below the folder (3 if not given).
//! folders = [
//!   { path = "~/Projects", kind = "projects", depth = 2 },
//!   { path = "~/Scripts", kind = "scripts" },
//! ]
//! # What a script runs in; the script and its arguments follow. It should keep
//! # the window open once the script exits.
//! terminal = ["foot", "--hold"]
//! # The commands a project's Actions run, in the project's folder. `{path}` is
//! # the folder and `{session}` its tmux session name.
//! open = ["foot", "tmux", "new-session", "-A", "-s", "{session}", "-c", "{path}", "nvim"]
//! shell = ["foot", "--working-directory={path}"]
//! lazygit = ["foot", "lazygit", "--path", "{path}"]
//! file_manager = ["xdg-open", "{path}"]
//! copy_path = ["wl-copy", "{path}"]
//! ```
//!
//! # Owns
//!
//! - Reading the table, with a `~` made the home folder, and logging what is wrong in it.
//! - The default commands, and filling `{path}` and `{session}` into a command.
//! - Whether "Open in Lazygit" is offered: when the table sets `lazygit`, or `lazygit` is on
//!   `PATH`.
//!
//! # Not here
//!
//! - Running a command: `main.rs`, through `kotodama_extension::launch`.
//! - Reading a command's list of strings: `kotodama_extension::config`.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use kotodama_extension::config;
use kotodama_protocol::Table;
use kotodama_protocol::Value;

use crate::projects::ProjectFolder;
use crate::scan::Folders;

/// How deep projects are looked for when a folder doesn't say.
pub const DEFAULT_DEPTH: usize = 3;

/// The user's preferred terminal, through the freedesktop proposal's launcher, as the apps
/// Extension starts terminal apps. The launcher drops its `--hold` for a terminal whose desktop
/// entry has no hold argument, so a POSIX shell keeps the window open instead, so the output can
/// be read: it runs the script, which is `$0`, with its arguments, then waits for Enter.
pub const DEFAULT_TERMINAL: &[&str] = &[
    "xdg-terminal-exec",
    "sh",
    "-c",
    r#""$0" "$@"; printf '\n[Exited with status %s. Press Enter to close.]' "$?"; read -r _"#,
];
/// A terminal in the project's folder. The launcher's `--dir` falls back to starting the terminal
/// there when the terminal has no argument for it.
pub const DEFAULT_OPEN: &[&str] = &["xdg-terminal-exec", "--dir={path}"];
/// A terminal in the project's folder, as [`DEFAULT_OPEN`].
pub const DEFAULT_SHELL: &[&str] = &["xdg-terminal-exec", "--dir={path}"];
/// Lazygit on the project, in a terminal.
pub const DEFAULT_LAZYGIT: &[&str] = &["xdg-terminal-exec", "lazygit", "--path", "{path}"];
/// The program [`DEFAULT_LAZYGIT`] runs, looked for on `PATH`.
const LAZYGIT_PROGRAM: &str = "lazygit";
/// Whichever file manager the desktop opens folders with.
pub const DEFAULT_FILE_MANAGER: &[&str] = &["xdg-open", "{path}"];
/// The Wayland clipboard.
pub const DEFAULT_COPY_PATH: &[&str] = &["wl-copy", "{path}"];

/// The `[extensions.folders]` table, read. Each command is a program and its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub folders: Folders,
    /// What a script with `terminal=true` runs in, the script and its arguments after it.
    pub terminal: Vec<String>,
    /// A project's [`OPEN`](crate::items::OPEN) Action.
    pub open: Vec<String>,
    /// A project's [`SHELL`](crate::items::SHELL) Action.
    pub shell: Vec<String>,
    /// A project's [`LAZYGIT`](crate::items::LAZYGIT) Action, `None` when it isn't offered.
    pub lazygit: Option<Vec<String>>,
    /// A project's [`FILE_MANAGER`](crate::items::FILE_MANAGER) Action.
    pub file_manager: Vec<String>,
    /// The [`COPY_PATH`](crate::items::COPY_PATH) Action, of a project or a script.
    pub copy_path: Vec<String>,
}

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|&word| word.to_owned()).collect()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            folders: Folders::default(),
            terminal: words(DEFAULT_TERMINAL),
            open: words(DEFAULT_OPEN),
            shell: words(DEFAULT_SHELL),
            lazygit: Some(words(DEFAULT_LAZYGIT)),
            file_manager: words(DEFAULT_FILE_MANAGER),
            copy_path: words(DEFAULT_COPY_PATH),
        }
    }
}

impl Settings {
    /// Reads the config table. A setting of the wrong shape is logged and left
    /// at its default, and a folder that is wrong is left out; the rest still
    /// apply. `home` stands for a leading `~`. Without a `lazygit` command in
    /// the table, "Open in Lazygit" is offered only when `lazygit` is in one of
    /// the `path` folders, looked for once here rather than per Item.
    #[must_use]
    pub fn from_config(config: &Table, home: Option<&Path>, path: Option<&OsStr>) -> Self {
        let mut settings = Settings::default();
        match config.get("folders") {
            Some(Value::Array(folders)) => {
                for (n, folder) in folders.iter().enumerate() {
                    if let Err(err) = settings.add_folder(folder, home) {
                        config::warn(format_args!("`folders[{n}]` {err}"));
                    }
                }
            }
            Some(_) => config::warn("`folders` should be a list of `{ path, kind }` tables"),
            None => {}
        }
        for (key, setting) in [
            ("terminal", &mut settings.terminal),
            ("open", &mut settings.open),
            ("shell", &mut settings.shell),
            ("file_manager", &mut settings.file_manager),
            ("copy_path", &mut settings.copy_path),
        ] {
            if let Some(command) = config::command(config, key) {
                *setting = command;
            }
        }
        settings.lazygit = match config::command(config, "lazygit") {
            Some(command) => Some(command),
            None if on_path(LAZYGIT_PROGRAM, path) => settings.lazygit,
            None => None,
        };
        settings
    }

    /// Adds the folder one entry of `folders` names.
    fn add_folder(&mut self, folder: &Value, home: Option<&Path>) -> Result<(), FolderError> {
        let Value::Table(folder) = folder else {
            return Err(FolderError::NotTable);
        };
        let path = match folder.get("path") {
            Some(Value::String(path)) => expand(path, home)?,
            _ => return Err(FolderError::NoPath),
        };
        match folder.get("kind") {
            Some(Value::String(kind)) if kind == "projects" => {
                let depth = match folder.get("depth") {
                    None => DEFAULT_DEPTH,
                    Some(Value::Integer(depth)) if *depth >= 0 => *depth as usize,
                    Some(_) => return Err(FolderError::Depth),
                };
                self.folders.projects.push(ProjectFolder { path, depth });
            }
            Some(Value::String(kind)) if kind == "scripts" => {
                if folder.contains_key("depth") {
                    config::warn("a scripts folder is searched whole; its `depth` is skipped");
                }
                self.folders.scripts.push(path);
            }
            _ => return Err(FolderError::Kind),
        }
        Ok(())
    }
}

/// Whether `program` is an executable file in one of the `path` folders.
fn on_path(program: &str, path: Option<&OsStr>) -> bool {
    path.is_some_and(|path| env::split_paths(path).any(|dir| executable(&dir.join(program))))
}

fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// `path` with a leading `~` made the home folder. It must be absolute.
fn expand(path: &str, home: Option<&Path>) -> Result<PathBuf, FolderError> {
    let expanded = match (path.strip_prefix('~'), home) {
        (Some(""), Some(home)) => home.to_owned(),
        (Some(rest), Some(home)) if rest.starts_with('/') => home.join(&rest[1..]),
        (Some(_), None) => return Err(FolderError::no_home(path)),
        _ => PathBuf::from(path),
    };
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Err(FolderError::relative(path))
    }
}

/// Why an entry of `folders` is left out. Its `Display` follows the entry's name in the log line.
#[derive(Debug, thiserror::Error)]
enum FolderError {
    #[error("should be a `{{ path, kind }}` table")]
    NotTable,
    #[error("needs a `path`")]
    NoPath,
    /// The path starts with `~`, and there is no home folder for it.
    #[error("has `{0}`, but HOME isn't set")]
    NoHome(String),
    #[error("has `{0}`, which should start with `/` or `~/`")]
    Relative(String),
    #[error("has a `depth` that isn't a whole number")]
    Depth,
    #[error("needs a `kind`, `projects` or `scripts`")]
    Kind,
}

impl FolderError {
    fn no_home(path: &str) -> Self {
        FolderError::NoHome(path.to_owned())
    }

    fn relative(path: &str) -> Self {
        FolderError::Relative(path.to_owned())
    }
}

/// `command` with `{path}` and `{session}` filled in, in every word.
pub fn fill(command: &[String], path: &Path, session: Option<&str>) -> Vec<String> {
    let path = path.to_string_lossy();
    command
        .iter()
        .map(|word| {
            let word = word.replace("{path}", &path);
            match session {
                Some(session) => word.replace("{session}", session),
                None => word,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
