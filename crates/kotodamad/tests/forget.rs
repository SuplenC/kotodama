//! Forgetting what the History learned: one Result, one Extension's Results,
//! or everything, and undoing each for a short while after; and an
//! Extension withdrawing the pick a run of its Action made.

#[path = "support/harness.rs"]
#[expect(dead_code, reason = "each test uses part of the harness")]
mod harness;

use std::time::Duration;

use harness::*;
use kotodama_protocol::*;
use kotodamad::clock::Clock;
use kotodamad::daemon::CLEAR_EXTENSION_HISTORY;
use kotodamad::daemon::Event;
use kotodamad::daemon::FORGET;
use kotodamad::daemon::FrontEndId;
use kotodamad::daemon::UNDO;
use kotodamad::daemon::UNDO_FOR;
use kotodamad::history::History;
use kotodamad::history::IDENTITIES;

fn apps() -> Vec<Item> {
    vec![app("Files"), app("Firefox"), app("Fish")]
}

/// An app, with the Identity that other Extensions give the same app.
fn identified(mut item: Item) -> Item {
    item.identity = Some(format!("app:{}", item.id));
    item
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

/// The row for `owner`'s Item `id` among the Results for `query`.
fn row(h: &mut Harness, ui: &FrontEnd, query: &str, owner: &str, id: &str) -> ShownResult {
    h.query(ui, query)
        .into_iter()
        .find(|row| row.extension == owner && row.item.id == id)
        .expect("the Result is shown")
}

/// The Daemon's own Actions a row offers, by id.
fn daemon_actions(row: &ShownResult) -> Vec<&str> {
    row.contributed
        .iter()
        .filter(|offered| offered.extension == "kotodama")
        .map(|offered| offered.action.id.as_str())
        .collect()
}

/// Runs one of the Daemon's own Actions on a Result, from the Action panel of
/// the Results on screen: the last the Daemon sent.
fn run_daemon_action(h: &mut Harness, ui: &FrontEnd, owner: &str, id: &str, action: &str) {
    ui.drain();
    run(h, ui, owner, id, "kotodama", action);
}

fn saved(h: &Harness) -> History {
    h.saved.0.borrow().clone().expect("the History was saved")
}

/// The History as it is kept on disk.
fn kept(history: &History) -> serde_json::Value {
    serde_json::to_value(history).unwrap()
}

/// The Undo Result, if there is something to undo.
fn undo_row(h: &mut Harness, ui: &FrontEnd) -> Option<ShownResult> {
    h.query(ui, "undo")
        .into_iter()
        .find(|row| row.extension == "kotodama" && row.item.id == UNDO)
}

fn undo(h: &mut Harness, ui: &FrontEnd) {
    let row = undo_row(h, ui).expect("something to undo");
    run(h, ui, "kotodama", UNDO, "kotodama", &row.item.actions[0].id);
    ui.drain();
}

#[test]
fn only_a_result_with_history_offers_to_forget_it() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    assert_eq!(
        daemon_actions(&row(&mut h, &ui, "fi", "apps", "firefox")),
        [] as [&str; 0]
    );

    pick(&mut h, &ui, "fi", "apps", "firefox");
    assert!(daemon_actions(&row(&mut h, &ui, "fi", "apps", "firefox")).contains(&FORGET));
    assert!(!daemon_actions(&row(&mut h, &ui, "fi", "apps", "fish")).contains(&FORGET));
}

#[test]
fn forgetting_a_result_drops_its_frecency_and_learned_choices() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    for _ in 0..3 {
        pick(&mut h, &ui, "fi", "apps", "firefox");
    }
    pick(&mut h, &ui, "files", "apps", "files");
    assert_eq!(first(&mut h, &ui, "fi"), "Firefox");

    run_daemon_action(&mut h, &ui, "apps", "firefox", FORGET);
    assert!(!ui.drain().contains(&ToFrontEnd::Close), "the menu stays open");
    let history = saved(&h);
    assert!(history.picks("apps", "firefox").is_none());
    assert!(history.picks("apps", "files").is_some(), "the rest stays");
    assert_eq!(first(&mut h, &ui, "fi"), "Files", "only Frecency is left");
    assert!(!daemon_actions(&row(&mut h, &ui, "fi", "apps", "firefox")).contains(&FORGET));
}

