use std::time::Duration;
use std::time::Instant;

use super::Keymap;
use super::settle_left;
use crate::engine::DEFAULT_SETTLE;

#[test]
fn keys_wait_a_bounded_moment_after_the_menu_closes() {
    let closed = Instant::now();
    let ms = Duration::from_millis;
    assert_eq!(settle_left(closed, DEFAULT_SETTLE, closed), ms(50));
    // Connecting and uploading the keymap count towards it.
    assert_eq!(settle_left(closed, DEFAULT_SETTLE, closed + ms(20)), ms(30));
    assert_eq!(settle_left(closed, DEFAULT_SETTLE, closed + ms(80)), Duration::ZERO);
    assert_eq!(settle_left(closed, ms(200), closed + ms(80)), ms(120));
}

#[test]
fn each_distinct_character_gets_one_key() {
    let keymap = Keymap::new("hi\nhi");
    assert_eq!(keymap.code('h'), Some(1));
    assert_eq!(keymap.code('i'), Some(2));
    assert_eq!(keymap.code('\n'), Some(3));
    assert_eq!(keymap.code('x'), None);
}

#[test]
fn the_keymap_names_unicode_keysyms_and_return_for_newline() {
    let xkb = Keymap::new("é\n").to_xkb();
    assert!(xkb.contains("<K1> = 9;"), "{xkb}");
    assert!(xkb.contains("key <K1> {[U00E9]};"), "{xkb}");
    assert!(xkb.contains("key <K2> {[Return]};"), "{xkb}");
    assert!(xkb.contains("maximum = 11;"), "{xkb}");
}
