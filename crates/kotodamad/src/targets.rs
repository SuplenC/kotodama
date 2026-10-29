//! Targeted Contributed actions: those an Extension offers on the Results
//! whose Identity it names while it runs, rather than on a whole Item kind.
//!
//! # Owns
//!
//! - [`Targets`]: each running Extension's targeted Actions, the Identities it last named for each,
//!   and the lookup from an Identity to the Actions on it.
//! - [`NotTargeted`]: why a `target_action` names nothing.
//!
//! # Not here
//!
//! - Contributed actions by Item kind, and merging both into a row: `daemon/rows.rs`.
//! - Taking in `FromExtension::TargetAction` and refreshing the menus: `daemon/from_extension.rs`.

use std::collections::HashMap;

use kotodama_protocol::Action;

use crate::daemon::ExtensionId;

/// The targeted Contributed actions of the running Extensions.
#[derive(Debug, Default)]
pub struct Targets {
    /// In Extension order, then Manifest order.
    actions: Vec<Targeted>,
    /// For each Identity some Action is offered on, those Actions, as
    /// indexes into `actions`, in order. A Result is looked up here by its
    /// Identity: an Item without one, or with none named, costs one miss.
    by_identity: HashMap<Box<str>, Vec<u32>>,
}

#[derive(Debug)]
struct Targeted {
    extension: ExtensionId,
    action: Action,
    /// The Identities the Extension named last, sorted, without repeats.
    identities: Box<[Box<str>]>,
}

/// A `target_action` naming an Action the Extension's Manifest doesn't
/// declare with `targeted = true`.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "target_action names `{0}`, which the Manifest doesn't declare as a Contributed action with `targeted = true`; \
     it is offered on nothing"
)]
pub struct NotTargeted(String);

impl Targets {
    /// Whether no Action is offered on any Identity.
    pub fn is_empty(&self) -> bool {
        self.by_identity.is_empty()
    }

    /// Takes the targeted Actions the running Extensions' Manifests declare,
    /// in Extension order. An Action an Extension still offers keeps the
    /// Identities it named.
    pub fn set_offered(&mut self, offered: impl IntoIterator<Item = (ExtensionId, Action)>) {
        let mut before = std::mem::take(&mut self.actions);
        self.actions = offered
            .into_iter()
            .map(|(extension, action)| {
                let kept = before
                    .iter_mut()
                    .find(|old| old.extension == extension && old.action.id == action.id)
                    .map(|old| std::mem::take(&mut old.identities));
                Targeted {
                    extension,
                    action,
                    identities: kept.unwrap_or_default(),
                }
            })
            .collect();
        self.reindex();
    }

    /// Drops every Identity `extension` named: its run ended, and the next
    /// one names them again.
    pub fn forget(&mut self, extension: ExtensionId) {
        let mut changed = false;
        for targeted in self
            .actions
            .iter_mut()
            .filter(|targeted| targeted.extension == extension)
        {
            changed |= !targeted.identities.is_empty();
            targeted.identities = Box::default();
        }
        if changed {
            self.reindex();
        }
    }

    /// Offers `extension`'s Action `action` on the Results of `identities`
    /// alone, in place of those it named before. Returns the Identities
    /// whose Actions changed: those it named before and those in
    /// `identities`.
    ///
    /// # Errors
    ///
    /// [`NotTargeted`] when `extension` offers no targeted Action `action`.
    pub fn target(
        &mut self,
        extension: ExtensionId,
        action: &str,
        identities: Vec<String>,
    ) -> Result<Vec<Box<str>>, NotTargeted> {
        let n = self
            .actions
            .iter()
            .position(|targeted| targeted.extension == extension && targeted.action.id == action)
            .ok_or_else(|| NotTargeted(action.to_owned()))?;
        let mut named: Vec<Box<str>> = identities.into_iter().map(String::into_boxed_str).collect();
        named.sort_unstable();
        named.dedup();
        let before = std::mem::replace(&mut self.actions[n].identities, named.into_boxed_slice());
        for identity in &before {
            if let Some(on) = self.by_identity.get_mut(identity) {
                on.retain(|&at| at != n as u32);
                if on.is_empty() {
                    self.by_identity.remove(identity);
                }
            }
        }
        for identity in &self.actions[n].identities {
            let on = self.by_identity.entry(identity.clone()).or_default();
            let at = on.partition_point(|&at| at < n as u32);
            on.insert(at, n as u32);
        }
        let mut changed = before.into_vec();
        changed.extend(self.actions[n].identities.iter().cloned());
        Ok(changed)
    }

    /// The Actions offered on the Results of `identity`, each with the
    /// Extension that runs it, in Extension order.
    pub fn on(&self, identity: &str) -> impl Iterator<Item = (ExtensionId, &Action)> {
        self.by_identity
            .get(identity)
            .into_iter()
            .flatten()
            .map(|&n| &self.actions[n as usize])
            .map(|targeted| (targeted.extension, &targeted.action))
    }

    fn reindex(&mut self) {
        self.by_identity.clear();
        for (n, targeted) in self.actions.iter().enumerate() {
            for identity in &targeted.identities {
                self.by_identity.entry(identity.clone()).or_default().push(n as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests;