#[test]
fn forgetting_a_result_forgets_the_thing_from_every_extension() {
    let mut h = Harness::new(vec![plain("apps"), plain("files")]);
    let apps_ext = h.up(0);
    h.add(&apps_ext, apps().into_iter().map(identified).collect());
    let files_ext = h.up(1);
    h.add(&files_ext, vec![identified(app("Firefox"))]);
    let ui = h.front_end();
    for _ in 0..3 {
        pick(&mut h, &ui, "fi", "apps", "firefox");
    }
    pick(&mut h, &ui, "fire", "files", "firefox");
    assert!(saved(&h).picks(IDENTITIES, "app:firefox").is_some());

    // Forgotten through the other Extension's Result of it.
    run_daemon_action(&mut h, &ui, "files", "firefox", FORGET);
    let history = saved(&h);
    assert!(history.picks(IDENTITIES, "app:firefox").is_none());
    assert!(history.picks("apps", "firefox").is_none());
    assert!(history.picks("files", "firefox").is_none());
    assert!(history.is_empty(), "{history:?}");
    for owner in ["apps", "files"] {
        assert_eq!(
            daemon_actions(&row(&mut h, &ui, "fi", owner, "firefox")),
            [] as [&str; 0]
        );
    }
}

#[test]
fn forgetting_a_result_forgets_the_picks_of_the_aliases_for_it() {
    let mut h = Harness::with_config(
        vec![plain("apps")],
        config("[aliases]\nff = { extension = \"apps\", item = \"firefox\" }"),
    );
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    pick(&mut h, &ui, "ff", "apps", "firefox");
    assert!(saved(&h).picks("kotodama", "alias:ff").is_some());

    assert!(daemon_actions(&row(&mut h, &ui, "fi", "apps", "firefox")).contains(&FORGET));
    run_daemon_action(&mut h, &ui, "apps", "firefox", FORGET);
    assert!(saved(&h).is_empty());
}

#[test]
fn undoing_a_forget_restores_the_exact_history() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    assert!(undo_row(&mut h, &ui).is_none(), "nothing to undo");
    for hour in [8, 13, 20] {
        h.advance_to(hour);
        pick(&mut h, &ui, "fi", "apps", "firefox");
        pick(&mut h, &ui, "sh", "apps", "fish");
    }
    let before = kept(&saved(&h));

    run_daemon_action(&mut h, &ui, "apps", "firefox", FORGET);
    assert_ne!(kept(&saved(&h)), before);
    let row = undo_row(&mut h, &ui).expect("an Undo Result");
    assert_eq!(row.item.title, "Undo forgetting “Firefox”");
    undo(&mut h, &ui);
    assert_eq!(kept(&saved(&h)), before);
    assert_eq!(first(&mut h, &ui, "fi"), "Firefox");
    assert!(undo_row(&mut h, &ui).is_none(), "undone once");
}

#[test]
fn the_latest_change_is_undone_first() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    pick(&mut h, &ui, "fi", "apps", "firefox");
    pick(&mut h, &ui, "fi", "apps", "fish");
    let before = kept(&saved(&h));
    run_daemon_action(&mut h, &ui, "apps", "firefox", FORGET);
    run_daemon_action(&mut h, &ui, "apps", "fish", FORGET);

    assert_eq!(undo_row(&mut h, &ui).unwrap().item.title, "Undo forgetting “Fish”");
    undo(&mut h, &ui);
    assert!(saved(&h).picks("apps", "fish").is_some());
    assert!(saved(&h).picks("apps", "firefox").is_none());
    undo(&mut h, &ui);
    assert_eq!(kept(&saved(&h)), before);
}

