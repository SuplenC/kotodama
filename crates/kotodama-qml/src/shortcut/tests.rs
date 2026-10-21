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
