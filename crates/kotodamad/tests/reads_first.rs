//! Reads first (ADR 0017): Items are indexed off the keystroke path, and a
//! search never sees half of a change. The Items expected at a time of day
//! are hot ahead of it: read back off their Shelf before they are shown.

mod common;

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use common::*;
use kotodama_protocol::*;
use kotodamad::daemon::RERANK_FOR;
use kotodamad::daemon::{Event, HOT_CAPACITY};

/// How long a change may take to show: about one frame of a slow typist.
const STALENESS: Duration = Duration::from_millis(50);

/// A Daemon that indexes on its own thread, and the events that thread sends.
fn indexing(extensions: Vec<kotodamad::discovery::Found>) -> (Harness, Receiver<Event>) {
    let mut h = Harness::new(extensions);
    let (events, indexed) = mpsc::channel();
    h.daemon.index_in_background(events);
    (h, indexed)
}

/// Waits for the indexing thread's next version and hands it to the Daemon,
/// with the hot Items it sent before it.
fn next_version(h: &mut Harness, indexed: &Receiver<Event>) {
    next(h, indexed, |event| matches!(event, Event::Indexed(_)));
}

/// Waits for the hot Items the indexing thread plans next, and hands them
/// to the Daemon, with the versions it sent before them.
fn next_hot(h: &mut Harness, indexed: &Receiver<Event>) {
    next(h, indexed, |event| matches!(event, Event::Kept(_)));
}

/// Hands the Daemon what the indexing thread sends, up to the first event
/// that is `awaited`.
fn next(h: &mut Harness, indexed: &Receiver<Event>, awaited: fn(&Event) -> bool) {
    loop {
        let event = indexed
            .recv_timeout(Duration::from_secs(5))
            .expect("the indexing thread sent nothing");
        let done = awaited(&event);
        h.event(event);
        if done {
            return;
        }
    }
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
    h.advance(RERANK_FOR);
    next_version(&mut h, &indexed);
    let late = ui.results().expect("the new version is shown");
    let mut late = titles(&late);
    // The row on screen stays put.
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

/// Picks `id` after typing `query`.
fn pick(h: &mut Harness, ui: &FrontEnd, apps: &Ext, query: &str, id: &str) {
    h.query(ui, query);
    run(h, ui, "apps", id, "apps", "open");
    ui.drain();
    apps.drain();
}

/// Firefox picked at half past eight on five mornings, and Mail at eight
/// on five evenings, with at most `hot` Items hot.
fn routine(hot: usize) -> (Harness, FrontEnd) {
    let mut h = Harness::new(vec![plain("apps")]);
    h.daemon.set_hot_capacity(hot);
    let apps = h.up(0);
    let titles = ["Firefox", "Fire Alarm", "Mail", "Files", "Terminal"];
    h.add(&apps, titles.map(app).into());
    let ui = h.front_end();
    for _ in 0..5 {
        h.advance_to(8);
        h.advance(Duration::from_secs(30 * 60));
        pick(&mut h, &ui, &apps, "fire", "firefox");
        h.advance_to(20);
        pick(&mut h, &ui, &apps, "mail", "mail");
    }
    (h, ui)
}

/// Waits for `minute` past the next `hour` o'clock.
fn advance_to(h: &mut Harness, hour: u64, minute: u64) {
    h.advance_to(hour);
    h.advance(Duration::from_secs(minute * 60));
}

#[test]
fn an_item_picked_at_half_past_eight_is_hot_by_twenty_past_and_cold_again_later() {
    let (mut h, ui) = routine(HOT_CAPACITY);
    let (mut cold, cold_ui) = routine(0);
    let firefox = ("apps".to_owned(), "firefox".to_owned());
    let mail = ("apps".to_owned(), "mail".to_owned());
    for (hour, minute, hot) in [
        (2, 0, vec![]),
        (8, 20, vec![firefox.clone()]),
        (16, 0, vec![]),
        (20, 0, vec![mail.clone()]),
        (3, 0, vec![]),
        (8, 20, vec![firefox.clone()]),
    ] {
        advance_to(&mut h, hour, minute);
        advance_to(&mut cold, hour, minute);
        assert_eq!(
            h.daemon.hot(),
            hot,
            "hot, and read back, at {hour}:{minute:02}"
        );
        assert_eq!(cold.daemon.hot(), [], "none hot without room for them");
        for query in ["", "f", "fire", "mail", "files"] {
            assert_eq!(
                titles(&h.query(&ui, query)),
                titles(&cold.query(&cold_ui, query)),
                "{query:?} at {hour}:{minute:02}"
            );
        }
    }
}

#[test]
fn the_hot_items_are_planned_and_read_on_the_indexing_thread() {
    let (mut h, indexed) = indexing(vec![plain("apps")]);
    let apps = h.up(0);
    let titles = ["Firefox", "Fire Alarm", "Mail"];
    h.add(&apps, titles.map(app).into());
    next_version(&mut h, &indexed);
    let ui = h.front_end();
    for _ in 0..5 {
        h.advance_to(8);
        h.advance(Duration::from_secs(30 * 60));
        pick(&mut h, &ui, &apps, "fire", "firefox");
    }
    // Plans asked for while picking.
    while let Ok(event) = indexed.recv_timeout(Duration::from_millis(200)) {
        h.event(event);
    }
    advance_to(&mut h, 2, 0);
    next_hot(&mut h, &indexed);
    assert_eq!(h.daemon.hot(), []);
    advance_to(&mut h, 8, 20);
    assert_eq!(h.daemon.hot(), [], "planned on the Daemon's own thread");
    next_hot(&mut h, &indexed);
    let firefox = ("apps".to_owned(), "firefox".to_owned());
    assert_eq!(h.daemon.hot(), [firefox]);
}

#[test]
fn an_item_with_a_keyword_is_read_before_the_keyword_is_typed() {
    let (mut h, indexed) = indexing(vec![plain("apps")]);
    let apps = h.up(0);
    let mut firefox = app("Firefox");
    firefox.keyword = Some("ff".into());
    h.add(&apps, vec![firefox, app("Mail")]);
    next_version(&mut h, &indexed);
    let firefox = ("apps".to_owned(), "firefox".to_owned());
    assert_eq!(h.daemon.pinned(), [firefox]);
    let ui = h.front_end();
    assert_eq!(titles(&h.query(&ui, "ff")), ["Firefox"]);
}

#[test]
fn no_more_items_are_hot_than_there_is_room_for() {
    let (mut h, _) = routine(1);
    advance_to(&mut h, 8, 20);
    assert_eq!(h.daemon.hot().len(), 1);
}

#[test]
fn the_menu_shown_by_its_own_keybind_warms_the_cache_as_the_command_does() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Firefox"), app("Mail")]);
    let menu = h.front_end();
    let command = h.front_end();
    h.query(&menu, "");

    let opened = FromFrontEnd::Open {
        query: String::new(),
        view: None,
    };
    let mut warmed = h.daemon.warmed();
    for (front_end, message) in [
        (&command, opened),
        (&command, FromFrontEnd::Toggle),
        (&menu, FromFrontEnd::Shown),
    ] {
        let sent = format!("{message:?}");
        h.front_end_sends(front_end, message);

        warmed += 1;
        assert_eq!(h.daemon.warmed(), warmed, "warmed once on {sent}");
    }
    h.query(&menu, "f");
    assert_eq!(h.daemon.warmed(), warmed, "a keystroke doesn't warm it");
}

