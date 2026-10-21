use kotodama_protocol::Action;
use kotodama_protocol::ItemKind;

use super::*;

fn item(id: &str) -> Item {
    Item {
        id: id.into(),
        kind: ItemKind::Url {
            url: format!("https://example.org/{id}"),
        },
        identity: Some(format!("url:{id}")),
        title: format!("Title of {id}"),
        subtitle: Some("A subtitle".into()),
        icon: Some("web-browser".into()),
        keyword: None,
        keywords: vec!["more".into()],
        hidden: false,
        app_only: false,
        actions: vec![Action {
            id: "open".into(),
            title: "Open".into(),
            shortcut: None,
            arguments: vec![],
            confirm: false,
        }],
    }
}

fn shelve(shelf: &Shelf, items: &[Item]) -> Vec<Place> {
    let mut buf = Vec::new();
    let mut at = 0;
    items
        .iter()
        .map(|item| {
            encode(item, &mut buf);
            shelf.put(at, &buf).unwrap();
            let place = Place {
                at,
                len: buf.len() as u32,
            };
            at = place.end();
            place
        })
        .collect()
}

#[test]
fn an_item_comes_back_off_a_shelf_in_memory_as_it_went_on() {
    let shelf = Shelf::in_memory();
    let items = [item("a"), item("b")];
    let places = shelve(&shelf, &items);
    assert_eq!(shelf.get(places[1]), Some(items[1].clone()));
    assert_eq!(shelf.get(places[0]), Some(items[0].clone()));
}

#[test]
fn an_item_comes_back_off_a_shelf_in_a_file_that_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let shelf = Shelf::new(Some(dir.path()));
    let items = [item("a"), item("b")];
    let places = shelve(&shelf, &items);
    assert_eq!(shelf.get(places[1]), Some(items[1].clone()));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}
