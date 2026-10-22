//! What a key is called under the current keyboard layout, for the binds
//! made on a keycode (`code:12`), which `j/binds` gives no key name.
//!
//! The keymap is the one Hyprland hands every Wayland client (see
//! `crate::keyboard`), in xkb's text format, already compiled: every key
//! named in `xkb_keycodes` and its keysyms listed in `xkb_symbols`, one list
//! per layout. Reading the two sections is all it takes, so no xkb library is
//! needed. The current layout is the index `j/devices` gives the main
//! keyboard.
//!
//! Hyprland serializes its keymap without pretty-printing, which writes each
//! keysym as its number (`0x31`), not its name (`1`). A character's keysym
//! maps to the character, and a few common others to their names.
//!
//! # Owns
//!
//! - Reading a keymap's keycodes and the keysym each key types without modifiers, per layout.
//! - Reading the main keyboard's current layout from `j/devices`.
//!
//! # Not here
//!
//! - Fetching the keymap over Wayland: `crate::keyboard`.
//! - Naming a bind's key: [`crate::binds::name_keys`].

use std::borrow::Cow;
use std::collections::HashMap;

use serde::Deserialize;

/// A keymap in xkb's text format, borrowed from its text.
#[derive(Debug, Default)]
pub struct Keymap<'a> {
    /// Each key's name in `xkb_keycodes`, as `AE03`, by keycode.
    names: HashMap<u32, &'a str>,
    /// What is between the braces of each `key <NAME> { … }` in
    /// `xkb_symbols`, by the key's name.
    keys: HashMap<&'a str, &'a str>,
}

impl<'a> Keymap<'a> {
    /// Reads `text`. What it can't read is left out, so a key in it has no
    /// name rather than a wrong one.
    pub fn parse(text: &'a str) -> Keymap<'a> {
        Keymap {
            names: section(text, "xkb_keycodes").map(keycodes).unwrap_or_default(),
            keys: section(text, "xkb_symbols").map(symbols).unwrap_or_default(),
        }
    }

    /// What the key `keycode` types without modifiers in `layout` (0 is the
    /// first), as `3`, `"` or `Escape`. A key with fewer layouts than that
    /// wraps around, as xkb does. `None` for a key the keymap doesn't have,
    /// one that types nothing, or a keysym written as a number this doesn't
    /// know.
    pub fn keysym(&self, keycode: u32, layout: u32) -> Option<Cow<'a, str>> {
        let name = self.names.get(&keycode)?;
        let groups = groups(self.keys.get(name)?);
        let count = u32::try_from(groups.len()).ok().filter(|&count| count > 0)?;
        let group = usize::try_from(layout % count).ok()?;
        let keysym = groups[group].filter(|&keysym| keysym != "NoSymbol")?;
        match keysym.strip_prefix("0x") {
            Some(hex) => name_of(u32::from_str_radix(hex, 16).ok()?),
            None => Some(Cow::Borrowed(keysym)),
        }
    }
}

/// The keysyms with a name that isn't the character they type, among those
/// a bind is likely made on.
const NAMED: [(u32, &str); 32] = [
    (0x20, "space"),
    (0xff08, "BackSpace"),
    (0xff09, "Tab"),
    (0xff0d, "Return"),
    (0xff13, "Pause"),
    (0xff14, "Scroll_Lock"),
    (0xff1b, "Escape"),
    (0xff50, "Home"),
    (0xff51, "Left"),
    (0xff52, "Up"),
    (0xff53, "Right"),
    (0xff54, "Down"),
    (0xff55, "Prior"),
    (0xff56, "Next"),
    (0xff57, "End"),
    (0xff61, "Print"),
    (0xff63, "Insert"),
    (0xff67, "Menu"),
    (0xff7f, "Num_Lock"),
    (0xff8d, "KP_Enter"),
    (0xffe5, "Caps_Lock"),
    (0xffff, "Delete"),
    (0x1008_ff02, "XF86MonBrightnessUp"),
    (0x1008_ff03, "XF86MonBrightnessDown"),
    (0x1008_ff11, "XF86AudioLowerVolume"),
    (0x1008_ff12, "XF86AudioMute"),
    (0x1008_ff13, "XF86AudioRaiseVolume"),
    (0x1008_ff14, "XF86AudioPlay"),
    (0x1008_ff15, "XF86AudioStop"),
    (0x1008_ff16, "XF86AudioPrev"),
    (0x1008_ff17, "XF86AudioNext"),
    (0x1008_ff30, "XF86Favorites"),
];

/// The first function key's keysym, `F1`; `F35`'s is 34 past it.
const F1: u32 = 0xffbe;

