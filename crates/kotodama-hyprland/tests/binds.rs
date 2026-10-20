//! Hyprland's binds, from recorded `hyprctl binds -j` output, as Indexed
//! items and as the Shortcuts View.

use kotodama_hyprland::binds;
use kotodama_hyprland::binds::Bind;
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
    assert_eq!(screenshot.dispatch(), "exec grim -g \"$(slurp)\" - | wl-copy");
    let close = binds::find(&binds, &items[0].id).unwrap();
    assert_eq!(close.dispatch(), "killactive");
    assert!(binds::find(&binds, "nope").is_none());
}

#[test]
fn output_that_isnt_binds_is_an_error() {
    assert!(binds::parse("not json").is_err());
    assert!(binds::parse("{}").is_err());
}
