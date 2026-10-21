//! The config: taking a new one in and reading the Daemon's settings from it.
//!
//! # Owns
//!
//! - Applying a reloaded config, or keeping the last good one when it fails.
//! - Reading the engine, History, keyword and app settings, and passing each Extension its table.
//! - The problems the user sees.
//!
//! # Not here
//!
//! - Loading and watching the config files: `crate::config`.
//! - What each setting means: `crate::engine`, `crate::keywords`, `crate::apps`.

use std::collections::BTreeMap;
use std::time::Instant;

use kotodama_protocol::Problem;
use kotodama_protocol::Table;
use kotodama_protocol::ToExtension;
use kotodama_protocol::ToFrontEnd;

use super::Daemon;
use super::Desktop;
use super::Extension;
use super::ExtensionId;
use super::FrontEndId;
use super::Launcher;
use super::OWN;
use super::Own;
use super::OwnAction;
use super::Source;
use super::State;
use super::clear_history;
use super::read_query;
use crate::apps::Apps;
use crate::config;
use crate::config::extension_settings;
use crate::configured;
use crate::conflict;
use crate::conflict::Conflict;
use crate::empty_query::EmptyQuery;
use crate::engine;
use crate::engine::Engine;
use crate::history::Settings;
use crate::history::Store;
use crate::keywords;
use crate::kind_order::KindOrder;
use crate::paths::Overrides;
use crate::problem;
use crate::problem::Report;

impl<L: Launcher, D: Desktop, S: Store> Daemon<L, D, S> {
    pub(super) fn set_config(&mut self, config: Table, now: Instant) {
        self.config = config;
        self.problems.dirty |= self.problems.unreadable.take().is_some();
        let mut report = Report::new();
        engine::check_tables(&self.config, &mut report);
        self.paths = Overrides::from_config(&self.config, &mut report);
        self.load_engine(&mut report);
        self.load_keywords(&mut report);
        self.load_settings(&mut report);
        let empty_query = EmptyQuery::from_config(&self.config, &mut report);
        if empty_query != self.empty_query {
            self.empty_query = empty_query;
            self.mark_late();
        }
        let kinds = KindOrder::from_config(&self.config, &mut report);
        if kinds != self.kinds {
            self.kinds = kinds;
            self.mark_late();
        }
        self.problems.dirty |= problem::replace(&mut self.problems.config, report.into_problems());
        self.configure_extensions(now);
        self.load_apps();
        for ext in &mut self.extensions {
            if ext.sender().is_none() {
                continue;
            }
            let table = extension_settings(&self.config, ext.name(), ext.found.inline);
            if ext.config.as_ref() != Some(&table) {
                ext.send(ToExtension::Config(table.clone()));
                ext.config = Some(table);
            }
        }
    }

    /// The config can't be loaded, as `err` says: the last good one stays,
    /// and the user hears why until it loads again.
    pub(super) fn config_failed(&mut self, err: &config::Error) {
        let problem = Problem {
            about: err.path.display().to_string(),
            message: format!("{}; keeping the last good config", err.reason),
        };
        log::warn!("{}: {}", problem.about, problem.message);
        self.problems.unreadable = Some(problem);
        self.problems.dirty = true;
    }

    /// Reads the engine's settings from the config. The desktop hears of its
    /// own; the rest is read where it is used.
    pub(super) fn load_engine(&mut self, report: &mut Report) {
        let engine = Engine::from_config(&self.config, report);
        if engine.desktop != self.engine.desktop {
            self.desktop.configure(engine.desktop);
        }
        if (engine.near_misses, engine.fallbacks) != (self.engine.near_misses, self.engine.fallbacks) {
            self.mark_late();
        }
        self.engine = engine;
    }

    /// Brings the running Extensions in line with those installed, as the
    /// config switches them off, changes them or adds to them.
    pub(super) fn configure_extensions(&mut self, now: Instant) {
        let mut report = Report::new();
        let found = configured::configure(&self.installed, &self.config, &mut report);
        self.problems.dirty |= problem::replace(&mut self.problems.extensions, report.into_problems());
        self.set_extensions(found, now);
    }

    /// Reads the History's settings from the config. Open sessions are ranked
    /// again if one that ranks changed.
    pub(super) fn load_settings(&mut self, report: &mut Report) {
        let settings = Settings::from_config(&self.config, report);
        let old = *self.history.settings();
        if settings != old {
            self.history.set_settings(settings);
            let unranked = Settings {
                usage_log: old.usage_log,
                undo_secs: old.undo_secs,
                max_results: old.max_results,
                max_queries: old.max_queries,
                ..settings
            };
            if unranked != old {
                self.mark_late();
            }
        }
    }

