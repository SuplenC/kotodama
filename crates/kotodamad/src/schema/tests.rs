
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
