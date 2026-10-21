//! The Extension's settings, from its `[extensions.apps]` config table, and the
//! Items the apps become under them.
//!
//! ```toml
//! [extensions.apps]
//! # What starts apps with `Terminal=true`; the app's command follows it.
//! terminal = ["ghostty", "-e"]
//! # Apps to leave out, by desktop id, with or without `.desktop`.
//! hidden = ["qt5ct", "qt6ct"]
//! # Names to show instead of the app's own, by desktop id.
//! names = { nvim = "Neovim" }
//! # An app that closes within this many seconds of starting was picked by
//! # mistake: its pick is withdrawn. 0 never withdraws one; at most 30.
//! withdraw_within = 5
//! ```
//!
//! # Owns
//!
//! - The config table and its defaults.
//! - The Item an app becomes: its title, keywords, identity and Actions.
//! - Which command each Action runs.
//!
//! # Not here
//!
//! - Finding the apps: `scan.rs`.
//! - Starting the command, and withdrawing the pick: `main.rs`.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;

use kotodama_extension::config;
use kotodama_protocol::Action;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::Table;
use kotodama_protocol::Value;

use crate::scan::App;
use crate::scan::app_id;

/// The primary Action, which starts the app.
pub const OPEN: &str = "open";
/// What a desktop action's Action id starts with; its own id follows.
pub const DESKTOP_ACTION: &str = "desktop-action:";

/// The terminal used when the config names none: the freedesktop proposal's
/// launcher, which starts the user's preferred terminal.
const DEFAULT_TERMINAL: &str = "xdg-terminal-exec";

/// How soon after starting an app may exit for its pick to be withdrawn, when
/// the config says nothing: long enough to see the wrong app open and close
/// it, short enough that an app closed after real use still counts.
pub const DEFAULT_WITHDRAW_WITHIN: Duration = Duration::from_secs(5);

/// The longest `withdraw_within` the config may set, in seconds. The Daemon
/// withdraws a pick only for a minute after the run, and the app's time
/// starts once it has its scope, so this leaves room for a slow start.
const MAX_WITHDRAW_WITHIN: f64 = 30.0;

/// The settings the config table sets.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// What runs an app with `Terminal=true`, the app's command after it.
    pub terminal: Vec<String>,
    /// App ids, without `.desktop`.
    pub hidden: HashSet<String>,
    /// Names by app id, without `.desktop`.
    pub names: HashMap<String, String>,
    /// An app whose launch closes this soon after it starts, with no copy of
    /// it running from before, has its pick withdrawn. Zero never withdraws
    /// one.
    pub withdraw_within: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            terminal: vec![DEFAULT_TERMINAL.to_owned()],
            hidden: HashSet::new(),
            names: HashMap::new(),
            withdraw_within: DEFAULT_WITHDRAW_WITHIN,
        }
    }
}

impl Settings {
    /// Reads the config table. A setting of the wrong shape is logged and left
    /// at its default; the others still apply.
    #[must_use]
    pub fn from_config(config: &Table) -> Self {
        let mut settings = Settings::default();
        if let Some(terminal) = config::command(config, "terminal") {
            settings.terminal = terminal;
        }
        if let Some(value) = config.get("hidden") {
            match config::strings(value) {
                Some(hidden) => settings.hidden = hidden.iter().map(|id| app_id(id)).collect(),
                None => config::warn("`hidden` should be a list of desktop ids"),
            }
        }
        match config.get("names") {
            Some(Value::Table(names)) => {
                for (id, name) in names {
                    match name {
                        Value::String(name) => {
                            settings.names.insert(app_id(id), name.clone());
                        }
                        _ => config::warn(format_args!("`names.{id}` should be a string")),
                    }
                }
            }
            Some(_) => config::warn("`names` should be a table of names by desktop id"),
            None => {}
        }
        if let Some(value) = config.get("withdraw_within") {
            match seconds(value) {
                Some(within) => settings.withdraw_within = within,
                None => config::warn("`withdraw_within` should be a number of seconds from 0 to 30"),
            }
        }
        settings
    }

    /// The Items for `apps`, without the hidden ones.
    pub fn items<'a>(&'a self, apps: &'a BTreeMap<String, App>) -> impl Iterator<Item = Item> + 'a {
        apps.values()
            .filter(|app| !self.hidden.contains(&app.id))
            .map(|app| self.item(app))
    }

    /// The Item for `app`, under its configured name if it has one.
    fn item(&self, app: &App) -> Item {
        let renamed = self.names.get(&app.id);
        let title = renamed.unwrap_or(&app.name).clone();
        // Found by its generic name, by its own name when renamed, and by its
        // desktop id, besides its keywords.
        let mut keywords = app.keywords.clone();
        keywords.extend(app.generic_name.iter().cloned());
        if renamed.is_some() {
            keywords.push(app.name.clone());
        }
        if !app.id.eq_ignore_ascii_case(&title) {
            keywords.push(app.id.clone());
        }
        let mut actions = Vec::with_capacity(1 + app.actions.len());
        actions.push(Action::new(OPEN, "Open"));
        actions.extend(
            app.actions
                .iter()
                .map(|desktop| Action::new(format!("{DESKTOP_ACTION}{}", desktop.id), desktop.name.clone())),
        );
        Item {
            id: app.id.clone(),
            kind: ItemKind::App {
                desktop_id: app.desktop_id.clone(),
            },
            // The same app from another Extension, such as a Folder source,
            // shares its History.
            identity: Some(format!("app:{}", app.desktop_id)),
            title,
            subtitle: app.generic_name.clone().or_else(|| app.comment.clone()),
            icon: app.icon.clone(),
            keyword: None,
            keywords,
            hidden: false,
            app_only: false,
            actions,
        }
    }
}

/// `value` as a number of seconds, from 0 to [`MAX_WITHDRAW_WITHIN`].
fn seconds(value: &Value) -> Option<Duration> {
    let seconds = match value {
        Value::Integer(seconds) => *seconds as f64,
        Value::Float(seconds) => *seconds,
        _ => return None,
    };
    (0.0..=MAX_WITHDRAW_WITHIN)
        .contains(&seconds)
        .then(|| Duration::from_secs_f64(seconds))
}

/// The command an Action of `app` runs: [`OPEN`] or one of its desktop
/// actions. `None` for an Action the app doesn't have.
pub fn command<'a>(app: &'a App, action: &str) -> Option<&'a [String]> {
    if action == OPEN {
        return Some(&app.command);
    }
    let own = action.strip_prefix(DESKTOP_ACTION)?;
    app.actions
        .iter()
        .find(|desktop| desktop.id == own)
        .map(|desktop| desktop.command.as_slice())
}