#[test]
fn undo_is_there_only_for_a_short_while() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    pick(&mut h, &ui, "fi", "apps", "firefox");
    run_daemon_action(&mut h, &ui, "apps", "firefox", FORGET);
    h.advance(UNDO_FOR.checked_sub(Duration::from_secs(1)).unwrap());
    assert!(undo_row(&mut h, &ui).is_some());
    assert!(h.daemon.next_deadline().is_some(), "it is put away on time");
    h.advance(Duration::from_secs(1));
    assert!(undo_row(&mut h, &ui).is_none());
    assert!(h.daemon.next_deadline().is_none());
}

#[test]
fn undo_seconds_sets_how_long_undo_is_there() {
    let mut h = Harness::with_config(vec![plain("apps")], config("[history]\nundo_seconds = 5"));
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    pick(&mut h, &ui, "fi", "apps", "firefox");
    run_daemon_action(&mut h, &ui, "apps", "firefox", FORGET);
    h.advance(Duration::from_secs(4));
    assert!(undo_row(&mut h, &ui).is_some());
    h.advance(Duration::from_secs(1));
    assert!(undo_row(&mut h, &ui).is_none());
}

#[test]
fn clearing_the_history_empties_the_usage_log_and_can_be_undone() {
    let mut h = Harness::with_config(vec![plain("apps")], config("[history]\nusage_log = true"));
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    for hour in [8, 20] {
        h.advance_to(hour);
        for _ in 0..3 {
            pick(&mut h, &ui, "fi", "apps", "firefox");
        }
    }
    let before = kept(&saved(&h));

    h.query(&ui, "clear history");
    run(&mut h, &ui, "kotodama", "clear-history", "kotodama", "run");
    assert!(saved(&h).is_empty());
    assert_eq!(*h.saved.2.borrow(), [None]);

    let row = undo_row(&mut h, &ui).expect("an Undo Result");
    assert_eq!(row.item.title, "Undo clearing history");
    undo(&mut h, &ui);
    assert_eq!(kept(&saved(&h)), before);
    assert_eq!(*h.saved.2.borrow(), [None], "the log stays forgotten");
}

#[test]
fn clearing_one_extensions_history_keeps_the_rest_and_can_be_undone() {
    let mut h = Harness::with_config(
        vec![plain("apps"), plain("projects")],
        config("[aliases]\nff = { extension = \"apps\", item = \"firefox\" }"),
    );
    let apps_ext = h.up(0);
    h.add(&apps_ext, vec![identified(app("Firefox")), app("Files")]);
    let projects_ext = h.up(1);
    h.add(&projects_ext, vec![project("fiddle")]);
    let ui = h.front_end();
    for _ in 0..3 {
        pick(&mut h, &ui, "fi", "apps", "firefox");
        pick(&mut h, &ui, "fid", "projects", "fiddle");
    }
    pick(&mut h, &ui, "files", "apps", "files");
    pick(&mut h, &ui, "ff", "apps", "firefox");
    let before = kept(&saved(&h));
    assert_eq!(
        daemon_actions(&row(&mut h, &ui, "fi", "projects", "fiddle")),
        [FORGET, CLEAR_EXTENSION_HISTORY]
    );
    let offered = row(&mut h, &ui, "fi", "apps", "files");
    let clear = offered
        .contributed
        .iter()
        .find(|offered| offered.action.id == CLEAR_EXTENSION_HISTORY)
        .unwrap();
    assert_eq!(clear.action.title, "Clear the history of apps");

    run_daemon_action(&mut h, &ui, "apps", "files", CLEAR_EXTENSION_HISTORY);
    let history = saved(&h);
    let left: Vec<(&str, &str)> = history.picked().map(|(e, i, _)| (e, i)).collect();
    assert_eq!(left, [("projects", "fiddle")], "with its Identity and Alias");
    assert_eq!(first(&mut h, &ui, "fid"), "fiddle");
    assert_eq!(*h.saved.2.borrow(), [Some("apps".to_owned())]);
    assert_eq!(
        daemon_actions(&row(&mut h, &ui, "fi", "apps", "files")),
        [] as [&str; 0]
    );

    let row = undo_row(&mut h, &ui).expect("an Undo Result");
    assert_eq!(row.item.title, "Undo clearing the history of apps");
    undo(&mut h, &ui);
    assert_eq!(kept(&saved(&h)), before);
}

