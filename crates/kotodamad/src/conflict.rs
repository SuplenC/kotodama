//! Keywords two things ask for. The Aliases, the Quicklinks and the apps'
//! words make one table, read whenever the config or the installed
//! Extensions change, in which each keyword has one holder, by precedence:
//!
//! 1. an Alias;
//! 2. a Quicklink;
//! 3. an app's word the user's `[extensions.<name>] word` sets;
//! 4. an app's word its Manifest sets;
//! 5. an app's Extension name, which stands in without a word;
//!
//! and between two apps on one step, the Extension whose name sorts first.
//! An app whose word is held goes by its Extension's name, and if that is
//! held too, by the name and `-2`, `-3` and so on, so its Items stay
//! reachable.
//!
//! # Owns
//!
//! - [`Conflict`]: what lost a keyword, and what it got instead.
//! - [`report`]: the one place conflicts are told to the user.
//!
//! # Not here
//!
//! - Reading the Aliases and Quicklinks: `keywords.rs`.
//! - Giving each app its word: `apps.rs`, `Apps::new`.

use std::fmt;

use crate::problem::Report;

/// What holds a keyword.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Holder {
    Alias,
    Quicklink,
    /// The app of the Extension of this name.
    App(String),
}

/// A keyword two things asked for, and what the one that lost it got.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Conflict {
    /// An Alias and a Quicklink share `keyword`. The Alias's Item goes
    /// first; the Quicklink still matches it, below.
    QuicklinkUnderAlias { keyword: String },
    /// The app of `extension` asked for `word`, which `holder` holds, and
    /// goes by `instead`.
    WordTaken {
        extension: String,
        word: String,
        holder: Holder,
        instead: String,
    },
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Conflict::QuicklinkUnderAlias { keyword } => write!(
                f,
                "the Alias `{keyword}` and the Quicklink `{keyword}` share a keyword; the Alias goes first"
            ),
            Conflict::WordTaken {
                extension,
                word,
                holder,
                instead,
            } => {
                let held = match holder {
                    Holder::Alias => "an Alias".to_owned(),
                    Holder::Quicklink => "a Quicklink".to_owned(),
                    Holder::App(other) => format!("the {other} app's word"),
                };
                write!(
                    f,
                    "the {extension} app's word `{word}` is {held}; the app goes by `{instead}`"
                )
            }
        }
    }
}

impl Conflict {
    /// The keyword two things ask for.
    pub fn keyword(&self) -> &str {
        match self {
            Conflict::QuicklinkUnderAlias { keyword } => keyword,
            Conflict::WordTaken { word, .. } => word,
        }
    }
}

/// Tells the user about each conflict, as a problem.
pub fn report(conflicts: &[Conflict], report: &mut Report) {
    for conflict in conflicts {
        report.add(format!("keyword `{}`", conflict.keyword()), conflict.to_string());
    }
}
