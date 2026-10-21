//! The Daemon follows the time zone while it runs. A test binary of its own,
//! since it changes `TZ`, which nothing else in the process may read
//! meanwhile.

mod common;

use std::time::Duration;

use common::*;
use kotodamad::clock::Clock;

/// Sets the time zone, as a POSIX `TZ` rule, so no zone file is needed.
fn set_zone(rule: &str) {
    // SAFETY: the one test of this binary runs alone, and the Daemon it
    // drives starts no thread.
    unsafe { std::env::set_var("TZ", rule) };
}

fn pick(h: &mut Harness, ui: &FrontEnd, query: &str, id: &str) {
    h.query(ui, query);
    run(h, ui, "apps", id, "apps", "open");
    ui.drain();
}

#[test]
fn the_time_of_day_follows_a_change_of_time_zone() {
    set_zone("UTC0");
    let mut h = Harness::new(vec![plain("apps")]);
    h.daemon.set_clock(Clock::local_starting_at(START));
    let apps = h.up(0);
    h.add(&apps, vec![app("Mail"), app("Music")]);
    let ui = h.front_end();
    for _ in 0..10 {
        h.advance_to(8);
        pick(&mut h, &ui, "mail", "mail");
        h.advance_to(20);
        pick(&mut h, &ui, "music", "music");
    }
    h.advance_to(8);
    assert_eq!(titles(&h.query(&ui, "")), ["Mail", "Music", "apps"], "08:00 in UTC");

    set_zone("NZST-12");
    h.advance(Duration::from_secs(60));

    assert_eq!(
        titles(&h.query(&ui, "")),
        ["Music", "Mail", "apps"],
        "20:01 twelve hours east"
    );
}
