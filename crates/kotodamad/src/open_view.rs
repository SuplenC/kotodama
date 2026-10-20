//! `kotodama open --view <extension>/<item>`: what it names, and the request
//! the Daemon holds back from the menus until the View is there (ADR 0013).

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
mod tests {
    use super::*;

    #[test]
    fn a_view_name_is_an_extension_and_an_item_id_that_may_hold_slashes() {
        assert_eq!(
            ViewName::parse("folders/src/kotodama"),
            Some(ViewName {
                extension: "folders".into(),
                item: "src/kotodama".into(),
            })
        );
        assert_eq!(
            ViewName::parse("clipboard/history").map(|name| name.to_string()),
            Some("clipboard/history".into())
        );
        for bad in ["history", "/history", "clipboard/", ""] {
            assert_eq!(ViewName::parse(bad), None, "{bad}");
        }
    }
}