/// The name of the keysym `value`, as xkbcommon writes a keysym when it
/// serializes a keymap without pretty-printing, as it does for Wayland
/// clients: a character's keysym is that character.
fn name_of(value: u32) -> Option<Cow<'static, str>> {
    if let Some((_, name)) = NAMED.iter().find(|(keysym, _)| *keysym == value) {
        return Some(Cow::Borrowed(name));
    }
    if (F1..F1 + 35).contains(&value) {
        return Some(Cow::Owned(format!("F{}", value - F1 + 1)));
    }
    // Latin-1 keysyms are their code points, and every other character's
    // is its code point plus 0x0100_0000.
    let code = match value {
        0x21..=0x7e | 0xa1..=0xff => value,
        0x0100_0100..=0x0110_ffff => value - 0x0100_0000,
        _ => return None,
    };
    char::from_u32(code).map(|c| Cow::Owned(c.to_string()))
}

/// The body of the `xkb_<kind>` section of a keymap, between its braces.
fn section<'a>(text: &'a str, kind: &str) -> Option<&'a str> {
    let start = text.find(kind)?;
    let open = start + text[start..].find('{')? + 1;
    // A section's body holds blocks of its own; the section ends where the
    // braces opened in it are closed.
    let mut depth = 1_usize;
    for (at, c) in text[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[open..open + at]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The `<NAME> = 12;` lines of `xkb_keycodes`, by keycode. An `alias` line
/// starts with its keyword, so it is left out: `xkb_symbols` names each key
/// by the name its keycode has.
fn keycodes(body: &str) -> HashMap<u32, &str> {
    body.lines()
        .filter_map(|line| {
            let line = line.trim().strip_prefix('<')?;
            let (name, rest) = line.split_once('>')?;
            let code = rest.trim().strip_prefix('=')?.trim().strip_suffix(';')?.trim();
            Some((code.parse().ok()?, name))
        })
        .collect()
}

/// Each `key <NAME> { … };` of `xkb_symbols`: its body, by the key's name.
fn symbols(body: &str) -> HashMap<&str, &str> {
    let mut keys = HashMap::new();
    let mut rest = body;
    while let Some(at) = rest.find("key <") {
        rest = &rest[at + "key <".len()..];
        let Some((name, after)) = rest.split_once('>') else {
            break;
        };
        let Some((block, after)) = after.split_once('{').and_then(|(_, block)| block.split_once('}')) else {
            break;
        };
        keys.insert(name, block);
        rest = after;
    }
    keys
}

/// The first keysym of each of a key's layouts, in order: `None` for one
/// that types nothing. A body lists them as `symbols[Group1]= [ 3, numbersign ]`
/// (`symbols[1]` from newer xkbcommon), or as bare lists, one per layout;
/// its other statements (`type`, `actions`, `repeat`) are skipped.
fn groups(body: &str) -> Vec<Option<&str>> {
    let mut groups: Vec<Option<&str>> = Vec::new();
    for statement in statements(body) {
        let (group, list) = if let Some(named) = statement.strip_prefix("symbols[") {
            let Some((index, value)) = named.split_once(']') else {
                continue;
            };
            let index = index.trim().trim_start_matches("Group").parse::<usize>().ok();
            let Some(value) = value.trim_start().strip_prefix('=') else {
                continue;
            };
            (index.and_then(|index| index.checked_sub(1)), value.trim())
        } else if statement.starts_with('[') {
            (Some(groups.len()), statement)
        } else {
            continue;
        };
        let Some(group) = group else {
            continue;
        };
        let Some(list) = list.strip_prefix('[').and_then(|list| list.strip_suffix(']')) else {
            continue;
        };
        let keysym = list
            .split(',')
            .next()
            .map(str::trim)
            .filter(|keysym| !keysym.is_empty());
        if groups.len() <= group {
            groups.resize(group + 1, None);
        }
        groups[group] = keysym;
    }
    groups
}

/// A key's body split at its top-level commas, each part trimmed: the
/// commas inside a `[ … ]` list or an action's `( … )` stay.
fn statements(body: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let mut depth = 0_i32;
    let mut start = 0;
    for (at, c) in body.char_indices() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                statements.push(body[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    statements.push(body[start..].trim());
    statements
}

/// Reads `j/devices` for the current layout of the main keyboard: the one
/// whose keymap Hyprland hands Wayland clients. `None` when there is no main
/// keyboard, or the answer can't be read: Hyprland writes the index as
/// `none`, which isn't JSON, for a keyboard without one.
pub fn active_layout(devices: &str) -> Option<u32> {
    #[derive(Deserialize)]
    struct Devices {
        #[serde(default)]
        keyboards: Vec<Keyboard>,
    }
    #[derive(Deserialize)]
    struct Keyboard {
        #[serde(default)]
        main: bool,
        #[serde(default)]
        active_layout_index: u32,
    }
    let devices: Devices = serde_json::from_str(devices).ok()?;
    devices
        .keyboards
        .iter()
        .find(|keyboard| keyboard.main)
        .map(|keyboard| keyboard.active_layout_index)
}

#[cfg(test)]
mod tests;
