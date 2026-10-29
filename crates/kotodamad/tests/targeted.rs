//! Targeted Contributed actions, offered on the Results whose Identity the
//! contributing Extension names; the role the Daemon reports for an Action
//! that still needs its Arguments; and an Item resent with its Actions
//! reordered, which the menu shows without searching again.

#[path = "support/harness.rs"]
#[expect(dead_code, reason = "each test uses part of the harness")]
mod harness;

use harness::*;
use kotodama_protocol::*;
use kotodamad::discovery::Found;

/// An Extension that offers `remove` on the Items it names, as a package
/// manager offers to uninstall the apps it installed.
fn packages() -> Found {
    manifest(
        "name = \"packages\"\ncommand = [\"x\"]\n\
         [[contributed_actions]]\ntargeted = true\nid = \"remove\"\ntitle = \"Remove\"\n\
         keywords = [\"remove\", \"uninstall\"]\nrole = \"destructive\"\nconfirm = true",
    )
}

fn identified(title: &str) -> Item {
    let mut item = app(title);
    item.identity = Some(format!("app:{}.desktop", item.id));
    item
}

fn target(identities: &[&str]) -> FromExtension {
    FromExtension::TargetAction {
        action: "remove".into(),
        identities: identities.iter().map(|&identity| identity.to_owned()).collect(),
    }
}

fn contributed(row: &ShownResult) -> Vec<(&str, &str)> {
    row.contributed
        .iter()
        .map(|offered| (offered.extension.as_str(), offered.action.id.as_str()))
        .collect()
}

#[test]
fn a_targeted_action_is_offered_on_the_results_with_an_identity_it_names_and_no_other() {
    let mut h = Harness::new(vec![packages(), plain("apps")]);
    let packages = h.up(0);
    let apps = h.up(1);
    h.add(&apps, vec![identified("Gimp"), identified("Giggle"), app("Gitg")]);
    let ui = h.front_end();

    h.send(&packages, target(&["app:gimp.desktop"]));
    let rows = h.query(&ui, "gi");

    let gimp = rows.iter().find(|row| row.item.id == "gimp").unwrap();
    assert_eq!(contributed(gimp), [("packages", "remove")]);
    assert_eq!(gimp.contributed[0].action.role, ActionRole::Destructive);
    assert_eq!(gimp.contributed[0].action.keywords, ["remove", "uninstall"]);
    for other in rows.iter().filter(|row| row.item.id != "gimp") {
        assert_eq!(contributed(other), [], "{}", other.item.title);
    }
}

#[test]
fn a_targeted_action_is_offered_on_every_result_of_the_identity_whichever_extension_owns_it() {
    let mut h = Harness::new(vec![packages(), plain("apps"), plain("files")]);
    let packages = h.up(0);
    let apps = h.up(1);
    let files = h.up(2);
    h.add(&apps, vec![identified("Gimp")]);
    let mut desktop_file = item(
        "gimp-file",
        "Gimp",
        ItemKind::File {
            path: "/a/gimp.desktop".into(),
        },
    );
    desktop_file.identity = Some("app:gimp.desktop".into());
    h.add(&files, vec![desktop_file]);
    let ui = h.front_end();

    h.send(&packages, target(&["app:gimp.desktop"]));
    let rows = h.query(&ui, "gimp");

    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(contributed(row), [("packages", "remove")], "{}", row.extension);
    }
}

#[test]
fn each_target_action_replaces_the_set_before_and_the_open_menu_follows() {
    let mut h = Harness::new(vec![packages(), plain("apps")]);
    let packages = h.up(0);
    let apps = h.up(1);
    h.add(&apps, vec![identified("Gimp"), identified("Giggle")]);
    let ui = h.front_end();
    h.send(&packages, target(&["app:gimp.desktop"]));
    h.query(&ui, "gi");

    h.send(&packages, target(&["app:giggle.desktop"]));
    let rows = ui.results().expect("the menu is told the Actions changed");

    let offered: Vec<_> = rows
        .iter()
        .map(|row| (row.item.id.as_str(), row.contributed.len()))
        .collect();
    assert!(offered.contains(&("gimp", 0)), "{offered:?}");
    assert!(offered.contains(&("giggle", 1)), "{offered:?}");

    h.send(&packages, target(&[]));
    let rows = ui.results().expect("the menu is told the Actions changed");
    assert!(rows.iter().all(|row| row.contributed.is_empty()));
}

#[test]
fn a_targeted_action_runs_in_the_extension_that_offers_it_on_the_item_alone() {
    let mut h = Harness::new(vec![packages(), plain("apps")]);
    let packages = h.up(0);
    let apps = h.up(1);
    h.add(&apps, vec![identified("Gimp")]);
    let ui = h.front_end();
    h.send(&packages, target(&["app:gimp.desktop"]));
    h.query(&ui, "gimp");
    packages.drain();

    run(&mut h, &ui, "apps", "gimp", "packages", "remove");

    let runs: Vec<_> = packages
        .drain()
        .into_iter()
        .filter_map(|message| match message {
            ToExtension::RunAction { action, item, .. } => Some((action, item)),
            _ => None,
        })
        .collect();
    assert_eq!(runs, [("remove".to_owned(), identified("Gimp").to_ref())]);
}