#[test]
fn undo_keeps_what_was_picked_since() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    for _ in 0..3 {
        pick(&mut h, &ui, "fi", "apps", "firefox");
    }
    h.query(&ui, "clear history");
    run(&mut h, &ui, "kotodama", "clear-history", "kotodama", "run");
    pick(&mut h, &ui, "fis", "apps", "fish");

    undo(&mut h, &ui);
    let history = saved(&h);
    assert!(history.picks("apps", "fish").is_some(), "picked since");
    assert!(history.picks("apps", "firefox").is_some(), "put back");
    assert_eq!(first(&mut h, &ui, "fi"), "Firefox");
}

#[test]
fn clearing_an_empty_history_leaves_nothing_to_undo() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    h.query(&ui, "clear history");
    run(&mut h, &ui, "kotodama", "clear-history", "kotodama", "run");
    assert!(undo_row(&mut h, &ui).is_none());
}

#[test]
fn a_picked_quicklink_offers_forget_among_its_own_actions() {
    let mut h = Harness::with_config(
        vec![],
        config("[quicklinks.gh]\ntitle = \"GitHub\"\nurl = \"https://github.com\""),
    );
    let ui = h.front_end();
    h.query(&ui, "gh");
    run(&mut h, &ui, "kotodama", "quicklink:gh", "kotodama", "open");
    ui.drain();
    let row = row(&mut h, &ui, "gh", "kotodama", "quicklink:gh");
    assert_eq!(row.contributed, []);
    let actions: Vec<&str> = row.item.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(actions, ["open", FORGET]);

    run_daemon_action(&mut h, &ui, "kotodama", "quicklink:gh", FORGET);
    assert!(saved(&h).is_empty());
}

/// The offers to undo, and their withdrawals, sent to a Front end since it
/// was last drained.
fn undo_offers(ui: &FrontEnd) -> Vec<ToFrontEnd> {
    ui.drain()
        .into_iter()
        .filter(|message| matches!(message, ToFrontEnd::OfferUndo { .. } | ToFrontEnd::WithdrawUndo { .. }))
        .collect()
}

/// The change the one offer in `offers` names.
fn offered(offers: &[ToFrontEnd], title: &str) -> u64 {
    match offers {
        [ToFrontEnd::OfferUndo { change, title: offer }] if offer == title => *change,
        other => panic!("expected one offer of {title:?}, got {other:?}"),
    }
}

#[test]
fn forget_has_a_key_binding() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    pick(&mut h, &ui, "fi", "apps", "firefox");

    let row = row(&mut h, &ui, "fi", "apps", "firefox");

    let forget = row
        .contributed
        .iter()
        .find(|offered| offered.action.id == FORGET)
        .expect("Forget is offered");
    assert_eq!(forget.action.shortcut.as_deref(), Some(FORGET_SHORTCUT));
}

#[test]
fn every_front_end_is_offered_to_undo_a_forget_and_the_offer_undoes_it() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    let other = h.front_end();
    for _ in 0..3 {
        pick(&mut h, &ui, "fi", "apps", "firefox");
    }
    let before = kept(&saved(&h));
    other.drain();

    run_daemon_action(&mut h, &ui, "apps", "firefox", FORGET);
    let change = offered(&undo_offers(&ui), "Forgot “Firefox”");
    assert_eq!(offered(&undo_offers(&other), "Forgot “Firefox”"), change);
    assert_ne!(first(&mut h, &ui, "fi"), "Firefox");

    h.front_end_sends(&ui, FromFrontEnd::Undo { change });
    assert_eq!(kept(&saved(&h)), before);
    for front_end in [&ui, &other] {
        assert_eq!(undo_offers(front_end), [ToFrontEnd::WithdrawUndo { change }]);
    }
    assert_eq!(first(&mut h, &ui, "fi"), "Firefox");
    assert!(undo_row(&mut h, &ui).is_none(), "undone once");
}

