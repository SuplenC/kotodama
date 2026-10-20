//! Reads first (ADR 0015): Items are indexed off the keystroke path, and a
//! search never sees half of a change.

mod common;

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use common::*;
use kotodama_protocol::*;
use kotodamad::daemon::Event;

/// How long a change may take to show: about one frame of a slow typist.
const STALENESS: Duration = Duration::from_millis(50);

/// A Daemon that indexes on its own thread, and the events that thread sends.
fn indexing(extensions: Vec<kotodamad::discovery::Found>) -> (Harness, Receiver<Event>) {
    let mut h = Harness::new(extensions);
    let (events, indexed) = mpsc::channel();
    h.daemon.index_in_background(events);
    (h, indexed)
}

/// Waits for the indexing thread's next version and hands it to the Daemon.
fn next_version(h: &mut Harness, indexed: &Receiver<Event>) {
    let event = indexed
        .recv_timeout(Duration::from_secs(5))
        .expect("the indexing thread sent nothing");
    h.event(event);
}

fn clip(id: &str, text: &str) -> Item {
    item(
        id,
        text,
        ItemKind::Url {
            url: format!("https://example.org/{id}"),
        },
    )
}

#[test]
fn a_copied_clip_is_found_within_the_staleness_bound() {
    let (mut h, indexed) = indexing(vec![plain("clipboard")]);
    let clipboard = h.up(0);
    h.add(&clipboard, vec![clip("1", "cargo test")]);
    next_version(&mut h, &indexed);
    let ui = h.front_end();

    let copied = Instant::now();
    h.add(&clipboard, vec![clip("2", "git push origin main")]);
    let event = indexed.recv_timeout(STALENESS);
    assert!(
        event.is_ok(),
        "not indexed within {STALENESS:?} of the copy"
    );
    h.event(event.unwrap());
    assert!(copied.elapsed() < STALENESS * 2);
    assert_eq!(titles(&h.query(&ui, "push")), ["git push origin main"]);
}

#[test]
fn a_keystroke_while_items_are_indexed_searches_the_last_version_whole() {
    let (mut h, indexed) = indexing(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Firefox")]);
    next_version(&mut h, &indexed);
    let ui = h.front_end();

    h.add(&apps, vec![app("Fire Alarm"), app("Fireworks")]);
    assert_eq!(titles(&h.query(&ui, "fire")), ["Firefox"]);
    next_version(&mut h, &indexed);
    let late = ui.results().expect("the new version is shown");
    let mut late = titles(&late);
    // The row on screen stays put (ADR 0003).
    assert_eq!(late.remove(0), "Firefox");
    late.sort_unstable();
    assert_eq!(late, ["Fire Alarm", "Fireworks"]);
}

#[test]
fn a_version_indexed_before_its_extension_stopped_is_dropped() {
    let (mut h, indexed) = indexing(vec![plain("apps")]);
    let apps = h.up(0);
    let ui = h.front_end();
    h.add(&apps, vec![app("Firefox")]);
    let event = indexed
        .recv_timeout(Duration::from_secs(5))
        .expect("indexed");
    h.down(&apps);
    h.event(event);
    assert_eq!(titles(&h.query(&ui, "fire")), Vec::<&str>::new());
}

#[test]
fn a_restarted_extensions_items_are_indexed_again() {
    let (mut h, indexed) = indexing(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Firefox")]);
    next_version(&mut h, &indexed);
    h.down(&apps);
    h.advance(Duration::from_secs(1));
    let apps = h.up(0);
    h.add(&apps, vec![app("Fire Alarm")]);
    next_version(&mut h, &indexed);
    let ui = h.front_end();
    assert_eq!(titles(&h.query(&ui, "fire")), ["Fire Alarm"]);
}

#[test]
fn an_item_hidden_on_the_indexing_thread_leaves_the_search() {
    let (mut h, indexed) = indexing(vec![plain("apps")]);
    let apps = h.up(0);
    let ui = h.front_end();
    h.add(&apps, vec![app("Firefox"), app("Fire Alarm")]);
    next_version(&mut h, &indexed);
    let mut alarm = app("Fire Alarm");
    alarm.hidden = true;
    h.add(&apps, vec![alarm]);
    next_version(&mut h, &indexed);
    assert_eq!(titles(&h.query(&ui, "fire")), ["Firefox"]);
}
