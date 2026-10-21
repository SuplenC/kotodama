//! The key bindings a Front end keeps for itself. An Extension's Action
//! bound to one of them never gets it: the Daemon drops the binding.
//!
//! # Owns
//!
//! - [`RESERVED_SHORTCUTS`] and [`is_reserved_shortcut`].
//! - The keys of the Daemon's own Forget and of undoing a change to the History.
//!
//! # Not here
//!
//! - Matching a binding against a key event: the Front end's (`kotodama-qml`'s `shortcut.rs`).
//! - Dropping reserved bindings from what Extensions send: the Daemon's `reserved.rs`.

/// The key of the Daemon's own Forget Action in the Action panel.
pub const FORGET_SHORTCUT: &str = "ctrl+shift+backspace";
/// The key that undoes the change to the History a Front end offers to undo.
pub const UNDO_SHORTCUT: &str = "ctrl+z";
/// Every binding no Extension's Action may take: the Action panel (Ctrl+K),
/// moving the selection (Ctrl+N, Ctrl+P), undo, and Forget.
pub const RESERVED_SHORTCUTS: [&str; 5] = ["ctrl+k", "ctrl+n", "ctrl+p", UNDO_SHORTCUT, FORGET_SHORTCUT];

/// Whether `shortcut` names one of the [`RESERVED_SHORTCUTS`], however it is
/// spelled: names are case-insensitive, modifiers go in any order, and
/// `control`, `meta`, `cmd`, `return` and `del` are the same keys as `ctrl`,
/// `super`, `super`, `enter` and `delete`.
///
/// # Examples
///
/// ```
/// use kotodama_protocol::is_reserved_shortcut;
///
/// assert!(is_reserved_shortcut("Shift+Ctrl+Backspace"));
/// assert!(!is_reserved_shortcut("ctrl+o"));
/// ```
#[must_use]
pub fn is_reserved_shortcut(shortcut: &str) -> bool {
    let Some(keys) = Keys::parse(shortcut) else {
        return false;
    };
    RESERVED_SHORTCUTS
        .iter()
        .filter_map(|reserved| Keys::parse(reserved))
        .any(|reserved| reserved == keys)
}

const CTRL: u8 = 1;
const SHIFT: u8 = 2;
const ALT: u8 = 4;
const SUPER: u8 = 8;

const MODIFIER_NAMES: [(&str, u8); 7] = [
    ("ctrl", CTRL),
    ("control", CTRL),
    ("shift", SHIFT),
    ("alt", ALT),
    ("super", SUPER),
    ("meta", SUPER),
    ("cmd", SUPER),
];

const KEY_ALIASES: [(&str, &str); 2] = [("return", "enter"), ("del", "delete")];

/// A binding as the modifiers held and the key pressed.
#[derive(Debug)]
struct Keys<'a> {
    modifiers: u8,
    key: &'a str,
}

impl<'a> Keys<'a> {
    /// The keys `shortcut` names, or `None` if it names no key or an unknown
    /// modifier.
    fn parse(shortcut: &'a str) -> Option<Self> {
        let mut parts = shortcut.split('+').map(str::trim).rev();
        let key = parts.next().filter(|key| !key.is_empty())?;
        let key = KEY_ALIASES
            .iter()
            .find(|(alias, _)| key.eq_ignore_ascii_case(alias))
            .map_or(key, |&(_, name)| name);
        let mut modifiers = 0;
        for part in parts {
            let (_, modifier) = MODIFIER_NAMES
                .iter()
                .find(|(name, _)| part.eq_ignore_ascii_case(name))?;
            modifiers |= modifier;
        }
        Some(Keys { modifiers, key })
    }
}

impl PartialEq for Keys<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.modifiers == other.modifiers && self.key.eq_ignore_ascii_case(other.key)
    }
}

#[cfg(test)]
mod tests;
