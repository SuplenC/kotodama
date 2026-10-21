//! Action key bindings, such as `ctrl+o`, matched against Qt's key events.
//! Nothing here touches Qt, so it is tested without it.
//!
//! # Owns
//!
//! - [`matches`]: whether a binding names a key event, and the key and modifier names a binding may
//!   use.
//!
//! # Not here
//!
//! - Which Action a key runs: `KotodamaSearch::run_shortcut` in `lib.rs`.
//! - Which shortcut a contributed Action keeps: `rows`.

#![forbid(unsafe_code)]

// Qt::KeyboardModifier.
const SHIFT: i32 = 0x0200_0000;
const CONTROL: i32 = 0x0400_0000;
const ALT: i32 = 0x0800_0000;
const META: i32 = 0x1000_0000;
const MODIFIERS: i32 = SHIFT | CONTROL | ALT | META;

// Qt::Key.
const F1: i32 = 0x0100_0030;

/// Whether `shortcut` names the key `key` (a `Qt::Key`) pressed with exactly
/// `modifiers` (`Qt::KeyboardModifiers`). Names are case-insensitive. A binding
/// without Ctrl, Alt or Super never matches, since the key alone types into the
/// search.
pub fn matches(shortcut: &str, key: i32, modifiers: i32) -> bool {
    // Checked on every key pressed with a modifier, so nothing is allocated.
    let mut parts = shortcut.split('+').map(str::trim).rev();
    let Some(keys) = parts.next().and_then(key_codes) else {
        return false;
    };
    let mut wanted = 0;
    for part in parts {
        let Some(modifier) = named(&MODIFIER_NAMES, part) else {
            return false;
        };
        wanted |= modifier;
    }
    wanted & (CONTROL | ALT | META) != 0 && wanted == modifiers & MODIFIERS && keys.contains(&key)
}

const MODIFIER_NAMES: [(&str, i32); 7] = [
    ("ctrl", CONTROL),
    ("control", CONTROL),
    ("shift", SHIFT),
    ("alt", ALT),
    ("super", META),
    ("meta", META),
    ("cmd", META),
];

/// Key names, each with the two codes it may arrive as; most keys have one,
/// given twice.
const KEY_NAMES: [(&str, [i32; 2]); 15] = [
    ("enter", [0x0100_0004, 0x0100_0005]),
    ("return", [0x0100_0004, 0x0100_0005]),
    ("tab", [0x0100_0001; 2]),
    ("backspace", [0x0100_0003; 2]),
    ("delete", [0x0100_0007; 2]),
    ("del", [0x0100_0007; 2]),
    ("space", [0x20; 2]),
    ("home", [0x0100_0010; 2]),
    ("end", [0x0100_0011; 2]),
    ("left", [0x0100_0012; 2]),
    ("up", [0x0100_0013; 2]),
    ("right", [0x0100_0014; 2]),
    ("down", [0x0100_0015; 2]),
    ("pageup", [0x0100_0016; 2]),
    ("pagedown", [0x0100_0017; 2]),
];

/// The value `names` gives `name`, ignoring case.
fn named<T: Copy>(names: &[(&str, T)], name: &str) -> Option<T> {
    names
        .iter()
        .find(|(known, _)| name.eq_ignore_ascii_case(known))
        .map(|&(_, value)| value)
}

/// The `Qt::Key` codes a key name may arrive as, or `None` if it names no key.
/// A one-character name is a key Qt names by its uppercase character.
fn key_codes(name: &str) -> Option<[i32; 2]> {
    if let Some(codes) = named(&KEY_NAMES, name) {
        return Some(codes);
    }
    if let Some(n) = name
        .strip_prefix(['f', 'F'])
        .and_then(|n| n.parse::<i32>().ok())
        .filter(|n| (1..=12).contains(n))
    {
        return Some([F1 + n - 1; 2]);
    }
    match name.as_bytes() {
        [c] if c.is_ascii_graphic() => Some([i32::from(c.to_ascii_uppercase()); 2]),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
