
use kotodama_protocol::Action;
use kotodama_protocol::ItemKind;

use super::*;

fn item(id: &str, title: &str, actions: &[&str]) -> Item {
    Item {
        id: id.into(),
        kind: ItemKind::Url {
            url: format!("https://example.org/{id}"),
        },
        identity: None,
        title: title.into(),
        subtitle: Some("A subtitle long enough to fill the Shelf".into()),
        icon: None,
        keyword: None,
        keywords: vec![],
        hidden: false,
        app_only: false,
        actions: actions
            .iter()
            .map(|&id| Action {
                id: id.into(),
                title: format!("{id} it"),
                shortcut: None,
                arguments: vec![],
                confirm: false,
            })
            .collect(),
    }
}

fn read(segment: &Segment, id: &str) -> Option<Item> {
    let (shelf, place) = segment.payload(segment.find(id)?)?;
    shelf.get(place)
}

#[test]
fn items_read_back_as_last_sent_after_the_shelf_drops_its_garbage() {
    let dir = tempfile::tempdir().unwrap();
    for mut segment in [Segment::default(), Segment::new(Some(dir.path().into()))] {
        changed_over_and_over(&mut segment);
    }
}

fn changed_over_and_over(segment: &mut Segment) {
    let ids: Vec<String> = (0..100).map(|n| format!("item-{n}")).collect();
    let shelves: std::collections::HashSet<u64> = (0..20)
        .map(|round| {
            let items = ids
                .iter()
                .map(|id| item(id, &format!("{id} round {round}"), &["open"]))
                .collect();
            segment.edit(Edit::Add(items));
            segment.shelf.as_ref().unwrap().id()
        })
        .collect();
    assert!(shelves.len() > 1, "the Shelf was copied");
    for id in &ids {
        assert_eq!(read(segment, id).unwrap().title, format!("{id} round 19"));
    }
}

#[test]
fn an_earlier_version_reads_its_own_items_after_a_later_one_replaced_them() {
    let mut segment = Segment::default();
    segment.edit(Edit::Add(vec![item("a", "First", &["open"])]));
    let earlier = segment.clone();
    segment.next_version();
    segment.edit(Edit::Add(vec![item("a", "Second", &["open"])]));
    assert_eq!(read(&earlier, "a").unwrap().title, "First");
    assert_eq!(read(&segment, "a").unwrap().title, "Second");
}

#[test]
fn each_action_on_offer_comes_with_the_first_row_whose_item_has_it() {
    let mut segment = Segment::default();
    segment.edit(Edit::Add(vec![
        item("a", "A", &["open"]),
        item("b", "B", &["open", "copy"]),
        item("c", "C", &["copy"]),
    ]));
    segment.edit(Edit::Remove(vec!["a".into()]));
    let offered: Vec<_> = segment.offered().collect();
    assert_eq!(offered, [("url", "open", 1), ("url", "copy", 1)]);
}
