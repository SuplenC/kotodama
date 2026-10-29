//! Which files a file field takes: those whose name ends in one of the
//! extensions its `accept` lists, or one a MIME type it lists is known by in
//! the shared MIME-info database (`$XDG_DATA_DIRS/mime/globs2`), which every
//! freedesktop system has.
//!
//! # Owns
//!
//! - [`Accept`], what one field takes, and [`Globs`], the MIME types' extensions read so far.
//!
//! # Not here
//!
//! - Listing and ranking the files: `completion.rs`.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

/// The name endings, `.conf` and the like, lowercase, that a field takes;
/// none takes any file.
#[derive(Debug, Default)]
pub struct Accept {
    endings: Vec<String>,
}

impl Accept {
    /// What `accept` takes: an extension as it is (`conf`, `tar.gz`), a
    /// MIME type (`application/pdf`, or `image/*` for every image) by the
    /// extensions `globs` knows it by.
    pub fn new(accept: &[String], globs: &mut Globs) -> Self {
        let mut endings = Vec::new();
        for wanted in accept {
            let wanted = wanted.trim().to_lowercase();
            if wanted.contains('/') {
                endings.extend(
                    globs
                        .extensions(&wanted)
                        .iter()
                        .map(|extension| format!(".{extension}")),
                );
            } else if !wanted.is_empty() {
                endings.push(format!(".{}", wanted.trim_start_matches('.')));
            }
        }
        endings.sort_unstable();
        endings.dedup();
        // A field that asks only for a MIME type nothing is known by takes
        // nothing, rather than everything.
        if endings.is_empty() && accept.iter().any(|wanted| !wanted.trim().is_empty()) {
            endings.push(String::from("/"));
        }
        Accept { endings }
    }

    /// Whether a file named `name` is one the field takes.
    pub fn takes(&self, name: &str) -> bool {
        if self.endings.is_empty() {
            return true;
        }
        let name = name.as_bytes();
        self.endings.iter().any(|ending| {
            name.len() > ending.len() && name[name.len() - ending.len()..].eq_ignore_ascii_case(ending.as_bytes())
        })
    }
}

/// The extensions of each MIME type asked about so far, from the shared
/// MIME-info database.
#[derive(Debug, Default)]
pub struct Globs {
    /// Where `globs2` files are read from; the XDG data directories when
    /// `None`.
    dirs: Option<Vec<PathBuf>>,
    known: HashMap<String, Vec<String>>,
}

impl Globs {
    /// Globs read from `<dir>/mime/globs2` of each of `dirs` alone.
    #[cfg(test)]
    pub fn in_dirs(dirs: Vec<PathBuf>) -> Self {
        Globs {
            dirs: Some(dirs),
            known: HashMap::new(),
        }
    }

    /// The extensions files of `mime` have, `image/*` standing for every
    /// image type.
    fn extensions(&mut self, mime: &str) -> &[String] {
        if !self.known.contains_key(mime) {
            let dirs = self.dirs.clone().unwrap_or_else(data_dirs);
            let mut extensions = Vec::new();
            for dir in dirs {
                if let Ok(globs) = fs::read_to_string(dir.join("mime/globs2")) {
                    extensions.extend(extensions_in(&globs, mime));
                }
            }
            extensions.sort_unstable();
            extensions.dedup();
            self.known.insert(mime.to_owned(), extensions);
        }
        &self.known[mime]
    }
}

/// The extensions `globs`, a `globs2` file, gives `mime`: from its lines
/// `weight:type/subtype:*.ext`, those whose pattern is a plain extension.
fn extensions_in<'a>(globs: &'a str, mime: &'a str) -> impl Iterator<Item = String> + 'a {
    let family = mime.strip_suffix('*');
    globs.lines().filter_map(move |line| {
        let mut fields = line.split(':');
        let (_weight, kind, pattern) = (fields.next()?, fields.next()?, fields.next()?);
        let matches = match family {
            Some(family) => kind.starts_with(family),
            None => kind == mime,
        };
        let extension = pattern.strip_prefix("*.")?;
        (matches && !extension.contains(['*', '?', '['])).then(|| extension.to_lowercase())
    })
}

/// `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`, with their usual defaults.
fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match env::var_os("XDG_DATA_HOME").filter(|dir| !dir.is_empty()) {
        Some(dir) => dirs.push(PathBuf::from(dir)),
        None => dirs.extend(env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))),
    }
    let shared = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    dirs.extend(shared.split(':').filter(|dir| !dir.is_empty()).map(PathBuf::from));
    dirs
}

#[cfg(test)]
mod tests;
