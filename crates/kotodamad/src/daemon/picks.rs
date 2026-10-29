//! Running what the user picks: an Action of a Result, an Action of the
//! catalog, or one of the Daemon's own.
//!
//! # Owns
//!
//! - Sending an Action to its Extension, or running the Daemon's own.
//! - Asking for an Action's arguments, and the pick an Action offers.
//! - Forgetting a Result or an Extension's history, and what the History learns from a pick.
//!
//! # Not here
//!
//! - Undoing a change to the History: `undo.rs`.
//! - The History itself: `crate::history`.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use kotodama_protocol::Action;
use kotodama_protocol::ActionRef;
use kotodama_protocol::FORGET_SHORTCUT;
use kotodama_protocol::FormInput;
use kotodama_protocol::Item;
use kotodama_protocol::KindName;
use kotodama_protocol::OfferedAction;
use kotodama_protocol::ResultRef;
use kotodama_protocol::ToExtension;
use kotodama_protocol::ToFrontEnd;
use kotodama_protocol::ViewContent;

use super::ArgumentForm;
use super::CLEAR_EXTENSION_HISTORY;
use super::Daemon;
use super::Desktop;
use super::Done;
use super::ExtensionId;
use super::FORGET;
use super::FrontEndId;
use super::Launcher;
use super::Missing;
use super::OWN;
use super::OwnAction;
use super::Session;
use super::Source;
use super::Undoable;
use super::UndoneBy;
use super::Update;
use super::action_titled;
use super::after_keyword;
use super::alias_id;
use crate::arguments;
use crate::history::Change;
use crate::history::IDENTITIES;
use crate::history::ResultKey;
use crate::history::Store;
use crate::index::ItemRef;
use crate::index::Slot;
use crate::pick;
use crate::pick::Pick;
use crate::problem;
use crate::targets::Targets;
use crate::usage::Entry;
use crate::usage::Used;

impl<L: Launcher, D: Desktop, S: Store> Daemon<L, D, S> {
    /// Runs an Action from the Action catalog that `proposer` proposed and the
    /// user confirmed, on the one Indexed item of its kind that offers it, as
    /// if the user had picked that Item's Action. When several offer it, the
    /// user picks one first.
    pub(super) fn run_catalog_action(
        &mut self,
        front_end: FrontEndId,
        proposer: ExtensionId,
        kind: &KindName,
        action: ActionRef,
        arguments: BTreeMap<String, String>,
        now: Instant,
    ) -> Result<(), String> {
        let asking = &self.extensions[proposer.0];
        if !asking.found.manifest.action_catalog {
            return Err(format!("{} may not run Actions from the Action catalog", asking.name()));
        }
        let target = self
            .extension_named(&action.extension)
            .filter(|&target| target != proposer && self.extensions[target.0].sender().is_some())
            .ok_or_else(|| format!("{} isn't running", action.extension))?;
        let mut offering: Vec<Slot> = self.index.offering(target, kind.as_str(), &action.id).collect();
        let id = match offering.len() {
            1 => self
                .index
                .id(offering.swap_remove(0))
                .map(str::to_owned)
                .ok_or_else(|| format!("no {kind} Item offers `{}`", action.id))?,
            2.. => return self.offer_pick(front_end, target, &offering, action, arguments),
            0 => {
                let contributed = self.extensions[target.0]
                    .found
                    .manifest
                    .contributed_actions
                    .iter()
                    .find(|offered| offered.to.kind() == Some(kind) && offered.action.id == action.id);
                return Err(match contributed {
                    Some(offered) => format!(
                        "pick a {kind} in the search, then “{}” in its Action panel",
                        offered.action.title
                    ),
                    None => format!("no {kind} Item offers `{}`", action.id),
                });
            }
        };
        let result = ResultRef {
            extension: action.extension.clone(),
            id,
        };
        self.run_action(front_end, &result, &action, arguments, Missing::AskInForm, now)
    }

