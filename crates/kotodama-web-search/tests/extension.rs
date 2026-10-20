//! Drives the web search Extension the way the Daemon does, over its stdio.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use kotodama_extension::testing::Spawned;
use kotodama_protocol::*;

const WAIT: Duration = Duration::from_secs(10);

/// Starts the Extension with `path` first on its `PATH`, and sends it
/// `config`.
fn start(config: &str, path: &Path) -> Spawned {
    let search = std::env::var("PATH").unwrap_or_default();
    let mut web = Spawned::start(
        Command::new(env!("CARGO_BIN_EXE_kotodama-web-search"))
            .env("PATH", format!("{}:{search}", path.display()))
            .env("KOTODAMA_LAUNCH_PREFIX", ""),
        WAIT,
    );
    web.send(ToExtension::Config(toml::from_str(config).expect("parse the config")));
    web
}

fn ask(web: &mut Spawned, id: u64, query: &str) -> Vec<LiveResult> {
    web.send(ToExtension::LiveQuery {
        id,
        query: query.into(),
        prefix: None,
    });
    match web.recv() {
        FromExtension::LiveResults {
            live_query,
            results,
            done: true,
        } if live_query == id => results,
        other => panic!("expected live_results for {id}, got {other:?}"),
    }
}

fn titles(results: &[LiveResult]) -> Vec<&str> {
    results.iter().map(|r| r.item.title.as_str()).collect()
}

#[test]
fn a_query_is_answered_with_a_search_of_each_default_engine() {
    let dir = tempfile::tempdir().unwrap();
    let mut web = start("", dir.path());
    let results = ask(&mut web, 1, "rust book");
    assert_eq!(
        titles(&results),
        [
            "Search DuckDuckGo for rust book",
            "Search Google for rust book",
            "Search Wikipedia for rust book"
        ]
    );
    assert!(results.iter().all(|r| !r.answer && r.item.kind.name() == "url"));
}

#[test]
fn engines_come_from_the_config_and_an_address_comes_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut web = start(
        r#"engines = [{ name = "Crates", url = "https://crates.io/search?q={query}" }]"#,
        dir.path(),
    );
    let results = ask(&mut web, 7, "crates.io");
    assert_eq!(titles(&results), ["Open crates.io", "Search Crates for crates.io"]);
    assert!(results[0].answer);
    assert_eq!(
        results[1].item.kind,
        ItemKind::Url {
            url: "https://crates.io/search?q=crates.io".into()
        }
    );
}

#[test]
fn opening_a_row_runs_xdg_open_on_its_url() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("opened");
    let opener = dir.path().join("xdg-open");
    fs::write(
        &opener,
        format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n", out.display()),
    )
    .unwrap();
    fs::set_permissions(&opener, fs::Permissions::from_mode(0o755)).unwrap();
    let mut web = start("", dir.path());
    let row = ask(&mut web, 1, "żółw & co").remove(0).item;

    web.start_run(2, "open", row.to_ref(), BTreeMap::default());
    assert_eq!(web.outcome(2, |_| false), Outcome::Close);
    for _ in 0..100 {
        if out.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        "https://duckduckgo.com/?q=%C5%BC%C3%B3%C5%82w%20%26%20co\n"
    );

    web.start_run(3, "copy", row.to_ref(), BTreeMap::default());
    assert!(matches!(web.outcome(3, |_| false), Outcome::Failed { .. }));
}

#[test]
fn the_manifest_declares_a_fallback_that_may_answer() {
    let manifest = Manifest::from_toml(include_str!("../manifest.toml")).unwrap();
    assert_eq!(manifest.name, "web-search");
    let live = manifest.live_query.unwrap();
    assert!(live.fallback);
    assert_eq!(live.debounce_ms, 0);
}
