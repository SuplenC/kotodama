//! Sending Indexed items as changes.
//!
//! # Owns
//!
//! What the Daemon has of the Extension's Items, as a fingerprint per id,
//! and the `remove_items` and `add_items` that bring it up to date.
//!
//! # Not here
//!
//! Making the Items, which the Extension does; storing them, which the
//! Daemon does.

use std::collections::HashMap;
use std::hash::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::io::Write;

use kotodama_protocol::Action;
use kotodama_protocol::Argument;
use kotodama_protocol::ChannelWriter;
use kotodama_protocol::Error;
use kotodama_protocol::FromExtension;
use kotodama_protocol::Item;

/// What the Daemon has of each Item, by id, so only what changed is sent
/// again. A fingerprint stands in for each Item, so the Items aren't kept
/// twice.
#[derive(Debug, Default)]
pub struct SentItems(HashMap<String, u64>);

impl SentItems {
    /// Makes `items` what the Daemon has: sends the ids of the Items gone, then
    /// the Items added or changed, and nothing when nothing changed.
    ///
    /// # Errors
    ///
    /// When a message can't be written to the Daemon.
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
            if self.0.remove(&item.id) == Some(print) {
                next.insert(item.id, print);
            } else {
                next.insert(item.id.clone(), print);
                added.push(item);
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
        app_only,
    } = item;
    (
        id, kind, identity, title, subtitle, icon, keyword, keywords, hidden, app_only,
    )
        .hash(&mut hasher);
    for Action {
        id,
        title,
        shortcut,
        arguments,
        confirm,
    } in actions
    {
        (id, title, shortcut, confirm).hash(&mut hasher);
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
mod tests;