    /// Asks `front_end` which of the Items in `offering` the Action runs on,
    /// in a list View of the Daemon's own, in place of any it asked before.
    pub(super) fn offer_pick(
        &mut self,
        front_end: FrontEndId,
        target: ExtensionId,
        offering: &[Slot],
        action: ActionRef,
        arguments: BTreeMap<String, String>,
    ) -> Result<(), String> {
        let items: Vec<Item> = offering
            .iter()
            .filter_map(|&slot| self.index.item(slot))
            .map(Arc::unwrap_or_clone)
            .collect();
        if items.is_empty() {
            return Err("those Items are gone".to_owned());
        }
        let title = items
            .iter()
            .flat_map(|item| &item.actions)
            .find(|offered| offered.id == action.id)
            .map_or_else(|| action.id.clone(), |offered| offered.title.clone());
        self.cancel_pick(front_end);
        let Some(session) = self.sessions.get(&front_end) else {
            return Ok(());
        };
        let id = format!("pick-{}", self.next_id);
        self.next_id += 1;
        let view = pick::view(id.clone(), &title, items);
        session.send(ToFrontEnd::PushView {
            extension: OWN.to_owned(),
            view: view.clone(),
            contributed: BTreeMap::new(),
        });
        self.pick_lists.push(target, view);
        self.picks.insert(
            front_end,
            Pick {
                view: id,
                target,
                action,
                arguments,
            },
        );
        Ok(())
    }

    /// The user picked `result` in the list of `front_end`'s pick: the list
    /// goes, and the proposed Action runs on it.
    pub(super) fn run_pick(&mut self, front_end: FrontEndId, result: &ResultRef, now: Instant) -> Result<(), String> {
        let gone = || "that Result is gone".to_owned();
        let listed = self.picks.get(&front_end).is_some_and(|pick| {
            self.pick_lists
                .get(pick.target, &pick.view)
                .is_some_and(|open| open.items().iter().any(|item| item.id == result.id))
        });
        if !listed {
            return Err(gone());
        }
        let pick = self.cancel_pick(front_end).ok_or_else(gone)?;
        let result = ResultRef {
            extension: pick.action.extension.clone(),
            id: result.id.clone(),
        };
        self.run_action(
            front_end,
            &result,
            &pick.action,
            pick.arguments,
            Missing::AskInForm,
            now,
        )
    }

    /// Drops `front_end`'s pick, if it has one, and takes its list off the
    /// menu, unless the menu took it off already.
    pub(super) fn cancel_pick(&mut self, front_end: FrontEndId) -> Option<Pick> {
        let pick = self.picks.remove(&front_end)?;
        self.pick_lists.pop(pick.target, &pick.view);
        if let Some(session) = self.sessions.get(&front_end) {
            session.send(ToFrontEnd::PopView {
                extension: OWN.to_owned(),
                view: pick.view.clone(),
            });
        }
        Some(pick)
    }

    /// The user left the list of `front_end`'s pick `view`, with Esc.
    pub(super) fn pick_popped(&mut self, front_end: FrontEndId, view: &str) {
        if self.picks.get(&front_end).is_some_and(|pick| pick.view == view)
            && let Some(pick) = self.picks.remove(&front_end)
        {
            self.pick_lists.pop(pick.target, &pick.view);
        }
    }

    /// The user typed `text` into the list of `front_end`'s pick `view`.
    pub(super) fn filter_pick(&mut self, front_end: FrontEndId, view: &str, text: String) {
        let Some(pick) = self.picks.get(&front_end).filter(|pick| pick.view == view) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let matches = self
            .pick_lists
            .filter(pick.target, view, &text, &mut self.matcher, &mut self.tiers);
        if let Some(session) = self.sessions.get(&front_end) {
            session.send(ToFrontEnd::FilteredView {
                extension: OWN.to_owned(),
                view: view.to_owned(),
                text,
                matches,
            });
        }
    }

