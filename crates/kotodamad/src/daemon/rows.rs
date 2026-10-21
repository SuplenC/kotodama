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
use crate::history::IDENTITIES;
use crate::history::Store;
use crate::index::ItemRef;
use crate::keywords::Alias;
use crate::rank;
use crate::rank::Text;

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

    /// What the History keeps a Result under, as [`history_key`] gives it.
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
        if session
            .pattern
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
                self.contributed_for(owner, item.kind.name(), &item.actions),
            ),
            None => (OWN.to_owned(), Vec::new()),
        };
        self.offer_forgetting(&extension, &mut item, &mut contributed);
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
        })
    }

    /// Other running Extensions' Actions for Items of `kind`, in Extension
    /// order. When a shortcut clashes, the owner's binding in `own` wins, then
    /// the earlier Extension's.
    pub(super) fn contributed_for(&self, owner: ExtensionId, kind: &str, own: &[Action]) -> Vec<OfferedAction> {
        let Some(offered) = self.contributed.get(kind) else {
            return Vec::new();
        };
        let mut taken: Vec<&str> = own.iter().filter_map(|action| action.shortcut.as_deref()).collect();
        offered
            .iter()
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
                entries.push(CatalogEntry {
                    extension: ext.name().to_owned(),
                    kind: contributed.kind.clone(),
                    action: contributed.action.clone(),
                });
            }
            let own: BTreeMap<(String, String), Action> = self
                .index
                .offered(ExtensionId(id))
                .into_iter()
                .map(|(kind, action)| ((kind, action.id.clone()), action))
                .collect();
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
