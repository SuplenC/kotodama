use kotodama_protocol::ItemKind;

use super::*;

fn item(id: &str, title: &str) -> Item {
    Item {
        id: id.into(),
        kind: ItemKind::Url {
            url: format!("https://example.com/{id}"),
        },
        identity: None,
        title: title.into(),
        subtitle: None,
        icon: None,
        keyword: None,
        keywords: Vec::new(),
        hidden: false,
        app_only: false,
        weak_keywords: Vec::new(),
        yields: false,
        actions: vec![Action::new("open", "Open")],
    }
}

fn ids(items: &[Item]) -> Vec<&str> {
    items.iter().map(|item| item.id.as_str()).collect()
}

#[test]
fn only_items_added_or_changed_are_sent_again() {
    let mut sent = SentItems::default();
    let (removed, added) = sent.update([item("a", "A"), item("b", "B")]);
    assert!(removed.is_empty());
    assert_eq!(ids(&added), ["a", "b"]);

    let (removed, added) = sent.update([item("a", "A"), item("b", "Bee"), item("c", "C")]);
    assert!(removed.is_empty());
    assert_eq!(ids(&added), ["b", "c"]);

    let (removed, added) = sent.update([item("a", "A"), item("b", "Bee"), item("c", "C")]);
    assert!(removed.is_empty() && added.is_empty());
}

#[test]
fn items_gone_are_removed_by_id_in_order() {
    let mut sent = SentItems::default();
    sent.update([item("c", "C"), item("a", "A"), item("b", "B")]);
    let (removed, added) = sent.update([item("b", "B")]);
    assert_eq!(removed, ["a", "c"]);
    assert!(added.is_empty());
}

#[test]
fn an_argument_changing_sends_the_item_again() {
    let mut sent = SentItems::default();
    let mut asks = item("a", "A");
    sent.update([asks.clone()]);
    asks.actions[0].arguments.push(Argument::new("Query"));
    let (_, added) = sent.update([asks]);
    assert_eq!(ids(&added), ["a"]);
}

#[test]
fn asking_to_confirm_sends_the_item_again() {
    let mut sent = SentItems::default();
    let mut clear = item("a", "A");
    sent.update([clear.clone()]);
    clear.actions[0].confirm = true;
    let (_, added) = sent.update([clear]);
    assert_eq!(ids(&added), ["a"]);
}
