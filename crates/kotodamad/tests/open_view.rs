//! `kotodama open --view <extension>/<item>`: the Daemon runs the Item's
//! primary Action and opens the menus only once the View it pushes is there,
//! so their first frame shows it (ADR 0014).

mod common;

use std::collections::BTreeMap;
use std::time::Duration;

use common::*;
use kotodama_protocol::*;

/// The git Extension's `branches` Item, whose primary Action lists them.
fn branches_item() -> Item {
    let mut item = item(
        "branches",
        "Branches",
        ItemKind::Url {
            url: "https://git/branches".into(),
        },
    );
    item.actions = vec![action("list"), action("prune")];
    item
}

fn branches_view() -> View {
    View {
        id: "branches".into(),
        title: "Branches".into(),
        content: ViewContent::Detail {
            markdown: "main".into(),
            metadata: vec![],
        },
    }
}

/// What a menu was sent, but the Results.
fn sent(ui: &FrontEnd) -> Vec<ToFrontEnd> {
    ui.drain()
        .into_iter()
        .filter(|message| !matches!(message, ToFrontEnd::Results { .. }))
        .collect()
}

fn open_on(h: &mut Harness, command: &FrontEnd, view: &str) {
    h.front_end_sends(
        command,
        FromFrontEnd::Open {
            query: String::new(),
            view: Some(view.into()),
        },
    );
}

fn opened(view: &str) -> ToFrontEnd {
    ToFrontEnd::Open {
        query: String::new(),
        view: Some(view.into()),
    }
}

/// The invocation of the one Action the Extension was asked to run, which
/// must be `action` on `item`.
fn ran(ext: &Ext, action: &str, item: &str) -> u64 {
    let sent = ext.drain();
    match &sent[..] {
        [
            ToExtension::RunAction {
                invocation,
                action: ran,
                item: on,
                ..
            },
        ] if ran == action && on.id == item => *invocation,
        _ => panic!("expected {action} on {item}, got {sent:?}"),
    }
}

/// A Daemon with git up for a while and its `branches` Item indexed, a menu,
/// and the `kotodama` command connected.
fn git() -> (Harness, Ext, FrontEnd, FrontEnd) {
    let mut h = Harness::new(vec![plain("git")]);
    let git = h.up(0);
    h.add(&git, vec![branches_item()]);
    h.advance(Duration::from_secs(10));
    let menu = h.front_end();
    let command = h.front_end();
    git.drain();
    (h, git, menu, command)
}

#[test]
fn the_menu_opens_once_the_view_the_items_primary_action_pushes_is_there() {
    let (mut h, git, menu, command) = git();

    open_on(&mut h, &command, "git/branches");
    assert_eq!(sent(&menu), [], "nothing to open on yet");
    let invocation = ran(&git, "list", "branches");

    h.send(&git, FromExtension::PushView(branches_view()));
    h.send(
        &git,
        FromExtension::ActionDone {
            invocation,
            outcome: Outcome::KeepOpen,
        },
    );
    assert_eq!(
        sent(&menu),
        [
            ToFrontEnd::PushView {
                extension: "git".into(),
                view: branches_view(),
                contributed: BTreeMap::new(),
            },
            opened("git/branches"),
        ]
    );
    assert!(
        !sent(&command)
            .iter()
            .any(|message| matches!(message, ToFrontEnd::Open { .. })),
        "the command that asked isn't a menu"
    );
}

fn failed(message: &str) -> ToFrontEnd {
    ToFrontEnd::ActionFailed {
        message: message.into(),
    }
}

fn done(h: &mut Harness, ext: &Ext, invocation: u64, outcome: Outcome) {
    h.send(
        ext,
        FromExtension::ActionDone {
            invocation,
            outcome,
        },
    );
}

#[test]
fn a_name_with_no_view_behind_it_opens_the_search_at_once_and_says_why() {
    let (mut h, git, menu, command) = git();

    for name in ["git/tags", "svn/branches", "branches"] {
        open_on(&mut h, &command, name);
        assert_eq!(
            sent(&menu),
            [
                opened(name),
                failed(&format!("No View `{name}`; name one as <extension>/<item>")),
            ]
        );
    }
    assert_eq!(git.drain(), [], "nothing runs");
}