    pub(super) fn run_action(
        &mut self,
        front_end: FrontEndId,
        result: &ResultRef,
        action: &ActionRef,
        mut arguments: BTreeMap<String, String>,
        missing: Missing,
        now: Instant,
    ) -> Result<(), String> {
        let gone = || "that Result is gone".to_owned();
        let owner = self.extension_named(&result.extension).ok_or_else(gone)?;
        let session = self.sessions.get(&front_end).ok_or_else(gone)?;
        let searched = match self.index.find(owner, &result.id) {
            Some(slot) => self.index.item(slot).map(ItemRef::Resident),
            None => session.live[owner.0]
                .results
                .iter()
                .rev()
                .find(|live| live.item.id == result.id)
                .map(|live| ItemRef::Borrowed(&live.item)),
        };
        // Or an Item of one of the owner's list Views.
        let in_view = || self.views.item(owner, &result.id).map(ItemRef::Borrowed);
        let found_searched = searched.is_some();
        let item = searched.or_else(in_view).ok_or_else(gone)?;

        let own = (action.extension == result.extension)
            .then(|| item.actions.iter().find(|own| own.id == action.id))
            .flatten();
        let (target, wanted) = match own {
            Some(own) => Some((owner, own)),
            None => self
                .extension_named(&action.extension)
                .filter(|&ext| ext != owner)
                .and_then(|ext| {
                    Some((
                        ext,
                        contributed_to(&self.contributed, &self.targets, &item, ext, &action.id)?,
                    ))
                }),
        }
        .ok_or_else(|| format!("`{}` has no Action `{}`", item.title, action.id))?;

        // Arguments are typed in the search, after the Item's keyword.
        if found_searched
            && let Some(keyword) = &item.keyword
            && let Some(typed) = after_keyword(session.searched(), &keyword.to_lowercase())
        {
            arguments::fill(&wanted.arguments, typed, &mut arguments);
        }
        if let Some(absent) = arguments::missing(&wanted.arguments, &arguments) {
            if missing == Missing::Fail {
                return Err(format!("`{}` needs {}", wanted.title, absent.name));
            }
            let id = format!("arguments-{}", self.next_id);
            self.next_id += 1;
            let view = arguments::form(id.clone(), &wanted.title, &wanted.arguments, &arguments);
            let fields = match &view.content {
                ViewContent::Form { fields, .. } => fields
                    .iter()
                    .filter(|field| matches!(field.input, FormInput::File { .. }))
                    .cloned()
                    .collect(),
                _ => Vec::new(),
            };
            session.send(ToFrontEnd::PushView {
                extension: OWN.to_owned(),
                view,
                contributed: BTreeMap::new(),
            });
            self.argument_forms.insert(
                id,
                ArgumentForm {
                    front_end,
                    result: result.clone(),
                    action: action.clone(),
                    arguments,
                    fields,
                },
            );
            return Ok(());
        }
        if let Some(home) = &self.home {
            arguments::absolute_paths(&wanted.arguments, &mut arguments, home);
        }

        let ext = &self.extensions[target.0];
        if ext.sender().is_none() {
            return Err(format!("{} isn't running", ext.name()));
        }
        let invocation = self.next_id;
        self.next_id += 1;
        let answering = ext.send(ToExtension::RunAction {
            invocation,
            action: action.id.clone(),
            item: item.to_ref(),
            arguments,
        });
        // An Extension that stopped reading is stopped once this event is
        // handled, so the Action fails at once rather than when its ExtensionDown
        // arrives.
        if !answering {
            return Err(format!("{} stopped answering", ext.name()));
        }
        self.invocations.insert(invocation, (front_end, target));
        let change = self.picked(front_end, result, action, now);
        if !change.is_empty() {
            // Kept by the invocation's number, which the Extension withdraws
            // the pick by.
            self.undos.push(Undoable {
                id: invocation,
                by: UndoneBy::Extension(target),
                change,
                until: now + self.history.settings().undo_for(),
            });
        }
        Ok(())
    }

