//! Keeps Extensions' Actions off the key bindings a Front end keeps for
//! itself (`kotodama_protocol::RESERVED_SHORTCUTS`): a reserved binding an
//! Extension sends is dropped as it arrives, so no Front end ever sees it,
//! and the Action is kept without it. The Daemon tells the Extension which
//! binding it dropped, and a Manifest's Contributed action bound to one is a
//! problem of the Manifest.
//!
//! # Owns
//!
//! - Dropping reserved bindings from an Extension's messages and its Contributed actions, and
//!   saying which: [`Released`].
//!
//! # Not here
//!
//! - Which bindings are reserved: `kotodama_protocol::is_reserved_shortcut`.
//! - Clashes between two Extensions' bindings: `Daemon::contributed_for`.
//! - Sending the Extension its `rejected`: `daemon/from_extension.rs:extension_message`.

use kotodama_protocol::Action;
use kotodama_protocol::FromExtension;
use kotodama_protocol::Item;
use kotodama_protocol::Manifest;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;
use kotodama_protocol::is_reserved_shortcut;

/// A binding dropped because the menu keeps its key for itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Released {
    /// The id of the Action it was on.
    pub action: String,
    /// The binding as the Extension wrote it.
    pub shortcut: String,
}

impl Released {
    /// Why it was dropped, in words for the Extension's author.
    #[must_use]
    pub fn reason(&self) -> String {
        format!(
            "Action `{}` is bound to `{}`, a key the menu keeps for itself; the Action is kept without the \
             binding",
            self.action, self.shortcut
        )
    }
}

/// Drops every reserved binding of the Actions an Extension's message carries:
/// those of its Items, Live results and list Views. Returns the bindings it
/// dropped.
pub fn release_message(message: &mut FromExtension) -> Vec<Released> {
    let mut released = Vec::new();
    match message {
        FromExtension::AddItems { items } => release_items(items, &mut released),
        FromExtension::LiveResults { results, .. } => {
            release_items(results.iter_mut().map(|result| &mut result.item), &mut released);
        }
        FromExtension::PushView(view)
        | FromExtension::UpdateView(view)
        | FromExtension::ReplaceView { with: view, .. } => release_view(view, &mut released),
        FromExtension::RemoveItems { .. }
        | FromExtension::ActionDone { .. }
        | FromExtension::PopView { .. }
        | FromExtension::JobProgress(_)
        | FromExtension::RequestActionCatalog
        | FromExtension::WithdrawPick { .. }
        | FromExtension::Status { .. }
        | FromExtension::TargetAction { .. } => {}
    }
    released
}

/// Drops the binding of each of `actions` bound to a reserved key, and adds
/// it to `released`.
pub fn release_actions<'a>(actions: impl IntoIterator<Item = &'a mut Action>, released: &mut Vec<Released>) {
    for action in actions {
        if let Some(shortcut) = action.shortcut.take_if(|shortcut| is_reserved_shortcut(shortcut)) {
            released.push(Released {
                action: action.id.clone(),
                shortcut,
            });
        }
    }
}

/// The Contributed actions of `manifest` bound to a reserved key, whose
/// bindings the Daemon drops.
pub fn in_manifest(manifest: &Manifest) -> impl Iterator<Item = Released> {
    manifest.contributed_actions.iter().filter_map(|contributed| {
        let shortcut = contributed.action.shortcut.as_deref()?;
        is_reserved_shortcut(shortcut).then(|| Released {
            action: contributed.action.id.clone(),
            shortcut: shortcut.to_owned(),
        })
    })
}

fn release_items<'a>(items: impl IntoIterator<Item = &'a mut Item>, released: &mut Vec<Released>) {
    for item in items {
        release_actions(&mut item.actions, released);
    }
}

/// Only a list View's Items have Actions.
fn release_view(view: &mut View, released: &mut Vec<Released>) {
    match &mut view.content {
        ViewContent::List { items, .. } => release_items(items, released),
        ViewContent::Form { .. } | ViewContent::Detail { .. } | ViewContent::Progress { .. } => {}
    }
}

#[cfg(test)]
mod tests;
