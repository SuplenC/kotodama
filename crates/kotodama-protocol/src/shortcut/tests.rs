use super::*;

#[test]
fn every_reserved_binding_is_reserved_as_written() {
    for shortcut in RESERVED_SHORTCUTS {
        assert!(is_reserved_shortcut(shortcut), "{shortcut}");
    }
}

#[test]
fn a_reserved_binding_is_reserved_however_it_is_spelled() {
    assert!(is_reserved_shortcut("Ctrl+Z"));
    assert!(is_reserved_shortcut("control + z"));
    assert!(is_reserved_shortcut("shift+ctrl+backspace"));
    assert!(is_reserved_shortcut("CTRL+SHIFT+BACKSPACE"));
}

#[test]
fn other_modifiers_or_keys_are_free() {
    assert!(!is_reserved_shortcut("ctrl+o"));
    assert!(!is_reserved_shortcut("ctrl+shift+z"));
    assert!(!is_reserved_shortcut("alt+z"));
    assert!(!is_reserved_shortcut("ctrl+backspace"));
    assert!(!is_reserved_shortcut("ctrl+shift+k"));
}

#[test]
fn a_binding_that_names_no_key_is_not_reserved() {
    assert!(!is_reserved_shortcut(""));
    assert!(!is_reserved_shortcut("ctrl+"));
    assert!(!is_reserved_shortcut("banana+z"));
}