#[test]
fn a_targeted_action_isnt_offered_on_items_it_doesnt_name() {
    let mut h = Harness::new(vec![packages(), plain("apps")]);
    let packages = h.up(0);
    let apps = h.up(1);
    h.add(&apps, vec![identified("Gimp"), identified("Giggle")]);
    let ui = h.front_end();
    h.send(&packages, target(&["app:gimp.desktop"]));
    h.query(&ui, "giggle");
    packages.drain();

    run(&mut h, &ui, "apps", "giggle", "packages", "remove");

    assert!(
        packages
            .drain()
            .iter()
            .all(|message| !matches!(message, ToExtension::RunAction { .. }))
    );
    assert!(
        ui.drain()
            .iter()
            .any(|message| matches!(message, ToFrontEnd::ActionFailed { .. }))
    );
}

#[test]
fn an_action_the_manifest_doesnt_declare_targeted_is_refused() {
    let mut h = Harness::new(vec![plain("packages"), plain("apps")]);
    let packages = h.up(0);
    let apps = h.up(1);
    h.add(&apps, vec![identified("Gimp")]);
    let ui = h.front_end();
    packages.drain();

    h.send(&packages, target(&["app:gimp.desktop"]));

    assert!(
        packages
            .drain()
            .iter()
            .any(|message| matches!(message, ToExtension::Rejected { reason } if reason.contains("`remove`")))
    );
    assert_eq!(contributed(&h.query(&ui, "gimp")[0]), []);
}

#[test]
fn the_targets_go_with_the_extensions_run() {
    let mut h = Harness::new(vec![packages(), plain("apps")]);
    let packages = h.up(0);
    let apps = h.up(1);
    h.add(&apps, vec![identified("Gimp")]);
    let ui = h.front_end();
    h.send(&packages, target(&["app:gimp.desktop"]));

    h.down(&packages);
    h.up(0);

    assert_eq!(contributed(&h.query(&ui, "gimp")[0]), []);
}

#[test]
fn a_targeted_action_isnt_offered_on_the_contributing_extensions_own_items() {
    let mut h = Harness::new(vec![packages()]);
    let packages = h.up(0);
    h.add(&packages, vec![identified("Gimp")]);
    let ui = h.front_end();

    h.send(&packages, target(&["app:gimp.desktop"]));

    assert_eq!(contributed(&h.query(&ui, "gimp")[0]), []);
}

fn with_version(title: &str) -> Item {
    let mut item = identified(title);
    item.keyword = Some("go".into());
    item.actions = vec![
        Action::new("install", "Install").arguments(vec![Argument::new("version")]),
        Action::new("purge", "Purge")
            .arguments(vec![Argument::new("version")])
            .role(ActionRole::Destructive),
        Action::new("pick", "Pick").arguments(vec![Argument::new("version").optional()]),
    ];
    item
}

fn roles(row: &ShownResult) -> Vec<ActionRole> {
    row.item.actions.iter().map(|action| action.role).collect()
}

#[test]
fn an_action_whose_required_arguments_the_query_leaves_empty_is_a_form_unless_destructive() {
    let mut h = Harness::new(vec![plain("toolchains")]);
    let toolchains = h.up(0);
    h.add(&toolchains, vec![with_version("Go")]);
    let ui = h.front_end();

    let asked = &h.query(&ui, "go")[0];
    assert_eq!(
        roles(asked),
        [ActionRole::Form, ActionRole::Destructive, ActionRole::Normal]
    );

    let filled = &h.query(&ui, "go 1.23")[0];
    assert_eq!(
        roles(filled),
        [ActionRole::Normal, ActionRole::Destructive, ActionRole::Normal]
    );
}

#[test]
fn an_extension_declares_a_form_itself() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    let mut settings = app("Settings");
    settings.actions = vec![Action::new("open", "Open").role(ActionRole::Form)];
    h.add(&apps, vec![settings]);
    let ui = h.front_end();

    assert_eq!(roles(&h.query(&ui, "settings")[0]), [ActionRole::Form]);
}

#[test]
fn an_item_resent_with_its_actions_reordered_shows_its_new_primary_in_the_open_menu() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    let mut wifi = app("Wi-Fi");
    wifi.actions = vec![Action::new("off", "Turn off"), Action::new("on", "Turn on")];
    h.add(&apps, vec![wifi.clone(), app("Wiki")]);
    let ui = h.front_end();
    let before = h.query(&ui, "wi");

    wifi.actions.reverse();
    h.add(&apps, vec![wifi]);
    let after = ui.results().expect("the menu is told the Item changed");

    assert_eq!(titles(&after), titles(&before));
    let row = after.iter().find(|row| row.item.id == "wi-fi").unwrap();
    assert_eq!(row.item.actions[0].id, "on");
}

#[test]
fn typing_on_after_an_items_actions_changed_finds_what_it_did_and_a_new_title_is_searched() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    let mut wifi = app("Wi-Fi");
    wifi.actions = vec![Action::new("off", "Turn off"), Action::new("on", "Turn on")];
    h.add(&apps, vec![wifi.clone(), app("Wiki")]);
    let ui = h.front_end();
    h.query(&ui, "wi");

    wifi.actions.reverse();
    h.add(&apps, vec![wifi.clone()]);
    let narrowed = h.query(&ui, "wi-");

    assert_eq!(titles(&narrowed), ["Wi-Fi"]);
    assert_eq!(narrowed[0].item.actions[0].id, "on");

    wifi.title = "Wireless".into();
    h.add(&apps, vec![wifi]);
    assert_eq!(titles(&h.query(&ui, "wir")), ["Wireless"]);
}
