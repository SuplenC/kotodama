//! Action key bindings, such as `ctrl+o`, matched against Qt's key events.
//! Nothing here touches Qt, so it is tested without it.

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

// Each with the two codes it may arrive as; most keys have one, given twice.
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

/// The Qt::Key codes a key name may arrive as, or `None` if it names no key.
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
mod tests {
    use super::*;

    // Qt::Key values, from qnamespace.h.
    const KEY_O: i32 = 0x4f;
    const KEY_1: i32 = 0x31;
    const KEY_COMMA: i32 = 0x2c;
    const KEY_RETURN: i32 = 0x0100_0004;
    const KEY_ENTER: i32 = 0x0100_0005;
    const KEY_F5: i32 = 0x0100_0034;

    #[test]
    fn a_letter_with_ctrl_matches_only_with_exactly_ctrl_held() {
        assert!(matches("ctrl+o", KEY_O, CONTROL));
        assert!(matches("Ctrl+O", KEY_O, CONTROL));
        assert!(!matches("ctrl+o", KEY_O, CONTROL | SHIFT));
        assert!(!matches("ctrl+o", KEY_O, ALT));
        assert!(!matches("ctrl+o", KEY_1, CONTROL));
        assert!(matches("ctrl+shift+o", KEY_O, CONTROL | SHIFT));
    }

    #[test]
    fn a_keypad_key_counts_as_the_same_key() {
        const KEYPAD: i32 = 0x2000_0000;
        assert!(matches("alt+1", KEY_1, ALT | KEYPAD));
    }

    #[test]
    fn named_keys_digits_and_punctuation_match() {
        assert!(matches("alt+enter", KEY_RETURN, ALT));
        assert!(matches("alt+enter", KEY_ENTER, ALT));
        assert!(matches("super+f5", KEY_F5, META));
        assert!(matches("ctrl+1", KEY_1, CONTROL));
        assert!(matches("ctrl+,", KEY_COMMA, CONTROL));
    }

    #[test]
    fn a_binding_that_would_type_into_the_search_never_matches() {
        assert!(!matches("o", KEY_O, 0));
        assert!(!matches("shift+o", KEY_O, SHIFT));
        assert!(!matches("enter", KEY_RETURN, 0));
    }

    #[test]
    fn a_binding_that_names_no_key_or_an_unknown_one_never_matches() {
        assert!(!matches("ctrl", KEY_O, CONTROL));
        assert!(!matches("ctrl+banana", KEY_O, CONTROL));
        assert!(!matches("ctrl+o+p", KEY_O, CONTROL));
        assert!(!matches("", KEY_O, CONTROL));
    }
}
