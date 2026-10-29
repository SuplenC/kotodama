use kotodama_protocol::ItemKind;
use kotodama_protocol::NamespacedKind;

use super::*;

fn bound(id: &str, shortcut: &str) -> Action {
    Action::new(id, id).shortcut(shortcut)
}

fn note(actions: Vec<Action>) -> Item {
    Item {
        id: "note".into(),
        kind: ItemKind::Namespaced(NamespacedKind::new("notes.note").expect("a valid kind")),
        title: "Note".into(),
        subtitle: None,
        icon: None,
        keyword: None,
        identity: None,
        keywords: Vec::new(),
        hidden: false,
        app_only: false,
        weak_keywords: Vec::new(),
        yields: false,
        actions,
    }
}

#[test]
fn a_reserved_binding_is_dropped_and_the_action_kept() {
    let mut actions = vec![bound("undo-mine", "Ctrl+Z"), bound("open", "ctrl+o")];

    let mut released = Vec::new();
    release_actions(&mut actions, &mut released);

    assert_eq!(
        released,
        [Released {
            action: "undo-mine".into(),
            shortcut: "Ctrl+Z".into()
        }]
    );
    assert_eq!(actions[0].id, "undo-mine");
    assert_eq!(actions[0].shortcut, None);
    assert_eq!(actions[1].shortcut.as_deref(), Some("ctrl+o"));
}

#[test]
fn a_list_views_items_lose_their_reserved_bindings() {
    let mut message = FromExtension::ReplaceView {
        view: "form".into(),
        with: View {
            id: "notes".into(),
            title: "Notes".into(),
            content: ViewContent::List {
                items: vec![note(vec![bound("forget-mine", "ctrl+shift+backspace")])],
                filters_itself: false,
            },
        },
    };

    assert_eq!(release_message(&mut message).len(), 1);
    let FromExtension::ReplaceView { with, .. } = &message else {
        panic!("still a ReplaceView");
    };
    let ViewContent::List { items, .. } = &with.content else {
        panic!("still a list");
    };
    assert_eq!(items[0].actions[0].shortcut, None);
}
