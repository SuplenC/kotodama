//! Every key of the config, with its default and what it does: what
//! `kotodamad config --schema` prints, and what `kotodamad config` fills in
//! where the config leaves a key out.
//!
//! # Owns
//!
//! - [`KEYS`], and printing them ([`print_schema`]) or the merged config ([`print_config`]).
//!
//! # Not here
//!
//! - Reading each key: its table's owner (`engine.rs`, `paths.rs`, `history.rs`, `empty_query.rs`,
//!   `kind_order.rs`, `keywords.rs`, `configured.rs`, `apps.rs`).

use std::io;
use std::io::Write;
use std::path::Path;

use kotodama_protocol::Table;
use kotodama_protocol::Value;

use crate::config;
use crate::config::Sources;
use crate::config::to_toml;
use crate::paths::Paths;

/// One key of the config.
#[derive(Debug)]
pub struct Key {
    /// Its dotted name; `<name>` and `<keyword>` stand for any.
    pub name: &'static str,
    /// Its default as TOML, if it has one that needs no reading of the
    /// environment.
    pub default: Option<&'static str>,
    pub doc: &'static str,
    /// Read once, when `kotodamad` starts, rather than as the config
    /// reloads.
    pub restart: bool,
}

const fn key(name: &'static str, default: Option<&'static str>, doc: &'static str) -> Key {
    Key {
        name,
        default,
        doc,
        restart: false,
    }
}

const fn at_start(name: &'static str, doc: &'static str) -> Key {
    Key {
        name,
        default: None,
        doc,
        restart: true,
    }
}

/// Every key, by table.
pub const KEYS: &[Key] = &[
    key(
        "paths.extension_dirs",
        Some("[]"),
        "More Extension directories, read after the default ones: an Extension in one replaces one of \
         the same name before it. Absolute paths, or starting with `~/`.",
    ),
    key(
        "paths.default_extension_dirs",
        Some("true"),
        "Whether `$XDG_DATA_DIRS/kotodama/extensions` and `extensions/` in the config directory are \
         read; false reads only `extension_dirs`.",
    ),
    at_start(
        "paths.state_dir",
        "Where the History and the Usage log are kept. Default: `$XDG_STATE_HOME/kotodama`.",
    ),
    at_start(
        "paths.cache_dir",
        "Where the Items wait that aren't shown. Default: `$XDG_CACHE_HOME/kotodama`.",
    ),
    at_start(
        "paths.socket",
        "Where Front ends connect. `KOTODAMA_SOCKET` wins, and is how Front ends find a socket moved \
         here. Default: `$XDG_RUNTIME_DIR/kotodama.sock`.",
    ),
    key(
        "search.rows",
        Some("10"),
        "Rows a Front end shows until it says otherwise: 1 to 50.",
    ),
    key(
        "search.near_misses",
        Some("true"),
        "Whether a search adds Results a typo away when real matches leave rows empty.",
    ),
    key(
        "search.fallbacks",
        Some("true"),
        "Whether a search asks fallbacks, such as web search, for what real matches leave rows empty. \
         A fallback's trigger still routes queries to it.",
    ),
    key(
        "extensions.retry_max_seconds",
        Some("30"),
        "The longest wait before a crashed Extension restarts; waits double from 100 ms.",
    ),
    key(
        "extensions.give_up_after",
        Some("0"),
        "Crashes in a row after which an Extension stays down until its program or Manifest changes; \
         0 never gives up.",
    ),
    key(
        "extensions.<name>.enable",
        Some("true"),
        "false switches the Extension off without uninstalling it.",
    ),
    key(
        "extensions.<name>.command",
        None,
        "The program and its arguments, replacing the Manifest's. With no Extension of that name \
         installed, defines one: its table then takes the Manifest's keys too (`title`, `word`, \
         `live_query`, ...). A bare program name is looked up on PATH, unlike a Manifest's.",
    ),
    key(
        "extensions.<name>.env",
        Some("{}"),
        "Variables set in the Extension's environment.",
    ),
    key(
        "extensions.<name>.live_query",
        None,
        "Replaces the Manifest's Live query `prefixes`, `triggers` or `debounce_ms`.",
    ),
    key(
        "extensions.<name>.word",
        None,
        "The word that names the Extension's app, replacing the Manifest's.",
    ),
    key(
        "extensions.<name>.in_search",
        None,
        "Whether the Extension's Items are in the search, or in its app alone, replacing the Manifest's.",
    ),
    key(
        "desktop.settle_ms",
        Some("50"),
        "How long keys wait after the menu closes, so the window that gets the keyboard back takes \
         them: 0 to 5000. Slow apps may need more.",
    ),
    key(
        "desktop.paste",
        Some("\"ctrl+v\""),
        "How a paste reaches the window: \"ctrl+v\", \"ctrl+shift+v\" (terminals), or \"type\", which \
         types text rather than pasting it.",
    ),
    key(
        "log.level",
        Some("\"info\""),
        "How much the Daemon logs: \"error\", \"warn\", \"info\" or \"debug\". `KOTODAMA_LOG` wins.",
    ),
    key(
        "history.picks_to_learn",
        Some("3"),
        "Picks of one Result for one query that make it the query's Learned choice: 1 to 255.",
    ),
    key(
        "history.picks_to_take_over",
        Some("2"),
        "Picks of another Result, since the choice was last picked, that replace it: 1 to 255.",
    ),
    key(
        "history.half_life_days",
        Some("14"),
        "How long a pick takes to count half as much.",
    ),
    key(
        "history.lift_threshold",
        Some("5.0"),
        "The Frecency that lifts a Result one Tier: about a week of daily use.",
    ),
    key(
        "history.time_of_day",
        Some("0.5"),
        "How much less a pick counts at a time of day it is seldom made: 0 to 1.",
    ),
    key(
        "history.usage_log",
        Some("false"),
        "Whether queries and picks go in the Usage log, to tune ranking from real use.",
    ),
    key(
        "history.undo_seconds",
        Some("60"),
        "How long Forget and Clear history can be undone.",
    ),
    key(
        "history.max_results",
        Some("2000"),
        "The most Results the History keeps; past it the least used go.",
    ),
    key(
        "history.max_queries",
        Some("1000"),
        "The most queries whose Learned choice is kept; past it the least recent go.",
    ),
    key(
        "empty_query.most_used",
        Some("3"),
        "How many of the Results most used at this time of day the Empty query shows first: 0 to 50.",
    ),
    key(
        "empty_query.menu",
        None,
        "The Fixed menu after them: `{ extension, item }` each. Without it, the apps' entry Items.",
    ),
    key(
        "ranking.kinds",
        Some("[\"app\", \"project\", \"url\", \"*\", \"file\"]"),
        "On an equal match, kinds earlier in the list rank first; `*` stands for every other kind.",
    ),
    key(
        "aliases.<keyword>",
        None,
        "An Alias: `{ extension, item }`, and `action` to make one of the Item's Actions the primary one.",
    ),
    key(
        "quicklinks.<keyword>",
        None,
        "A Quicklink: `url` or `command`, `title`, `opener`; `{query}` is what follows the keyword.",
    ),
    key(
        "quicklink_defaults.opener",
        None,
        "What opens a Quicklink's URL, the URL its last argument. Default: `xdg-open`.",
    ),
];

