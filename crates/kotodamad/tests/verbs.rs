//! Verbs: a word of the query that names one of an Action's `keywords`
//! picks the Action Enter runs on the Results the rest of the query finds,
//! as `remove chrome` makes Remove the primary Action of Google Chrome.

#[path = "support/harness.rs"]
#[expect(dead_code, reason = "each test uses part of the harness")]
mod harness;

use harness::*;
use kotodama_protocol::*;
use kotodamad::discovery::Found;
use kotodamad::history::History;
use kotodamad::history::IDENTITIES;

/// An Extension that offers `remove` on the Items it names, as a package
/// manager offers to uninstall the apps it installed.
fn packages() -> Found {
    manifest(
        "name = \"packages\"\ncommand = [\"x\"]\n\
         [[contributed_actions]]\ntargeted = true\nid = \"remove\"\ntitle = \"Remove\"\n\
         keywords = [\"remove\", \"uninstall\"]\nrole = \"destructive\"",
    )
}

fn identified(title: &str) -> Item {
    let mut item = app(title);
    item.identity = Some(format!("app:{}.desktop", item.id));
    item
}

/// A desktop with Google Chrome, Chromium and Firefox installed through the
/// package manager, and an app whose title starts with `remove`.
struct Desk {
    h: Harness,
    packages: Ext,
    apps: Ext,
    ui: FrontEnd,
}

impl Desk {
    fn new() -> Self {
        Self::with_config("")
    }

    fn with_config(toml: &str) -> Self {
        Self::with_history(toml, History::default())
    }

    fn with_history(toml: &str, history: History) -> Self {
        let mut h = Harness::with_history(vec![packages(), plain("apps")], config(toml), history);
        let packages = h.up(0);
        let apps = h.up(1);
        h.add(
            &apps,
            vec![
                identified("Google Chrome"),
                identified("Chromium"),
                identified("Firefox"),
                identified("Remove Background"),
                identified("Chrome Remote Desktop"),
            ],
        );
        let ui = h.front_end();
        let mut desk = Desk { h, packages, apps, ui };
        desk.target(&["google-chrome", "chromium", "firefox"]);
        desk
    }

    /// Offers `remove` on the apps `ids` alone.
    fn target(&mut self, ids: &[&str]) {
        let identities = ids.iter().map(|id| format!("app:{id}.desktop")).collect();
        self.h.send(
            &self.packages,
            FromExtension::TargetAction {
                action: "remove".into(),
                identities,
            },
        );
    }

    fn query(&mut self, text: &str) -> Vec<ShownResult> {
        self.h.query(&self.ui, text)
    }
}

fn remove() -> ActionRef {
    ActionRef {
        extension: "packages".into(),
        id: "remove".into(),
    }
}

fn primaries(rows: &[ShownResult]) -> Vec<(&str, Option<&str>)> {
    rows.iter()
        .map(|row| {
            (
                row.item.title.as_str(),
                row.primary.as_ref().map(|action| action.id.as_str()),
            )
        })
        .collect()
}

#[test]
fn a_verb_before_or_after_the_subject_makes_its_action_primary() {
    let mut desk = Desk::new();

    for query in ["remove chrome", "chrome remove", "uninstall chrome"] {
        let rows = desk.query(query);

        assert_eq!(rows[0].item.title, "Google Chrome", "{query}");
        assert_eq!(rows[0].primary, Some(remove()), "{query}");
    }
}

#[test]
fn three_letters_of_a_verb_name_it_and_two_do_not() {
    let mut desk = Desk::new();

    let rows = desk.query("rem chrome");
    let chrome = rows.iter().find(|row| row.item.title == "Google Chrome").unwrap();
    assert_eq!(chrome.primary, Some(remove()));

    let rows = desk.query("re chrome");
    assert!(rows.iter().all(|row| row.primary.is_none()), "{:?}", primaries(&rows));
}

#[test]
fn a_verb_alone_chooses_nothing_and_leaves_the_search_as_it_was() {
    let mut desk = Desk::new();

    let rows = desk.query("remove");

    assert_eq!(rows[0].item.title, "Remove Background");
    for removable in ["Google Chrome", "Chromium", "Firefox"] {
        assert!(!titles(&rows).contains(&removable), "{:?}", titles(&rows));
    }
    assert!(rows.iter().all(|row| row.primary.is_none()));
}

