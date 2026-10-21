//! The engine as the config sets it up: which Extensions run and how, how a
//! search fills its rows, how crashed Extensions restart, and how text goes
//! into windows.

mod common;

use std::collections::BTreeMap;
use std::time::Duration;

use common::*;
use kotodama_protocol::*;
use kotodamad::daemon::{Event, ExtensionId};
use kotodamad::engine::{DesktopSettings, Paste};

/// A fallback, such as web search, asked at once.
fn fallback() -> kotodamad::discovery::Found {
    manifest("name = \"web\"\ncommand = [\"x\"]\n[live_query]\nfallback = true\ndebounce_ms = 0")
}

fn asked(ext: &Ext) -> Vec<String> {
    ext.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToExtension::LiveQuery { query, .. } => Some(query),
            _ => None,
        })
        .collect()
}

/// The settings the Extension was last sent.
fn settings(ext: &Ext) -> Option<Table> {
    ext.drain()
        .into_iter()
        .rev()
        .find_map(|message| match message {
            ToExtension::Config(table) => Some(table),
            _ => None,
        })
}

#[test]
fn an_extension_switched_off_in_the_config_never_starts() {
    let h = Harness::with_config(
        vec![plain("apps"), plain("emoji")],
        config("[extensions.emoji]\nenable = false\n"),
    );

    assert_eq!(*h.launches.0.borrow(), [(ExtensionId(0), 1)]);
}

#[test]
fn switching_an_extension_off_stops_it_and_switching_it_on_starts_it_again() {
    let mut h = Harness::new(vec![plain("apps"), plain("emoji")]);
    let emoji = h.up(1);

    h.event(Event::Config(config(
        "[extensions.emoji]\nenable = false\n",
    )));
    assert_eq!(*h.launches.1.borrow(), [(ExtensionId(1), emoji.generation)]);

    h.event(Event::Config(Table::new()));
    assert_eq!(h.launches.0.borrow().len(), 3);
    assert_eq!(
        h.launches
            .0
            .borrow()
            .last()
            .map(|&(extension, _)| extension),
        Some(ExtensionId(1))
    );
}

#[test]
fn an_extension_defined_in_the_config_runs_its_command_and_gets_only_its_own_settings() {
    let mut h = Harness::with_config(
        vec![plain("apps")],
        config(
            "[extensions.notes]\ncommand = [\"notes\", \"--kotodama\"]\nenv = { NOTES_DEBUG = \"1\" }\n\
             title = \"Notes\"\nword = \"note\"\nfolder = \"/home/me/notes\"\n",
        ),
    );

    let started = h.launches.2.borrow().last().cloned().unwrap();
    assert_eq!(started.manifest.name, "notes");
    assert_eq!(started.manifest.command, ["notes", "--kotodama"]);
    assert_eq!(
        started.env,
        BTreeMap::from([("NOTES_DEBUG".to_owned(), "1".to_owned())])
    );
    assert!(started.inline);

    let notes = h.up(1);
    assert_eq!(
        settings(&notes),
        Some(Table::from([(
            "folder".to_owned(),
            Value::String("/home/me/notes".into())
        )]))
    );
}

#[test]
fn overriding_an_installed_extensions_command_restarts_it_with_that_command() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);

    h.event(Event::Config(config(
        "[extensions.apps]\ncommand = [\"/home/me/apps/target/debug/apps\"]\nenv = { RUST_LOG = \"debug\" }\n",
    )));

    assert_eq!(*h.launches.1.borrow(), [(ExtensionId(0), apps.generation)]);
    let started = h.launches.2.borrow().last().cloned().unwrap();
    assert_eq!(
        started.manifest.command,
        ["/home/me/apps/target/debug/apps"]
    );
    assert_eq!(started.env["RUST_LOG"], "debug");
    let apps = h.up(0);
    assert_eq!(settings(&apps), Some(Table::new()));
}

#[test]
fn an_extension_that_keeps_crashing_stays_down_after_give_up_after_crashes() {
    let mut h = Harness::with_config(
        vec![plain("apps")],
        config("[extensions]\ngive_up_after = 2\n"),
    );
    let apps = h.up(0);
    h.down(&apps);
    h.advance(Duration::from_secs(1));
    let apps = h.up(0);

    h.down(&apps);
    h.advance(Duration::from_secs(60));

    assert_eq!(h.launches.0.borrow().len(), 2);
    let (_ui, greeting) = h.greeted_front_end(OUTBOX);
    let problems = greeting.iter().rev().find_map(|message| match message {
        ToFrontEnd::Status { problems, .. } => Some(problems.clone()),
        _ => None,
    });
    let problem = &problems.unwrap()[0];
    assert_eq!(problem.about, "extension apps");
    assert!(
        problem.message.contains("2 times in a row"),
        "{}",
        problem.message
    );
}

#[test]
fn retry_max_seconds_caps_the_wait_before_a_crashed_extension_restarts() {
    let mut h = Harness::with_config(
        vec![plain("apps")],
        config("[extensions]\nretry_max_seconds = 1\n"),
    );

    // Waits double from 100 ms: the seventh would be 6.4 s.
    for _ in 0..7 {
        let apps = h.up(0);
        h.down(&apps);
        h.advance(Duration::from_secs(1));
    }

    assert_eq!(h.launches.0.borrow().len(), 8);
}

#[test]
fn with_near_misses_off_a_typo_finds_only_real_matches() {
    let mut h = Harness::with_config(
        vec![plain("apps")],
        config("[search]\nnear_misses = false\n"),
    );
    let apps = h.up(0);
    h.add(&apps, vec![app("Firefox"), app("Fire Alarm")]);
    let ui = h.front_end();

    assert_eq!(titles(&h.query(&ui, "firef")), ["Firefox"]);
}

#[test]
fn rows_from_the_config_are_what_a_search_fills_until_the_front_end_says() {
    let mut h = Harness::with_config(vec![plain("apps")], config("[search]\nrows = 1\n"));
    let apps = h.up(0);
    h.add(&apps, vec![app("Firefox"), app("Fire Alarm")]);
    let ui = h.front_end();

    // One real match fills the one row, so no near miss is looked for.
    assert_eq!(titles(&h.query(&ui, "firef")), ["Firefox"]);
}

#[test]
fn with_fallbacks_off_a_fallback_isnt_asked_what_real_matches_leave_empty() {
    for (toml, wanted) in [
        ("", vec!["weather".to_owned()]),
        ("[search]\nfallbacks = false\n", vec![]),
    ] {
        let mut h = Harness::with_config(vec![plain("apps"), fallback()], config(toml));
        h.up(0);
        let web = h.up(1);
        web.drain();
        let ui = h.front_end();

        h.query(&ui, "weather");
        h.advance(Duration::from_millis(1));

        assert_eq!(asked(&web), wanted, "{toml}");
    }
}

#[test]
fn the_desktop_hears_how_to_put_text_into_windows_as_the_config_says() {
    let mut h = Harness::with_config(
        vec![],
        config("[desktop]\nsettle_ms = 200\npaste = \"ctrl+shift+v\"\n"),
    );
    assert_eq!(
        h.outputs.3.borrow().last(),
        Some(&DesktopSettings {
            settle: Duration::from_millis(200),
            paste: Paste::CtrlShiftV,
        })
    );

    h.event(Event::Config(Table::new()));

    assert_eq!(
        h.outputs.3.borrow().last(),
        Some(&DesktopSettings::default())
    );
}
