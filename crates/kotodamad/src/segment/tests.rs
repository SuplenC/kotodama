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
        weak_keywords: Vec::new(),
        yields: false,
        actions: actions
            .iter()
            .map(|&id| Action {
                id: id.into(),
                title: format!("{id} it"),
                shortcut: None,
                arguments: vec![],
                confirm: false,
                keywords: Vec::new(),
                role: kotodama_protocol::ActionRole::Normal,
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
fn weak_keywords_stay_apart_from_the_keywords_after_the_arenas_are_compacted() {
    let mut segment = Segment::default();
    let mut first = item("first", "Some Notes", &["open"]);
    first.keywords = vec!["notes".into(), "text editor".into()];
    let mut chat = item("chat", "Google Chat", &["open"]);
    chat.keywords = vec!["messages".into()];
    chat.weak_keywords = vec!["chrome-abcdef-Default".into()];
    segment.edit(Edit::Add(vec![first, chat]));
    let row = segment.find("chat").unwrap();
    let before = segment.keys[row as usize].starts;

    segment.edit(Edit::Remove(vec!["first".into()]));
    for round in 0..50 {
        let filler = (0..20).map(|n| item(&format!("filler-{n}"), &format!("Filler {round}"), &["open"]));
        segment.edit(Edit::Add(filler.collect()));
    }

    assert!(segment.keys[row as usize].starts < before, "the arenas were compacted");
    let texts = segment.texts(row).unwrap();
    assert_eq!(texts.keywords().chars.to_string(), "messages");
    assert_eq!(texts.weak().to_string(), "chrome-abcdef-default");
    assert_eq!(texts.keywords().starts, [0]);
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

#[test]
fn an_item_resent_with_only_its_actions_changed_keeps_its_search_keys() {
    let mut segment = Segment::default();
    segment.edit(Edit::Add(vec![
        item("a", "Wi-Fi", &["off", "on"]),
        item("b", "Wiki", &["open"]),
    ]));
    let row = segment.find("a").unwrap();
    let garbage = segment.garbage;
    segment.next_version();

    segment.edit(Edit::Add(vec![item("a", "Wi-Fi", &["on"])]));

    assert!(!segment.keys_changed());
    assert_eq!(segment.find("a"), Some(row));
    assert_eq!(segment.garbage, garbage, "the texts stay where they were");
    let actions: Vec<String> = read(&segment, "a").unwrap().actions.into_iter().map(|a| a.id).collect();
    assert_eq!(actions, ["on"]);
    let offered: Vec<_> = segment.offered().collect();
    assert_eq!(offered, [("url", "on", row), ("url", "open", 1)]);
}

#[test]
fn an_item_resent_with_anything_searched_changed_changes_the_keys() {
    let wifi = || item("a", "Wi-Fi", &["off"]);
    let resent = [
        item("a", "WiFi", &["off"]),
        Item {
            subtitle: None,
            ..wifi()
        },
        Item {
            keywords: vec!["wireless".into()],
            ..wifi()
        },
        Item {
            keyword: Some("wifi".into()),
            ..wifi()
        },
        Item {
            weak_keywords: vec!["nm".into()],
            ..wifi()
        },
        Item {
            identity: Some("setting:wifi".into()),
            ..wifi()
        },
        Item {
            kind: ItemKind::Namespaced(kotodama_protocol::NamespacedKind::new("x.setting").unwrap()),
            ..wifi()
        },
    ];
    for resent in resent {
        let mut segment = Segment::default();
        segment.edit(Edit::Add(vec![item("a", "Wi-Fi", &["off", "on"])]));
        segment.next_version();

        segment.edit(Edit::Add(vec![resent.clone()]));

        assert!(segment.keys_changed(), "{resent:?}");
        assert_eq!(read(&segment, "a").unwrap(), resent);
    }
}

#[test]
fn removing_an_item_changes_the_keys() {
    let mut segment = Segment::default();
    segment.edit(Edit::Add(vec![item("a", "Wi-Fi", &["off"])]));
    segment.next_version();

    segment.edit(Edit::Remove(vec!["a".into()]));

    assert!(segment.keys_changed());
}

fn with_verbs(id: &str, keywords: &[&str]) -> Item {
    let mut item = item(id, "Trash", &[]);
    item.actions = vec![Action::new("trash", "Move to Trash").keywords(keywords.iter().copied())];
    item
}

#[test]
fn a_version_offering_an_action_keyword_the_last_one_didnt_says_so() {
    let mut segment = Segment::default();
    segment.edit(Edit::Add(vec![with_verbs("a", &["Delete"])]));
    assert!(segment.verbs_changed());
    assert_eq!(segment.verbs().collect::<Vec<_>>(), ["delete"]);

    segment.next_version();
    segment.edit(Edit::Add(vec![with_verbs("b", &["delete"])]));
    assert!(!segment.verbs_changed(), "the same keyword, folded");

    segment.next_version();
    segment.edit(Edit::Remove(vec!["a".into(), "b".into()]));
    assert_eq!(segment.verbs().count(), 0);

    segment.next_version();
    segment.edit(Edit::Add(vec![with_verbs("a", &["delete"])]));
    assert!(segment.verbs_changed(), "offered again");
    let row = segment.find("a").unwrap();
    assert_eq!(segment.named_action(row, |keyword| keyword == "delete"), Some("trash"));
    assert_eq!(segment.named_action(row, |keyword| keyword == "remove"), None);
}
