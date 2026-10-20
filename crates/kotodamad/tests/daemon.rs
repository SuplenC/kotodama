//! The Daemon's state machine, driven through its events: Extensions and Front ends are
//! channels here, and starting a process or typing is recorded instead of done.

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

use common::*;
use kotodama_protocol::*;
use kotodamad::daemon::{
    Event, ExtensionId, FrontEndId, HANDSHAKE_TIMEOUT, MAX_LIVE_RESULTS, Output,
};
use kotodamad::discovery::Found;
use kotodamad::usage::Used;

#[test]
fn indexed_items_are_found_by_a_fuzzy_query_best_match_first() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(
        &apps,
        vec![
            app("Files"),
            app("Firefox"),
            app("Calculator"),
            app("Fire Alarm"),
        ],
    );
    let ui = h.front_end();

    let rows = h.query(&ui, "firef");
    // Fire Alarm is a near miss: "fire" with an extra letter.
    assert_eq!(titles(&rows), ["Firefox", "Fire Alarm"]);
    assert_eq!(rows[0].extension, "apps");
    assert_eq!(rows[0].highlights, [0, 1, 2, 3, 4]);

    assert_eq!(titles(&h.query(&ui, "fi")).len(), 3);
}

fn calculator() -> Found {
    manifest(
        "name = \"calc\"\ncommand = [\"x\"]\n[live_query]\ntriggers = ['^[0-9]']\ndebounce_ms = 0",
    )
}

fn web() -> Found {
    manifest(
        "name = \"web\"\ncommand = [\"x\"]\n[live_query]\nprefixes = [\"g \"]\ndebounce_ms = 100",
    )
}

fn live_queries(ext: &Ext) -> Vec<(u64, String, Option<String>)> {
    ext.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToExtension::LiveQuery { id, query, prefix } => Some((id, query, prefix)),
            _ => None,
        })
        .collect()
}

fn live(title: &str, score: f32, answer: bool) -> LiveResult {
    LiveResult {
        item: item(
            title,
            title,
            ItemKind::Url {
                url: format!("https://{title}"),
            },
        ),
        score: Fraction::new(score).unwrap(),
        answer,
    }
}

#[test]
fn a_live_query_waits_for_the_debounce_and_drops_the_routing_prefix() {
    let mut h = Harness::new(vec![plain("apps"), web()]);
    h.up(0);
    let web = h.up(1);
    web.drain();
    let ui = h.front_end();

    h.query(&ui, "g ru");
    h.advance(Duration::from_millis(50));
    h.query(&ui, "g rust");
    h.advance(Duration::from_millis(60));
    assert_eq!(live_queries(&web), [], "still inside the debounce");

    h.advance(Duration::from_millis(40));
    let sent = live_queries(&web);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].1.as_str(), sent[0].2.as_deref()),
        ("rust", Some("g "))
    );
}

#[test]
fn only_extensions_whose_prefix_or_trigger_matches_get_a_live_query() {
    let mut h = Harness::new(vec![calculator(), web()]);
    let calc = h.up(0);
    let web = h.up(1);
    let ui = h.front_end();

    h.query(&ui, "firefox");
    h.advance(Duration::from_secs(1));
    assert_eq!(live_queries(&calc), []);
    assert_eq!(live_queries(&web), []);

    h.query(&ui, "2+2");
    assert_eq!(live_queries(&calc).len(), 1, "debounce 0 sends at once");
    h.advance(Duration::from_secs(1));
    assert_eq!(live_queries(&web), []);
}

#[test]
fn changing_the_query_cancels_the_live_query_in_flight_and_drops_its_results() {
    let mut h = Harness::new(vec![calculator()]);
    let calc = h.up(0);
    calc.drain();
    let ui = h.front_end();

    h.query(&ui, "2+2");
    let (id, ..) = live_queries(&calc)[0].clone();
    h.query(&ui, "2+3");
    let sent = calc.drain();
    assert!(
        sent.contains(&ToExtension::CancelLiveQuery { id }),
        "{sent:?}"
    );

    // A straggler for the old query is ignored.
    h.send(
        &calc,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![live("4", 1.0, true)],
            done: true,
        },
    );
    assert_eq!(ui.results(), None);
}

#[test]
fn no_live_results_send_the_front_end_nothing_new() {
    let mut h = Harness::new(vec![calculator()]);
    let calc = h.up(0);
    calc.drain();
    let ui = h.front_end();

    h.query(&ui, "42");
    let (id, ..) = live_queries(&calc)[0].clone();
    h.send(
        &calc,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![],
            done: true,
        },
    );
    assert_eq!(ui.drain(), []);
}

#[test]
fn a_late_result_goes_below_the_visible_rows_however_well_it_scores() {
    let mut h = Harness::new(vec![plain("apps"), web()]);
    let apps = h.up(0);
    let web = h.up(1);
    h.add(
        &apps,
        vec![app("g rust one"), app("g rust two"), app("g rust three")],
    );
    let ui = h.front_end();
    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 0, rows: 2 });

    let shown = titles(&h.query(&ui, "g rust")).join(",");
    h.advance(Duration::from_millis(100));
    let (id, ..) = live_queries(&web)[0].clone();
    h.send(
        &web,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![live("rust", 1.0, false)],
            done: true,
        },
    );

    let rows = ui.results().unwrap();
    let titles = titles(&rows);
    assert_eq!(
        titles[..2].join(","),
        shown.split(',').take(2).collect::<Vec<_>>().join(",")
    );
    assert_eq!(titles[2], "rust", "first row below the visible ones");
}

#[test]
fn below_the_visible_rows_live_results_compete_with_indexed_items_on_score() {
    let mut h = Harness::new(vec![plain("apps"), web()]);
    let apps = h.up(0);
    let web = h.up(1);
    // Of the live results' kind, so that only the score tells them apart.
    h.add(
        &apps,
        vec![
            live("g rust", 0.0, false).item,
            live("g xrxuxsxt", 0.0, false).item,
        ],
    );
    let ui = h.front_end();
    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 0, rows: 1 });

    assert_eq!(titles(&h.query(&ui, "g rust")), ["g rust", "g xrxuxsxt"]);
    h.advance(Duration::from_millis(100));
    let (id, ..) = live_queries(&web)[0].clone();
    h.send(
        &web,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![live("perfect", 1.0, false), live("poor", 0.0, false)],
            done: true,
        },
    );
    assert_eq!(
        titles(&ui.results().unwrap()),
        ["g rust", "perfect", "g xrxuxsxt", "poor"]
    );
}

