use super::*;

#[test]
fn a_shared_kinds_index_is_its_place_among_the_shared_kinds() {
    let kinds = [
        ItemKind::App { desktop_id: "x".into() },
        ItemKind::File { path: "/x".into() },
        ItemKind::Folder { path: "/x".into() },
        ItemKind::Project { path: "/x".into() },
        ItemKind::Window { address: "x".into() },
        ItemKind::Url { url: "x".into() },
    ];
    assert_eq!(kinds.len(), SHARED_KINDS.len());
    for kind in &kinds {
        assert_eq!(kind.shared_index().map(|n| SHARED_KINDS[n]), Some(kind.name()));
    }
    let namespaced = ItemKind::Namespaced(NamespacedKind::new("ext.thing").expect("a valid namespaced kind"));
    assert_eq!(namespaced.shared_index(), None);
}
