//! Placeholders: an Item that `yields` stands for a thing until an Item of
//! its Identity that doesn't yield is indexed, from any Extension, and is
//! then hidden entirely, as an app not installed yet gives way to the app.

#[path = "support/harness.rs"]
#[expect(dead_code, reason = "each test uses part of the harness")]
mod harness;

use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use harness::*;
use kotodama_protocol::*;
use kotodamad::daemon::Event;

const CHROME: &str = "app:google-chrome.desktop";

/// What an OS offers while Google Chrome isn't installed: Enter installs it,
/// then opens it.
fn placeholder(id: &str) -> Item {
    let mut item = item(
        id,
        "Google Chrome",
        ItemKind::App {
            desktop_id: "google-chrome.desktop".into(),
        },
    );
    item.identity = Some(CHROME.into());
    item.yields = true;
    item.actions = vec![Action::new("install", "Install and open").keywords(["install"])];
    item
}

/// Google Chrome, installed.
fn chrome() -> Item {
    let mut item = app("Google Chrome");
    item.identity = Some(CHROME.into());
    item
}

fn chromium() -> Item {
    let mut item = app("Chromium");
    item.identity = Some("app:chromium.desktop".into());
    item
}

/// Each row's Extension and Item id.
fn shown(rows: &[ShownResult]) -> Vec<(&str, &str)> {
    rows.iter()
        .map(|row| (row.extension.as_str(), row.item.id.as_str()))
        .collect()
}

fn remove(ids: &[&str]) -> FromExtension {
    FromExtension::RemoveItems {
        ids: ids.iter().map(|&id| id.to_owned()).collect(),
    }
}

/// An OS Extension with the placeholder, and the apps Extension with
/// Chromium.
fn desk() -> (Harness, Ext, Ext, FrontEnd) {
    let mut h = Harness::new(vec![plain("os"), plain("apps")]);
    let os = h.up(0);
    let apps = h.up(1);
    h.add(&os, vec![placeholder("chrome")]);
    h.add(&apps, vec![chromium()]);
    let ui = h.front_end();
    (h, os, apps, ui)
}

#[test]
fn a_placeholder_shows_until_an_item_with_its_identity_is_indexed_and_comes_back_once_it_goes() {
    let (mut h, _os, apps, ui) = desk();
    assert_eq!(shown(&h.query(&ui, "google")), [("os", "chrome")]);

    h.add(&apps, vec![chrome()]);
    assert_eq!(shown(&h.query(&ui, "google")), [("apps", "google-chrome")]);

    h.send(&apps, remove(&["google-chrome"]));
    assert_eq!(shown(&h.query(&ui, "google")), [("os", "chrome")]);
}

#[test]
fn a_placeholder_that_stepped_aside_offers_none_of_its_actions() {
    let (mut h, _os, apps, ui) = desk();
    h.add(&apps, vec![chrome()]);

    let rows = h.query(&ui, "install google");

    assert!(rows.iter().all(|row| row.extension != "os"), "{:?}", shown(&rows));
    assert!(rows.iter().all(|row| row.primary.is_none()), "{:?}", shown(&rows));
}

#[test]
fn what_was_picked_through_the_placeholder_counts_for_the_real_item() {
    let (mut h, _os, apps, ui) = desk();
    assert_eq!(
        shown(&h.query(&ui, "chr"))[0],
        ("apps", "chromium"),
        "with nothing learned"
    );
    for _ in 0..3 {
        h.query(&ui, "chr");
        run(&mut h, &ui, "os", "chrome", "os", "install");
    }
    assert_eq!(shown(&h.query(&ui, "chr"))[0], ("os", "chrome"));

    h.add(&apps, vec![chrome()]);
    let rows = h.query(&ui, "chr");

    assert_eq!(shown(&rows)[..2], [("apps", "google-chrome"), ("apps", "chromium")]);
    assert!(rows.iter().all(|row| row.extension != "os"), "{:?}", shown(&rows));
}

#[test]
fn the_empty_query_shows_the_real_item_in_place_of_the_placeholder_picked() {
    let (mut h, _os, apps, ui) = desk();
    for _ in 0..2 {
        h.query(&ui, "google");
        run(&mut h, &ui, "os", "chrome", "os", "install");
    }
    assert!(shown(&h.query(&ui, "")).contains(&("os", "chrome")));

    h.add(&apps, vec![chrome()]);
    let rows = h.query(&ui, "");

    assert!(rows.iter().all(|row| row.extension != "os" || row.item.id != "chrome"));
    assert_eq!(shown(&rows)[0], ("apps", "google-chrome"));
}

#[test]
fn an_item_that_yields_with_no_identity_to_yield_to_doesnt_decode() {
    let message = br#"{"add_items": {"items": [{"id": "chrome", "kind": "app",
        "desktop_id": "google-chrome.desktop", "title": "Google Chrome", "yields": true,
        "actions": [{"id": "install", "title": "Install and open"}]}]}}"#;

    let err = WireFormat::Json.decode::<FromExtension>(message).unwrap_err();

    assert!(format!("{err:?}").contains("identity"), "{err:?}");
}

#[test]
fn two_placeholders_of_one_identity_both_show_until_the_real_item_comes() {
    let mut h = Harness::new(vec![plain("os"), plain("store"), plain("apps")]);
    let os = h.up(0);
    let store = h.up(1);
    let apps = h.up(2);
    h.add(&os, vec![placeholder("chrome")]);
    h.add(&store, vec![placeholder("google-chrome")]);
    let ui = h.front_end();

    let rows = h.query(&ui, "google");
    let mut extensions: Vec<&str> = rows.iter().map(|row| row.extension.as_str()).collect();
    extensions.sort_unstable();
    assert_eq!(extensions, ["os", "store"]);

    h.add(&apps, vec![chrome()]);
    assert_eq!(shown(&h.query(&ui, "google")), [("apps", "google-chrome")]);
}