/// Writes every key with its default and what it does.
///
/// # Errors
///
/// When `out` can't be written.
pub fn print_schema(out: &mut impl Write) -> io::Result<()> {
    for key in KEYS {
        match key.default {
            Some(default) => writeln!(out, "{} = {default}", key.name)?,
            None => writeln!(out, "{}", key.name)?,
        }
        writeln!(out, "    {}", key.doc)?;
        if key.restart {
            writeln!(out, "    Read when kotodamad starts.")?;
        }
        writeln!(out)?;
    }
    Ok(())
}

/// Writes the merged `config` as TOML, with every default filled in, and
/// after each value where it came from: a file `sources` names, or the
/// default.
///
/// # Errors
///
/// When `out` can't be written.
pub fn print_config(config: &Table, sources: &Sources, paths: &Paths, out: &mut impl Write) -> io::Result<()> {
    for (what, dirs) in [
        ("Config directories", &paths.config_dirs),
        ("Extension directories", &paths.extension_dirs),
    ] {
        writeln!(out, "# {what}, least important first:")?;
        for dir in dirs {
            writeln!(out, "#   {}", dir.display())?;
        }
    }
    writeln!(out)?;
    let mut merged = defaults(paths);
    config::merge(&mut merged, config.clone());
    print_table(&merged, "", "", sources, out)
}

/// The defaults of every key that has one, the paths as `paths` has them.
fn defaults(paths: &Paths) -> Table {
    let mut text = String::new();
    for key in KEYS {
        if let Some(default) = key.default
            && !key.name.contains('<')
        {
            text.push_str(&format!("{} = {default}\n", key.name));
        }
    }
    let mut defaults = config::parse(&text).expect("the schema's defaults are TOML");
    let state = paths.history.parent().unwrap_or(Path::new("/"));
    let mut path_defaults = Table::new();
    for (key, path) in [
        ("state_dir", state),
        ("cache_dir", &paths.cache),
        ("socket", &paths.socket),
    ] {
        path_defaults.insert(key.into(), Value::String(path.display().to_string()));
    }
    config::merge(
        &mut defaults,
        Table::from([("paths".to_owned(), Value::Table(path_defaults))]),
    );
    defaults
}