#[test]
fn an_answer_from_a_zero_debounce_extension_takes_the_top_row() {
    let mut h = Harness::new(vec![plain("apps"), calculator()]);
    let apps = h.up(0);
    let calc = h.up(1);
    h.add(&apps, vec![app("2048 game"), app("2 player chess")]);
    let ui = h.front_end();

    h.query(&ui, "2");
    let (id, ..) = live_queries(&calc)[0].clone();
    h.send(
        &calc,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![live("= 2", 0.1, true)],
            done: true,
        },
    );
    assert_eq!(titles(&ui.results().unwrap())[0], "= 2");
}

#[test]
fn an_answer_from_a_debounced_extension_is_ranked_like_any_late_result() {
    let mut h = Harness::new(vec![plain("apps"), web()]);
    let apps = h.up(0);
    let web = h.up(1);
    h.add(&apps, vec![app("g rust")]);
    let ui = h.front_end();

    h.query(&ui, "g rust");
    h.advance(Duration::from_millis(100));
    let (id, ..) = live_queries(&web)[0].clone();
    h.send(
        &web,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![live("claims to answer", 1.0, true)],
            done: true,
        },
    );
    assert_eq!(
        titles(&ui.results().unwrap()),
        ["g rust", "claims to answer"]
    );
}

#[test]
fn an_item_added_while_typing_is_found_by_the_next_keystroke() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Firefox")]);
    let ui = h.front_end();

    assert_eq!(titles(&h.query(&ui, "fir")), ["Firefox"]);
    h.add(&apps, vec![app("Fire Alarm")]);
    ui.drain();
    assert_eq!(titles(&h.query(&ui, "fire")), ["Firefox", "Fire Alarm"]);
}

fn pinner() -> Found {
    manifest(
        "name = \"pinner\"\ncommand = [\"x\"]\naction_catalog = true\n\
         [[contributed_actions]]\nkind = \"project\"\nid = \"pin\"\ntitle = \"Pin a version here\"\nshortcut = \"ctrl+p\"",
    )
}

fn projects() -> Found {
    manifest(
        "name = \"projects\"\ncommand = [\"x\"]\n\
         [[contributed_actions]]\nkind = \"project\"\nid = \"own-contribution\"\ntitle = \"Never shown on own Items\"",
    )
}

fn runs(ext: &Ext) -> Vec<(u64, String, ItemRef)> {
    ext.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToExtension::RunAction {
                invocation,
                action,
                item,
                ..
            } => Some((invocation, action, item)),
            _ => None,
        })
        .collect()
}

#[test]
fn contributed_actions_follow_the_owners_actions_and_are_never_primary() {
    let mut h = Harness::new(vec![pinner(), projects()]);
    h.up(0);
    let projects = h.up(1);
    h.add(&projects, vec![project("kotodama")]);
    let ui = h.front_end();

    let row = &h.query(&ui, "kotodama")[0];
    assert_eq!(
        row.item.actions[0].id, "open",
        "the owner's Action stays primary"
    );
    let contributed: Vec<_> = row
        .contributed
        .iter()
        .map(|c| (c.extension.as_str(), c.action.id.as_str()))
        .collect();
    assert_eq!(contributed, [("pinner", "pin")]);
}

#[test]
fn when_shortcuts_clash_the_owners_binding_wins() {
    let mut h = Harness::new(vec![pinner(), projects()]);
    h.up(0);
    let projects = h.up(1);
    let mut item = project("kotodama");
    item.actions[0].shortcut = Some("ctrl+p".into());
    h.add(&projects, vec![item]);
    let ui = h.front_end();

    let row = &h.query(&ui, "kotodama")[0];
    assert_eq!(row.item.actions[0].shortcut.as_deref(), Some("ctrl+p"));
    assert_eq!(row.contributed[0].action.shortcut, None);
}

#[test]
fn a_contributed_action_reaches_the_contributing_extension_with_the_item() {
    let mut h = Harness::new(vec![pinner(), projects()]);
    let pinner = h.up(0);
    let projects = h.up(1);
    h.add(&projects, vec![project("kotodama")]);
    let ui = h.front_end();
    h.query(&ui, "kotodama");
    pinner.drain();
    projects.drain();

    run(&mut h, &ui, "projects", "kotodama", "pinner", "pin");
    let sent = runs(&pinner);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, "pin");
    assert_eq!(sent[0].2, project("kotodama").to_ref());
    assert_eq!(runs(&projects), []);

    // The contributor's answer closes the menu that ran it.
    h.send(
        &pinner,
        FromExtension::ActionDone {
            invocation: sent[0].0,
            outcome: Outcome::Close,
        },
    );
    assert!(ui.drain().contains(&ToFrontEnd::Close));
}

#[test]
fn running_a_contributed_action_counts_as_a_pick() {
    let mut h = Harness::new(vec![pinner(), projects()]);
    h.up(0);
    let projects = h.up(1);
    h.add(&projects, vec![project("kotodama"), project("kotlin")]);
    let ui = h.front_end();
    assert_eq!(titles(&h.query(&ui, "kot")), ["kotlin", "kotodama"]);

    run(&mut h, &ui, "projects", "kotodama", "pinner", "pin");
    ui.drain();
    assert_eq!(titles(&h.query(&ui, "kot")), ["kotodama", "kotlin"]);
}

#[test]
fn an_action_the_result_does_not_offer_is_refused() {
    let mut h = Harness::new(vec![pinner(), projects()]);
    let pinner = h.up(0);
    let projects = h.up(1);
    h.add(&projects, vec![project("kotodama"), app("Firefox")]);
    let ui = h.front_end();
    h.query(&ui, "f");
    pinner.drain();

    // `pin` is for projects, not apps.
    run(&mut h, &ui, "projects", "firefox", "pinner", "pin");
    assert_eq!(runs(&pinner), []);
    assert!(
        ui.drain()
            .iter()
            .any(|m| matches!(m, ToFrontEnd::ActionFailed { .. }))
    );
}

fn catalogs(ext: &Ext) -> Vec<Vec<CatalogEntry>> {
    ext.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToExtension::ActionCatalog { entries } => Some(entries),
            _ => None,
        })
        .collect()
}