#[test]
fn an_action_that_fails_opens_the_search_with_its_message() {
    let (mut h, git, menu, command) = git();
    open_on(&mut h, &command, "git/branches");
    let invocation = ran(&git, "list", "branches");

    let message = "not a repository".to_owned();
    done(&mut h, &git, invocation, Outcome::Failed { message });
    assert_eq!(
        sent(&menu),
        [opened("git/branches"), failed("not a repository")]
    );
}

#[test]
fn an_action_that_closes_the_menu_closes_it_rather_than_opening_it() {
    let (mut h, git, menu, command) = git();
    open_on(&mut h, &command, "git/branches");
    let invocation = ran(&git, "list", "branches");

    done(&mut h, &git, invocation, Outcome::Close);
    assert_eq!(sent(&menu), [ToFrontEnd::Close], "it may have been open");
}

#[test]
fn a_starting_extension_is_waited_for_until_it_has_indexed_its_items() {
    let mut h = Harness::new(vec![plain("git")]);
    let menu = h.front_end();
    let command = h.front_end();

    open_on(&mut h, &command, "git/branches");
    let git = h.up(0);
    assert_eq!(sent(&menu), [], "started, but nothing indexed yet");
    git.drain();

    h.add(&git, vec![branches_item()]);
    let invocation = ran(&git, "list", "branches");
    h.send(&git, FromExtension::PushView(branches_view()));
    done(&mut h, &git, invocation, Outcome::KeepOpen);
    assert_eq!(sent(&menu).last(), Some(&opened("git/branches")));
}

#[test]
fn an_item_an_extension_that_just_started_has_yet_to_send_is_waited_for() {
    let mut h = Harness::new(vec![plain("git")]);
    let git = h.up(0);
    h.add(&git, vec![project("kotodama")]);
    let menu = h.front_end();
    let command = h.front_end();
    git.drain();

    open_on(&mut h, &command, "git/branches");
    assert_eq!(sent(&menu), [], "its Items may come in more than one batch");
    h.add(&git, vec![branches_item()]);
    ran(&git, "list", "branches");
}

#[test]
fn a_view_that_takes_too_long_opens_the_search_and_may_still_come() {
    let (mut h, git, menu, command) = git();
    open_on(&mut h, &command, "git/branches");
    let invocation = ran(&git, "list", "branches");

    h.advance(Duration::from_millis(999));
    assert_eq!(sent(&menu), []);
    h.advance(Duration::from_millis(1));
    assert_eq!(sent(&menu), [opened("git/branches")]);

    h.send(&git, FromExtension::PushView(branches_view()));
    done(&mut h, &git, invocation, Outcome::KeepOpen);
    assert_eq!(
        sent(&menu),
        [ToFrontEnd::PushView {
            extension: "git".into(),
            view: branches_view(),
            contributed: BTreeMap::new(),
        }],
        "pushed over the search, and not opened twice"
    );
}

#[test]
fn an_action_that_fails_after_the_menu_opened_without_it_still_says_why() {
    let (mut h, git, menu, command) = git();
    open_on(&mut h, &command, "git/branches");
    let invocation = ran(&git, "list", "branches");
    h.advance(Duration::from_secs(1));
    assert_eq!(sent(&menu), [opened("git/branches")]);
    assert_eq!(h.daemon.next_deadline(), None, "nothing more to wait for");

    let message = "not a repository".to_owned();
    done(&mut h, &git, invocation, Outcome::Failed { message });
    assert_eq!(sent(&menu), [failed("not a repository")]);
}

#[test]
fn an_extension_that_never_starts_opens_the_search_in_time_and_says_so() {
    let mut h = Harness::new(vec![plain("git")]);
    let menu = h.front_end();
    let command = h.front_end();
    open_on(&mut h, &command, "git/branches");

    let wait = Duration::from_secs(1);
    assert!(
        h.daemon
            .next_deadline()
            .is_some_and(|at| at <= h.now + wait)
    );
    h.advance(wait);
    assert_eq!(
        sent(&menu),
        [
            opened("git/branches"),
            failed("No View `git/branches` turned up"),
        ]
    );
}

#[test]
fn an_extension_that_stops_before_its_view_is_there_opens_the_search_and_says_so() {
    let (mut h, git, menu, command) = git();
    open_on(&mut h, &command, "git/branches");
    ran(&git, "list", "branches");

    h.down(&git);
    assert_eq!(
        sent(&menu)[..2],
        [
            opened("git/branches"),
            failed("git stopped before its View opened"),
        ]
    );
}

