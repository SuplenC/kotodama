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
//! terminal = ["ghostty", "--wait-after-command=true", "-e"]
//! # The commands a project's Actions run, in the project's folder. `{path}` is
//! # the folder and `{session}` its tmux session name.
//! open = ["ghostty", "-e", "tmux", "new-session", "-A", "-s", "{session}", "-c", "{path}", "nvim"]
//! shell = ["ghostty", "--working-directory={path}"]
//! lazygit = ["ghostty", "-e", "lazygit", "--path", "{path}"]
//! file_manager = ["xdg-open", "{path}"]
//! copy_path = ["wl-copy", "{path}"]
//! ```

use std::path::{Path, PathBuf};

use kotodama_extension::config;
use kotodama_protocol::{Table, Value};

use crate::projects::ProjectFolder;
use crate::scan::Folders;

/// How deep projects are looked for when a folder doesn't say.
pub const DEFAULT_DEPTH: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub folders: Folders,
    pub terminal: Vec<String>,
    pub open: Vec<String>,
    pub shell: Vec<String>,
    pub lazygit: Vec<String>,
    pub file_manager: Vec<String>,
    pub copy_path: Vec<String>,
}

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|&word| word.to_owned()).collect()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            folders: Folders::default(),
            terminal: words(&["ghostty", "--wait-after-command=true", "-e"]),
            open: words(&[
                "ghostty",
                "-e",
                "tmux",
                "new-session",
                "-A",
                "-s",
                "{session}",
                "-c",
                "{path}",
                "nvim",
            ]),
            shell: words(&["ghostty", "--working-directory={path}"]),
            lazygit: words(&["ghostty", "-e", "lazygit", "--path", "{path}"]),
            file_manager: words(&["xdg-open", "{path}"]),
            copy_path: words(&["wl-copy", "{path}"]),
        }
    }
}

impl Settings {
    /// Reads the config table. A setting of the wrong shape is logged and left
    /// at its default, and a folder that is wrong is left out; the rest still
    /// apply. `home` stands for a leading `~`.
    pub fn from_config(config: &Table, home: Option<&Path>) -> Self {
        let mut settings = Settings::default();
        match config.get("folders") {
            Some(Value::Array(folders)) => {
                for (n, folder) in folders.iter().enumerate() {
                    if let Err(message) = settings.add_folder(folder, home) {
                        config::warn(format_args!("`folders[{n}]` {message}"));
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
            ("lazygit", &mut settings.lazygit),
            ("file_manager", &mut settings.file_manager),
            ("copy_path", &mut settings.copy_path),
        ] {
            if let Some(command) = config::command(config, key) {
                *setting = command;
            }
        }
        settings
    }

    fn add_folder(&mut self, folder: &Value, home: Option<&Path>) -> Result<(), String> {
        let Value::Table(folder) = folder else {
            return Err("should be a `{ path, kind }` table".into());
        };
        let path = match folder.get("path") {
            Some(Value::String(path)) => expand(path, home)?,
            _ => return Err("needs a `path`".into()),
        };
        match folder.get("kind") {
            Some(Value::String(kind)) if kind == "projects" => {
                let depth = match folder.get("depth") {
                    None => DEFAULT_DEPTH,
                    Some(Value::Integer(depth)) if *depth >= 0 => *depth as usize,
                    Some(_) => return Err("has a `depth` that isn't a whole number".into()),
                };
                self.folders.projects.push(ProjectFolder { path, depth });
            }
            Some(Value::String(kind)) if kind == "scripts" => {
                if folder.contains_key("depth") {
                    config::warn("a scripts folder is searched whole; its `depth` is skipped");
                }
                self.folders.scripts.push(path);
            }
            _ => return Err("needs a `kind`, `projects` or `scripts`".into()),
        }
        Ok(())
    }
}

/// `path` with a leading `~` made the home folder. It must be absolute.
fn expand(path: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let expanded = match (path.strip_prefix('~'), home) {
        (Some(""), Some(home)) => home.to_owned(),
        (Some(rest), Some(home)) if rest.starts_with('/') => home.join(&rest[1..]),
        (Some(_), None) => return Err(format!("has `{path}`, but HOME isn't set")),
        _ => PathBuf::from(path),
    };
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Err(format!("has `{path}`, which should start with `/` or `~/`"))
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
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn table(pairs: &[(&str, Value)]) -> Table {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect::<BTreeMap<_, _>>()
    }

    fn string(text: &str) -> Value {
        Value::String(text.into())
    }

    #[test]
    fn folders_are_read_with_their_kind_and_depth_and_a_wrong_one_is_left_out() {
        let config = table(&[(
            "folders",
            Value::Array(vec![
                Value::Table(table(&[
                    ("path", string("~/Projects")),
                    ("kind", string("projects")),
                    ("depth", Value::Integer(1)),
                ])),
                Value::Table(table(&[
                    ("path", string("/srv/code")),
                    ("kind", string("projects")),
                ])),
                Value::Table(table(&[
                    ("path", string("~/Scripts")),
                    ("kind", string("scripts")),
                ])),
                Value::Table(table(&[
                    ("path", string("relative")),
                    ("kind", string("scripts")),
                ])),
                Value::Table(table(&[("path", string("/x")), ("kind", string("music"))])),
            ]),
        )]);
        let settings = Settings::from_config(&config, Some(Path::new("/home/me")));
        assert_eq!(
            settings.folders.projects,
            [
                ProjectFolder {
                    path: "/home/me/Projects".into(),
                    depth: 1
                },
                ProjectFolder {
                    path: "/srv/code".into(),
                    depth: DEFAULT_DEPTH
                },
            ]
        );
        assert_eq!(
            settings.folders.scripts,
            [PathBuf::from("/home/me/Scripts")]
        );
    }

    #[test]
    fn a_command_of_the_wrong_shape_keeps_its_default() {
        let config = table(&[
            ("open", string("nvim")),
            ("terminal", Value::Array(vec![string("foot")])),
        ]);
        let settings = Settings::from_config(&config, None);
        assert_eq!(settings.open, Settings::default().open);
        assert_eq!(settings.terminal, ["foot"]);
    }

    #[test]
    fn placeholders_are_filled_in_inside_words() {
        let command = words(&["tmux", "-s", "{session}", "--dir={path}"]);
        assert_eq!(
            fill(&command, Path::new("/p/my app"), Some("my app")),
            ["tmux", "-s", "my app", "--dir=/p/my app"]
        );
    }
}