#[test]
fn the_action_catalog_lists_actions_by_kind_without_items_and_follows_extensions() {
    let mut h = Harness::new(vec![plain("apps"), pinner(), projects()]);
    let apps = h.up(0);
    let pinner = h.up(1);
    h.add(&apps, vec![app("Firefox"), app("Files")]);
    h.send(&pinner, FromExtension::RequestActionCatalog);

    let summary = |entries: &[CatalogEntry]| {
        entries
            .iter()
            .map(|e| format!("{}:{}:{}", e.extension, e.kind, e.action.id))
            .collect::<Vec<_>>()
    };
    let first = catalogs(&pinner);
    assert_eq!(first.len(), 1);
    assert_eq!(summary(&first[0]), ["apps:app:open", "pinner:project:pin"]);

    let projects = h.up(2);
    let updated = catalogs(&pinner);
    assert_eq!(
        summary(updated.last().unwrap()),
        [
            "apps:app:open",
            "pinner:project:pin",
            "projects:project:own-contribution"
        ]
    );

    h.down(&projects);
    assert_eq!(
        summary(catalogs(&pinner).last().unwrap()),
        ["apps:app:open", "pinner:project:pin"]
    );
}

#[test]
fn an_extension_whose_manifest_does_not_ask_never_receives_the_catalog() {
    let mut h = Harness::new(vec![plain("apps"), pinner()]);
    let apps = h.up(0);
    let pinner = h.up(1);
    h.send(&apps, FromExtension::RequestActionCatalog);
    h.send(&pinner, FromExtension::RequestActionCatalog);
    h.add(&apps, vec![app("Firefox")]);

    assert_eq!(catalogs(&apps), Vec::<Vec<CatalogEntry>>::new());
    assert!(!catalogs(&pinner).is_empty());
}

fn statuses(ui: &FrontEnd) -> Vec<Vec<String>> {
    ui.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToFrontEnd::Status { down } => Some(down),
            _ => None,
        })
        .collect()
}

#[test]
fn a_stopped_extensions_results_disappear_and_a_marker_shows_it_is_down() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Firefox")]);
    let ui = h.front_end();
    h.query(&ui, "fire");

    h.down(&apps);
    let messages = ui.drain();
    assert!(messages.contains(&ToFrontEnd::Results { results: vec![] }));
    assert!(messages.contains(&ToFrontEnd::Status {
        down: vec!["apps".into()]
    }));
}

#[test]
fn a_stopped_extension_is_restarted_with_growing_backoff_and_marked_up_again() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ui = h.front_end();
    let launches = |h: &Harness| h.launches.0.borrow().len();

    let first = h.up(0);
    h.down(&first);
    h.advance(Duration::from_millis(99));
    assert_eq!(launches(&h), 1);
    h.advance(Duration::from_millis(1));
    assert_eq!(launches(&h), 2, "restarted after 100 ms");

    let second = h.up(0);
    h.down(&second);
    h.advance(Duration::from_millis(199));
    assert_eq!(launches(&h), 2);
    h.advance(Duration::from_millis(1));
    assert_eq!(launches(&h), 3, "then after 200 ms");

    statuses(&ui);
    h.up(0);
    // Back up: the marker goes once the handshake is done.
    let ui2 = h.front_end();
    h.query(&ui2, "x");
    assert_eq!(statuses(&ui), [Vec::<String>::new()]);
}

#[test]
fn messages_from_a_previous_run_of_an_extension_are_ignored() {
    let mut h = Harness::new(vec![plain("apps")]);
    let old = h.up(0);
    h.down(&old);
    h.advance(Duration::from_secs(1));
    let _new = h.up(0);
    let ui = h.front_end();

    h.add(&old, vec![app("Ghost")]);
    assert_eq!(h.query(&ui, "ghost"), []);
}

#[test]
fn a_config_change_reaches_only_the_extensions_whose_table_changed() {
    let table = |toml: &str| -> Table {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.toml"), toml).unwrap();
        kotodamad::config::load(&[dir.path().to_owned()]).unwrap()
    };
    let mut h = Harness::with_config(
        vec![plain("apps"), plain("web")],
        table("[extensions.apps]\nterminal = \"foot\""),
    );
    let apps = h.up(0);
    let web = h.up(1);
    let configs = |ext: &Ext| -> Vec<Table> {
        ext.drain()
            .into_iter()
            .filter_map(|m| match m {
                ToExtension::Config(table) => Some(table),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        configs(&apps),
        [BTreeMap::from([(
            "terminal".into(),
            Value::String("foot".into())
        )])],
        "the first message after the handshake"
    );
    assert_eq!(configs(&web), [Table::new()]);

    h.event(Event::Config(table(
        "[extensions.apps]\nterminal = \"kitty\"\n[other]\nx = 1",
    )));
    assert_eq!(
        configs(&apps),
        [BTreeMap::from([(
            "terminal".into(),
            Value::String("kitty".into())
        )])]
    );
    assert_eq!(configs(&web), []);
}

fn typer() -> Found {
    plain("typer")
}

fn type_text(h: &mut Harness, ui: &FrontEnd, typer: &Ext, text: &str) {
    h.add(typer, vec![app("Snippet")]);
    h.query(ui, "snippet");
    typer.drain();
    run(h, ui, "typer", "snippet", "typer", "open");
    let (invocation, ..) = runs(typer)[0].clone();
    h.send(
        typer,
        FromExtension::ActionDone {
            invocation,
            outcome: Outcome::Type { text: text.into() },
        },
    );
}

#[test]
fn typing_waits_until_every_front_end_has_closed() {
    let mut h = Harness::new(vec![typer()]);
    let typer = h.up(0);
    let ui = h.front_end();
    let other = h.front_end();

    type_text(&mut h, &ui, &typer, "hello");
    assert!(ui.drain().contains(&ToFrontEnd::Close));
    assert!(other.drain().contains(&ToFrontEnd::Close));
    assert_eq!(*h.outputs.0.borrow(), []);

    h.front_end_sends(&ui, FromFrontEnd::Closed);
    assert_eq!(*h.outputs.0.borrow(), []);
    h.event(Event::FrontEndDown {
        front_end: other.id,
    });
    assert_eq!(*h.outputs.0.borrow(), [Output::Type("hello".into())]);
}

#[test]
fn open_and_toggle_go_to_every_front_end_but_the_one_that_asked() {
    let mut h = Harness::new(vec![]);
    let menu = h.front_end();
    let other = h.front_end();
    let command = h.front_end();
    menu.drain();
    other.drain();

    h.front_end_sends(
        &command,
        FromFrontEnd::Open {
            query: "fir".into(),
            view: None,
        },
    );
    h.front_end_sends(&command, FromFrontEnd::Toggle);
    let expected = [
        ToFrontEnd::Open {
            query: "fir".into(),
            view: None,
        },
        ToFrontEnd::Toggle,
    ];
    assert_eq!(menu.drain(), expected);
    assert_eq!(other.drain(), expected);
    assert_eq!(command.drain(), []);
}

#[test]
fn copying_closes_every_front_end_and_copies_without_waiting_for_them() {
    let mut h = Harness::new(vec![typer()]);
    let typer = h.up(0);
    let ui = h.front_end();
    let other = h.front_end();

    h.add(&typer, vec![app("Snippet")]);
    h.query(&ui, "snippet");
    typer.drain();
    run(&mut h, &ui, "typer", "snippet", "typer", "open");
    let (invocation, ..) = runs(&typer)[0].clone();
    h.send(
        &typer,
        FromExtension::ActionDone {
            invocation,
            outcome: Outcome::Copy(Clipboard::Text("84".into())),
        },
    );
    assert!(ui.drain().contains(&ToFrontEnd::Close));
    assert!(other.drain().contains(&ToFrontEnd::Close));
    assert_eq!(
        *h.outputs.0.borrow(),
        [Output::Copy(Clipboard::Text("84".into()))]
    );
}

#[test]
fn typing_goes_ahead_when_a_front_end_never_answers() {
    let mut h = Harness::new(vec![typer()]);
    let typer = h.up(0);
    let ui = h.front_end();

    type_text(&mut h, &ui, &typer, "hello");
    h.advance(Duration::from_millis(499));
    assert_eq!(*h.outputs.0.borrow(), []);
    h.advance(Duration::from_millis(1));
    assert_eq!(*h.outputs.0.borrow(), [Output::Type("hello".into())]);
}

#[test]
fn a_new_item_taking_a_removed_visible_items_place_in_the_index_is_still_late() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Fire one"), app("Fire two")]);
    let ui = h.front_end();
    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 0, rows: 1 });
    assert_eq!(titles(&h.query(&ui, "fire"))[0], "Fire one");

    h.send(
        &apps,
        FromExtension::RemoveItems {
            ids: vec!["fire-one".into()],
        },
    );
    h.add(&apps, vec![app("Fire newcomer")]);
    assert_eq!(
        titles(&ui.results().unwrap()),
        ["Fire two", "Fire newcomer"]
    );
}