    /// Runs an Action on one of the Daemon's own Results, then closes the menu.
    pub(super) fn run_own(
        &mut self,
        front_end: FrontEndId,
        result: &ResultRef,
        action: &ActionRef,
        arguments: &BTreeMap<String, String>,
        now: Instant,
    ) -> Result<(), String> {
        let gone = || "that Result is gone".to_owned();
        let session = self.sessions.get(&front_end).ok_or_else(gone)?;
        let n = self
            .own
            .iter()
            .position(|own| own.item.id == result.id)
            .ok_or_else(gone)?;
        let item = &self.own[n].item;
        if action.extension != OWN || !item.actions.iter().any(|own| own.id == action.id) {
            return Err(format!("`{}` has no Action `{}`", item.title, action.id));
        }
        match self.own[n].does {
            OwnAction::Quicklink(quicklink) => {
                let quicklink = &self.keywords.quicklinks[quicklink];
                let query = match arguments.get("query") {
                    Some(query) => query.as_str(),
                    None => after_keyword(&session.query, &quicklink.keyword).unwrap_or(""),
                };
                self.desktop.launch(quicklink.command(query));
                session.send(ToFrontEnd::Close);
                self.picked(front_end, result, action, now);
            }
            OwnAction::App(n) => {
                let word = &self.apps.get(n).ok_or_else(gone)?.word;
                let rest = self.after_word(session, n).unwrap_or_default();
                let text = format!("{word} {rest}");
                // The Front end takes the query from here, so the app, its
                // intro card included, is in the Results that follow, with
                // no query back first.
                session.send(ToFrontEnd::SetQuery { text: text.clone() });
                self.picked(front_end, result, action, now);
                if let Some(session) = self.sessions.get_mut(&front_end) {
                    session.entered = Some(n);
                }
                self.query(front_end, text, now);
            }
            OwnAction::ClearHistory => {
                session.send(ToFrontEnd::Close);
                let change = self.history.clear();
                self.store.forget_usage(None);
                let done = Done::new("Cleared history", "Undo clearing history");
                self.changed(done, change, now);
            }
            OwnAction::Problems => {
                session.send(ToFrontEnd::PushView {
                    extension: OWN.to_owned(),
                    view: problem::view(&self.problems.sent),
                    contributed: BTreeMap::new(),
                });
                self.problems_open.insert(front_end);
            }
            OwnAction::Undo => {
                if let Some(latest) = self.latest_undo().map(|undo| undo.id) {
                    self.undo(latest);
                }
            }
        }
        Ok(())
    }

    /// Forgets one Result: everything the History keeps for it. The menu
    /// stays open.
    pub(super) fn forget_result(
        &mut self,
        front_end: FrontEndId,
        result: &ResultRef,
        now: Instant,
    ) -> Result<(), String> {
        let (session, item) = self.shown_item(front_end, result)?;
        let keys = self.history_keys(session, &result.extension, &item);
        let done = Done::new(
            format!("Forgot “{}”", item.title),
            format!("Undo forgetting “{}”", item.title),
        );
        let change = self.history.forget(&keys);
        self.changed(done, change, now);
        Ok(())
    }

    /// Forgets every Result of the Extension that owns `result`, the things
    /// they stand for, and the Aliases that name them. The menu stays open.
    pub(super) fn clear_extension_history(
        &mut self,
        front_end: FrontEndId,
        result: &ResultRef,
        now: Instant,
    ) -> Result<(), String> {
        let name = result.extension.as_str();
        let (_, item) = self.shown_item(front_end, result)?;
        let ext = self
            .extension_named(name)
            .ok_or_else(|| format!("`{}` has no Action `{CLEAR_EXTENSION_HISTORY}`", item.title))?;
        let live = self
            .sessions
            .get(&front_end)
            .map(|session| session.live[ext.0].results.as_slice())
            .unwrap_or_default();
        // Their Identities go too, whoever else offers them: the History
        // doesn't know which Extension a pick of one came through.
        let identities: HashSet<&str> = self
            .index
            .identities(ext)
            .chain(live.iter().filter_map(|live| live.item.identity.as_deref()))
            .collect();
        let aliases: HashSet<String> = self
            .keywords
            .aliases
            .iter()
            .filter(|(_, alias)| alias.extension == name)
            .map(|(keyword, _)| alias_id(keyword))
            .collect();
        let change = self.history.forget_where(|(extension, id)| {
            extension == name
                || (extension == IDENTITIES && identities.contains(id))
                || (extension == OWN && aliases.contains(id))
        });
        self.store.forget_usage(Some(name));
        let done = Done::new(
            format!("Cleared the history of {name}"),
            format!("Undo clearing the history of {name}"),
        );
        self.changed(done, change, now);
        Ok(())
    }

