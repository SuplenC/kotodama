//! Results as the Front ends see them: the rows, and the Items behind them.
//!
//! # Owns
//!
//! - Finding the Item a source, an Alias or a History key names.
//! - Building a row: its texts, its Actions and the contributed ones.
//! - The catalog of every Action an Extension offers.
//!
//! # Not here
//!
//! - Ranking the candidates: `search.rs`.

use std::collections::BTreeMap;

use kotodama_protocol::Action;
use kotodama_protocol::ActionRef;
use kotodama_protocol::ActionRole;
use kotodama_protocol::ArgumentKind;
use kotodama_protocol::CatalogEntry;
use kotodama_protocol::Item;
use kotodama_protocol::KindName;
use kotodama_protocol::OfferedAction;
use kotodama_protocol::ShownResult;

use super::Daemon;
use super::Desktop;
use super::ExtensionId;
use super::Launcher;
use super::OWN;
use super::OwnAction;
use super::Session;
use super::Source;
use super::after_keyword;
use super::named_only;
use crate::arguments;
use crate::history::IDENTITIES;
use crate::history::Store;
use crate::index::ItemRef;
use crate::keywords::Alias;
use crate::rank;
use crate::rank::Text;
use crate::verbs::Verb;

impl<L: Launcher, D: Desktop, S: Store> Daemon<L, D, S> {
    /// Where the Result `alias` names comes from in this session:
    /// [`Source::Unavailable`] when its Item is gone or its Extension is down,
    /// and `None` for a hidden Item, which only `kotodama open --view` reaches,
    /// or for one a running Live query Extension may still answer with.
    pub(super) fn alias_source(&self, session: &Session, alias: &Alias) -> Option<Source> {
        if let Some(source) = self.source_of(session, &alias.extension, &alias.item) {
            return Some(source);
        }
        let Some(ext) = self.extension_named(&alias.extension) else {
            return Some(Source::Unavailable);
        };
        let running = &self.extensions[ext.0];
        let answers_live = running.sender().is_some() && running.debounce().is_some();
        let hidden = self
            .hidden
            .get(&ext)
            .is_some_and(|hidden| hidden.contains_key(&alias.item));
        (!answers_live && !hidden).then_some(Source::Unavailable)
    }

    /// Where the Result named by an Extension's name and an Item id comes from
    /// in this session, if it is there.
    pub(super) fn source_of(&self, session: &Session, extension: &str, id: &str) -> Option<Source> {
        if extension == OWN {
            return self.own_source(id);
        }
        let ext = self.extension_named(extension)?;
        if let Some(slot) = self.index.find(ext, id) {
            return Some(Source::Indexed(slot));
        }
        session.live[ext.0]
            .results
            .iter()
            .position(|live| live.item.id == id)
            .map(|n| Source::Live(ext, n as u32))
    }

    /// Where the Result named by an Extension's name and an Item id comes
    /// from, if the Daemon or the Index holds it: the same in every session.
    pub(super) fn held_source(&self, extension: &str, id: &str) -> Option<Source> {
        if extension == OWN {
            return self.own_source(id);
        }
        let ext = self.extension_named(extension)?;
        self.index.find(ext, id).map(Source::Indexed)
    }

    /// The Daemon's own Result with the id `id`, if there is one.
    pub(super) fn own_source(&self, id: &str) -> Option<Source> {
        self.own
            .iter()
            .position(|own| own.item.id == id)
            .map(|n| Source::Own(n as u32))
    }

    /// Where the Results the History keeps under `extension` and `id` come
    /// from in this session: every one of the Identity's, for [`IDENTITIES`],
    /// or else the one [`source_of`](Self::source_of) finds.
    pub(super) fn sources_of(&self, session: &Session, extension: &str, id: &str) -> Vec<Source> {
        if extension != IDENTITIES {
            return self.source_of(session, extension, id).into_iter().collect();
        }
        let indexed = self.index.identified(id).map(Source::Indexed);
        let live = session.live.iter().enumerate().flat_map(|(ext, live)| {
            live.results
                .iter()
                .enumerate()
                .filter(|(_, result)| result.item.identity.as_deref() == Some(id))
                .map(move |(n, _)| Source::Live(ExtensionId(ext), n as u32))
        });
        indexed.chain(live).collect()
    }