/// xorshift64*: the same Items, queries and picks on every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
        from[self.below(from.len())]
    }
}

const WORDS: &[&str] = &[
    "Firefox",
    "fire",
    "fox",
    "Terminal",
    "term",
    "Mail",
    "mailbox",
    "Files",
    "Café",
    "naïve",
    "Straße",
    "日本語",
    "getUserName",
    "VSCode",
    "code",
    "Settings",
    "set",
    "git",
    "push",
];

fn words(random: &mut Random, most: usize) -> String {
    let words: Vec<&str> = (0..1 + random.below(most))
        .map(|_| random.pick(WORDS))
        .collect();
    words.join(random.pick(&[" ", "-", ""]))
}

fn random_item(random: &mut Random) -> Item {
    let id = format!("item-{}", random.below(30));
    // Some titles longer than nucleo scores optimally.
    let most = [3, 3, 3, 16][random.below(4)];
    let mut item = item(
        &id,
        &words(random, most),
        ItemKind::Url {
            url: format!("https://example.org/{id}"),
        },
    );
    item.identity = (random.below(4) == 0).then(|| format!("thing:{}", random.below(4)));
    item.subtitle = (random.below(2) == 0).then(|| words(random, 3));
    item.keyword = (random.below(6) == 0).then(|| random.pick(WORDS).to_lowercase());
    item.keywords = (0..random.below(2)).map(|_| words(random, 2)).collect();
    item.hidden = random.below(12) == 0;
    item
}