#[test]
fn a_bigger_viewport_protects_the_rows_it_shows_at_once() {
    let mut h = Harness::new(vec![plain("apps"), web()]);
    let apps = h.up(0);
    let web = h.up(1);
    h.add(&apps, vec![app("g rust a"), app("g rust bb")]);
    let ui = h.front_end();
    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 0, rows: 1 });
    assert_eq!(titles(&h.query(&ui, "g rust")), ["g rust a", "g rust bb"]);

    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 0, rows: 2 });
    h.advance(Duration::from_millis(100));
    let (id, ..) = live_queries(&web)[0].clone();
    h.send(
        &web,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![live("perfect", 1.0, false)],
            done: true,
        },
    );
    assert_eq!(
        titles(&ui.results().unwrap()),
        ["g rust a", "g rust bb", "perfect"]
    );
}

#[test]
fn a_stopped_extensions_jobs_are_not_shown_to_new_front_ends() {
    let mut h = Harness::new(vec![plain("installer")]);
    let installer = h.up(0);
    h.send(
        &installer,
        FromExtension::JobProgress(Job {
            id: "install".into(),
            title: "Installing".into(),
            state: JobState::Running,
            progress: None,
            message: None,
        }),
    );
    h.down(&installer);

    let (tx, rx) = mpsc::sync_channel(OUTBOX);
    h.event(Event::FrontEndUp {
        front_end: FrontEndId(99),
        sender: tx,
        disconnect: Box::new(|| {}),
    });
    let jobs: Vec<_> = rx
        .try_iter()
        .filter(|m| matches!(m, ToFrontEnd::JobProgress { .. }))
        .collect();
    assert_eq!(jobs, []);
}

fn stops(h: &Harness) -> Vec<(ExtensionId, u64)> {
    h.launches.1.borrow().clone()
}

#[test]
fn an_extension_that_never_finishes_its_handshake_is_stopped_and_restarted() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ui = h.front_end();

    h.advance(HANDSHAKE_TIMEOUT - Duration::from_millis(1));
    assert_eq!(stops(&h), []);
    h.advance(Duration::from_millis(1));
    assert_eq!(stops(&h), [(ExtensionId(0), 1)]);

    // Stopping it ends the process, which reads as any other stop: a marker,
    // then a restart.
    h.event(Event::ExtensionDown {
        extension: ExtensionId(0),
        generation: 1,
    });
    assert!(ui.drain().contains(&ToFrontEnd::Status {
        down: vec!["apps".into()]
    }));
    h.advance(Duration::from_millis(100));
    assert_eq!(h.launches.0.borrow().len(), 2);
}

#[test]
fn an_extension_that_stops_reading_is_stopped_instead_of_queueing_without_end() {
    let mut h = Harness::new(vec![calculator()]);
    let calc = h.up_with_room(0, 4);
    let ui = h.front_end();

    // Config plus a Live query and a cancel per keystroke: it never reads them.
    for n in 0..3 {
        h.query(&ui, &n.to_string());
    }
    assert_eq!(stops(&h), [(ExtensionId(0), 1)]);
    drop(calc);
}

#[test]
fn running_an_action_on_an_extension_that_stopped_reading_fails_without_blocking_queries() {
    let mut h = Harness::new(vec![plain("apps")]);
    // Room for the config only, which it never reads.
    let apps = h.up_with_room(0, 1);
    h.add(&apps, vec![app("Firefox"), app("Files")]);
    let ui = h.front_end();
    h.query(&ui, "fire");

    let failed = |ui: &FrontEnd| {
        ui.drain()
            .iter()
            .filter(|m| matches!(m, ToFrontEnd::ActionFailed { .. }))
            .count()
    };
    run(&mut h, &ui, "apps", "firefox", "apps", "open");
    assert_eq!(failed(&ui), 1);
    assert_eq!(stops(&h), [(ExtensionId(0), 1)]);

    // The Daemon goes on answering while the Extension is stopped.
    assert_eq!(titles(&h.query(&ui, "files")), ["Files"]);

    // The stop lands: the Action doesn't fail twice, and it is restarted.
    h.down(&apps);
    assert_eq!(failed(&ui), 0);
    h.advance(Duration::from_millis(100));
    assert_eq!(h.launches.0.borrow().len(), 2);
}