#[test]
fn clearing_offers_to_undo_it_in_words_of_its_own() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    pick(&mut h, &ui, "fi", "apps", "firefox");

    run_daemon_action(&mut h, &ui, "apps", "firefox", CLEAR_EXTENSION_HISTORY);
    offered(&undo_offers(&ui), "Cleared the history of apps");
    undo(&mut h, &ui);

    h.query(&ui, "clear history");
    run(&mut h, &ui, "kotodama", "clear-history", "kotodama", "run");
    offered(&undo_offers(&ui), "Cleared history");
}

#[test]
fn the_offer_is_withdrawn_when_its_time_runs_out() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    pick(&mut h, &ui, "fi", "apps", "firefox");
    run_daemon_action(&mut h, &ui, "apps", "firefox", FORGET);
    let change = offered(&undo_offers(&ui), "Forgot “Firefox”");

    h.advance(UNDO_FOR);

    assert_eq!(undo_offers(&ui), [ToFrontEnd::WithdrawUndo { change }]);
    h.front_end_sends(&ui, FromFrontEnd::Undo { change });
    assert!(saved(&h).picks("apps", "firefox").is_none(), "too late");
}

#[test]
fn undoing_through_the_undo_result_withdraws_the_offer() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    pick(&mut h, &ui, "fi", "apps", "firefox");
    run_daemon_action(&mut h, &ui, "apps", "firefox", FORGET);
    let change = offered(&undo_offers(&ui), "Forgot “Firefox”");
    let row = undo_row(&mut h, &ui).expect("an Undo Result");

    run(&mut h, &ui, "kotodama", UNDO, "kotodama", &row.item.actions[0].id);

    assert_eq!(undo_offers(&ui), [ToFrontEnd::WithdrawUndo { change }]);
}

#[test]
fn a_front_end_that_connects_is_offered_the_latest_change() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    pick(&mut h, &ui, "fi", "apps", "firefox");
    pick(&mut h, &ui, "sh", "apps", "fish");
    run_daemon_action(&mut h, &ui, "apps", "firefox", FORGET);
    run_daemon_action(&mut h, &ui, "apps", "fish", FORGET);
    let (sender, offers) = std::sync::mpsc::sync_channel(64);

    h.event(Event::FrontEndUp {
        front_end: FrontEndId(99),
        sender,
        disconnect: Box::new(|| {}),
    });

    let offers: Vec<_> = offers
        .try_iter()
        .filter(|message| matches!(message, ToFrontEnd::OfferUndo { .. }))
        .collect();
    let [ToFrontEnd::OfferUndo { title, .. }] = offers.as_slice() else {
        panic!("one offer, got {offers:?}");
    };
    assert_eq!(title, "Forgot “Fish”");
}

/// Runs the primary Action of `owner`'s Result `id` with `query` typed, as
/// [`pick`] does, and returns the run's invocation, as `ext` was asked it.
fn launch(h: &mut Harness, ui: &FrontEnd, ext: &Ext, query: &str, owner: &str, id: &str) -> u64 {
    ext.drain();
    pick(h, ui, query, owner, id);
    ext.drain()
        .into_iter()
        .find_map(|message| match message {
            ToExtension::RunAction { invocation, .. } => Some(invocation),
            _ => None,
        })
        .expect("the Extension was asked to run the Action")
}

#[test]
fn a_withdrawn_pick_leaves_the_history_exactly_as_it_was_before_the_pick() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps().into_iter().map(identified).collect());
    let ui = h.front_end();
    for hour in [8, 13, 20] {
        h.advance_to(hour);
        pick(&mut h, &ui, "fi", "apps", "firefox");
        pick(&mut h, &ui, "fi", "apps", "fish");
        pick(&mut h, &ui, "f", "apps", "files");
    }
    h.advance_to(9);
    let before = kept(&saved(&h));
    let empty = titles(&h.query(&ui, "")).join(", ");
    let searched = titles(&h.query(&ui, "fi")).join(", ");

    // The Learned choice's pick, one that takes over from it, and one for a
    // query it was never picked for.
    for (query, id) in [("fi", "firefox"), ("fi", "fish"), ("fil", "files")] {
        let invocation = launch(&mut h, &ui, &ext, query, "apps", id);
        assert_ne!(kept(&saved(&h)), before, "{id} picked");
        h.send(&ext, FromExtension::WithdrawPick { invocation });
        assert_eq!(kept(&saved(&h)), before, "{id} withdrawn");
    }

    assert_eq!(titles(&h.query(&ui, "")).join(", "), empty);
    assert_eq!(titles(&h.query(&ui, "fi")).join(", "), searched);
}

