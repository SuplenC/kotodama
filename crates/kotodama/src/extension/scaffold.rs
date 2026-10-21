//! `kotodama extension new <name>`: writes a small Extension to start from, in shell, Python or
//! Rust. Each offers one Item, which types a greeting, and answers the Live query
//! `<name> <text>` with `<text>`, to copy; each passes `kotodama extension check` as written.
//!
//! # Owns
//!
//! - The scaffolds, in `scaffold/`, and the words they are written with.
//!
//! # Not here
//!
//! - The example Extensions, which show more: `examples/`.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use kotodama_protocol::PROTOCOL_VERSION;
use kotodama_protocol::is_valid_name;

/// The name the Daemon's own Results go by, which no Extension may take.
const RESERVED_NAME: &str = "kotodama";
const MANIFEST: &str = include_str!("scaffold/manifest.toml.in");

/// The language a scaffold is written in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Language {
    Shell,
    /// The default: an interpreter every desktop has, and no build step.
    #[default]
    Python,
    Rust,
}

impl FromStr for Language {
    type Err = ();

    fn from_str(name: &str) -> Result<Self, ()> {
        match name {
            "shell" | "bash" | "sh" => Ok(Self::Shell),
            "python" | "py" => Ok(Self::Python),
            "rust" | "rs" => Ok(Self::Rust),
            _ => Err(()),
        }
    }
}

/// What to write, from the command line.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Options {
    pub(crate) name: String,
    pub(crate) language: Language,
    /// The directory to write the Extension's directory in.
    pub(crate) parent: PathBuf,
}

impl Options {
    #[must_use]
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            language: Language::default(),
            parent: PathBuf::from("."),
        }
    }
}

/// Why the scaffold wasn't written. The OS's reason is part of the message, as the command
/// prints the message alone.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ScaffoldError {
    #[error(
        "`{0}` can't name an Extension: use lowercase letters, digits and hyphens, starting with a letter or digit"
    )]
    InvalidName(String),
    #[error("the name `{RESERVED_NAME}` is the Daemon's own; pick another")]
    ReservedName,
    #[error("{} already exists", .0.display())]
    Exists(PathBuf),
    #[error("couldn't write {}: {reason}", path.display())]
    Write { path: PathBuf, reason: io::Error },
}

impl ScaffoldError {
    fn write(path: &Path) -> impl FnOnce(io::Error) -> Self {
        let path = path.to_owned();
        move |reason| Self::Write { path, reason }
    }
}

/// One file of a scaffold.
#[derive(Debug, PartialEq, Eq)]
struct File {
    /// Relative to the Extension's directory.
    path: &'static str,
    contents: String,
    executable: bool,
}

/// The files of the Extension `name` in `language`.
fn files(name: &str, language: Language) -> Vec<File> {
    let fill = |template: &str, command: &str| {
        template
            .replace("{{command}}", command)
            .replace("{{name}}", name)
            .replace("{{protocol}}", &PROTOCOL_VERSION.to_string())
    };
    let make = |path, contents, executable| File {
        path,
        contents,
        executable,
    };
    match language {
        Language::Shell => vec![
            make(
                "manifest.toml",
                fill(MANIFEST, r#"command = ["./extension.sh"]"#),
                false,
            ),
            make(
                "extension.sh",
                fill(include_str!("scaffold/shell/extension.sh.in"), ""),
                true,
            ),
        ],
        Language::Python => vec![
            make(
                "manifest.toml",
                fill(MANIFEST, r#"command = ["./extension.py"]"#),
                false,
            ),
            make(
                "extension.py",
                fill(include_str!("scaffold/python/extension.py.in"), ""),
                true,
            ),
        ],
        Language::Rust => vec![
            make(
                "manifest.toml",
                fill(
                    MANIFEST,
                    "# The binary `cargo install --path . --root .` builds.\ncommand = [\"bin/kotodama-{{name}}\"]",
                ),
                false,
            ),
            make(
                "Cargo.toml",
                fill(include_str!("scaffold/rust/Cargo.toml.in"), ""),
                false,
            ),
            make("src/main.rs", fill(include_str!("scaffold/rust/main.rs.in"), ""), false),
            make(
                ".gitignore",
                fill(include_str!("scaffold/rust/gitignore.in"), ""),
                false,
            ),
        ],
    }
}

/// Writes the scaffold to a new directory named after the Extension, and returns its path.
pub(crate) fn write(options: &Options) -> Result<PathBuf, ScaffoldError> {
    if !is_valid_name(&options.name) {
        return Err(ScaffoldError::InvalidName(options.name.clone()));
    }
    if options.name == RESERVED_NAME {
        return Err(ScaffoldError::ReservedName);
    }
    let dir = options.parent.join(&options.name);
    if dir.exists() {
        return Err(ScaffoldError::Exists(dir));
    }
    fs::create_dir_all(&options.parent).map_err(ScaffoldError::write(&options.parent))?;
    fs::create_dir(&dir).map_err(ScaffoldError::write(&dir))?;
    for file in files(&options.name, options.language) {
        let path = dir.join(file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ScaffoldError::write(parent))?;
        }
        fs::write(&path, &file.contents).map_err(ScaffoldError::write(&path))?;
        if file.executable {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).map_err(ScaffoldError::write(&path))?;
        }
    }
    Ok(dir)
}

/// What to do with the scaffold in `dir` once it is written.
pub(crate) fn next_steps(options: &Options, dir: &Path) -> String {
    let shown = dir.display();
    let installed = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_owned());
    let build = match options.language {
        Language::Rust => format!("  cargo install --path {shown} --root {shown}\n"),
        Language::Shell | Language::Python => String::new(),
    };
    format!(
        "Wrote the Extension `{name}` in {shown}. To try it:\n\n{build}  kotodama extension check {shown}\n  ln -s \
         \"{installed}\" ~/.config/kotodama/extensions/{name}\n\nthen type `{name} hello` in the menu.",
        name = options.name,
        installed = installed.display(),
    )
}

#[cfg(test)]
mod tests;