#[test]
fn a_restarted_extension_gets_the_live_query_already_typed() {
    let mut h = Harness::new(vec![calculator()]);
    let calc = h.up(0);
    let ui = h.front_end();
    h.query(&ui, "2+2");
    h.down(&calc);
    h.advance(Duration::from_millis(100));

    let calc = h.up(0);
    let sent = live_queries(&calc);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, "2+2");
}

#[test]
fn rows_scrolled_past_are_protected_like_the_ones_on_screen() {
    let mut h = Harness::new(vec![plain("apps"), web()]);
    let apps = h.up(0);
    let web = h.up(1);
    h.add(
        &apps,
        vec![app("g rust a"), app("g rust bb"), app("g rust ccc")],
    );
    let ui = h.front_end();
    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 0, rows: 1 });
    assert_eq!(
        titles(&h.query(&ui, "g rust")),
        ["g rust a", "g rust bb", "g rust ccc"]
    );

    // Scrolled so the second row is the one on screen.
    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 1, rows: 1 });
    h.advance(Duration::from_millis(100));
    let (id, ..) = live_queries(&web)[0].clone();
    h.send(
        &web,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![live("rust", 1.0, false)],
            done: true,
        },
    );
    assert_eq!(
        titles(&ui.results().unwrap()),
        ["g rust a", "g rust bb", "rust", "g rust ccc"]
    );
}

#[test]
fn a_front_end_that_stops_reading_is_cut_off_and_the_others_carry_on() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Firefox")]);
    let stuck = h.front_end_with_room(2);
    let ui = h.front_end();

    for text in ["f", "fi", "fir"] {
        h.front_end_sends(&stuck, FromFrontEnd::Query { text: text.into() });
    }
    assert!(stuck.closed.load(Ordering::SeqCst));
    assert_eq!(titles(&h.query(&ui, "fire")), ["Firefox"]);
    assert!(!ui.closed.load(Ordering::SeqCst));
}

#[test]
fn live_results_for_one_query_are_capped() {
    let mut h = Harness::new(vec![calculator()]);
    let calc = h.up(0);
    let ui = h.front_end();
    h.query(&ui, "1");
    let (id, ..) = live_queries(&calc)[0].clone();

    for batch in 0..3 {
        let results = (0..MAX_LIVE_RESULTS / 2)
            .map(|n| {
                live(
                    &format!("{batch}-{n}"),
                    if batch == 2 { 1.0 } else { 0.5 },
                    false,
                )
            })
            .collect();
        h.send(
            &calc,
            FromExtension::LiveResults {
                live_query: id,
                results,
                done: false,
            },
        );
    }
    // Only the first MAX_LIVE_RESULTS are kept, so the third batch never shows,
    // though it scores best.
    let rows = ui.results().unwrap();
    assert!(rows.iter().all(|row| !row.item.title.starts_with("2-")));
}

#[test]
fn an_extension_installed_while_running_is_started_and_searched() {
    let mut h = Harness::new(vec![plain("apps")]);
    h.up(0);
    let ui = h.front_end();

    h.event(Event::Extensions(vec![plain("apps"), plain("bookmarks")]));
    assert_eq!(h.launches.0.borrow().last(), Some(&(ExtensionId(1), 1)));
    let bookmarks = h.up(1);
    h.add(&bookmarks, vec![app("Kotodama")]);
    assert_eq!(titles(&h.query(&ui, "koto")), ["Kotodama"]);
}

#[test]
fn an_extension_removed_while_running_is_stopped_and_never_marked_down_or_restarted() {
    let mut h = Harness::new(vec![plain("apps"), plain("bookmarks")]);
    let apps = h.up(0);
    let bookmarks = h.up(1);
    h.add(&apps, vec![app("Firefox")]);
    h.add(&bookmarks, vec![app("Fire Brigade")]);
    let ui = h.front_end();
    assert_eq!(h.query(&ui, "fire").len(), 2);

    h.event(Event::Extensions(vec![plain("apps")]));
    assert_eq!(stops(&h), [(ExtensionId(1), bookmarks.generation)]);
    assert_eq!(titles(&ui.results().unwrap()), ["Firefox"]);

    // Its process ending and later messages change nothing.
    h.down(&bookmarks);
    h.add(&bookmarks, vec![app("Fire Ghost")]);
    h.advance(Duration::from_secs(60));
    assert_eq!(h.launches.0.borrow().len(), 2);
    assert!(statuses(&ui).iter().all(|down| down.is_empty()));
    assert_eq!(titles(&h.query(&ui, "fire")), ["Firefox"]);
}

#[test]
fn a_changed_manifest_restarts_the_extension_at_once_with_the_new_one() {
    let mut h = Harness::new(vec![plain("projects"), plain("pinner")]);
    let projects = h.up(0);
    let old = h.up(1);
    h.add(&projects, vec![project("kotodama")]);
    let ui = h.front_end();
    assert_eq!(h.query(&ui, "kotodama")[0].contributed, []);

    h.event(Event::Extensions(vec![plain("projects"), pinner()]));
    assert_eq!(stops(&h), [(ExtensionId(1), old.generation)]);
    let (ext, generation) = *h.launches.0.borrow().last().unwrap();
    assert_eq!(ext, ExtensionId(1));
    assert!(generation > old.generation, "a new run, started at once");
    h.up(1);
    let row = &h.query(&ui, "kotodama")[0];
    assert_eq!(row.contributed[0].action.id, "pin");
}

#[test]
fn an_unchanged_extension_list_changes_nothing() {
    let mut h = Harness::new(vec![plain("apps")]);
    h.up(0);
    h.event(Event::Extensions(vec![plain("apps")]));
    assert_eq!(stops(&h), []);
    assert_eq!(h.launches.0.borrow().len(), 1);
}

fn launcher_apps() -> Vec<Item> {
    vec![app("Files"), app("Firefox"), app("Fish")]
}

/// Picks a Result of `owner` by running its primary Action with `query` typed.
fn pick(h: &mut Harness, ui: &FrontEnd, query: &str, owner: &str, id: &str) {
    h.query(ui, query);
    run(h, ui, owner, id, owner, "open");
    ui.drain();
}

