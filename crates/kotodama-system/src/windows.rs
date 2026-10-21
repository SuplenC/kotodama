//! The open windows, as wlr-foreign-toplevel-management reports them, as
//! Items of the shared kind `window`, and what each window Action asks the
//! compositor.
//!
//! A toplevel has no name of its own in the protocol, so each gets a number,
//! counted up for as long as the Extension runs: a window closed and another
//! opened never share an Item id, so neither inherits the other's History.
//!
//! # Owns
//!
//! - The windows by number, and which changes make their Items change.
//! - The windows' Items, and the [`Request`] each window Action makes.
//!
//! # Not here
//!
//! - The Wayland connection, and sending a [`Request`]: [`crate::toplevels`].
//! - Sending the Items: `main.rs`.

use std::collections::BTreeMap;

use kotodama_protocol::Action;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;

/// The Action that focuses a window.
pub const FOCUS: &str = "focus";
/// The Action that closes a window.
pub const CLOSE: &str = "close";
/// The Action that minimizes a window.
pub const MINIMIZE: &str = "minimize";
/// The Action that maximizes a window, or puts a maximized one back.
pub const MAXIMIZE: &str = "maximize";
/// The Action that makes a window fullscreen, or puts a fullscreen one back.
pub const FULLSCREEN: &str = "fullscreen";
/// What a window's Item id starts with; its number follows.
const ID: &str = "window-";

/// One toplevel, as the compositor last described it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Window {
    pub title: String,
    /// The Wayland app id, most often the app's desktop id.
    pub app_id: String,
    pub maximized: bool,
    pub minimized: bool,
    pub fullscreen: bool,
}

/// What a window Action asks the compositor to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Activate,
    Close,
    Minimize,
    SetMaximized(bool),
    SetFullscreen(bool),
}

/// Why a window Action can't run.
#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("That window is closed")]
    Gone,
    #[error("A window has no Action `{action}`")]
    UnknownAction { action: String },
}

impl RequestError {
    fn unknown_action(action: &str) -> Self {
        RequestError::UnknownAction {
            action: action.to_owned(),
        }
    }
}

/// The open windows, by number.
#[derive(Debug, Default)]
pub struct Windows(BTreeMap<u64, Window>);

impl Windows {
    /// Takes in the compositor's latest description of window `number`; says whether
    /// its Item changed. Focus and the window's state change no Item: the
    /// compositor reports them on every focus change, and the Item shows
    /// neither.
    pub fn update(&mut self, number: u64, window: Window) -> bool {
        let changed = self
            .0
            .get(&number)
            .is_none_or(|old| old.title != window.title || old.app_id != window.app_id);
        self.0.insert(number, window);
        changed
    }

    /// Forgets window `number`; says whether it was there.
    pub fn remove(&mut self, number: u64) -> bool {
        self.0.remove(&number).is_some()
    }

    /// Forgets every window, as when the compositor is gone.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Every window as an Item, by number.
    pub fn items(&self) -> impl Iterator<Item = Item> + '_ {
        self.0.iter().map(|(&number, window)| item(number, window))
    }

    /// The window an Item names by its kind's address, and the request
    /// `action` makes of it.
    ///
    /// # Errors
    ///
    /// [`RequestError`] when the window is gone or `action` isn't a window Action.
    pub fn request(&self, action: &str, address: &str) -> Result<(u64, Request), RequestError> {
        let number = number(address).ok_or(RequestError::Gone)?;
        let window = self.0.get(&number).ok_or(RequestError::Gone)?;
        let request = match action {
            FOCUS => Request::Activate,
            CLOSE => Request::Close,
            MINIMIZE => Request::Minimize,
            MAXIMIZE => Request::SetMaximized(!window.maximized),
            FULLSCREEN => Request::SetFullscreen(!window.fullscreen),
            _ => return Err(RequestError::unknown_action(action)),
        };
        Ok((number, request))
    }
}

/// The number in a window's Item id.
fn number(id: &str) -> Option<u64> {
    id.strip_prefix(ID)?.parse().ok()
}

fn item(number: u64, window: &Window) -> Item {
    let id = format!("{ID}{number}");
    let title = if window.title.is_empty() {
        window.app_id.clone()
    } else {
        window.title.clone()
    };
    Item {
        kind: ItemKind::Window { address: id.clone() },
        id,
        identity: None,
        title,
        subtitle: (!window.app_id.is_empty()).then(|| window.app_id.clone()),
        // An app id is most often the app's desktop id, so its icon's name.
        icon: (!window.app_id.is_empty()).then(|| window.app_id.clone()),
        keyword: None,
        keywords: vec![window.app_id.clone()],
        hidden: false,
        actions: vec![
            Action::new(FOCUS, "Focus Window"),
            Action::new(CLOSE, "Close Window"),
            Action::new(MINIMIZE, "Minimize"),
            Action::new(MAXIMIZE, "Toggle Maximized"),
            Action::new(FULLSCREEN, "Toggle Fullscreen"),
        ],
    }
}

#[cfg(test)]
mod tests;