#[test]
fn the_subject_ranks_as_it_would_typed_alone() {
    let mut desk = Desk::new();
    // Chromium is used, through a query that teaches nothing about `chrom`.
    desk.query("ium");
    for _ in 0..5 {
        run(&mut desk.h, &desk.ui, "apps", "chromium", "apps", "open");
    }

    let removable = |row: &&ShownResult| row.item.title != "Chrome Remote Desktop";
    let alone = desk.query("chrom");
    let alone: Vec<&str> = alone
        .iter()
        .filter(removable)
        .map(|row| row.item.title.as_str())
        .collect();
    let with_verb = desk.query("remove chrom");
    let chosen: Vec<&str> = with_verb
        .iter()
        .filter(|row| row.primary == Some(remove()))
        .map(|row| row.item.title.as_str())
        .collect();

    assert_eq!(alone.len(), 2);
    assert_eq!(chosen, alone);
    assert_eq!(&titles(&with_verb)[..2], alone);
}

#[test]
fn a_result_that_doesnt_offer_the_verb_gets_no_primary_from_it() {
    let mut desk = Desk::new();

    let rows = desk.query("remove chrom");

    assert!(
        rows.iter()
            .filter(|row| row.item.title == "Chrome Remote Desktop")
            .all(|row| row.primary.is_none()),
        "{:?}",
        primaries(&rows)
    );
}

#[test]
fn the_alias_picks_the_item_and_the_verb_picks_the_action() {
    let mut desk = Desk::with_config("[aliases]\nff = { extension = \"apps\", item = \"firefox\" }");

    for query in ["remove ff", "ff remove", "rem ff"] {
        let rows = desk.query(query);

        assert_eq!(rows[0].item.title, "Firefox", "{query}");
        assert_eq!(rows[0].alias.as_deref(), Some("ff"), "{query}");
        assert_eq!(rows[0].primary, Some(remove()), "{query}");
    }
}

#[test]
fn a_verb_on_an_items_own_action_chooses_it() {
    let mut desk = Desk::new();
    let mut notes = app("Notes");
    notes.actions = vec![
        Action::new("open", "Open").keywords(["open"]),
        Action::new("trash", "Move to Trash").keywords(["delete", "trash"]),
    ];
    desk.h.add(&desk.apps, vec![notes]);

    let rows = desk.query("delete notes");
    assert_eq!(rows[0].item.title, "Notes");
    assert_eq!(
        rows[0].primary,
        Some(ActionRef {
            extension: "apps".into(),
            id: "trash".into(),
        })
    );

    let rows = desk.query("open notes");
    assert_eq!(rows[0].item.title, "Notes");
    assert_eq!(rows[0].primary, None, "the usual first Action is no choice");
}

#[test]
fn a_verb_isnt_a_word_of_an_actions_title() {
    let mut desk = Desk::new();
    let mut notes = app("Notes");
    notes.actions = vec![Action::new("open", "Open"), Action::new("trash", "Move to Trash")];
    desk.h.add(&desk.apps, vec![notes]);

    let rows = desk.query("trash notes");

    assert!(rows.iter().all(|row| row.primary.is_none()), "{:?}", primaries(&rows));
}

#[test]
fn a_pick_through_a_verb_is_learned_for_the_query_and_adds_no_frecency() {
    let mut desk = Desk::new();

    for _ in 0..3 {
        desk.query("remove chrome");
        run(&mut desk.h, &desk.ui, "apps", "google-chrome", "packages", "remove");
    }

    let history = desk.h.saved.0.borrow().clone().expect("the History was saved");
    let learned: Vec<(&str, &str)> = history
        .learned("remove chrome", u64::MAX)
        .iter()
        .map(|choice| (choice.extension, choice.id))
        .collect();
    assert_eq!(learned, [(IDENTITIES, "app:google-chrome.desktop")]);
    assert!(history.picks(IDENTITIES, "app:google-chrome.desktop").is_none());
    assert!(history.picks("apps", "google-chrome").is_none());
}

#[test]
fn another_action_picked_on_a_result_the_verb_found_counts_as_any_pick() {
    let mut desk = Desk::new();
    desk.query("remove chrome");

    run(&mut desk.h, &desk.ui, "apps", "google-chrome", "apps", "open");

    let history = desk.h.saved.0.borrow().clone().expect("the History was saved");
    assert!(history.picks(IDENTITIES, "app:google-chrome.desktop").is_some());
}

#[test]
fn the_open_menu_drops_a_primary_its_result_no_longer_offers() {
    let mut desk = Desk::new();
    desk.query("remove chrome");

    desk.target(&["firefox"]);
    let rows = desk.ui.results().expect("the menu is told the Actions changed");

    let chrome = rows.iter().find(|row| row.item.title == "Google Chrome").unwrap();
    assert_eq!(chrome.primary, None);
}
