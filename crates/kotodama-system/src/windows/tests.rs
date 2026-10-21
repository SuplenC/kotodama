use kotodama_protocol::ItemKind;

use super::CLOSE;
use super::FOCUS;
use super::FULLSCREEN;
use super::MAXIMIZE;
use super::Request;
use super::Window;
use super::Windows;

fn window(title: &str, app_id: &str) -> Window {
    Window {
        title: title.into(),
        app_id: app_id.into(),
        ..Window::default()
    }
}

#[test]
fn a_window_is_an_item_of_kind_window_titled_by_its_title_with_its_app_id_beside() {
    let mut windows = Windows::default();
    windows.update(3, window("README.md - Kate", "org.kde.kate"));

    let items: Vec<_> = windows.items().collect();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "window-3");
    assert_eq!(
        items[0].kind,
        ItemKind::Window {
            address: "window-3".into()
        }
    );
    assert_eq!(items[0].title, "README.md - Kate");
    assert_eq!(items[0].subtitle.as_deref(), Some("org.kde.kate"));
    assert_eq!(items[0].icon.as_deref(), Some("org.kde.kate"));
    assert_eq!(items[0].actions[0].id, FOCUS);
}

#[test]
fn a_window_without_a_title_is_titled_by_its_app_id() {
    let mut windows = Windows::default();
    windows.update(1, window("", "foot"));

    assert_eq!(windows.items().next().map(|item| item.title).as_deref(), Some("foot"));
}

#[test]
fn only_a_new_title_or_app_id_changes_the_item() {
    let mut windows = Windows::default();
    assert!(windows.update(1, window("one", "foot")));

    let focused = Window {
        maximized: true,
        ..window("one", "foot")
    };
    assert!(!windows.update(1, focused));
    assert!(windows.update(1, window("two", "foot")));
}

#[test]
fn maximize_and_fullscreen_undo_themselves() {
    let mut windows = Windows::default();
    windows.update(1, window("one", "foot"));
    assert_eq!(
        windows.request(MAXIMIZE, "window-1").unwrap(),
        (1, Request::SetMaximized(true))
    );

    windows.update(
        1,
        Window {
            maximized: true,
            fullscreen: true,
            ..window("one", "foot")
        },
    );

    assert_eq!(
        windows.request(MAXIMIZE, "window-1").unwrap(),
        (1, Request::SetMaximized(false))
    );
    assert_eq!(
        windows.request(FULLSCREEN, "window-1").unwrap(),
        (1, Request::SetFullscreen(false))
    );
}

#[test]
fn a_closed_window_has_no_request() {
    let mut windows = Windows::default();
    windows.update(1, window("one", "foot"));
    assert!(windows.remove(1));

    assert!(windows.request(CLOSE, "window-1").is_err());
    assert!(windows.request(CLOSE, "0x55d0c6a1b2c0").is_err());
    assert_eq!(windows.items().count(), 0);
}

#[test]
fn an_unknown_action_is_refused() {
    let mut windows = Windows::default();
    windows.update(1, window("one", "foot"));

    let err = windows.request("float", "window-1").unwrap_err();

    assert_eq!(err.to_string(), "A window has no Action `float`");
}