#[test]
fn a_placeholder_yields_to_the_real_item_from_its_own_extension_whichever_comes_first() {
    let mut h = Harness::new(vec![plain("os")]);
    let os = h.up(0);
    let ui = h.front_end();
    let installed = Item {
        id: "chrome-installed".into(),
        ..chrome()
    };

    h.add(&os, vec![placeholder("chrome"), installed.clone()]);
    assert_eq!(shown(&h.query(&ui, "google")), [("os", "chrome-installed")]);

    h.send(&os, remove(&["chrome-installed"]));
    assert_eq!(shown(&h.query(&ui, "google")), [("os", "chrome")]);

    h.add(&os, vec![installed, placeholder("chrome")]);
    assert_eq!(shown(&h.query(&ui, "google")), [("os", "chrome-installed")]);
}

#[test]
fn an_item_sent_again_as_a_placeholder_steps_aside_and_one_that_stops_yielding_counts() {
    let mut h = Harness::new(vec![plain("os"), plain("apps")]);
    let os = h.up(0);
    let apps = h.up(1);
    let ui = h.front_end();
    h.add(&apps, vec![chrome()]);
    let installed = Item {
        id: "chrome".into(),
        ..chrome()
    };
    h.add(&os, vec![installed.clone()]);
    assert_eq!(shown(&h.query(&ui, "google")).len(), 2);

    h.add(&os, vec![placeholder("chrome")]);
    assert_eq!(shown(&h.query(&ui, "google")), [("apps", "google-chrome")]);

    h.add(&os, vec![installed]);
    h.send(&apps, remove(&["google-chrome"]));
    assert_eq!(shown(&h.query(&ui, "google")), [("os", "chrome")]);
    assert!(!h.query(&ui, "google")[0].item.yields);
}

#[test]
fn the_open_menu_swaps_the_placeholder_for_the_real_item_and_back() {
    let (mut h, _os, apps, ui) = desk();
    h.query(&ui, "google");

    h.add(&apps, vec![chrome()]);
    let rows = ui.results().expect("the menu is told the Items changed");
    assert_eq!(shown(&rows), [("apps", "google-chrome")]);

    h.send(&apps, remove(&["google-chrome"]));
    let rows = ui.results().expect("the menu is told the Items changed");
    assert_eq!(shown(&rows), [("os", "chrome")]);
}

#[test]
fn the_real_item_sent_again_with_its_actions_reordered_only_sends_the_rows_again() {
    let (mut h, os, apps, ui) = desk();
    let mut real = chrome();
    real.actions = vec![Action::new("open", "Open"), Action::new("window", "New window")];
    h.add(&apps, vec![real.clone()]);
    h.query(&ui, "google");

    real.actions.reverse();
    h.add(&apps, vec![real]);
    let rows = ui.results().expect("the menu is told the Item changed");
    assert_eq!(shown(&rows), [("apps", "google-chrome")]);
    assert_eq!(rows[0].item.actions[0].id, "window");
    assert_eq!(shown(&h.query(&ui, "google ")), [("apps", "google-chrome")]);

    h.add(&os, vec![placeholder("chrome")]);
    assert_eq!(
        ui.results(),
        None,
        "the placeholder sent again changes nothing on screen"
    );
}

#[test]
fn a_hidden_item_with_the_identity_leaves_the_placeholder_and_a_hidden_placeholder_never_shows() {
    let (mut h, os, apps, ui) = desk();
    let hidden = Item {
        hidden: true,
        ..chrome()
    };

    h.add(&apps, vec![hidden]);
    assert_eq!(shown(&h.query(&ui, "google")), [("os", "chrome")]);

    let hidden = Item {
        hidden: true,
        ..placeholder("chrome")
    };
    h.add(&os, vec![hidden]);
    assert_eq!(shown(&h.query(&ui, "google")), []);
}

#[test]
fn an_item_only_its_app_shows_still_makes_the_placeholder_yield() {
    let (mut h, _os, apps, ui) = desk();
    let in_app = Item {
        app_only: true,
        ..chrome()
    };

    h.add(&apps, vec![in_app]);

    assert_eq!(shown(&h.query(&ui, "google")), []);
}

#[test]
fn the_placeholder_comes_back_when_the_real_items_extension_stops() {
    let (mut h, _os, apps, ui) = desk();
    h.add(&apps, vec![chrome()]);

    h.down(&apps);

    assert_eq!(shown(&h.query(&ui, "google")), [("os", "chrome")]);
}

/// Hands the Daemon what the indexing thread sends until it sends nothing
/// for a while.
fn settle(h: &mut Harness, indexed: &Receiver<Event>) {
    while let Ok(event) = indexed.recv_timeout(Duration::from_millis(200)) {
        h.event(event);
    }
}

#[test]
fn a_placeholder_steps_aside_and_comes_back_on_the_indexing_thread() {
    let mut h = Harness::new(vec![plain("os"), plain("apps")]);
    let (events, indexed) = mpsc::channel();
    h.daemon.index_in_background(events);
    let os = h.up(0);
    let apps = h.up(1);
    let ui = h.front_end();
    h.add(&os, vec![placeholder("chrome")]);
    settle(&mut h, &indexed);
    assert_eq!(shown(&h.query(&ui, "google")), [("os", "chrome")]);

    h.add(&apps, vec![chrome()]);
    settle(&mut h, &indexed);
    assert_eq!(shown(&h.query(&ui, "google")), [("apps", "google-chrome")]);

    h.down(&apps);
    settle(&mut h, &indexed);
    assert_eq!(shown(&h.query(&ui, "google")), [("os", "chrome")]);
}
