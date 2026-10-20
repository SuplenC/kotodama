//! Finding scripts: every executable file in a scripts folder and the
//! folders in it, with the options its header comment sets.

use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// How much of a script is read for its header.
const HEADER_BYTES: usize = 1024;
/// How many of its first lines may hold the header.
const HEADER_LINES: usize = 10;
/// How many subfolders deep scripts are looked for, so a stray dependency
/// tree in a scripts folder isn't searched and watched whole.
const MAX_DEPTH: usize = 4;
/// Shows a subfolder before the script's name: `work › backup`.
pub const SEPARATOR: &str = " › ";

/// One executable file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Script {
    pub path: PathBuf,
    /// The subfolders it is in, from the scripts folder down.
    pub folders: Vec<String>,
    /// Its file name, without the extension.
    pub stem: String,
    pub header: Header,
}

impl Script {
    /// Its subfolders, then its name: the header's, or its file's.
    pub fn title(&self) -> String {
        let name = self.header.name.as_deref().unwrap_or(&self.stem);
        let mut title = String::new();
        for folder in &self.folders {
            title.push_str(folder);
            title.push_str(SEPARATOR);
        }
        title.push_str(name);
        title
    }
}

/// The options in a `# kotodama: name=Deploy staging terminal=false
/// args=prompt` comment, among the script's first lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    /// Shown instead of the file's name.
    pub name: Option<String>,
    /// Runs in a terminal; `false` runs it in the background, as a job.
    pub terminal: bool,
    /// Asks for its arguments before it runs.
    pub prompt: bool,
}

impl Default for Header {
    fn default() -> Self {
        Header {
            name: None,
            terminal: true,
            prompt: false,
        }
    }
}

impl Header {
    /// Reads the header from the start of a script. The first comment line
    /// that starts with `kotodama:` is the header; an option it doesn't know
    /// is skipped with a warning.
    pub fn parse(text: &str) -> Header {
        let mut header = Header::default();
        let Some(options) = text.lines().take(HEADER_LINES).find_map(|line| {
            let line = line.trim_start();
            let comment = line
                .strip_prefix("//")
                .or_else(|| line.strip_prefix("--"))
                .or_else(|| line.starts_with('#').then(|| line.trim_start_matches('#')))?;
            comment.trim_start().strip_prefix("kotodama:")
        }) else {
            return header;
        };
        // A value runs on to the next `key=`, so a name can have spaces.
        let mut pairs: Vec<(&str, String)> = Vec::new();
        for word in options.split_whitespace() {
            match word.split_once('=') {
                Some((key, value))
                    if !key.is_empty() && key.bytes().all(|b| b.is_ascii_lowercase()) =>
                {
                    pairs.push((key, value.to_owned()));
                }
                _ => match pairs.last_mut() {
                    Some((_, value)) => {
                        value.push(' ');
                        value.push_str(word);
                    }
                    None => warn(&format!("header: `{word}` isn't a `key=value` option")),
                },
            }
        }
        for (key, value) in pairs {
            let value = unquote(&value);
            match (key, value) {
                ("name", "") => warn("header: `name=` needs a name"),
                ("name", name) => header.name = Some(name.to_owned()),
                ("terminal", "true") => header.terminal = true,
                ("terminal", "false") => header.terminal = false,
                ("args", "prompt") => header.prompt = true,
                ("args", "none") => header.prompt = false,
                _ => warn(&format!(
                    "header: skipped the unknown option `{key}={value}`"
                )),
            }
        }
        header
    }

    /// Reads the header of the script at `path`, from its first bytes only.
    fn read(path: &Path) -> Header {
        let mut start = Vec::with_capacity(HEADER_BYTES);
        match File::open(path)
            .and_then(|file| file.take(HEADER_BYTES as u64).read_to_end(&mut start))
        {
            Ok(_) => Header::parse(&String::from_utf8_lossy(&start)),
            Err(_) => Header::default(),
        }
    }
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|value| value.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

/// What a search found.
#[derive(Debug, Default)]
pub struct Found {
    pub scripts: Vec<Script>,
    /// The folders searched, whose changes can add or remove a script.
    pub dirs: Vec<PathBuf>,
}

/// Finds the scripts in `folder` and its subfolders, down to [`MAX_DEPTH`].
/// Hidden files and folders are skipped, as a `.git` folder's hooks would
/// be, and so are links to folders, so a loop can't be followed; links to
/// files count.
pub fn find(folder: &Path, found: &mut Found) {
    search(folder, &mut Vec::new(), found);
}

fn search(dir: &Path, folders: &mut Vec<String>, found: &mut Found) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    found.dirs.push(dir.to_owned());
    let mut entries: Vec<_> = entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() {
            if folders.len() == MAX_DEPTH {
                continue;
            }
            folders.push(name);
            search(&path, folders, found);
            folders.pop();
            continue;
        }
        // Follows a link, to see what it points at.
        let is_executable = fs::metadata(&path)
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0);
        if !is_executable {
            continue;
        }
        let stem = match name.rsplit_once('.') {
            Some((stem, _)) if !stem.is_empty() => stem.to_owned(),
            _ => name,
        };
        let header = Header::read(&path);
        found.scripts.push(Script {
            path,
            folders: folders.clone(),
            stem,
            header,
        });
    }
}

fn warn(message: &str) {
    eprintln!("kotodama-folders: {message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_sets_a_name_with_spaces_the_terminal_and_a_prompt() {
        let header = Header::parse(
            "#!/usr/bin/env bash\n# kotodama: name=Deploy staging terminal=false args=prompt\necho\n",
        );
        assert_eq!(
            header,
            Header {
                name: Some("Deploy staging".into()),
                terminal: false,
                prompt: true,
            }
        );
    }

    #[test]
    fn without_a_header_a_script_runs_in_a_terminal_without_asking() {
        assert_eq!(
            Header::parse("#!/bin/sh\necho kotodama: name=x\n"),
            Header::default()
        );
    }

    #[test]
    fn other_comment_styles_and_quotes_work_too() {
        assert_eq!(
            Header::parse("#!/usr/bin/env node\n// kotodama: name=\"Say hi\"\n").name,
            Some("Say hi".into())
        );
        assert!(!Header::parse("-- kotodama: terminal=false\n").terminal);
        assert!(Header::parse("## kotodama: args=prompt").prompt);
    }

    #[test]
    fn unknown_options_are_skipped_and_the_rest_apply() {
        let header = Header::parse("# kotodama: colour=red terminal=false name=x");
        assert!(!header.terminal);
        assert_eq!(header.name.as_deref(), Some("x"));
    }

    #[test]
    fn a_header_past_the_first_lines_is_not_one() {
        let text = format!("{}# kotodama: name=late\n", "#\n".repeat(HEADER_LINES));
        assert_eq!(Header::parse(&text).name, None);
    }
}