    /// Reads the Aliases and Quicklinks from the config, and rebuilds the
    /// Daemon's own Items from them.
    pub(super) fn load_keywords(&mut self, report: &mut Report) {
        let keywords = keywords::from_config(&self.config, report);
        if keywords == self.keywords && !self.own.is_empty() {
            return;
        }
        self.keywords = keywords;
        let aliases = &self.keywords.aliases;
        self.last_seen.retain(|(extension, id), _| {
            aliases
                .values()
                .any(|alias| alias.extension == *extension && alias.item == *id)
        });
        self.repin();
        self.build_own();
    }

    /// Reads each installed Extension's app from its Manifest and the
    /// config. Reports the keywords lost, once each time they change. When they changed, the Index
    /// reads the Segments the search shows, every session's query is read again, and the
    /// entry Items are rebuilt.
    pub(super) fn load_apps(&mut self) {
        let installed = self
            .extensions
            .iter()
            .enumerate()
            .filter(|(_, ext)| !matches!(ext.state, State::Removed))
            .map(|(n, ext)| (ExtensionId(n), &ext.found.manifest));
        let mut report = Report::new();
        let apps = Apps::new(installed, &self.config, &self.keywords, &mut report);
        self.problems.dirty |= problem::replace(&mut self.problems.apps, report.into_problems());
        let conflicts: Vec<Conflict> = (self.keywords.conflicts().into_iter())
            .chain(apps.conflicts().iter().cloned())
            .collect();
        if conflicts != self.conflicts {
            let mut report = Report::new();
            conflict::report(&conflicts, &mut report);
            self.problems.dirty |= problem::replace(&mut self.problems.conflicts, report.into_problems());
            self.conflicts = conflicts;
        }
        if apps == self.apps {
            return;
        }
        // The same app may sit at another place in the new order.
        let entered: Vec<(FrontEndId, Option<usize>)> = self
            .sessions
            .iter()
            .map(|(&id, session)| {
                let extension = session.entered.and_then(|n| self.apps.get(n));
                (id, extension.and_then(|app| apps.of(app.extension)))
            })
            .collect();
        self.apps = apps;
        self.index.set_searched(self.apps.searched(self.extensions.len()));
        for (id, entered) in entered {
            if let Some(session) = self.sessions.get_mut(&id) {
                session.entered = entered;
                read_query(&self.apps, &mut self.matcher, session);
            }
        }
        self.build_own();
    }

    /// Builds the Daemon's own Items from the Quicklinks and the apps.
    pub(super) fn build_own(&mut self) {
        let quicklinks = self.keywords.quicklinks.iter().enumerate();
        let extensions = &self.extensions;
        let entries = (self.apps.iter().enumerate())
            .filter(|(_, app)| extensions[app.extension.0].usable())
            .map(|(n, app)| {
                let name = extensions[app.extension.0].name();
                Own::new(app.entry(name), OwnAction::App(n))
            });
        let problems = (!self.problems.sent.is_empty())
            .then(|| Own::new(problem::item(self.problems.sent.len()), OwnAction::Problems));
        self.own = quicklinks
            .map(|(n, quicklink)| Own::new(quicklink.item(), OwnAction::Quicklink(n)))
            .chain(entries)
            .chain([Own::new(clear_history(), OwnAction::ClearHistory)])
            .chain(problems)
            .collect();
        self.load_undo_item();
        // The own Items' positions changed, so rows showing them can't be
        // kept where they are.
        for session in self.sessions.values_mut() {
            session.shown.retain(|(source, _)| !matches!(source, Source::Own(_)));
        }
        self.mark_late();
    }

    /// Puts the list of problems together again, and if it changed, updates
    /// the lists open and the own Item that opens it, and has
    /// [`flush`](Self::flush) send it to every Front end.
    pub(super) fn update_problems(&mut self) {
        self.problems.dirty = false;
        let list = self
            .problems
            .list(self.extensions.iter().filter_map(Extension::problem));
        if list == self.problems.sent {
            return;
        }
        let count = self.problems.sent.len();
        self.problems.sent = list;
        self.problems.unsent = true;
        for id in &self.problems_open {
            if let Some(session) = self.sessions.get(id) {
                session.send(ToFrontEnd::UpdateView {
                    extension: OWN.to_owned(),
                    view: problem::view(&self.problems.sent),
                    contributed: BTreeMap::new(),
                });
            }
        }
        if self.problems.sent.len() != count {
            self.build_own();
        }
    }
}