fn first(h: &mut Harness, ui: &FrontEnd, query: &str) -> String {
    h.query(ui, query)[0].item.title.clone()
}

#[test]
fn three_picks_of_a_result_for_a_query_make_it_that_querys_learned_choice() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, launcher_apps());
    let ui = h.front_end();
    // Files is picked the most, so Frecency alone keeps it first for "fi". It
    // is picked after a query that teaches "fi" nothing.
    for _ in 0..5 {
        pick(&mut h, &ui, "fles", "apps", "files");
    }
    assert_eq!(first(&mut h, &ui, "fi"), "Files");

    pick(&mut h, &ui, "fi", "apps", "firefox");
    pick(&mut h, &ui, "fi", "apps", "firefox");
    assert_eq!(first(&mut h, &ui, "fi"), "Files", "two picks teach nothing");

    pick(&mut h, &ui, "fi", "apps", "firefox");
    assert_eq!(first(&mut h, &ui, "fi"), "Firefox");
    assert_eq!(
        first(&mut h, &ui, " fi "),
        "Firefox",
        "spacing doesn't matter"
    );
    assert_eq!(first(&mut h, &ui, "fir"), "Firefox", "only Firefox matches");
    assert_eq!(
        first(&mut h, &ui, "f"),
        "Files",
        "learned 5 times for \"fles\", which starts with \"f\" too"
    );
    assert_eq!(
        first(&mut h, &ui, "fl"),
        "Files",
        "a query that isn't a prefix learns nothing"
    );
}

#[test]
fn two_picks_of_another_result_replace_a_learned_choice() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, launcher_apps());
    let ui = h.front_end();
    for _ in 0..5 {
        pick(&mut h, &ui, "fles", "apps", "files");
    }
    for _ in 0..3 {
        pick(&mut h, &ui, "fi", "apps", "firefox");
    }
    assert_eq!(first(&mut h, &ui, "fi"), "Firefox");

    pick(&mut h, &ui, "fi", "apps", "fish");
    assert_eq!(first(&mut h, &ui, "fi"), "Firefox", "one stray pick");
    pick(&mut h, &ui, "fi", "apps", "fish");
    // Files was picked more than either, so only a Learned choice puts Fish
    // first.
    assert_eq!(first(&mut h, &ui, "fi"), "Fish");
}

#[test]
fn frecency_breaks_a_tie_and_a_pick_only_ever_weakens() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, launcher_apps());
    let ui = h.front_end();
    assert_eq!(
        first(&mut h, &ui, "fi"),
        "Fish",
        "the shortest title wins a tie"
    );

    pick(&mut h, &ui, "firefox", "apps", "firefox");
    assert_eq!(first(&mut h, &ui, "fi"), "Firefox");

    h.advance(Duration::from_secs(7 * 24 * 60 * 60));
    assert_eq!(first(&mut h, &ui, "fi"), "Firefox", "a week later");
    h.advance(Duration::from_secs(60 * 24 * 60 * 60));
    assert_eq!(first(&mut h, &ui, "fi"), "Firefox", "two months later");

    pick(&mut h, &ui, "fish", "apps", "fish");
    assert_eq!(
        first(&mut h, &ui, "fi"),
        "Fish",
        "a recent pick counts more"
    );
}

#[test]
fn frecency_and_learned_choices_apply_to_live_query_results() {
    let mut h = Harness::new(vec![web()]);
    let web = h.up(0);
    let ui = h.front_end();
    let search = |h: &mut Harness| -> Vec<String> {
        h.query(&ui, "g rust");
        h.advance(Duration::from_millis(100));
        let (id, ..) = live_queries(&web)[0].clone();
        h.send(
            &web,
            FromExtension::LiveResults {
                live_query: id,
                results: vec![
                    live("Rust", 0.9, false),
                    live("Rust lang", 0.5, false),
                    live("Rustup", 0.4, false),
                ],
                done: true,
            },
        );
        titles(&ui.results().unwrap())
            .into_iter()
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(search(&mut h), ["Rust", "Rust lang", "Rustup"]);
    run(&mut h, &ui, "web", "Rustup", "web", "open");
    assert_eq!(
        search(&mut h),
        ["Rust", "Rustup", "Rust lang"],
        "Frecency breaks the tie, under the exact match"
    );
    run(&mut h, &ui, "web", "Rustup", "web", "open");
    run(&mut h, &ui, "web", "Rustup", "web", "open");
    assert_eq!(
        search(&mut h),
        ["Rustup", "Rust", "Rust lang"],
        "the Learned choice, picked over the exact match"
    );
}

/// The same URL, as `bookmarks` or `visited` offers it.
fn url(id: &str, title: &str, url: &str) -> Item {
    let mut item = item(id, title, ItemKind::Url { url: url.into() });
    item.identity = Some(format!("url:{url}"));
    item
}

/// Two Extensions that offer the same thing, and a Front end.
fn bookmarks_and_visited() -> (Harness, Ext, Ext, FrontEnd) {
    let mut h = Harness::new(vec![plain("apps"), plain("bookmarks"), plain("visited")]);
    let apps = h.up(0);
    let bookmarks = h.up(1);
    let visited = h.up(2);
    h.add(&apps, vec![app("Git")]);
    h.add(
        &bookmarks,
        vec![
            url("gh", "GitHub", "https://github.com"),
            url("gitea", "Gitea", "https://gitea.com"),
        ],
    );
    h.add(
        &visited,
        vec![
            url("1", "github.com", "https://github.com"),
            url("2", "GitLab", "https://gitlab.com"),
        ],
    );
    let ui = h.front_end();
    (h, bookmarks, visited, ui)
}

#[test]
fn a_pick_through_one_extension_raises_the_same_thing_from_another() {
    let (mut h, _bookmarks, _visited, ui) = bookmarks_and_visited();
    pick(&mut h, &ui, "hub", "bookmarks", "gh");
    let rows = h.query(&ui, "git");
    assert_eq!(
        titles(&rows)[..3],
        ["Git", "GitHub", "github.com"],
        "under the exact title, both Results of the picked thing, the one picked through first"
    );

    pick(&mut h, &ui, "hub", "visited", "1");
    pick(&mut h, &ui, "hub", "visited", "1");
    let rows = h.query(&ui, "git");
    assert_eq!(titles(&rows)[..3], ["Git", "github.com", "GitHub"]);
}

#[test]
fn the_result_picked_through_leads_even_when_the_other_matches_better() {
    let (mut h, _bookmarks, _visited, ui) = bookmarks_and_visited();
    assert_eq!(titles(&h.query(&ui, "github")), ["GitHub", "github.com"]);
    pick(&mut h, &ui, "hub", "visited", "1");
    assert_eq!(
        titles(&h.query(&ui, "github")),
        ["github.com", "GitHub"],
        "the thing ranks by its exact title, and leads with the way the user goes to it"
    );
}

#[test]
fn a_learned_choice_follows_the_same_thing_to_another_extension() {
    let (mut h, bookmarks, _visited, ui) = bookmarks_and_visited();
    for _ in 0..3 {
        pick(&mut h, &ui, "git", "bookmarks", "gh");
    }
    assert_eq!(first(&mut h, &ui, "git"), "GitHub");

    // The bookmark goes; the page is still in the visited ones.
    h.send(
        &bookmarks,
        FromExtension::RemoveItems {
            ids: vec!["gh".into()],
        },
    );
    assert_eq!(
        first(&mut h, &ui, "git"),
        "github.com",
        "the Learned choice, picked over the exact title"
    );
}

#[test]
fn the_empty_query_shows_the_same_thing_once_by_the_result_picked_through_most() {
    let (mut h, _bookmarks, _visited, ui) = bookmarks_and_visited();
    pick(&mut h, &ui, "hub", "bookmarks", "gh");
    pick(&mut h, &ui, "hub", "visited", "1");
    pick(&mut h, &ui, "hub", "visited", "1");
    pick(&mut h, &ui, "lab", "visited", "2");
    assert_eq!(titles(&h.query(&ui, "")), ["github.com", "GitLab"]);
}

#[test]
fn the_empty_query_is_ranked_again_when_the_menu_opens() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Mail"), app("Music")]);
    let menu = h.front_end();
    let command = h.front_end();
    for _ in 0..3 {
        h.advance_to(8);
        pick(&mut h, &menu, "mail", "apps", "mail");
        h.advance_to(20);
        pick(&mut h, &menu, "music", "apps", "music");
    }
    h.advance_to(8);
    assert_eq!(titles(&h.query(&menu, "")), ["Mail", "Music"]);

    h.advance_to(20);
    h.front_end_sends(&command, FromFrontEnd::Toggle);
    assert_eq!(titles(&menu.results().unwrap()), ["Music", "Mail"]);
    assert_eq!(command.drain(), [], "it never searched");
}