fn random_query(random: &mut Random) -> String {
    let mut text = String::new();
    for n in 0..1 + random.below(2) {
        if n > 0 {
            text.push(' ');
        }
        let word: Vec<char> = random.pick(WORDS).chars().collect();
        let mut word = word[..1 + random.below(word.len())].to_vec();
        if word.len() > 2 && random.below(3) == 0 {
            let at = random.below(word.len() - 1);
            word.swap(at, at + 1);
        }
        text.extend(word);
    }
    text
}

/// One Daemon as it runs: indexing on its own thread, hot Items, and each
/// keystroke narrowing the last one's matches. The other searches every
/// Item afresh on each keystroke, as each change arrives, with nothing hot.
struct Twins {
    fast: Harness,
    fast_ui: FrontEnd,
    fast_exts: Vec<Ext>,
    indexed: Receiver<Event>,
    naive: Harness,
    naive_ui: FrontEnd,
    naive_exts: Vec<Ext>,
}

impl Twins {
    fn new() -> Self {
        let found = || vec![plain("apps"), plain("clips")];
        let (mut fast, indexed) = indexing(found());
        let fast_exts = vec![fast.up(0), fast.up(1)];
        let fast_ui = fast.front_end();
        let mut naive = Harness::new(found());
        naive.daemon.set_hot_capacity(0);
        let naive_exts = vec![naive.up(0), naive.up(1)];
        let naive_ui = naive.front_end();
        Twins {
            fast,
            fast_ui,
            fast_exts,
            indexed,
            naive,
            naive_ui,
            naive_exts,
        }
    }

    fn send(&mut self, ext: usize, message: FromExtension) {
        self.fast.send(&self.fast_exts[ext], message.clone());
        next_version(&mut self.fast, &self.indexed);
        self.naive.send(&self.naive_exts[ext], message);
    }

    /// Types `text` a character at a time, and returns the last Results.
    fn type_in(&mut self, text: &str, context: &str) -> Vec<ShownResult> {
        let mut typed = String::new();
        let mut fast = Vec::new();
        for c in text.chars() {
            typed.push(c);
            fast = self.fast.query(&self.fast_ui, &typed);
            self.naive.query(&self.naive_ui, "");
            let naive = self.naive.query(&self.naive_ui, &typed);
            assert_eq!(
                fast,
                naive,
                "{context}: typing {typed:?}: {:?} rather than {:?}",
                titles(&fast),
                titles(&naive)
            );
        }
        fast
    }

    fn pick(&mut self, result: &ShownResult) {
        for (h, ui, exts) in [
            (&mut self.fast, &self.fast_ui, &self.fast_exts),
            (&mut self.naive, &self.naive_ui, &self.naive_exts),
        ] {
            let action = &result.item.actions[0].id;
            run(
                h,
                ui,
                &result.extension,
                &result.item.id,
                &result.extension,
                action,
            );
            ui.drain();
            for ext in exts {
                ext.drain();
            }
        }
    }

    fn advance(&mut self, by: Duration) {
        self.fast.advance(by);
        self.naive.advance(by);
    }

    fn assert_same(&self, context: &str) {
        assert_eq!(
            self.fast_ui.results(),
            self.naive_ui.results(),
            "{context}: Results sent unasked"
        );
    }
}

#[test]
fn results_are_those_of_a_full_search_after_any_changes_picks_and_keystrokes() {
    for seed in 1..=100u64 {
        let mut random = Random(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let mut twins = Twins::new();
        for step in 0..40 {
            let context = format!("seed {seed}, step {step}");
            let ext = random.below(2);
            match random.below(6) {
                0 | 1 => {
                    let items = (0..1 + random.below(6))
                        .map(|_| random_item(&mut random))
                        .collect();
                    twins.send(ext, FromExtension::AddItems { items });
                }
                2 => {
                    let ids = (0..1 + random.below(3))
                        .map(|_| format!("item-{}", random.below(30)))
                        .collect();
                    twins.send(ext, FromExtension::RemoveItems { ids });
                }
                3 => {
                    let rows = twins.type_in(&random_query(&mut random), &context);
                    if !rows.is_empty() {
                        let row = rows[random.below(rows.len().min(5))].clone();
                        twins.pick(&row);
                    }
                }
                4 => {
                    twins.type_in(&random_query(&mut random), &context);
                }
                _ => {
                    let hours = random.below(30) as u64;
                    twins.advance(Duration::from_secs(hours * 3600 + 7 * 60));
                }
            }
            twins.assert_same(&context);
        }
    }
}
