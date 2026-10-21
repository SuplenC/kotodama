//! The search: ranking a session's query against the index, the Live query
//! Results, the Daemon's own Results and what the History learned.
//!
//! # Owns
//!
//! - Sending each session's Results when they change, and the Live queries that are due.
//! - Adding the candidates of each source, the fallbacks and the near misses.
//! - What the History adds to a candidate: frecency, named picks and learned queries.
//!
//! # Not here
//!
//! - The empty query: `empty.rs`.
//! - Turning a candidate into a row: `rows.rs`.
//! - Matching and scoring text: `crate::rank`.

use std::time::Instant;

use kotodama_protocol::Item;
use kotodama_protocol::Scope;
use kotodama_protocol::ToExtension;
use kotodama_protocol::ToFrontEnd;

use super::Candidate;
use super::Class;
use super::Daemon;
use super::Desktop;
use super::ExtensionId;
use super::FrontEndId;
use super::HOT_SETTLE;
use super::Keyword;
use super::Launcher;
use super::Live;
use super::MAX_RESULTS;
use super::OWN;
use super::Session;
use super::Source;
use super::Standing;
use super::Update;
use super::alias_id;
use super::take_answers;
use crate::clock::When;
use crate::history;
use crate::history::IDENTITIES;
use crate::history::Store;
use crate::index::Reach;
use crate::most_used::Counts;
use crate::rank;
use crate::rank::Haystack;
use crate::rank::Matched;
use crate::rank::Tier;

impl<L: Launcher, D: Desktop, S: Store> Daemon<L, D, S> {
    /// Sends what the last event changed: due Live queries, Results, the Action
    /// catalog and the status.
    pub(super) fn flush(&mut self, now: Instant) {
        self.send_due_live_queries(now);
        // First, as the own Item that opens the list may change the Results.
        if self.problems.dirty {
            self.update_problems();
        }

        let ids: Vec<FrontEndId> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.update != Update::None)
            .map(|(&id, _)| id)
            .collect();
        if !ids.is_empty() {
            for id in ids {
                self.refresh(id, now);
            }
            // A refresh asks fallbacks what real matches left rows empty for.
            self.send_due_live_queries(now);
        }

        let versions = self.versions();
        if versions != self.hot_versions {
            self.hot_versions = versions;
            self.hot_due = self.hot_due.min(now + HOT_SETTLE);
        }
        if self.hot_due <= now {
            self.plan_hot(now);
        }
        self.index.trim();
        // Once the menu has what it shows, so its first keystroke finds what
        // it reads in the CPU's cache.
        if std::mem::take(&mut self.warm) {
            std::hint::black_box(self.index.warm());
            self.warmed += 1;
        }

        if self.catalog_dirty {
            self.catalog_dirty = false;
            if self.extensions.iter().any(|ext| ext.catalog) {
                let catalog = self.compute_catalog();
                if self.catalog.as_ref() != Some(&catalog) {
                    for ext in self.extensions.iter().filter(|ext| ext.catalog) {
                        ext.send(ToExtension::ActionCatalog {
                            entries: catalog.clone(),
                        });
                    }
                    self.catalog = Some(catalog);
                }
            } else {
                self.catalog = None;
            }
        }

