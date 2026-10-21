//! The engine's settings: how many rows a search fills, whether it adds
//! near misses and asks fallbacks, how crashed Extensions restart, how keys
//! go into the window the user came from, and how much the Daemon logs.
//!
//! ```toml
//! [search]
//! rows = 10            # rows a Front end shows until it says otherwise
//! near_misses = true   # Results a typo away, below every real match
//! fallbacks = true     # ask fallbacks, such as web search, what leaves rows empty
//!
//! [extensions]
//! retry_max_seconds = 30  # the longest wait before a crashed Extension restarts
//! give_up_after = 0       # crashes in a row before it stays down; 0 never gives up
//!
//! [desktop]
//! settle_ms = 50        # how long keys wait after the menu closes
//! paste = "ctrl+v"      # or "ctrl+shift+v", for terminals, or "type"
//!
//! [log]
//! level = "info"        # or "debug", "warn", "error"; `KOTODAMA_LOG` wins
//! ```
//!
//! Every setting is read when the config loads, never on a keystroke, and
//! takes effect at once.
//!
//! # Owns
//!
//! - [`Engine`]: the settings above, and [`Engine::from_config`].
//! - [`DesktopSettings`] and [`Paste`]: what `desktop.rs` reads.
//! - [`check_tables`]: the config's top-level tables, so a misspelt one is a problem.
//!
//! # Not here
//!
//! - `[history]`: `history.rs`. `[paths]`: `paths.rs`. Each Extension's `[extensions.<name>]`:
//!   `configured.rs` and `apps.rs`.
//! - The keys with their docs and defaults, for `kotodamad config`: `schema.rs`.

use std::time::Duration;

use kotodama_protocol::Table;
use kotodama_protocol::Value;

use crate::problem::Report;

/// Rows a Front end shows until it says otherwise: what fits a menu on a
/// laptop screen at the default font.
pub const DEFAULT_ROWS: usize = 10;
/// The longest wait before a crashed Extension restarts. Waits double from
/// 100 ms with each crash in a row; 30 s keeps a broken Extension from
/// spinning while one that recovers is back soon.
pub const DEFAULT_RETRY_MAX: Duration = Duration::from_secs(30);
/// How long keys wait after the menu closes, so the window that gets the
/// keyboard back is ready for them (see `desktop.rs`). Two scheduler periods
/// of a busy machine: CFS aims to run every runnable thread within about
/// 24 ms on 8 CPUs.
pub const DEFAULT_SETTLE: Duration = Duration::from_millis(50);

/// The top-level tables of the config. Anything else is a problem.
const TABLES: [&str; 11] = [
    "paths",
    "search",
    "extensions",
    "desktop",
    "log",
    "history",
    "empty_query",
    "ranking",
    "aliases",
    "quicklinks",
    "quicklink_defaults",
];

/// The engine's settings, from the config's `[search]`, `[extensions]`,
/// `[desktop]` and `[log]` tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Engine {
    /// Rows a Front end shows until it says otherwise.
    pub rows: usize,
    /// Whether a search adds Results a typo away when real matches leave
    /// rows empty.
    pub near_misses: bool,
    /// Whether a search asks fallbacks, such as web search, for what real
    /// matches leave rows empty. A fallback's trigger still routes queries
    /// to it.
    pub fallbacks: bool,
    /// The longest wait before a crashed Extension restarts.
    pub retry_max: Duration,
    /// Crashes in a row after which an Extension stays down until its
    /// program or Manifest changes; 0 never gives up.
    pub give_up_after: u32,
    pub desktop: DesktopSettings,
    /// How much the Daemon logs, unless `KOTODAMA_LOG` says.
    pub log: Option<log::LevelFilter>,
}

impl Default for Engine {
    fn default() -> Self {
        Engine {
            rows: DEFAULT_ROWS,
            near_misses: true,
            fallbacks: true,
            retry_max: DEFAULT_RETRY_MAX,
            give_up_after: 0,
            desktop: DesktopSettings::default(),
            log: None,
        }
    }
}

