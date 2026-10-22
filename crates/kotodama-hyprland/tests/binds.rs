//! Hyprland's binds, from recorded `hyprctl binds -j` output under hyprlang
//! and under Lua, as Indexed items and as the Shortcuts View, with the keys
//! of the binds on a keycode named by a recorded keymap.

use std::borrow::Cow;

use kotodama_hyprland::binds;
use kotodama_hyprland::binds::Bind;
use kotodama_hyprland::keymap::Keymap;
use kotodama_protocol::Item;
use kotodama_protocol::ViewContent;

fn recorded() -> Vec<Bind> {
    binds::parse(include_str!("fixtures/binds.json")).expect("parse the recorded binds")
}

fn rows(items: &[Item]) -> Vec<(&str, &str)> {
    items
        .iter()
        .map(|item| (item.title.as_str(), item.subtitle.as_deref().unwrap_or("")))
        .collect()
}

#[test]
fn every_bind_with_a_description_is_an_item_titled_by_it_next_to_its_keys() {
    let items = binds::items(&recorded());
    // A mouse bind can't be run from the menu, so it isn't searchable.
    assert_eq!(
        rows(&items),
        [
            ("Close the window", "SUPER + Q"),
            ("Screenshot a region", "SUPER + SHIFT + S"),
            ("Raise the volume", "XF86AudioRaiseVolume"),
            ("Show the keyboard shortcuts", "SUPER + CTRL + F1"),
            ("Leave resize mode", "escape · submap resize"),
        ]
    );
    let screenshot = &items[1];
    assert_eq!(screenshot.kind.name(), "hyprland.shortcut");
    assert_eq!(screenshot.actions[0].id, binds::RUN);
    assert!(screenshot.keywords.iter().any(|word| word == "exec"));
}

#[test]
fn an_items_id_is_its_keys_so_it_stays_the_same_across_reads() {
    let first = binds::items(&recorded());
    let again = binds::items(&recorded());
    assert_eq!(first, again);
    let ids: std::collections::HashSet<_> = first.iter().map(|item| &item.id).collect();
    assert_eq!(ids.len(), first.len());
}

#[test]
fn the_shortcuts_view_lists_every_bind_grouped_by_modifier() {
    let view = binds::view(&recorded());
    assert_eq!(view.id, binds::VIEW);
    let ViewContent::List { items, .. } = view.content else {
        panic!("a list View");
    };
    // No modifier first, then by how many modifiers and their names; a
    // submap's binds after all the others. Within a group, the config's order.
    assert_eq!(
        rows(&items),
        [
            ("XF86AudioRaiseVolume", "Raise the volume"),
            ("ALT + Tab", "cyclenext"),
            ("SUPER + Return", "exec foot"),
            ("SUPER + Q", "Close the window"),
            ("SUPER + code:10", "workspace 1"),
            ("SUPER + mouse:272", "Drag a window"),
            ("SUPER + CTRL + F1", "Show the keyboard shortcuts"),
            ("SUPER + SHIFT + S", "Screenshot a region"),
            ("right", "resizeactive 10 0 · submap resize"),
            ("escape", "Leave resize mode · submap resize"),
        ]
    );
    // Typing in the View finds a bind by what it does, too.
    assert!(items[7].keywords.iter().any(|word| word == "Screenshot a region"));
}

#[test]
fn a_bind_runs_its_dispatcher_with_its_argument() {
    let binds = recorded();
    let items = binds::items(&binds);
    let screenshot = binds::find(&binds, &items[1].id).unwrap();
    assert_eq!(
        screenshot.dispatch().as_deref(),
        Some("exec grim -g \"$(slurp)\" - | wl-copy")
    );
    let close = binds::find(&binds, &items[0].id).unwrap();
    assert_eq!(close.dispatch().as_deref(), Some("killactive"));
    assert!(binds::find(&binds, "nope").is_none());
}

#[test]
fn output_that_isnt_binds_is_an_error() {
    assert!(binds::parse("not json").is_err());
    assert!(binds::parse("{}").is_err());
}

/// The Shortcuts View's rows, as (what the bind does, its keys).
fn keys_by_summary(binds: &[Bind]) -> Vec<(String, String)> {
    let ViewContent::List { items, .. } = binds::view(binds).content else {
        panic!("a list View");
    };
    items
        .into_iter()
        .map(|item| (item.subtitle.unwrap_or_default(), item.title))
        .collect()
}

/// Names the keys of the binds on a keycode from the recorded `us,fr`
/// keymap, in `layout`.
fn name_keys(binds: &mut [Bind], layout: u32) {
    let keymap = Keymap::parse(include_str!("fixtures/keymap.xkb"));
    binds::name_keys(binds, |code| keymap.keysym(code, layout).map(Cow::into_owned));
}

