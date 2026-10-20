//! Open windows, from recorded `j/clients` output, kept up to date by a
//! recorded stream of Hyprland's events.

use kotodama_hyprland::events::Change;
use kotodama_hyprland::windows::{self, Windows};
use kotodama_protocol::{Item, ItemKind};

fn recorded() -> Windows {
    Windows::from_clients(include_str!("fixtures/clients.json")).unwrap()
}

/// Applies each line as Hyprland sends it, and says which changed a window.
fn replay(windows: &mut Windows, lines: &str) -> Vec<bool> {
    lines
        .lines()
        .filter_map(Change::parse)
        .map(|change| windows.apply(&change))
        .collect()
}

fn rows(windows: &Windows) -> Vec<(String, String, String)> {
    windows
        .items()
        .map(|item: Item| {
            let ItemKind::Window { address } = item.kind else {
                panic!("a window Item");
            };
            assert_eq!(item.id, address);
            (address, item.title, item.subtitle.unwrap_or_default())
        })
        .collect()
}

fn row(address: &str, title: &str, subtitle: &str) -> (String, String, String) {
    (address.into(), title.into(), subtitle.into())
}

#[test]
fn every_mapped_client_is_a_window_item() {
    let windows = recorded();
    assert_eq!(
        rows(&windows),
        [
            row(
                "0x55d0c6a1b2c0",
                "Hyprland Wiki — Mozilla Firefox",
                "firefox · workspace 1"
            ),
            row("0x55d0c6b3e1c0", "~/src/kotodama", "foot · workspace 2"),
        ]
    );
    let item = windows.items().next().unwrap();
    assert_eq!(item.kind.name(), "window");
    assert_eq!(item.icon.as_deref(), Some("firefox"));
    assert_eq!(
        item.actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>(),
        [
            windows::FOCUS,
            windows::CLOSE,
            windows::MOVE,
            windows::FLOAT
        ]
    );
}

#[test]
fn a_window_opened_is_added() {
    let mut windows = recorded();
    let changed = replay(
        &mut windows,
        "openwindow>>55d0c6c2a4e0,3,org.gnome.Nautilus,Home, sweet home\n",
    );
    assert_eq!(changed, [true]);
    assert!(rows(&windows).contains(&row(
        "0x55d0c6c2a4e0",
        "Home, sweet home",
        "org.gnome.Nautilus · workspace 3"
    )));
}

#[test]
fn a_window_moved_or_retitled_is_changed_and_one_closed_is_gone() {
    let mut windows = recorded();
    replay(&mut windows, include_str!("fixtures/events.txt"));
    assert_eq!(
        rows(&windows),
        [
            row("0x55d0c6b3e1c0", "~/src/kotodama", "foot · workspace 2"),
            row(
                "0x55d0c6c2a4e0",
                "Downloads, 3 items",
                "org.gnome.Nautilus · workspace 4"
            ),
        ]
    );
}

#[test]
fn only_events_about_windows_change_them() {
    let mut windows = recorded();
    let changed = replay(&mut windows, include_str!("fixtures/events.txt"));
    // openwindow, windowtitlev2, movewindowv2 and closewindow; movewindow
    // repeats what movewindowv2 says, and the rest are about focus.
    assert_eq!(changed.iter().filter(|&&changed| changed).count(), 4);
    // An event about a window never seen changes nothing.
    assert_eq!(
        replay(
            &mut windows,
            "closewindow>>deadbeef\nwindowtitlev2>>deadbeef,x\n"
        ),
        [false, false]
    );
}

#[test]
fn a_config_reload_is_a_change_of_its_own() {
    assert_eq!(
        Change::parse("configreloaded>>"),
        Some(Change::ConfigReloaded)
    );
    assert_eq!(Change::parse("activewindowv2>>55d0c6a1b2c0"), None);
    assert_eq!(Change::parse("garbage"), None);
}

#[test]
fn an_action_is_the_dispatch_it_runs_on_its_window() {
    let address = "0x55d0c6a1b2c0";
    assert_eq!(
        windows::dispatch(windows::FOCUS, address, None).unwrap(),
        "focuswindow address:0x55d0c6a1b2c0"
    );
    assert_eq!(
        windows::dispatch(windows::CLOSE, address, None).unwrap(),
        "closewindow address:0x55d0c6a1b2c0"
    );
    assert_eq!(
        windows::dispatch(windows::FLOAT, address, None).unwrap(),
        "togglefloating address:0x55d0c6a1b2c0"
    );
    assert_eq!(
        windows::dispatch(windows::MOVE, address, Some("3")).unwrap(),
        "movetoworkspacesilent 3,address:0x55d0c6a1b2c0"
    );
    assert!(windows::dispatch(windows::MOVE, address, Some(" ")).is_err());
    assert!(windows::dispatch("dance", address, None).is_err());
}
