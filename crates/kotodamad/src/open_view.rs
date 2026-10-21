//! `kotodama open --view <extension>/<item>`: what it names, and the request
//! the Daemon holds back from the menus until the View is there.
//!
//! # Owns
//!
//! - [`ViewName`]: the View named on the command line.
//! - [`Opening`]: the request held back while the View opens.
//!
//! # Not here
//!
//! - Opening it and holding the menus back: `daemon/openings.rs`.

use std::fmt;
use std::time::Instant;

use crate::daemon::FrontEndId;

/// The Item `--view` names, whose primary Action pushes the View. Extension
/// names have no `/`, so the text splits at the first one, and the Item id
/// may have more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewName {
    pub extension: String,
    pub item: String,
}

impl ViewName {
    pub fn parse(text: &str) -> Option<Self> {
        let (extension, item) = text.split_once('/')?;
        (!extension.is_empty() && !item.is_empty()).then(|| ViewName {
            extension: extension.to_owned(),
            item: item.to_owned(),
        })
    }
}

impl fmt::Display for ViewName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}/{}", self.extension, self.item)
    }
}

/// A `kotodama open --view` the menus wait on.
pub struct Opening {
    /// The `kotodama` command that asked; the menus open everywhere else.
    pub from: FrontEndId,
    pub query: String,
    pub view: ViewName,
    /// The invocation of the Item's primary Action, once it runs. `None`
    /// while the Extension starts or hasn't indexed the Item yet.
    pub running: Option<u64>,
    /// The menus opened on the search before the Action was done.
    pub opened: bool,
    pub deadline: Instant,
}

impl Opening {
    /// Whether the menus still wait on it.
    pub fn awaited(&self) -> bool {
        !self.opened
    }
}

#[cfg(test)]
mod tests;