#[test]
fn a_bind_on_a_keycode_shows_the_key_the_layout_gives_it() {
    let mut binds = recorded();
    let ids = binds::view(&binds);

    name_keys(&mut binds, 0);

    let shown = keys_by_summary(&binds);
    assert!(
        shown.contains(&("workspace 1".to_owned(), "SUPER + 1".to_owned())),
        "{shown:?}"
    );
    // The ids stay the keycodes', whatever the layout calls their keys.
    let ids_of = |view: kotodama_protocol::View| match view.content {
        ViewContent::List { items, .. } => items.into_iter().map(|item| item.id).collect::<Vec<_>>(),
        _ => panic!("a list View"),
    };
    let named = ids_of(binds::view(&binds));
    assert_eq!(named, ids_of(ids));
    assert!(named.contains(&"/64/code:10".to_owned()));
}

#[test]
fn a_bind_on_a_keycode_the_keymap_doesnt_name_stays_its_keycode() {
    let mut binds = recorded();

    binds::name_keys(&mut binds, |_| None);

    assert!(keys_by_summary(&binds).contains(&("workspace 1".to_owned(), "SUPER + code:10".to_owned())));
}

fn recorded_lua() -> Vec<Bind> {
    binds::parse(include_str!("fixtures/binds-lua.json")).expect("parse the recorded Lua binds")
}

#[test]
fn a_lua_bind_runs_its_function_from_the_lua_registry() {
    let binds = recorded_lua();
    let items = binds::items(&binds);
    let close = binds::find(&binds, &items[0].id).unwrap();

    assert_eq!(close.description, "Close the window");
    assert_eq!(close.dispatch().as_deref(), Some("debug.getregistry()[4]"));
    // One Hyprland unbound has no function left to run.
    let mut unbound = close.clone();
    unbound.arg = "-2".to_owned();
    assert_eq!(unbound.dispatch(), None);
}

#[test]
fn a_lua_bind_isnt_found_by_its_functions_reference() {
    let binds = recorded_lua();

    let items = binds::items(&binds);
    let ViewContent::List { items: rows, .. } = binds::view(&binds).content else {
        panic!("a list View");
    };

    assert!(items.iter().all(|item| item.keywords.is_empty()), "{items:?}");
    let enter = rows.iter().find(|row| row.title == "SUPER + Return").unwrap();
    assert_eq!(enter.subtitle.as_deref(), Some("Lua function"));
    assert!(enter.keywords.is_empty(), "{:?}", enter.keywords);
}

#[test]
fn a_lua_bind_on_a_keycode_gets_it_from_the_plain_listing() {
    let mut binds = recorded_lua();
    // Under Lua, `j/binds` has neither the key nor the keycode.
    assert_eq!(binds.iter().filter(|bind| bind.lacks_keycode()).count(), 2);

    binds::keycodes_from_listing(&mut binds, include_str!("fixtures/binds-lua.txt"));

    assert!(!binds.iter().any(Bind::lacks_keycode));
    let ids: Vec<String> = binds::items(&binds).into_iter().map(|item| item.id).collect();
    assert!(ids.contains(&"/65/code:12".to_owned()), "{ids:?}");
    assert!(ids.contains(&"/64/code:10".to_owned()), "{ids:?}");
}

#[test]
fn a_lua_bind_on_a_keycode_shows_the_key_of_the_current_layout() {
    let mut binds = recorded_lua();
    binds::keycodes_from_listing(&mut binds, include_str!("fixtures/binds-lua.txt"));

    name_keys(&mut binds, 0);
    let us = binds::items(&binds);
    name_keys(&mut binds, 1);
    let fr = binds::items(&binds);

    let us = rows(&us);
    let fr = rows(&fr);
    assert!(
        us.contains(&("Move window to workspace 3", "SUPER + SHIFT + 3")),
        "{us:?}"
    );
    assert!(us.contains(&("Go to workspace 1", "SUPER + 1")), "{us:?}");
    assert!(
        fr.contains(&("Move window to workspace 3", "SUPER + SHIFT + \"")),
        "{fr:?}"
    );
}

#[test]
fn a_listing_read_after_the_binds_changed_is_left_alone() {
    let mut binds = recorded_lua();
    let listing = include_str!("fixtures/binds-lua.txt");
    let shorter = &listing[..listing.rfind("bindd").unwrap()];

    binds::keycodes_from_listing(&mut binds, shorter);

    assert_eq!(binds.iter().filter(|bind| bind.lacks_keycode()).count(), 2);
}