#[test]
fn a_toggle_or_open_while_a_view_is_awaited_waits_for_it_so_the_search_never_shows_first() {
    let (mut h, git, menu, command) = git();
    let other = h.front_end();
    open_on(&mut h, &command, "git/branches");
    let invocation = ran(&git, "list", "branches");

    h.front_end_sends(&other, FromFrontEnd::Toggle);
    let fir = FromFrontEnd::Open {
        query: "fir".into(),
        view: None,
    };
    h.front_end_sends(&other, fir);
    assert_eq!(sent(&menu), [], "held back with the View");

    h.send(&git, FromExtension::PushView(branches_view()));
    done(&mut h, &git, invocation, Outcome::KeepOpen);
    assert_eq!(
        sent(&menu),
        [
            ToFrontEnd::PushView {
                extension: "git".into(),
                view: branches_view(),
                contributed: BTreeMap::new(),
            },
            opened("git/branches"),
            ToFrontEnd::Toggle,
            ToFrontEnd::Open {
                query: "fir".into(),
                view: None,
            },
        ],
        "in the order they were asked for"
    );
    assert!(
        !sent(&other).contains(&ToFrontEnd::Toggle),
        "never to the asker"
    );

    h.front_end_sends(&other, FromFrontEnd::Toggle);
    assert_eq!(
        sent(&menu),
        [ToFrontEnd::Toggle],
        "nothing awaited any more"
    );
}

#[test]
fn a_toggle_held_back_goes_through_once_the_menus_open_on_the_search_without_the_view() {
    let (mut h, git, menu, command) = git();
    open_on(&mut h, &command, "git/branches");
    ran(&git, "list", "branches");
    h.front_end_sends(&command, FromFrontEnd::Toggle);
    assert_eq!(sent(&menu), []);

    h.advance(Duration::from_secs(1));
    assert_eq!(sent(&menu), [opened("git/branches"), ToFrontEnd::Toggle]);
}

/// The calculator's `history` Item, hidden or not, whose primary Action
/// shows its history.
fn calculator_history(hidden: bool) -> Item {
    let mut history = item(
        "history",
        "Calculator history",
        ItemKind::Url {
            url: "calc:history".into(),
        },
    );
    history.actions = vec![action("show-history")];
    history.hidden = hidden;
    history
}

#[test]
fn a_hidden_item_never_shows_in_the_search_but_opens_its_view() {
    // An Extension with only Live queries names its View with a hidden Item.
    let mut h = Harness::new(vec![plain("calc")]);
    let calc = h.up(0);
    h.add(&calc, vec![calculator_history(true)]);
    h.advance(Duration::from_secs(10));
    let menu = h.front_end();
    let command = h.front_end();
    calc.drain();

    assert_eq!(titles(&h.query(&menu, "calculator history")), [""; 0]);
    assert_eq!(titles(&h.query(&menu, "")), [""; 0]);

    open_on(&mut h, &command, "calc/history");
    ran(&calc, "show-history", "history");
}

#[test]
fn an_item_shows_in_the_search_only_while_it_isnt_hidden() {
    let mut h = Harness::new(vec![plain("calc")]);
    let calc = h.up(0);
    let menu = h.front_end();
    h.add(&calc, vec![calculator_history(true)]);
    h.add(&calc, vec![calculator_history(false)]);
    assert_eq!(
        titles(&h.query(&menu, "calculator history")),
        ["Calculator history"]
    );

    h.add(&calc, vec![calculator_history(true)]);
    assert_eq!(titles(&h.query(&menu, "calculator history")), [""; 0]);
}

#[test]
fn a_removed_hidden_item_no_longer_opens_its_view() {
    let mut h = Harness::new(vec![plain("calc")]);
    let calc = h.up(0);
    h.add(&calc, vec![calculator_history(true)]);
    h.send(
        &calc,
        FromExtension::RemoveItems {
            ids: vec!["history".into()],
        },
    );
    h.advance(Duration::from_secs(10));
    let menu = h.front_end();
    let command = h.front_end();
    calc.drain();

    open_on(&mut h, &command, "calc/history");
    assert_eq!(calc.drain(), [], "nothing runs");
    assert!(
        sent(&menu)
            .iter()
            .any(|message| matches!(message, ToFrontEnd::ActionFailed { .. })),
        "the menus open on the search and say why"
    );
}
