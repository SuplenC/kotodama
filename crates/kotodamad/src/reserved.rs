//! Keeps Extensions' Actions off the key bindings a Front end keeps for
//! itself (`kotodama_protocol::RESERVED_SHORTCUTS`): a reserved binding an
//! Extension sends is dropped as it arrives, so no Front end ever sees it.
//!
//! # Owns
//!
//! - Dropping reserved bindings from an Extension's messages and its
//!   Contributed actions.
//!
//! # Not here
//!
//! - Which bindings are reserved: `kotodama_protocol::is_reserved_shortcut`.
//! - Clashes between two Extensions' bindings: `Daemon::contributed_for`.

use kotodama_protocol::Action;
use kotodama_protocol::FromExtension;
use kotodama_protocol::Item;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;
use kotodama_protocol::is_reserved_shortcut;

/// Drops every reserved binding of the Actions an Extension's message carries:
/// those of its Items, Live results and list Views. Returns how many it
/// dropped.
pub fn release_message(message: &mut FromExtension) -> usize {
    match message {
        FromExtension::AddItems { items } => release_items(items),
        FromExtension::LiveResults { results, .. } => {
            release_items(results.iter_mut().map(|result| &mut result.item))
        }
        FromExtension::PushView(view)
        | FromExtension::UpdateView(view)
        | FromExtension::ReplaceView { with: view, .. } => release_view(view),
        FromExtension::RemoveItems { .. }
        | FromExtension::ActionDone { .. }
        | FromExtension::PopView { .. }
        | FromExtension::JobProgress(_)
        | FromExtension::RequestActionCatalog
        | FromExtension::WithdrawPick { .. }
        | FromExtension::Status { .. } => 0,
    }
}

/// Drops the binding of each of `actions` bound to a reserved key. Returns
/// how many it dropped.
pub fn release_actions<'a>(actions: impl IntoIterator<Item = &'a mut Action>) -> usize {
    let mut dropped = 0;
    for action in actions {
        if action.shortcut.as_deref().is_some_and(is_reserved_shortcut) {
            action.shortcut = None;
            dropped += 1;
        }
    }
    dropped
}

fn release_items<'a>(items: impl IntoIterator<Item = &'a mut Item>) -> usize {
    items
        .into_iter()
        .map(|item| release_actions(&mut item.actions))
        .sum()
}

/// Only a list View's Items have Actions.
fn release_view(view: &mut View) -> usize {
    match &mut view.content {
        ViewContent::List { items, .. } => release_items(items),
        ViewContent::Form { .. } | ViewContent::Detail { .. } | ViewContent::Progress { .. } => 0,
    }
}

#[cfg(test)]
mod tests;