    /// The Item of a Result shown in a Front end's session, with the session.
    pub(super) fn shown_item(
        &self,
        front_end: FrontEndId,
        result: &ResultRef,
    ) -> Result<(&Session, ItemRef<'_>), String> {
        let gone = || "that Result is gone".to_owned();
        let session = self.sessions.get(&front_end).ok_or_else(gone)?;
        let source = self
            .source_of(session, &result.extension, &result.id)
            .ok_or_else(gone)?;
        let (_, item) = self.item(session, source).ok_or_else(gone)?;
        Ok((session, item))
    }

    /// The ids the picks of the Aliases that name a Result are kept under.
    pub(super) fn alias_ids<'a>(&'a self, extension: &'a str, id: &'a str) -> impl Iterator<Item = String> + 'a {
        self.keywords
            .aliases
            .iter()
            .filter(move |(_, alias)| alias.extension == extension && alias.item == id)
            .map(|(keyword, _)| alias_id(keyword))
    }

    /// Everything the History may keep for one Result: its own picks, its
    /// Identity's and those of every other Result of that Identity, and the
    /// picks of the Aliases that name it.
    pub(super) fn history_keys(&self, session: &Session, extension: &str, item: &Item) -> Vec<ResultKey> {
        let mut keys = vec![(extension.to_owned(), item.id.clone())];
        if let Some(identity) = &item.identity {
            keys.push((IDENTITIES.to_owned(), identity.clone()));
            for source in self.sources_of(session, IDENTITIES, identity) {
                if let Some((other, item)) = self.item(session, source) {
                    keys.push((other.to_owned(), item.id.clone()));
                }
            }
        }
        keys.extend(self.alias_ids(extension, &item.id).map(|id| (OWN.to_owned(), id)));
        keys
    }

    /// Adds the Actions the Daemon offers on a Result the History keeps
    /// anything for: Forget, and clearing its Extension's History. They go
    /// with the Item's own Actions on the Daemon's own Results,
    /// and after the Contributed actions on the rest. Only Results with
    /// a History get them, so a row costs nothing more until something is
    /// picked through it.
    pub(super) fn offer_forgetting(&self, extension: &str, item: &mut Item, contributed: &mut Vec<OfferedAction>) {
        if self.history.is_empty() {
            return;
        }
        let picked = |extension: &str, id: &str| self.history.picks(extension, id).is_some();
        let remembered = picked(extension, &item.id)
            || item
                .identity
                .as_deref()
                .is_some_and(|identity| picked(IDENTITIES, identity))
            || self.alias_ids(extension, &item.id).any(|alias| picked(OWN, &alias));
        if !remembered {
            return;
        }
        let forget = action_titled(FORGET, "Forget".into()).shortcut(FORGET_SHORTCUT);
        if extension == OWN {
            item.actions.push(forget);
            return;
        }
        // Without `format!`: every Result the History keeps anything for pays
        // for this title on every keystroke, and `concat` skips the formatter.
        let clear = action_titled(CLEAR_EXTENSION_HISTORY, ["Clear the history of ", extension].concat());
        for action in [forget, clear] {
            contributed.push(OfferedAction {
                extension: OWN.to_owned(),
                action,
            });
        }
    }

    /// Records that the user picked a Result in a Front end's session, by
    /// running `action`. A Result an Alias named counts for the Alias alone.
    /// An answer goes in the Usage log only: neither Frecency nor Learned
    /// choices count it, and a Result picked below one isn't learned as
    /// picked over it. A pick in an app kept out of the search is never a
    /// Learned choice, and goes in the Usage log without its query.
    /// Returns the change to the History, to undo it.
    pub(super) fn picked(
        &mut self,
        front_end: FrontEndId,
        result: &ResultRef,
        action: &ActionRef,
        now: Instant,
    ) -> Change {
        let when = self.when(now);
        let at = when.at;
        let Some(session) = self.sessions.get(&front_end) else {
            return Change::default();
        };
        let source = self.source_of(session, &result.extension, &result.id);
        let row = session.shown.iter().position(|(shown, _)| Some(*shown) == source);
        let private = session.in_private_app(&self.apps);
        if self.history.settings().usage_log {
            self.store.log(Entry {
                at,
                used: Used::Pick {
                    query: if private { String::new() } else { session.query.clone() },
                    extension: result.extension.clone(),
                    id: result.id.clone(),
                    action: action.id.clone(),
                    row,
                },
            });
        }
        let change = if let Some((_, alias)) = session.via_alias.as_ref().filter(|(named, _)| Some(*named) == source) {
            self.history.touch(OWN, &alias_id(alias), when)
        } else if source.is_some_and(|source| session.is_answer(source)) {
            // An answer is the query worked out, a new Result for every
            // expression, and takes the top row whatever its Frecency: the
            // History would only fill up with it.
            return Change::default();
        } else if let Some(Source::Live(ext, _)) = source
            && self.extensions[ext.0].fallback()
        {
            // Frecency orders the fallbacks among themselves, but a fallback
            // is never a Learned choice, which would take over from the one
            // among the real matches.
            self.history.touch(&result.extension, &result.id, when)
        } else if private {
            self.history.touch(&result.extension, &result.id, when)
        } else {
            let key = source
                .and_then(|source| self.history_key(session, source))
                .unwrap_or((&result.extension, &result.id));
            // What ranked above it, nearest first: what it was picked over.
            // The same thing from another Extension wasn't.
            let above: Vec<ResultKey> = row
                .map(|at| {
                    session.shown[..at]
                        .iter()
                        .rev()
                        .filter(|&&(_, answer)| !answer)
                        .filter_map(|&(shown, _)| self.history_key(session, shown))
                        .filter(|&above| above != key)
                        .map(|(extension, id)| (extension.to_owned(), id.to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            let (extension, id) = (key.0.to_owned(), key.1.to_owned());
            let query = session.key.clone();
            let mut change = self.history.pick(&extension, &id, &query, &above, when);
            if extension == IDENTITIES {
                // The Result itself too, so it goes ahead of the same thing
                // from other Extensions.
                change.and(self.history.touch(&result.extension, &result.id, when));
            }
            change
        };
        self.store.save(&self.history);
        self.refresh_empty(Update::Late, None);
        change
    }
}

/// The Contributed action `id` that the Extension `by` offers on `item`: on
/// its kind, among `contributed`, or targeted at its Identity.
fn contributed_to<'a>(
    contributed: &'a HashMap<String, Vec<(ExtensionId, Action)>>,
    targets: &'a Targets,
    item: &Item,
    by: ExtensionId,
    id: &str,
) -> Option<&'a Action> {
    let by_kind = contributed.get(item.kind.name()).into_iter().flatten();
    let targeted = item
        .identity
        .as_deref()
        .into_iter()
        .flat_map(|identity| targets.on(identity));
    by_kind
        .map(|(ext, offered)| (*ext, offered))
        .chain(targeted)
        .find(|(ext, offered)| *ext == by && offered.id == id)
        .map(|(_, offered)| offered)
}