        let hung: Vec<FrontEndId> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.hung.get())
            .map(|(&id, _)| id)
            .collect();
        for id in hung {
            log::warn!("front end {} stopped reading; disconnecting it", id.0);
            if let Some(disconnect) = self.sessions.get_mut(&id).and_then(|session| session.disconnect.take()) {
                disconnect();
            }
            self.front_end_down(id);
        }

        for (id, ext) in self.extensions.iter_mut().enumerate() {
            if ext.hung.get() && !ext.stopping {
                log::warn!("{} stopped answering; stopping it", ext.name());
                ext.stopping = true;
                self.launcher.stop(ExtensionId(id), ext.generation);
            }
        }

        let down: Vec<String> = self
            .extensions
            .iter()
            .filter(|ext| ext.is_down())
            .map(|ext| ext.name().to_owned())
            .collect();
        let problems = std::mem::take(&mut self.problems.unsent);
        if down != self.down || problems {
            self.down = down;
            self.broadcast(|| ToFrontEnd::Status {
                down: self.down.clone(),
                problems: self.problems.sent.clone(),
            });
        }
    }

    pub(super) fn send_due_live_queries(&mut self, now: Instant) {
        for (&front_end, session) in &mut self.sessions {
            for (ext_id, live) in session.live.iter_mut().enumerate() {
                if live.due.is_none_or(|due| due > now) {
                    continue;
                }
                live.due = None;
                let id = self.next_id;
                self.next_id += 1;
                live.sent = Some(id);
                self.live_queries.insert(id, (front_end, ExtensionId(ext_id)));
                self.extensions[ext_id].send(ToExtension::LiveQuery {
                    id,
                    query: live.query.clone(),
                    prefix: live.prefix.clone(),
                });
            }
        }
    }

    /// Ranks one session's Results and sends them. Late Results rank with the
    /// rest while [`Session::reranks`]. After that the rows on
    /// screen stay where they are, and everything new goes below them, except
    /// an answer, which takes the top row.
    pub(super) fn refresh(&mut self, id: FrontEndId, now: Instant) {
        let Some(mut session) = self.sessions.remove(&id) else {
            return;
        };
        let fresh = session.update == Update::Fresh;
        let keep = !fresh && !session.reranks(now);
        session.update = Update::None;
        let typed = !session.searched().trim().is_empty();
        let app = session.scope.and_then(|n| self.apps.get(n));
        let app_scope = app.map(|app| app.scope(typed, self.extensions[app.extension.0].note()));

        if session.query.is_empty() || (app_scope.is_some() && !typed) {
            self.send_empty(&mut session, keep, fresh, app_scope, now);
            self.sessions.insert(id, session);
            return;
        }

        let mut candidates = Vec::new();
        self.scored.clear();
        let version = self.index.version();
        let within = session
            .last_search
            .within(&session.query[session.rest..], &session.folded, version);
        let narrow = within.is_some();
        self.kind_places.update(&self.kinds, self.index.kinds());
        let Daemon {
            index,
            matcher,
            tiers,
            kind_places,
            scored,
            ..
        } = self;
        index.search(
            &session.pattern,
            matcher,
            session.folded.filter(),
            session.reach,
            within,
            |slot, ranked, score, matched| {
                scored.push(slot);
                if let Some(tier) = tiers.texts(ranked.texts(), &session.folded, matched) {
                    let kind = kind_places.place(slot.segment, ranked.kind());
                    let source = Source::Indexed(slot);
                    candidates.push(Candidate::new(source, tier, kind, score, ranked.title_len()));
                }
            },
        );
        session.last_search.found(
            &session.query[session.rest..],
            &session.folded,
            version,
            self.scored.iter().copied(),
        );
        // Up to here the candidates are in slot order.
        let indexed = candidates.len();
        let fallbacks = self.add_live(&session, &mut candidates);
        self.add_own(&session, &mut candidates);
        let indexed = self.add_near_misses(&mut session, &mut candidates, indexed, narrow, fresh);
        let primary = self.learn(&mut session, &mut candidates, indexed, now);
        self.add_fallbacks(&mut session, &mut candidates, fallbacks, now);

        let answers = take_answers(&mut candidates);

        let mut kept: Vec<Candidate> = Vec::new();
        if keep {
            let visible = session.shown.iter().take(session.rows);
            // A fallback on screen moves down for a real match come late: it
            // never ranks above one.
            for (source, _) in visible.filter(|(_, answer)| !answer) {
                if let Some(at) = candidates
                    .iter()
                    .position(|c| c.source == *source && c.standing != Standing::Fallback)
                {
                    kept.push(candidates.swap_remove(at));
                }
            }
        }

        let room = MAX_RESULTS.saturating_sub(answers.len() + kept.len());
        if candidates.len() > room && room > 0 {
            candidates.select_nth_unstable_by(room - 1, Candidate::rank);
        }
        candidates.truncate(room);
        candidates.sort_unstable_by(Candidate::rank);
        self.lead_with_direct_picks(&session, &mut candidates);

        let ranked: Vec<Candidate> = answers.into_iter().chain(kept).chain(candidates).collect();
        let results = ranked
            .iter()
            .filter_map(|candidate| {
                let primary = primary
                    .as_ref()
                    .filter(|(source, _)| *source == candidate.source)
                    .map(|(_, action)| action.as_str());
                self.row(&session, candidate.source, primary)
            })
            .collect();
        let shown = ranked
            .iter()
            .map(|candidate| (candidate.source, candidate.standing == Standing::Answer))
            .collect();
        session.send_results(shown, results, app_scope, fresh);
        self.sessions.insert(id, session);
    }

    /// Sends what shows before anything is typed: the Empty query's Results,
    /// or with `app_scope` those of an app. With `keep`, the rows on screen
    /// stay put, as for any query.
    pub(super) fn send_empty(
        &mut self,
        session: &mut Session,
        keep: bool,
        fresh: bool,
        app_scope: Option<Scope>,
        now: Instant,
    ) {
        session.last_search.forget();
        let when = self.when(now);
        let mut ranked = match session.reach {
            Reach::Root => {
                session.empty_ranked_in = Some(history::part_of_day(when));
                self.empty_query(session, when)
            }
            Reach::App(_) => match session.scope {
                Some(n) => self.empty_app(n, when),
                None => Vec::new(),
            },
        };
        if keep {
            let visible: Vec<Source> = session
                .shown
                .iter()
                .take(session.rows)
                .map(|&(source, _)| source)
                .filter(|source| ranked.contains(source))
                .collect();
            ranked.retain(|source| !visible.contains(source));
            ranked.splice(0..0, visible);
        }
        session.via_alias = None;
        let results = ranked
            .iter()
            .filter_map(|&source| self.row(session, source, None))
            .collect();
        let shown = ranked.into_iter().map(|source| (source, false)).collect();
        session.send_results(shown, results, app_scope, fresh);
    }

    /// Adds the session's Live results to `candidates`, and returns those of
    /// fallback Extensions, which rank apart. Each is matched against what its
    /// Extension was asked, without its routing prefix; the Extension's own
    /// idea of what matches stands, at worst as letters scattered.
    pub(super) fn add_live(&mut self, session: &Session, candidates: &mut Vec<Candidate>) -> Vec<Candidate> {
        let mut fallbacks = Vec::new();
        for (ext, live) in session.live.iter().enumerate() {
            if live.results.is_empty() {
                continue;
            }
            let fallback = self.extensions[ext].fallback();
            let asked = rank::Query::new(&live.query);
            for (n, result) in live.results.iter().enumerate() {
                let haystack = Haystack::new(&result.item);
                let tier = self
                    .tiers
                    .item(&haystack, &result.item, &asked, Matched::ALL)
                    .unwrap_or(Tier::Scattered);
                let score = (result.score.get() * session.max_score as f32).round() as u32;
                let len = result.item.title.chars().count() as u32;
                let kind = self.kinds.place(&result.item.kind);
                let mut candidate = Candidate::new(Source::Live(ExtensionId(ext), n as u32), tier, kind, score, len);
                candidate.standing = if result.answer {
                    Standing::Answer
                } else if fallback {
                    Standing::Fallback
                } else {
                    Standing::Match
                };
                if candidate.standing == Standing::Fallback {
                    fallbacks.push(candidate);
                } else {
                    candidates.push(candidate);
                }
            }
        }
        fallbacks
    }

    /// Adds the Daemon's own Items that match the session's query to
    /// `candidates`. In an app, none: it shows its Extension's Items alone.
    pub(super) fn add_own(&mut self, session: &Session, candidates: &mut Vec<Candidate>) {
        let own = match session.reach {
            Reach::Root => &self.own[..],
            Reach::App(_) => &[],
        };
        for (n, own) in own.iter().enumerate() {
            let filter = session.folded.filter();
            if let Some((score, matched)) = own.haystack.score(&session.pattern, &mut self.matcher, filter)
                && let Some(tier) = self.tiers.item(&own.haystack, &own.item, &session.folded, matched)
            {
                let len = own.haystack.title.len() as u32;
                let kind = self.kinds.place(&own.item.kind);
                candidates.push(Candidate::new(
                    Source::Own(n as u32),
                    tier.min(own.tier_cap()),
                    kind,
                    score,
                    len,
                ));
            }
        }
    }

    /// Adds the fallback Extensions' results to `candidates`, after what
    /// the Daemon learned is applied to the real matches there, if those
    /// leave rows on screen empty, and asks the query of every fallback not
    /// asked it yet. With the rows full, its results would sit below them
    /// unseen. Frecency orders them among themselves, but takes no other part:
    /// a fallback is never a Learned choice, nor lifted a Tier.
    pub(super) fn add_fallbacks(
        &self,
        session: &mut Session,
        candidates: &mut Vec<Candidate>,
        mut fallbacks: Vec<Candidate>,
        now: Instant,
    ) {
        if candidates.len() >= session.rows {
            return;
        }
        let searched = self.index.searched();
        for (n, ext) in self.extensions.iter().enumerate() {
            let live = &session.live[n];
            if !self.engine.fallbacks || live.asked || !ext.fallback() || ext.sender().is_none() {
                continue;
            }
            if !session.asks(ExtensionId(n), searched) {
                continue;
            }
            if let Some(debounce) = ext.debounce() {
                let query = session.searched().to_owned();
                session.live[n] = Live::asking(query, None, now + debounce);
            }
        }
        if !self.history.is_empty() {
            let when = self.when(now);
            for fallback in &mut fallbacks {
                if let Some((extension, item)) = self.item(session, fallback.source) {
                    (fallback.frecency, fallback.direct) = self.frecency(extension, &item, when);
                }
            }
        }
        candidates.append(&mut fallbacks);
    }

    /// Adds the near misses of the session's query, Indexed items and the
    /// Daemon's own, to `candidates`, unless the real matches already fill the
    /// rows: near misses rank below every real match, so they would never be
    /// seen, except those on screen when the query hasn't changed.
    /// `narrowed` is whether this search narrowed the last one's matches;
    /// then near misses are looked for among the last one's, if it looked for
    /// them and allowed as many typos. The candidates before `indexed` are the
    /// Indexed items, in slot order, and stay so; returns how many there are
    /// now.
    pub(super) fn add_near_misses(
        &mut self,
        session: &mut Session,
        candidates: &mut Vec<Candidate>,
        indexed: usize,
        narrowed: bool,
        fresh: bool,
    ) -> usize {
        let last = session.last_search.nearly_for.take();
        let narrow = narrowed && last.is_some_and(|last| last.narrows_near_misses_of(&session.folded));
        let typos = session.folded.tolerates_typos();
        let look = self.engine.near_misses && typos && candidates.len() < session.rows;
        let mut nearly = std::mem::take(&mut session.last_search.nearly);

        let Daemon {
            index,
            own,
            tiers,
            kinds,
            kind_places,
            ..
        } = self;
        let folded = &session.folded;
        let known = &candidates[..];
        // The near miss `source` is, unless it is a candidate already.
        let mut near_miss = |source: Source| {
            let (texts, len, kind) = match source {
                Source::Indexed(slot) => {
                    if known[..indexed].binary_search_by_key(&source, |c| c.source).is_ok() {
                        return None;
                    }
                    let ranked = index.ranked(slot)?;
                    // Most rows fail on their letters: their texts aren't
                    // looked up.
                    let (letters, initials) = ranked.letters();
                    if !folded.may_nearly_match(letters, initials) {
                        return None;
                    }
                    let kind = kind_places.place(slot.segment, ranked.kind());
                    (ranked.texts(), ranked.title_len(), kind)
                }
                Source::Own(n) => {
                    if known[indexed..].iter().any(|c| c.source == source) {
                        return None;
                    }
                    let own = own.get(n as usize)?;
                    let len = own.haystack.title.len() as u32;
                    (own.haystack.texts(), len, kinds.place(&own.item.kind))
                }
                Source::Live(..) | Source::Unavailable => return None,
            };
            tiers
                .near_miss_texts(texts, folded)
                .then(|| Candidate::new(source, Tier::NearMiss, kind, 0, len))
        };
        let mut found = Vec::new();
        if look {
            let (within, all) = if narrow {
                (&nearly[..], None)
            } else {
                let initials = folded.typo_initials();
                (&[][..], Some(index.starting_with(initials, session.reach)))
            };
            let slots = within.iter().copied().chain(all.into_iter().flatten());
            let own_reached = match session.reach {
                Reach::Root => own.len() as u32,
                Reach::App(_) => 0,
            };
            let own = (0..own_reached).map(Source::Own);
            found.extend(slots.map(Source::Indexed).chain(own).filter_map(&mut near_miss));
        } else if typos && !fresh {
            let shown = session.shown.iter().take(session.rows);
            found.extend(shown.filter_map(|&(source, _)| near_miss(source)));
        }

        nearly.clear();
        if look {
            nearly.extend(session.last_search.narrowing.matched());
            nearly.extend(found.iter().filter_map(|candidate| match candidate.source {
                Source::Indexed(slot) => Some(slot),
                _ => None,
            }));
            nearly.sort_unstable();
            // A slot can be both a match and a near miss of another word.
            nearly.dedup();
            session.last_search.nearly_for = Some(session.folded.clone());
        }
        session.last_search.nearly = nearly;
        // The Indexed items first, in slot order, then the Daemon's own.
        found.sort_unstable_by_key(|candidate| candidate.source);
        let own = found.partition_point(|candidate| matches!(candidate.source, Source::Indexed(_)));
        candidates.extend(found.drain(own..));
        let added = found.len();
        candidates.splice(indexed..indexed, found);
        let indexed = indexed + added;
        candidates[..indexed].sort_unstable_by_key(|candidate| candidate.source);
        indexed
    }

    /// Applies what the Daemon learned from the user, and the user's
    /// keywords, to the candidates. Frecency breaks ties within a tier and,
    /// from [`Settings::lift_threshold`], lifts a Result a tier. What the query names by an
    /// Alias goes first, with exact matches the Learned choice wasn't picked
    /// over, then the Learned choice. The candidates before
    /// `indexed` must be the Indexed items, in slot order. Returns the Action
    /// an Alias makes primary, with the Result it is for.
    pub(super) fn learn(
        &mut self,
        session: &mut Session,
        candidates: &mut Vec<Candidate>,
        indexed: usize,
        now: Instant,
    ) -> Option<(Source, String)> {
        session.via_alias = None;
        if session.key.is_empty() {
            return None;
        }
        let when = self.when(now);
        if !self.history.is_empty() {
            self.add_frecency(session, candidates, indexed, when);
        }
        let primary = self.add_named(session, candidates, when);
        self.add_learned(session, candidates, when.at);
        primary
    }

    /// Gives each candidate its Frecency, and lifts a Tier those used enough,
    /// from [`Settings::lift_threshold`]. The candidates before `indexed`
    /// must be the Indexed items, in slot order.
    pub(super) fn add_frecency(&mut self, session: &Session, candidates: &mut [Candidate], indexed: usize, when: When) {
        let settings = *self.history.settings();
        self.cache_frecent();
        let mut frecencies = std::mem::take(&mut self.frecencies);
        self.frecent.picks.frecency(when, settings.time_of_day, &mut frecencies);
        for (&(slot, counts), &frecency) in self.frecent.slots.iter().zip(&frecencies) {
            if let Ok(n) = candidates[..indexed].binary_search_by_key(&Source::Indexed(slot), |c| c.source) {
                if counts != Counts::Direct {
                    candidates[n].frecency = frecency;
                }
                if counts != Counts::Shared {
                    candidates[n].direct = frecency;
                }
            }
        }
        self.frecencies = frecencies;
        for candidate in &mut candidates[indexed..] {
            if let Some((extension, item)) = self.item(session, candidate.source) {
                (candidate.frecency, candidate.direct) = self.frecency(extension, &item, when);
            }
        }
        for candidate in candidates.iter_mut() {
            if candidate.frecency >= settings.lift_threshold {
                candidate.tier = rank::lift(candidate.tier);
            }
        }
    }

    /// Ranks what the query names. An Alias, typed alone or before an
    /// Argument, goes first. A Quicklink's keyword or an Item's own keyword
    /// counts as an exact match, which a Learned choice can beat. An app's
    /// word counts as a word start, so an application named like the app
    /// ranks above its entry Item. In an app, only its Extension's Items'
    /// keywords count, typed first past its word. Returns the Action an Alias
    /// makes primary, with the Result it is for.
    pub(super) fn add_named(
        &self,
        session: &mut Session,
        candidates: &mut Vec<Candidate>,
        when: When,
    ) -> Option<(Source, String)> {
        let root = session.reach == Reach::Root;
        let mut words = session.key.split(' ');
        let first = if root { words.next() } else { words.nth(1) };
        let first = first.unwrap_or_default();
        let mut named: Vec<Keyword> = Vec::new();
        let mut primary = None;
        let alias = self
            .keywords
            .aliases
            .get_key_value(session.key.as_str())
            .or_else(|| self.keywords.aliases.get_key_value(first))
            .filter(|_| root);
        if let Some((keyword, alias)) = alias
            && let Some(source) = self.alias_source(session, alias)
        {
            named.push(Keyword {
                source,
                alias: true,
                frecency: Some(self.history.frecency(OWN, &alias_id(keyword), when)),
                tier: Tier::Exact,
            });
            if source != Source::Unavailable {
                primary = alias.action.clone().map(|action| (source, action));
            }
            session.via_alias = Some((source, keyword.clone()));
        }
        if root
            && let Some(n) = self
                .own
                .iter()
                .position(|own| own.item.keyword.as_deref() == Some(first))
        {
            named.push(Keyword {
                source: Source::Own(n as u32),
                alias: false,
                frecency: None,
                tier: self.own[n].tier_cap(),
            });
        }
        for slot in self.index.keyed(first, session.reach) {
            named.push(Keyword {
                source: Source::Indexed(slot),
                alias: false,
                frecency: None,
                tier: Tier::Exact,
            });
        }
        for Keyword {
            source,
            alias,
            frecency,
            tier,
        } in named
        {
            let at_n = match candidates.iter().position(|c| c.source == source) {
                Some(n) => n,
                None if source == Source::Unavailable => {
                    candidates.push(Candidate::new(source, Tier::Exact, 0, 0, 0));
                    candidates.len() - 1
                }
                None => {
                    let Some((extension, item)) = self.item(session, source) else {
                        continue;
                    };
                    let mut candidate = Candidate::new(
                        source,
                        tier,
                        self.kinds.place(&item.kind),
                        session.max_score,
                        item.title.chars().count() as u32,
                    );
                    (candidate.frecency, candidate.direct) = self.frecency(extension, &item, when);
                    candidates.push(candidate);
                    candidates.len() - 1
                }
            };
            let candidate = &mut candidates[at_n];
            if alias {
                candidate.class = Class::Named;
                candidate.by_alias = true;
            }
            candidate.tier = candidate.tier.max(tier.coarse());
            candidate.fine = candidate.fine.max(tier);
            if let Some(frecency) = frecency {
                candidate.frecency = frecency;
            }
        }
        primary
    }

    /// Ranks the strongest Learned choice that applies and is among the
    /// candidates (every Result of it, when several Extensions offer it) with
    /// the exact matches, and the exact matches first, unless the Learned
    /// choice was picked over them.
    pub(super) fn add_learned(&self, session: &Session, candidates: &mut [Candidate], at: u64) {
        let choices = self.history.learned(&session.key, at);
        let mut passed_over = None;
        for choice in &choices {
            for source in self.sources_of(session, choice.extension, choice.id) {
                if let Some(candidate) = candidates.iter_mut().find(|c| c.source == source) {
                    if candidate.class == Class::Rest {
                        // One that matches exactly, or is used enough to rank
                        // as if it did, goes with the exact matches.
                        candidate.class = if candidate.tier == Tier::Exact.coarse() {
                            Class::Named
                        } else {
                            Class::Learned
                        };
                    }
                    passed_over = Some(choice.passed_over);
                }
            }
            if passed_over.is_some() {
                break;
            }
        }
        // Exact matches go first, unless the Learned choice was picked over
        // them. With a Learned choice, only a true exact match does: one that
        // Frecency lifted to the tier doesn't.
        for candidate in candidates.iter_mut() {
            if candidate.class != Class::Rest || candidate.tier != Tier::Exact.coarse() {
                continue;
            }
            let named = match passed_over {
                None => true,
                Some(passed_over) => {
                    candidate.fine.coarse() == Tier::Exact.coarse()
                        && !self
                            .history_key(session, candidate.source)
                            .is_some_and(|(extension, id)| passed_over.iter().any(|(e, i)| e == extension && i == id))
                }
            };
            if named {
                candidate.class = Class::Named;
            }
        }
    }

    /// Reorders the Results of each Identity among the places they ranked
    /// in, so the one the user picked through most takes the best of them:
    /// the thing ranks by its best Result, and leads with the user's way to
    /// it. Everything else stays put.
    pub(super) fn lead_with_direct_picks(&self, session: &Session, ranked: &mut [Candidate]) {
        if !ranked.iter().any(|candidate| candidate.direct > 0.0) {
            return;
        }
        let identity = |candidate: &Candidate| self.identity(session, candidate.source);
        let mut done: Vec<&str> = Vec::new();
        for first in 0..ranked.len() {
            let Some(id) = identity(&ranked[first]) else {
                continue;
            };
            if done.contains(&id) {
                continue;
            }
            done.push(id);
            let places: Vec<usize> = (first..ranked.len())
                .filter(|&n| identity(&ranked[n]) == Some(id))
                .collect();
            if places.len() < 2 {
                continue;
            }
            let mut group: Vec<Candidate> = places.iter().map(|&n| ranked[n]).collect();
            // Stable, so equal direct picks keep their ranked order.
            group.sort_by(|a, b| b.direct.total_cmp(&a.direct));
            for (&n, candidate) in places.iter().zip(group) {
                ranked[n] = candidate;
            }
        }
    }

    /// A Result's Frecency, shared with every Result of its Identity, and
    /// that of picks of the Result itself.
    pub(super) fn frecency(&self, extension: &str, item: &Item, when: When) -> (f64, f64) {
        let own = self.history.frecency(extension, &item.id, when);
        match &item.identity {
            Some(identity) => (self.history.frecency(IDENTITIES, identity, when), own),
            None => (own, own),
        }
    }
}