/// How the Daemon puts text into the window the user came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesktopSettings {
    /// How long keys wait after the menu closes.
    pub settle: Duration,
    pub paste: Paste,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        DesktopSettings {
            settle: DEFAULT_SETTLE,
            paste: Paste::CtrlV,
        }
    }
}

/// How a paste reaches the window: the keys that paste the clipboard, or
/// typing the text instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Paste {
    #[default]
    CtrlV,
    /// What terminals paste with.
    CtrlShiftV,
    /// Types the text rather than pasting it, for windows that take no
    /// paste; an image is still pasted with Ctrl+V.
    Type,
}

impl Engine {
    /// The engine's tables of the merged config. A setting that can't be
    /// read goes in `report`, and keeps its default.
    pub fn from_config(config: &Table, report: &mut Report) -> Self {
        let mut engine = Engine::default();
        for (key, value) in table(config, "search", report) {
            let set = match key.as_str() {
                "rows" => integer(value, 1, 50)
                    .ok_or("should be a number of rows from 1 to 50")
                    .map(|rows| engine.rows = rows as usize),
                "near_misses" => flag(value).map(|on| engine.near_misses = on),
                "fallbacks" => flag(value).map(|on| engine.fallbacks = on),
                _ => Err("isn't a setting"),
            };
            if let Err(err) = set {
                report.add(format!("search.{key}"), err);
            }
        }
        for (key, value) in table(config, "extensions", report) {
            let set = match key.as_str() {
                // Each Extension's own table.
                _ if matches!(value, Value::Table(_)) => Ok(()),
                "retry_max_seconds" => integer(value, 1, 24 * 60 * 60)
                    .ok_or("should be a number of seconds from 1 to 86400")
                    .map(|secs| engine.retry_max = Duration::from_secs(secs as u64)),
                "give_up_after" => integer(value, 0, i64::from(u32::MAX))
                    .ok_or("should be a number of crashes, or 0 never to give up")
                    .map(|crashes| engine.give_up_after = crashes as u32),
                _ => Err("isn't a setting, nor an Extension's table"),
            };
            if let Err(err) = set {
                report.add(format!("extensions.{key}"), err);
            }
        }
        for (key, value) in table(config, "desktop", report) {
            let set = match key.as_str() {
                "settle_ms" => integer(value, 0, 5000)
                    .ok_or("should be a number of milliseconds from 0 to 5000")
                    .map(|ms| engine.desktop.settle = Duration::from_millis(ms as u64)),
                "paste" => match value {
                    Value::String(paste) => match paste.as_str() {
                        "ctrl+v" => Ok(Paste::CtrlV),
                        "ctrl+shift+v" => Ok(Paste::CtrlShiftV),
                        "type" => Ok(Paste::Type),
                        _ => Err("should be \"ctrl+v\", \"ctrl+shift+v\" or \"type\""),
                    },
                    _ => Err("should be \"ctrl+v\", \"ctrl+shift+v\" or \"type\""),
                }
                .map(|paste| engine.desktop.paste = paste),
                _ => Err("isn't a setting"),
            };
            if let Err(err) = set {
                report.add(format!("desktop.{key}"), err);
            }
        }
        for (key, value) in table(config, "log", report) {
            let set = match key.as_str() {
                "level" => match value {
                    Value::String(level) => level_filter(level),
                    _ => None,
                }
                .ok_or("should be \"error\", \"warn\", \"info\" or \"debug\"")
                .map(|level| engine.log = Some(level)),
                _ => Err("isn't a setting"),
            };
            if let Err(err) = set {
                report.add(format!("log.{key}"), err);
            }
        }
        engine
    }
}

/// The log level a name such as `debug` stands for, as `[log] level` and
/// `KOTODAMA_LOG` name it.
pub fn level_filter(name: &str) -> Option<log::LevelFilter> {
    match name {
        "error" => Some(log::LevelFilter::Error),
        "warn" => Some(log::LevelFilter::Warn),
        "info" => Some(log::LevelFilter::Info),
        "debug" => Some(log::LevelFilter::Debug),
        _ => None,
    }
}