#[test]
fn the_empty_query_shows_a_result_once_its_extension_sends_it() {
    let mut h = Harness::with_config(
        vec![plain("apps")],
        config("[empty_query]\nmenu = [{ extension = \"apps\", item = \"mail\" }]"),
    );
    let ui = h.front_end();
    assert_eq!(h.query(&ui, ""), []);
    let apps = h.up(0);
    h.add(&apps, vec![app("Mail")]);
    assert_eq!(titles(&ui.results().unwrap()), ["Mail"]);
}

#[test]
fn a_pick_of_a_live_query_result_raises_the_same_thing_indexed() {
    let mut h = Harness::new(vec![plain("bookmarks"), web()]);
    let bookmarks = h.up(0);
    let web = h.up(1);
    h.add(
        &bookmarks,
        vec![
            url("rustup", "Rustup", "https://rustup.rs"),
            url("rust", "Rust lang", "https://rust-lang.org"),
        ],
    );
    let ui = h.front_end();
    assert_eq!(titles(&h.query(&ui, "rust")), ["Rustup", "Rust lang"]);

    h.query(&ui, "g rust");
    h.advance(Duration::from_millis(100));
    let (id, ..) = live_queries(&web)[0].clone();
    let mut result = live("Rust lang", 0.9, false);
    result.item = url("r1", "rust-lang.org", "https://rust-lang.org");
    h.send(
        &web,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![result],
            done: true,
        },
    );
    run(&mut h, &ui, "web", "r1", "web", "open");
    ui.drain();

    assert_eq!(titles(&h.query(&ui, "rust")), ["Rust lang", "Rustup"]);
}

#[test]
fn the_history_is_saved_after_each_pick_and_a_new_daemon_ranks_by_it() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, launcher_apps());
    let ui = h.front_end();
    for _ in 0..3 {
        pick(&mut h, &ui, "fi", "apps", "files");
    }
    let saved = h.saved.0.borrow().clone().expect("saved after a pick");

    let mut h = Harness::with_history(vec![plain("apps")], Table::new(), saved);
    let apps = h.up(0);
    h.add(&apps, launcher_apps());
    let ui = h.front_end();
    assert_eq!(first(&mut h, &ui, "fi"), "Files");
}

#[test]
fn the_clear_history_action_forgets_every_pick() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, launcher_apps());
    let ui = h.front_end();
    for _ in 0..3 {
        pick(&mut h, &ui, "fi", "apps", "firefox");
    }
    assert_eq!(first(&mut h, &ui, "fi"), "Firefox");

    let row = h.query(&ui, "clear history")[0].clone();
    assert_eq!(
        (row.extension.as_str(), row.item.id.as_str()),
        ("kotodama", "clear-history")
    );
    run(&mut h, &ui, "kotodama", "clear-history", "kotodama", "run");
    assert!(ui.drain().contains(&ToFrontEnd::Close));
    assert!(h.saved.0.borrow().as_ref().unwrap().is_empty());
    assert_eq!(first(&mut h, &ui, "fi"), "Fish");
}

/// What the Daemon put in the usage log, without the times.
fn logged(h: &Harness) -> Vec<Used> {
    h.saved
        .1
        .borrow()
        .iter()
        .map(|entry| entry.used.clone())
        .collect()
}

#[test]
fn with_the_usage_log_on_every_query_and_pick_is_logged() {
    let mut h = Harness::with_config(vec![plain("apps")], config("[history]\nusage_log = true"));
    let apps = h.up(0);
    h.add(&apps, launcher_apps());
    let ui = h.front_end();
    h.query(&ui, "f");
    let rows = h.query(&ui, "fi");
    let row = rows.iter().position(|row| row.item.id == "firefox");
    run(&mut h, &ui, "apps", "firefox", "apps", "open");
    h.query(&ui, "");

    assert_eq!(
        logged(&h),
        [
            Used::Query { query: "f".into() },
            Used::Query { query: "fi".into() },
            Used::Pick {
                query: "fi".into(),
                extension: "apps".into(),
                id: "firefox".into(),
                action: "open".into(),
                row,
            },
        ]
    );
}

