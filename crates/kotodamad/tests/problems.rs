//! The problems: what is wrong that the user can fix, as Front ends hear of
//! it in `ToFrontEnd::Status` and find it in the Daemon's own View.

#[path = "support/harness.rs"]
#[expect(dead_code, reason = "each test uses part of the harness")]
mod harness;

use std::path::PathBuf;
use std::time::Duration;

use harness::*;
use kotodama_protocol::*;
use kotodamad::config;
use kotodamad::daemon::Ended;
use kotodamad::daemon::Event;
use kotodamad::daemon::ExtensionId;
use kotodamad::discovery::Discovered;

/// What each problem in the last Status `messages` hold is about, or `None`
/// without a Status.
fn last_problems(messages: &[ToFrontEnd]) -> Option<Vec<String>> {
    messages.iter().rev().find_map(|message| match message {
        ToFrontEnd::Status { problems, .. } => Some(problems.iter().map(|problem| problem.about.clone()).collect()),
        _ => None,
    })
}

/// The Markdown of the Detail View `message` pushes or updates, if it does.
fn markdown(message: &ToFrontEnd) -> Option<&str> {
    match message {
        ToFrontEnd::PushView { view, .. } | ToFrontEnd::UpdateView { view, .. } => match &view.content {
            ViewContent::Detail { markdown, .. } => Some(markdown),
            _ => None,
        },
        _ => None,
    }
}

#[test]
fn a_front_end_hears_of_every_problem_as_it_connects() {
    let mut h = Harness::with_config(
        vec![plain("apps")],
        config(
            "[history]\nhalf_life_days = 0\n\
             [aliases.gh]\nextension = \"apps\"\nitem = \"x\"\n\
             [quicklinks.gh]\nurl = \"https://github.com/search?q={query}\"\n",
        ),
    );

    let (_ui, greeting) = h.greeted_front_end(OUTBOX);

    assert_eq!(
        last_problems(&greeting),
        Some(vec!["history.half_life_days".to_owned(), "keyword `gh`".to_owned()])
    );
}

#[test]
fn a_misspelt_table_or_key_is_a_problem() {
    let mut h = Harness::with_config(
        vec![plain("apps")],
        config("[serach]\nrows = 5\n[search]\nrowz = 5\n[extensions.apps]\nenable = \"no\"\n"),
    );

    let (_ui, greeting) = h.greeted_front_end(OUTBOX);

    assert_eq!(
        last_problems(&greeting),
        Some(vec![
            "serach".to_owned(),
            "search.rowz".to_owned(),
            "extensions.apps.enable".to_owned()
        ])
    );
}

#[test]
fn fixing_the_config_clears_its_problems_and_every_front_end_hears_of_it() {
    let mut h = Harness::with_config(vec![], config("[history]\npicks_to_learn = 0\n"));
    let ui = h.front_end();

    h.event(Event::Config(config("[history]\npicks_to_learn = 2\n")));

    assert_eq!(last_problems(&ui.drain()), Some(vec![]));
}

#[test]
fn a_config_that_cant_be_loaded_is_a_problem_until_it_loads() {
    let mut h = Harness::new(vec![]);
    let ui = h.front_end();

    h.event(Event::ConfigFailed(config::Error {
        path: PathBuf::from("/home/me/.config/kotodama/a.toml"),
        reason: "expected `=`".into(),
    }));
    assert_eq!(
        last_problems(&ui.drain()),
        Some(vec!["/home/me/.config/kotodama/a.toml".to_owned()])
    );

    h.event(Event::Config(Table::new()));
    assert_eq!(last_problems(&ui.drain()), Some(vec![]));
}

#[test]
fn a_manifest_discovery_refused_is_a_problem() {
    let mut h = Harness::new(vec![]);
    let ui = h.front_end();

    h.event(Event::Extensions(Discovered {
        found: vec![],
        problems: vec![Problem {
            about: "/ext/broken/manifest.toml".into(),
            message: "missing field `command`".into(),
        }],
    }));

    assert_eq!(
        last_problems(&ui.drain()),
        Some(vec!["/ext/broken/manifest.toml".to_owned()])
    );
}

#[test]
fn an_extension_that_cant_start_is_a_problem_and_isnt_restarted() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ui = h.front_end();

    h.event(Event::ExtensionDown {
        extension: ExtensionId(0),
        generation: 1,
        ended: Ended::CantStart("its program /ext/x doesn't exist".into()),
    });
    h.advance(Duration::from_secs(600));

    assert_eq!(h.launches.0.borrow().len(), 1);
    assert_eq!(last_problems(&ui.drain()), Some(vec!["extension apps".to_owned()]));
}

#[test]
fn an_extension_refused_at_the_handshake_starts_again_once_its_manifest_changes() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ui = h.front_end();
    h.event(Event::ExtensionDown {
        extension: ExtensionId(0),
        generation: 1,
        ended: Ended::Refused("the peer needs protocol 2.0".into()),
    });
    h.advance(Duration::from_secs(600));
    assert_eq!(h.launches.0.borrow().len(), 1);

    let changed = manifest("name = \"apps\"\ncommand = [\"y\"]");
    h.event(Event::Extensions(vec![changed].into()));
    h.up(0);

    assert_eq!(h.launches.0.borrow().len(), 2);
    assert_eq!(last_problems(&ui.drain()), Some(vec![]));
}

#[test]
fn an_extension_that_stopped_is_a_problem_until_it_is_back_up() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    let ui = h.front_end();

    h.down(&apps);
    let messages = ui.drain();
    let problem = messages.iter().find_map(|message| match message {
        ToFrontEnd::Status { problems, .. } => problems.first().cloned(),
        _ => None,
    });
    assert_eq!(
        problem,
        Some(Problem {
            about: "extension apps".into(),
            message: "stopped (exit status: 1); restarting".into(),
        })
    );

    h.advance(Duration::from_secs(1));
    h.up(0);
    assert_eq!(last_problems(&ui.drain()), Some(vec![]));
}

#[test]
fn the_own_problems_item_opens_a_list_that_stays_up_to_date() {
    let mut h = Harness::with_config(vec![], config("[history]\ntime_of_day = 2\n"));
    let ui = h.front_end();
    assert_eq!(titles(&h.query(&ui, "problems")), ["1 problem"]);

    run(&mut h, &ui, "kotodama", "problems", "kotodama", "show");
    let pushed = ui.drain();
    let list = pushed.iter().find_map(markdown).expect("the list is pushed");
    assert!(list.contains("history.time\\_of\\_day"), "{list}");

    h.event(Event::Config(Table::new()));
    let updated = ui.drain();
    assert_eq!(updated.iter().find_map(markdown), Some("Nothing is wrong."));
    assert_eq!(titles(&h.query(&ui, "problems")), Vec::<&str>::new());
}

#[test]
fn a_list_popped_is_no_longer_kept_up_to_date() {
    let mut h = Harness::with_config(vec![], config("[history]\ntime_of_day = 2\n"));
    let ui = h.front_end();
    h.query(&ui, "problems");
    run(&mut h, &ui, "kotodama", "problems", "kotodama", "show");
    ui.drain();

    h.front_end_sends(
        &ui,
        FromFrontEnd::ViewPopped {
            extension: "kotodama".into(),
            view: "problems".into(),
        },
    );
    h.event(Event::Config(Table::new()));

    assert_eq!(ui.drain().iter().find_map(markdown), None);
}