    /// A Result's Item and the name of the Extension that owns it.
    pub(super) fn item<'a>(&'a self, session: &'a Session, source: Source) -> Option<(&'a str, ItemRef<'a>)> {
        match source {
            Source::Indexed(slot) => {
                let item = self.index.item(slot)?;
                Some((self.extensions[slot.extension().0].name(), ItemRef::Resident(item)))
            }
            Source::Live(ext, n) => Some((
                self.extensions[ext.0].name(),
                ItemRef::Borrowed(&session.live[ext.0].results.get(n as usize)?.item),
            )),
            Source::Own(n) => Some((OWN, ItemRef::Borrowed(&self.own.get(n as usize)?.item))),
            Source::Unavailable => None,
        }
    }

    /// A Result's id and Identity, and the name of the Extension that owns
    /// it, without reading an Indexed item off its Shelf.
    pub(super) fn names<'a>(
        &'a self,
        session: &'a Session,
        source: Source,
    ) -> Option<(&'a str, &'a str, Option<&'a str>)> {
        let item = match source {
            Source::Indexed(slot) => {
                return Some((
                    self.extensions[slot.extension().0].name(),
                    self.index.id(slot)?,
                    self.index.identity(slot),
                ));
            }
            Source::Live(ext, n) => &session.live[ext.0].results.get(n as usize)?.item,
            Source::Own(n) => &self.own.get(n as usize)?.item,
            Source::Unavailable => return None,
        };
        let extension = match source {
            Source::Live(ext, _) => self.extensions[ext.0].name(),
            _ => OWN,
        };
        Some((extension, &item.id, item.identity.as_deref()))
    }

    /// What the History keeps a Result under: its Identity under
    /// [`IDENTITIES`], or else its Extension and id.
    pub(super) fn history_key<'a>(&'a self, session: &'a Session, source: Source) -> Option<(&'a str, &'a str)> {
        let (extension, id, identity) = self.names(session, source)?;
        Some(match identity {
            Some(identity) => (IDENTITIES, identity),
            None => (extension, id),
        })
    }

    /// A Result's Identity, if it has one.
    pub(super) fn identity<'a>(&'a self, session: &'a Session, source: Source) -> Option<&'a str> {
        match source {
            Source::Indexed(slot) => self.index.identity(slot),
            _ => self.names(session, source)?.2,
        }
    }

    /// One Result as the Front end gets it. `primary`, one of the Item's own
    /// Actions, is moved to the front.
    pub(super) fn row(&mut self, session: &Session, source: Source, primary: Option<&str>) -> Option<ShownResult> {
        let (live_title, mut live_starts);
        let (owner, item, title): (Option<ExtensionId>, ItemRef, Text) = match source {
            Source::Indexed(slot) => (
                Some(slot.extension()),
                ItemRef::Resident(self.index.item(slot)?),
                self.index.texts(slot)?.title,
            ),
            Source::Live(ext, n) => {
                let item = &session.live[ext.0].results.get(n as usize)?.item;
                live_title = rank::folded(&item.title);
                live_starts = Vec::new();
                rank::word_starts(live_title.slice(..), Some(&item.title), &mut live_starts);
                let title = Text {
                    chars: live_title.slice(..),
                    starts: &live_starts,
                };
                (Some(ext), ItemRef::Borrowed(item), title)
            }
            Source::Own(n) => {
                let own = self.own.get(n as usize)?;
                (None, ItemRef::Borrowed(&own.item), own.haystack.texts().title)
            }
            Source::Unavailable => return self.unavailable_row(session),
        };
        self.highlights.clear();
        let pattern = match &session.subject {
            Some(subject) if session.by_verb.binary_search(&source).is_ok() => subject,
            _ => &session.pattern,
        };
        if pattern
            .indices(title.chars, &mut self.matcher, &mut self.highlights)
            .is_some()
        {
            self.highlights.sort_unstable();
            self.highlights.dedup();
        } else {
            self.highlights.clear();
            // A near miss: the words it nearly matches.
            if session.folded.tolerates_typos() {
                self.tiers
                    .near_miss_highlights(title, &session.folded, &mut self.highlights);
            }
        }
        let mut item = item.clone();
        if let Source::Own(n) = source {
            match self.own.get(n as usize).map(|own| own.does) {
                Some(OwnAction::Quicklink(quicklink)) => {
                    let quicklink = &self.keywords.quicklinks[quicklink];
                    if quicklink.takes_query()
                        && let Some(query) = after_keyword(&session.query, &quicklink.keyword)
                        && !query.is_empty()
                    {
                        item.title = format!("{} for “{query}”", item.title);
                    }
                }
                Some(OwnAction::App(app)) => {
                    if let Some(query) = self.after_word(session, app)
                        && !query.is_empty()
                    {
                        item.title = format!("Search {} for “{query}”", item.title);
                    }
                }
                _ => {}
            }
        }
        if let Some(at) = primary.and_then(|id| item.actions.iter().position(|a| a.id == id)) {
            item.actions[..=at].rotate_right(1);
        }
        let (extension, mut contributed) = match owner {
            Some(owner) => (
                self.extensions[owner.0].name().to_owned(),
                self.contributed_for(owner, item.kind.name(), item.identity.as_deref(), &item.actions),
            ),
            None => (OWN.to_owned(), Vec::new()),
        };
        let offered = contributed.iter_mut().map(|offered| &mut offered.action);
        Self::mark_forms(session, item.keyword.as_deref(), item.actions.iter_mut().chain(offered));
        self.offer_forgetting(&extension, &mut item, &mut contributed);
        let chosen = session
            .verb
            .as_ref()
            .filter(|_| session.by_verb.binary_search(&source).is_ok())
            .and_then(|verb| verb_choice(verb, &extension, &item.actions, &contributed))
            .filter(|chosen| !is_first(chosen, &extension, &item.actions));
        let alias = session
            .via_alias
            .as_ref()
            .filter(|(named, _)| *named == source)
            .map(|(_, keyword)| keyword.clone());
        Some(ShownResult {
            extension,
            item,
            contributed,
            highlights: self.highlights.clone(),
            alias,
            unavailable: None,
            primary: chosen,
        })
    }

    /// The row of the Alias the session's query names when it can't run: the
    /// Item as last seen, or its id, without Actions, and why it can't run.
    pub(super) fn unavailable_row(&self, session: &Session) -> Option<ShownResult> {
        let (_, keyword) = session
            .via_alias
            .as_ref()
            .filter(|(source, _)| *source == Source::Unavailable)?;
        let alias = self.keywords.aliases.get(keyword)?;
        let name = &alias.extension;
        let why = match self.extension_named(name) {
            None => format!("No Extension {name}"),
            Some(ext) if self.extensions[ext.0].sender().is_none() => {
                format!("{name} isn't running")
            }
            Some(_) => format!("No longer in {name}"),
        };
        let seen = self.last_seen.get(&(name.clone(), alias.item.clone()));
        let mut item = match seen {
            Some(item) => Item::clone(item),
            None => named_only(&alias.item),
        };
        item.actions.clear();
        Some(ShownResult {
            extension: name.clone(),
            item,
            contributed: Vec::new(),
            highlights: Vec::new(),
            alias: Some(keyword.clone()),
            unavailable: Some(why),
            primary: None,
        })
    }

    /// The Action the session's verb names on the Indexed item `source`, if
    /// it offers one, and whether that is the Item's first own Action: see
    /// [`verb_choice`].
    pub(super) fn verb_choice(&self, session: &Session, source: Source) -> Option<(ActionRef, bool)> {
        let verb = session.verb.as_ref()?;
        let Source::Indexed(slot) = source else {
            return None;
        };
        let (extension, item) = self.item(session, source)?;
        let contributed = self.contributed_for(
            slot.extension(),
            item.kind.name(),
            item.identity.as_deref(),
            &item.actions,
        );
        let chosen = verb_choice(verb, extension, &item.actions, &contributed)?;
        let first = is_first(&chosen, extension, &item.actions);
        Some((chosen, first))
    }

    /// Other running Extensions' Actions for an Item of `kind` whose Identity
    /// is `identity`, in Extension order, and within one Extension those
    /// offered on the kind before those targeted at the Identity. When a
    /// shortcut clashes, the owner's binding in `own` wins, then the earlier
    /// Extension's.
    pub(super) fn contributed_for(
        &self,
        owner: ExtensionId,
        kind: &str,
        identity: Option<&str>,
        own: &[Action],
    ) -> Vec<OfferedAction> {
        let by_kind = self.contributed.get(kind);
        let identity = identity.filter(|_| !self.targets.is_empty());
        if by_kind.is_none() && identity.is_none() {
            return Vec::new();
        }
        let by_kind = by_kind.into_iter().flatten().map(|(ext, action)| (*ext, action));
        let mut offered: Vec<(ExtensionId, &Action)> = by_kind.collect();
        if let Some(identity) = identity {
            let by_kind = offered.len();
            offered.extend(self.targets.on(identity));
            if offered.len() > by_kind && by_kind > 0 {
                offered.sort_by_key(|(ext, _)| ext.0);
            }
        }
        let mut taken: Vec<&str> = own.iter().filter_map(|action| action.shortcut.as_deref()).collect();
        offered
            .into_iter()
            .filter(|(ext, _)| *ext != owner)
            .map(|(ext, offered)| {
                let mut action = offered.clone();
                if let Some(shortcut) = offered.shortcut.as_deref() {
                    if taken.contains(&shortcut) {
                        action.shortcut = None;
                    } else {
                        taken.push(shortcut);
                    }
                }
                OfferedAction {
                    extension: self.extensions[ext.0].name().to_owned(),
                    action,
                }
            })
            .collect()
    }

    /// Reports [`ActionRole::Form`] for each of `actions` that would ask for
    /// its Arguments in a form if it ran: a required one is left
    /// unfilled by what follows the Item's `keyword` in the query. A
    /// destructive Action stays destructive.
    pub(super) fn mark_forms<'a>(
        session: &Session,
        keyword: Option<&str>,
        actions: impl IntoIterator<Item = &'a mut Action>,
    ) {
        let mut typed = None;
        for action in actions {
            if action.role != ActionRole::Normal
                || action
                    .arguments
                    .iter()
                    .all(|argument| argument.optional && argument.kind != ArgumentKind::Number)
            {
                continue;
            }
            let typed = typed.get_or_insert_with(|| {
                keyword
                    .and_then(|keyword| after_keyword(session.searched(), &keyword.to_lowercase()))
                    .unwrap_or_default()
            });
            let mut given = BTreeMap::new();
            arguments::fill(&action.arguments, typed, &mut given);
            if arguments::missing(&action.arguments, &given).is_some()
                || !arguments::invalid(&action.arguments, &given).is_empty()
            {
                action.role = ActionRole::Form;
            }
        }
    }

    /// Every Action on offer from running Extensions: their Contributed
    /// actions, then the Actions on their Indexed items, one entry per Item
    /// kind and Action id. Never the Items themselves.
    pub(super) fn compute_catalog(&self) -> Vec<CatalogEntry> {
        let mut entries = Vec::new();
        for (id, ext) in self.extensions.iter().enumerate() {
            if ext.sender().is_none() {
                continue;
            }
            for contributed in &ext.found.manifest.contributed_actions {
                // The catalog lists Actions by the kind they run on, and a
                // targeted one runs on no kind.
                let Some(kind) = contributed.to.kind() else {
                    continue;
                };
                entries.push(CatalogEntry {
                    extension: ext.name().to_owned(),
                    kind: kind.clone(),
                    action: contributed.action.clone(),
                });
            }
            let mut own: BTreeMap<(String, String), Action> = BTreeMap::new();
            for (kind, action) in self.index.offered(ExtensionId(id)) {
                own.entry((kind, action.id.clone())).or_insert(action);
            }
            for ((kind, _), action) in own {
                if let Ok(kind) = KindName::new(&kind) {
                    entries.push(CatalogEntry {
                        extension: ext.name().to_owned(),
                        kind,
                        action,
                    });
                }
            }
        }
        entries
    }
}

/// The Action `verb` names among a Result's Actions: the first of its own,
/// `own`, owned by `owner`, with a keyword the verb names, else the first
/// such of `contributed`.
fn verb_choice(verb: &Verb, owner: &str, own: &[Action], contributed: &[OfferedAction]) -> Option<ActionRef> {
    if let Some(action) = own.iter().find(|action| verb.names(action)) {
        return Some(ActionRef {
            extension: owner.to_owned(),
            id: action.id.clone(),
        });
    }
    let offered = contributed.iter().find(|offered| verb.names(&offered.action))?;
    Some(ActionRef {
        extension: offered.extension.clone(),
        id: offered.action.id.clone(),
    })
}

/// Whether `chosen` is the first of `own`, the Actions of a Result `owner`
/// owns: the Action Enter runs anyway.
fn is_first(chosen: &ActionRef, owner: &str, own: &[Action]) -> bool {
    chosen.extension == owner && own.first().is_some_and(|first| first.id == chosen.id)
}