/// Writes `table`, whose dotted name is `name` as `sources` has it and
/// `header` as TOML writes it.
fn print_table(table: &Table, name: &str, header: &str, sources: &Sources, out: &mut impl Write) -> io::Result<()> {
    let leaves: Vec<_> = table
        .iter()
        .filter(|(_, value)| !matches!(value, Value::Table(_)))
        .collect();
    if !leaves.is_empty() && !header.is_empty() {
        writeln!(out, "[{header}]")?;
    }
    for (key, value) in &leaves {
        let dotted = dotted(name, key);
        let source = sources
            .get(&dotted)
            .map_or("default".into(), |path| path.display().to_string());
        writeln!(out, "{} = {}  # {source}", toml_key(key), to_toml(value))?;
    }
    if !leaves.is_empty() {
        writeln!(out)?;
    }
    for (key, value) in table {
        if let Value::Table(table) = value {
            let header = if header.is_empty() {
                toml_key(key)
            } else {
                format!("{header}.{}", toml_key(key))
            };
            print_table(table, &dotted(name, key), &header, sources, out)?;
        }
    }
    Ok(())
}

/// The dotted name `sources` knows a key by.
fn dotted(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_owned()
    } else {
        format!("{prefix}.{key}")
    }
}

/// `key` as TOML writes it: bare if it can be, else quoted.
fn toml_key(key: &str) -> String {
    let bare = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        key.to_owned()
    } else {
        toml::Value::String(key.to_owned()).to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::empty_query::EmptyQuery;
    use crate::engine;
    use crate::engine::Engine;
    use crate::history::Settings;
    use crate::kind_order::KindOrder;
    use crate::paths::Overrides;
    use crate::problem::Report;

    fn paths() -> Paths {
        Paths {
            socket: PathBuf::from("/run/user/1000/kotodama.sock"),
            config_dirs: vec![PathBuf::from("/home/me/.config/kotodama")],
            extension_dirs: vec![PathBuf::from("/home/me/.config/kotodama/extensions")],
            history: PathBuf::from("/home/me/.local/state/kotodama/history.json"),
            usage_log: PathBuf::from("/home/me/.local/state/kotodama/usage.jsonl"),
            cache: PathBuf::from("/home/me/.cache/kotodama"),
        }
    }

    #[test]
    fn every_default_is_a_setting_the_daemon_reads_without_a_problem() {
        let config = defaults(&paths());
        let mut report = Report::new();

        engine::check_tables(&config, &mut report);
        Overrides::from_config(&config, &mut report);
        let engine = Engine::from_config(&config, &mut report);
        let history = Settings::from_config(&config, &mut report);
        let empty_query = EmptyQuery::from_config(&config, &mut report);
        let kinds = KindOrder::from_config(&config, &mut report);

        assert_eq!(report.into_problems(), []);
        assert_eq!(
            engine,
            Engine {
                log: Some(log::LevelFilter::Info),
                ..Engine::default()
            }
        );
        assert_eq!(history, Settings::default());
        assert_eq!(empty_query, EmptyQuery::default());
        assert_eq!(kinds, KindOrder::default());
    }

    #[test]
    fn the_config_shows_each_value_and_where_it_came_from() {
        let file = PathBuf::from("/home/me/.config/kotodama/config.toml");
        let config = config::parse(
            "[search]\nrows = 6\n[aliases]\nff = { extension = \"apps\", item = \"firefox\" }\n\
             \"g+\" = { extension = \"web\", item = \"google\" }\n",
        )
        .unwrap();
        let sources = Sources::from([
            ("search.rows".to_owned(), file.clone()),
            ("aliases.ff.extension".to_owned(), file.clone()),
            ("aliases.ff.item".to_owned(), file.clone()),
            ("aliases.g+.extension".to_owned(), file),
        ]);
        let mut out = Vec::new();

        print_config(&config, &sources, &paths(), &mut out).unwrap();

        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("[search]\nfallbacks = true  # default\nnear_misses = true  # default\nrows = 6  # /home/me/.config/kotodama/config.toml\n"), "{out}");
        assert!(
            out.contains("[aliases.ff]\nextension = \"apps\"  # /home/me/.config/kotodama/config.toml\n"),
            "{out}"
        );
        assert!(
            out.contains("[aliases.\"g+\"]\nextension = \"web\"  # /home/me/.config/kotodama/config.toml\n"),
            "{out}"
        );
        assert!(
            out.contains("state_dir = \"/home/me/.local/state/kotodama\"  # default\n"),
            "{out}"
        );
        assert!(config::parse(&out).is_ok(), "{out}");
    }

    #[test]
    fn the_schema_names_each_key_with_its_default() {
        let mut out = Vec::new();

        print_schema(&mut out).unwrap();

        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("search.rows = 10\n    Rows a Front end shows"), "{out}");
        assert!(out.contains("paths.socket\n    Where Front ends connect."), "{out}");
    }
}
