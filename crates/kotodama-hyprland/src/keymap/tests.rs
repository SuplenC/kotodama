use super::Keymap;
use super::active_layout;

/// A `us,fr` keymap, as `xkbcli compile-keymap --format 1` writes it: the
/// text xkbcommon makes of a keymap for Wayland clients.
const US_FR: &str = include_str!("../../tests/fixtures/keymap.xkb");

#[test]
fn a_keycode_is_named_by_the_keysym_it_types_in_the_layout() {
    let keymap = Keymap::parse(US_FR);

    assert_eq!(keymap.keysym(12, 0), Some("3"));
    assert_eq!(keymap.keysym(12, 1), Some("quotedbl"));
    assert_eq!(keymap.keysym(24, 0), Some("q"));
    assert_eq!(keymap.keysym(24, 1), Some("a"));
}

#[test]
fn a_key_with_one_layout_types_the_same_in_every_layout() {
    let keymap = Keymap::parse(US_FR);

    assert_eq!(keymap.keysym(9, 0), Some("Escape"));
    assert_eq!(keymap.keysym(9, 1), Some("Escape"));
    assert_eq!(keymap.keysym(372, 1), Some("XF86Favorites"));
}

#[test]
fn a_keys_other_statements_dont_hide_its_keysyms() {
    let keymap = Keymap::parse(US_FR);

    // `type= "CTRL+ALT",` comes before its list.
    assert_eq!(keymap.keysym(67, 0), Some("F1"));
}

#[test]
fn a_key_that_types_nothing_or_isnt_there_has_no_name() {
    let keymap = Keymap::parse(US_FR);

    // <ALT> types only at its second level.
    assert_eq!(keymap.keysym(204, 0), None);
    assert_eq!(keymap.keysym(7, 0), None);
    assert_eq!(keymap.keysym(1000, 0), None);
}

#[test]
fn older_xkbcommon_names_the_layouts_group1() {
    let text = "xkb_keymap {
xkb_keycodes \"evdev\" {
	minimum = 8;
	maximum = 255;
	<AE03> = 12;
	<AD01> = 24;
	alias <LatQ> = <AD01>;
	indicator 1 = \"Caps Lock\";
};
xkb_types \"complete\" {
	type \"TWO_LEVEL\" { modifiers= Shift; map[Shift]= 2; };
};
xkb_symbols \"pc+us+fr:2\" {
	name[Group1]=\"English (US)\";
	name[Group2]=\"French\";
	key <AE03> {
		type[Group2]= \"FOUR_LEVEL_ALPHABETIC\",
		symbols[Group1]= [ 3, numbersign ],
		symbols[Group2]= [ quotedbl, 3, numbersign, sterling ]
	};
	key <AD01> { [ q, Q ], [ a, A ] };
	modifier_map Shift { <LFSH>, <RTSH> };
};
};
";
    let keymap = Keymap::parse(text);

    assert_eq!(keymap.keysym(12, 0), Some("3"));
    assert_eq!(keymap.keysym(12, 1), Some("quotedbl"));
    assert_eq!(keymap.keysym(24, 1), Some("a"));
}

#[test]
fn text_that_isnt_a_keymap_names_nothing() {
    assert_eq!(Keymap::parse("").keysym(12, 0), None);
    assert_eq!(Keymap::parse("xkb_keymap { xkb_keycodes {").keysym(12, 0), None);
}

#[test]
fn the_current_layout_is_the_main_keyboards() {
    let devices = include_str!("../../tests/fixtures/devices.json");

    assert_eq!(active_layout(devices), Some(1));
    assert_eq!(active_layout(r#"{"mice": [], "keyboards": []}"#), None);
    // Hyprland writes `none` for a keyboard without a layout index.
    assert_eq!(
        active_layout(r#"{"keyboards": [{"main": true, "active_layout_index": none}]}"#),
        None
    );
}