#[test]
fn a_withdrawn_pick_ranks_as_if_it_was_never_made_later_on() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();
    let mut unpicked = Harness::new(vec![plain("apps")]);
    let unpicked_ext = unpicked.up(0);
    unpicked.add(&unpicked_ext, apps());
    let unpicked_ui = unpicked.front_end();
    for hour in [8, 13, 20] {
        for h in [&mut h, &mut unpicked] {
            h.advance_to(hour);
        }
        pick(&mut h, &ui, "fi", "apps", "firefox");
        pick(&mut unpicked, &unpicked_ui, "fi", "apps", "firefox");
        pick(&mut h, &ui, "sh", "apps", "fish");
        pick(&mut unpicked, &unpicked_ui, "sh", "apps", "fish");
    }

    for _ in 0..3 {
        let invocation = launch(&mut h, &ui, &ext, "fi", "apps", "fish");
        h.send(&ext, FromExtension::WithdrawPick { invocation });
    }

    for hour in [9, 21] {
        for h in [&mut h, &mut unpicked] {
            h.advance_to(hour);
        }
        for query in ["", "f", "fi"] {
            assert_eq!(
                titles(&h.query(&ui, query)),
                titles(&unpicked.query(&unpicked_ui, query)),
                "{query:?} at {hour}:00"
            );
        }
    }
}

#[test]
fn a_pick_is_never_offered_to_undo_and_withdrawing_it_offers_nothing() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();

    let invocation = launch(&mut h, &ui, &ext, "fi", "apps", "firefox");
    assert_eq!(undo_offers(&ui), []);
    assert!(undo_row(&mut h, &ui).is_none());
    h.front_end_sends(&ui, FromFrontEnd::Undo { change: invocation });
    assert!(saved(&h).picks("apps", "firefox").is_some(), "not the user's to undo");

    h.send(&ext, FromExtension::WithdrawPick { invocation });
    assert_eq!(undo_offers(&ui), []);
    assert!(saved(&h).is_empty());
}

#[test]
fn only_the_extension_that_ran_the_action_withdraws_its_pick_and_only_for_a_while() {
    let mut h = Harness::new(vec![plain("apps"), plain("other")]);
    let ext = h.up(0);
    let other = h.up(1);
    h.add(&ext, apps());
    let ui = h.front_end();

    let invocation = launch(&mut h, &ui, &ext, "fi", "apps", "firefox");
    h.send(&other, FromExtension::WithdrawPick { invocation });
    assert!(saved(&h).picks("apps", "firefox").is_some(), "another Extension's");
    h.send(
        &ext,
        FromExtension::WithdrawPick {
            invocation: invocation + 100,
        },
    );
    assert!(saved(&h).picks("apps", "firefox").is_some(), "another run's");

    h.advance(UNDO_FOR);
    h.send(&ext, FromExtension::WithdrawPick { invocation });
    assert!(saved(&h).picks("apps", "firefox").is_some(), "too late");
}

#[test]
fn withdrawing_the_first_of_two_launches_keeps_the_second() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ext = h.up(0);
    h.add(&ext, apps());
    let ui = h.front_end();

    let first = launch(&mut h, &ui, &ext, "fi", "apps", "firefox");
    h.advance(Duration::from_secs(5));
    launch(&mut h, &ui, &ext, "fi", "apps", "firefox");
    h.send(&ext, FromExtension::WithdrawPick { invocation: first });

    let when = Clock::starting_at(START).when(h.elapsed());
    let frecency = saved(&h).frecency("apps", "firefox", when);
    assert!((frecency - 1.0).abs() < 1e-3, "one pick left: {frecency}");
}
