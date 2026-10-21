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