/// The log level `KOTODAMA_LOG` names, which wins over `[log] level`.
pub fn log_from_env() -> Option<log::LevelFilter> {
    level_filter(&std::env::var("KOTODAMA_LOG").ok()?)
}

/// Reports each top-level key of the config that is no table the Daemon
/// reads.
pub fn check_tables(config: &Table, report: &mut Report) {
    for key in config.keys() {
        if !TABLES.contains(&key.as_str()) {
            report.add(key.as_str(), "isn't a setting");
        }
    }
}

/// The entries of the config's `[name]` table; none when it is missing, or
/// isn't a table, which goes in `report`.
fn table<'a>(
    config: &'a Table,
    name: &str,
    report: &mut Report,
) -> impl Iterator<Item = (&'a String, &'a Value)> + use<'a> {
    let table = match config.get(name) {
        Some(Value::Table(table)) => Some(table),
        Some(_) => {
            report.add(name, "should be a table");
            None
        }
        None => None,
    };
    table.into_iter().flatten()
}

fn integer(value: &Value, min: i64, max: i64) -> Option<i64> {
    match value {
        Value::Integer(n) if (min..=max).contains(n) => Some(*n),
        _ => None,
    }
}

fn flag(value: &Value) -> Result<bool, &'static str> {
    match value {
        Value::Bool(on) => Ok(*on),
        _ => Err("should be true or false"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(toml: &str) -> (Engine, Vec<String>) {
        let config = crate::config::parse(toml).unwrap();
        let mut report = Report::new();
        let engine = Engine::from_config(&config, &mut report);
        let abouts = report
            .into_problems()
            .into_iter()
            .map(|problem| problem.about)
            .collect();
        (engine, abouts)
    }

    #[test]
    fn an_empty_config_keeps_every_default() {
        assert_eq!(engine(""), (Engine::default(), vec![]));
    }

    #[test]
    fn each_table_sets_its_settings() {
        let (engine, problems) = engine(
            "[search]\nrows = 6\nnear_misses = false\nfallbacks = false\n\
             [extensions]\nretry_max_seconds = 5\ngive_up_after = 3\n\
             [extensions.calculator]\nword = \"calc\"\n\
             [desktop]\nsettle_ms = 120\npaste = \"ctrl+shift+v\"\n\
             [log]\nlevel = \"debug\"\n",
        );

        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(
            engine,
            Engine {
                rows: 6,
                near_misses: false,
                fallbacks: false,
                retry_max: Duration::from_secs(5),
                give_up_after: 3,
                desktop: DesktopSettings {
                    settle: Duration::from_millis(120),
                    paste: Paste::CtrlShiftV,
                },
                log: Some(log::LevelFilter::Debug),
            }
        );
    }

    #[test]
    fn a_bad_value_is_a_problem_and_keeps_its_default() {
        let (engine, problems) = engine(
            "[search]\nrows = 0\nnearmisses = false\n\
             [extensions]\ngive_up_after = -1\n\
             [desktop]\npaste = \"shift+insert\"\n\
             [log]\nlevel = \"loud\"\n",
        );

        assert_eq!(engine, Engine::default());
        assert_eq!(
            problems,
            [
                "search.nearmisses",
                "search.rows",
                "extensions.give_up_after",
                "desktop.paste",
                "log.level"
            ]
        );
    }

    #[test]
    fn a_table_the_daemon_doesnt_read_is_a_problem() {
        let config = crate::config::parse("[serach]\nrows = 5\n[search]\nrows = 5\n").unwrap();
        let mut report = Report::new();

        check_tables(&config, &mut report);

        let abouts: Vec<String> = report
            .into_problems()
            .into_iter()
            .map(|problem| problem.about)
            .collect();
        assert_eq!(abouts, ["serach"]);
    }
}