#[test]
fn the_usage_log_is_off_until_the_config_turns_it_on() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, launcher_apps());
    let ui = h.front_end();
    pick(&mut h, &ui, "fi", "apps", "firefox");
    assert_eq!(logged(&h), []);

    h.event(Event::Config(config("[history]\nusage_log = true")));
    h.query(&ui, "fis");
    h.event(Event::Config(config("")));
    h.query(&ui, "fish");
    assert_eq!(
        logged(&h),
        [Used::Query {
            query: "fis".into()
        }]
    );
}

#[test]
fn no_extension_may_take_the_daemons_own_name() {
    let manifest = Manifest::from_toml("name = \"kotodama\"\ncommand = [\"x\"]").unwrap();
    assert!(Found::new(manifest, PathBuf::from("/ext")).is_err());
}

#[test]
fn an_alias_puts_its_result_first_and_follows_config_reloads() {
    let mut h = Harness::with_config(
        vec![plain("apps")],
        config("[aliases]\nweb = { extension = \"apps\", item = \"firefox\" }"),
    );
    let apps = h.up(0);
    h.add(&apps, launcher_apps());
    let ui = h.front_end();
    assert_eq!(
        first(&mut h, &ui, "web"),
        "Firefox",
        "even though it doesn't match"
    );
    assert_eq!(first(&mut h, &ui, "WEB"), "Firefox");

    h.event(Event::Config(config(
        "[aliases]\nweb = { extension = \"apps\", item = \"files\" }",
    )));
    assert_eq!(first(&mut h, &ui, "web"), "Files");

    h.event(Event::Config(Table::new()));
    assert_eq!(h.query(&ui, "web"), []);
}

#[test]
fn an_alias_for_an_action_makes_it_primary() {
    let mut h = Harness::with_config(
        vec![plain("apps")],
        config(
            "[aliases]\npriv = { extension = \"apps\", item = \"firefox\", action = \"private\" }",
        ),
    );
    let apps = h.up(0);
    let mut firefox = app("Firefox");
    firefox.actions.push(action("private"));
    h.add(&apps, vec![firefox]);
    let ui = h.front_end();

    let row = &h.query(&ui, "priv")[0];
    let actions: Vec<&str> = row.item.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(actions, ["private", "open"]);
    let row = &h.query(&ui, "firefox")[0];
    assert_eq!(row.item.actions[0].id, "open", "only the Alias changes it");
}

#[test]
fn a_quicklink_keyword_takes_the_rest_of_the_query_into_its_template() {
    let mut h = Harness::with_config(
        vec![plain("apps")],
        config(
            "[quicklinks.yt]\ntitle = \"Search YouTube\"\n\
             url = \"https://www.youtube.com/results?search_query={query}\"\n\
             [quicklinks.logs]\ncommand = [\"journalctl\", \"-u\", \"{query}\"]\n\
             [quicklinks.gh]\nurl = \"https://github.com\"",
        ),
    );
    let apps = h.up(0);
    h.add(&apps, vec![app("Yt cats and dogs player")]);
    let ui = h.front_end();

    let rows = h.query(&ui, "yt cats & dogs/ü");
    assert_eq!(
        (rows[0].extension.as_str(), rows[0].item.title.as_str()),
        ("kotodama", "Search YouTube for “cats & dogs/ü”")
    );
    run(&mut h, &ui, "kotodama", "quicklink:yt", "kotodama", "open");
    assert!(ui.drain().contains(&ToFrontEnd::Close));

    h.query(&ui, "logs kotodamad.service");
    run(
        &mut h,
        &ui,
        "kotodama",
        "quicklink:logs",
        "kotodama",
        "open",
    );

    h.query(&ui, "gh");
    run(&mut h, &ui, "kotodama", "quicklink:gh", "kotodama", "open");
    assert_eq!(
        *h.outputs.1.borrow(),
        [
            vec![
                "xdg-open".to_owned(),
                "https://www.youtube.com/results?search_query=cats%20%26%20dogs%2F%C3%BC".into()
            ],
            vec!["journalctl".into(), "-u".into(), "kotodamad.service".into()],
            vec!["xdg-open".into(), "https://github.com".into()],
        ]
    );
    assert_eq!(
        titles(&h.query(&ui, "gh extra")),
        ["gh"],
        "a Quicklink without a query ignores more words"
    );
}

#[test]
fn quicklinks_follow_config_reloads() {
    let mut h = Harness::new(vec![]);
    let ui = h.front_end();
    assert_eq!(h.query(&ui, "gh"), []);

    h.event(Event::Config(config(
        "[quicklinks.gh]\nurl = \"https://github.com\"",
    )));
    assert_eq!(first(&mut h, &ui, "gh"), "gh", "titled by its keyword");

    h.event(Event::Config(config(
        "[quicklinks.gh]\ntitle = \"GitHub\"\nurl = \"https://github.com\"",
    )));
    assert_eq!(first(&mut h, &ui, "gh"), "GitHub");
}

#[test]
fn picks_of_a_result_count_towards_a_learned_choice_even_when_others_come_between() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, launcher_apps());
    let ui = h.front_end();
    for _ in 0..5 {
        pick(&mut h, &ui, "fles", "apps", "files");
    }
    pick(&mut h, &ui, "fi", "apps", "firefox");
    pick(&mut h, &ui, "fi", "apps", "fish");
    pick(&mut h, &ui, "fi", "apps", "firefox");
    assert_eq!(first(&mut h, &ui, "fi"), "Files");
    pick(&mut h, &ui, "fi", "apps", "firefox");
    assert_eq!(first(&mut h, &ui, "fi"), "Firefox");
}

#[test]
fn a_reload_never_leaves_a_row_showing_another_quicklink() {
    let mut h = Harness::with_config(
        vec![],
        config("[quicklinks.gh]\ntitle = \"GitHub\"\nurl = \"https://github.com\""),
    );
    let ui = h.front_end();
    assert_eq!(first(&mut h, &ui, "gh"), "GitHub");

    // "aa" sorts first, so it takes the place "gh" had among the Daemon's own
    // Items.
    h.event(Event::Config(config(
        "[quicklinks.aa]\ntitle = \"GitHub issues\"\nurl = \"https://github.com/issues\"\n\
         [quicklinks.gh]\ntitle = \"GitHub\"\nurl = \"https://github.com\"",
    )));
    assert_eq!(titles(&ui.results().unwrap())[0], "GitHub");
}
