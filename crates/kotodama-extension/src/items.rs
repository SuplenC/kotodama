//! Sending Indexed items as changes.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;

use kotodama_protocol::{Action, Argument, ChannelWriter, Error, FromExtension, Item};

/// What the Daemon has of each Item, by id, so only what changed is sent
/// again. A fingerprint stands in for each Item, so the Items aren't kept
/// twice.
#[derive(Debug, Default)]
pub struct SentItems(HashMap<String, u64>);

impl SentItems {
    /// Makes `items` what the Daemon has: sends the ids of the Items gone, then
    /// the Items added or changed, and nothing when nothing changed.
    pub fn publish<W: Write>(
        &mut self,
        writer: &mut ChannelWriter<W>,
        items: impl IntoIterator<Item = Item>,
    ) -> Result<(), Error> {
        let (removed, added) = self.update(items);
        if !removed.is_empty() {
            writer.send(&FromExtension::RemoveItems { ids: removed })?;
        }
        if !added.is_empty() {
            writer.send(&FromExtension::AddItems { items: added })?;
        }
        Ok(())
    }

    /// Makes `items` what the Daemon has, without sending anything. Returns
    /// the ids to remove, sorted, and the Items to add or replace.
    pub fn update(&mut self, items: impl IntoIterator<Item = Item>) -> (Vec<String>, Vec<Item>) {
        let mut next = HashMap::with_capacity(self.0.len());
        let mut added = Vec::new();
        for item in items {
            let print = fingerprint(&item);
            if self.0.remove(&item.id) != Some(print) {
                next.insert(item.id.clone(), print);
                added.push(item);
            } else {
                next.insert(item.id, print);
            }
        }
        let mut removed: Vec<String> = std::mem::replace(&mut self.0, next).into_keys().collect();
        removed.sort();
        (removed, added)
    }
}

/// A hash of everything an Item sends.
fn fingerprint(item: &Item) -> u64 {
    let mut hasher = DefaultHasher::new();
    let Item {
        id,
        kind,
        identity,
        title,
        subtitle,
        icon,
        keyword,
        keywords,
        hidden,
        actions,
    } = item;
    (
        id, kind, identity, title, subtitle, icon, keyword, keywords, hidden,
    )
        .hash(&mut hasher);
    for Action {
        id,
        title,
        shortcut,
        arguments,
    } in actions
    {
        (id, title, shortcut).hash(&mut hasher);
        for Argument {
            name,
            placeholder,
            optional,
        } in arguments
        {
            (name, placeholder, optional).hash(&mut hasher);
        }
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
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
        asks.actions[0].arguments.push(Argument {
            name: "Query".into(),
            placeholder: None,
            optional: false,
        });
        let (_, added) = sent.update([asks]);
        assert_eq!(ids(&added), ["a"]);
    }
}
