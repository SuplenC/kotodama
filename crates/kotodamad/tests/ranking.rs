//! What a user expects to see first, as scenarios: what is installed, what was
//! picked before and when, what is typed, and the rows expected. Each test is
//! named after the expectation it checks, with its id in the ranking
//! questionnaire (M1, L3, ...) in its doc comment. The invariants at the end
//! hold for any query.

mod common;

use std::time::Duration;

use common::*;
use kotodama_protocol::*;
use kotodamad::rank::{Haystack, Matched, Query, Tiers};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// One user's desktop: one Extension, `apps`, and a Front end.
struct World {
    h: Harness,
    apps: Ext,
    ui: FrontEnd,
}

impl World {
    fn new(titles: &[&str]) -> Self {
        Self::with(titles.iter().map(|title| app(title)).collect(), "")
    }

    fn with(items: Vec<Item>, toml: &str) -> Self {
        let mut h = Harness::with_config(vec![plain("apps")], config(toml));
        let apps = h.up(0);
        h.add(&apps, items);
        let ui = h.front_end();
        World { h, apps, ui }
    }

    fn install(&mut self, items: Vec<Item>) {
        self.h.add(&self.apps, items);
    }

    fn rows(&mut self, query: &str) -> Vec<String> {
        titles(&self.h.query(&self.ui, query))
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    fn first(&mut self, query: &str) -> String {
        self.rows(query).into_iter().next().unwrap_or_default()
    }

    /// Types `query` and runs the primary Action of the row titled `title`.
    fn pick(&mut self, query: &str, title: &str) {
        self.pick_by(query, title, None);
    }

    /// Like `pick`, running the Action `action` instead.
    fn pick_by(&mut self, query: &str, title: &str, action: Option<&str>) {
        let rows = self.h.query(&self.ui, query);
        let row = rows
            .iter()
            .find(|row| row.item.title == title)
            .unwrap_or_else(|| panic!("no row {title:?} for {query:?} in {:?}", titles(&rows)));
        let action = action.unwrap_or(&row.item.actions[0].id).to_owned();
        let (owner, id) = (row.extension.clone(), row.item.id.clone());
        run(&mut self.h, &self.ui, &owner, &id, &owner, &action);
        self.ui.drain();
        self.apps.drain();
    }

    fn picks(&mut self, times: usize, query: &str, title: &str) {
        for _ in 0..times {
            self.pick(query, title);
        }
    }

    /// Picks once a day for `days` days.
    fn daily(&mut self, days: usize, query: &str, title: &str) {
        for _ in 0..days {
            self.pick(query, title);
            self.later(DAY);
        }
    }

    fn later(&mut self, by: Duration) {
        self.h.advance(by);
    }
}

fn with_keywords(mut item: Item, keywords: &[&str]) -> Item {
    item.keywords = keywords.iter().map(|&k| k.to_owned()).collect();
    item
}

fn with_subtitle(mut item: Item, subtitle: &str) -> Item {
    item.subtitle = Some(subtitle.into());
    item
}

fn rows(world: &mut World, query: &str) -> Vec<String> {
    world.rows(query)
}

// Matching, nothing learned yet.

/// M1
#[test]
fn an_exact_title_comes_first() {
    let mut w = World::new(&["Filezilla", "File Roller", "Files", "Files Backup"]);
    assert_eq!(w.first("files"), "Files");
}

/// M2
#[test]
fn a_word_starting_with_the_query_beats_the_query_inside_a_word() {
    let mut w = World::new(&["Determinate", "Waterm", "Terminal"]);
    assert_eq!(w.first("term"), "Terminal");
}

/// M3, W3
#[test]
fn initials_count_like_a_word_start_and_win_a_tie_with_one() {
    let mut w = World::new(&["VSCodium", "Visual Studio Code", "Volume Settings Control"]);
    assert_eq!(
        rows(&mut w, "vsc"),
        ["Visual Studio Code", "Volume Settings Control", "VSCodium"]
    );
    let mut w = World::new(&["Firefox", "File Finder"]);
    assert_eq!(rows(&mut w, "ff"), ["File Finder", "Firefox"]);
}

/// M4
#[test]
fn an_exact_keyword_beats_a_title_that_starts_with_the_query() {
    let mut w = World::with(
        vec![
            app("Browser History Viewer"),
            with_keywords(app("Firefox"), &["browser", "web"]),
        ],
        "",
    );
    assert_eq!(
        rows(&mut w, "browser"),
        ["Firefox", "Browser History Viewer"]
    );
}

/// M5
#[test]
fn subtitles_are_searched_by_their_words() {
    let mut w = World::with(
        vec![with_subtitle(app("Firefox"), "Web Browser"), app("Files")],
        "",
    );
    assert_eq!(rows(&mut w, "web browser"), ["Firefox"]);
    assert_eq!(
        rows(&mut w, "eb"),
        Vec::<String>::new(),
        "only at the start of a word"
    );
}

/// M6
#[test]
fn with_nothing_learned_the_shortest_title_wins_a_tie() {
    let mut w = World::new(&["Firefox", "Files", "Fish"]);
    assert_eq!(rows(&mut w, "fi"), ["Fish", "Files", "Firefox"]);
}

/// M7
#[test]
fn one_letter_finds_only_words_that_start_with_it() {
    let mut w = World::new(&["Terminal", "Thunar", "Telegram", "Steam", "Settings"]);
    let rows = rows(&mut w, "t");
    assert_eq!(rows[0], "Thunar");
    let mut sorted = rows.clone();
    sorted.sort();
    assert_eq!(sorted, ["Telegram", "Terminal", "Thunar"]);
}

/// M8
#[test]
fn letters_together_beat_letters_scattered() {
    let mut w = World::new(&["Color Dev", "Encode Tool", "Code"]);
    assert_eq!(rows(&mut w, "code"), ["Code", "Encode Tool", "Color Dev"]);
}

// Case, accents and typos.

/// T1
#[test]
fn capitals_in_the_query_do_not_matter() {
    let mut w = World::new(&["Firefox"]);
    assert_eq!(rows(&mut w, "FI"), ["Firefox"]);
}

/// T2, T3
#[test]
fn accents_do_not_matter_either_way() {
    let mut w = World::new(&["Żółw Notes"]);
    assert_eq!(rows(&mut w, "zolw"), ["Żółw Notes"]);
    let mut w = World::new(&["Zolw Notes"]);
    assert_eq!(rows(&mut w, "żółw"), ["Zolw Notes"]);
}

/// T4
#[test]
fn a_missing_letter_still_finds_it() {
    let mut w = World::new(&["Fire Alarm", "Firefox"]);
    assert_eq!(w.first("firfox"), "Firefox");
}

/// T8
#[test]
fn a_real_match_comes_first() {
    let mut w = World::new(&["Steam", "Stream Deck"]);
    assert_eq!(w.first("stream"), "Stream Deck");
}

// Several words.

/// W1
#[test]
fn every_word_must_match() {
    let mut w = World::new(&["Firefox", "Fire Alarm"]);
    assert_eq!(rows(&mut w, "fire alarm"), ["Fire Alarm"]);
}

/// W2
#[test]
fn words_match_in_any_order() {
    let mut w = World::new(&["Fire Alarm"]);
    assert_eq!(rows(&mut w, "alarm fire"), ["Fire Alarm"]);
}

/// W4
#[test]
fn a_trailing_space_asks_for_a_whole_word() {
    let mut w = World::new(&["Firefox", "Fi Calculator"]);
    assert_eq!(rows(&mut w, "fi "), ["Fi Calculator", "Firefox"]);
}

/// W5
#[test]
fn an_items_keyword_puts_it_first_with_the_rest_as_its_argument() {
    let mut go = app("Go: use a version");
    go.keyword = Some("go".into());
    go.actions[0].arguments = vec![Argument {
        name: "version".into(),
        placeholder: None,
        optional: false,
    }];
    let mut w = World::with(vec![app("Gnome Online"), go], "");
    assert_eq!(w.first("go 1.23"), "Go: use a version");
}

// Learned choices.

/// L1
#[test]
fn what_a_query_learned_applies_as_it_gets_longer() {
    let mut w = World::new(&["Fire Alarm", "Firefox"]);
    w.picks(3, "fi", "Firefox");
    assert_eq!(w.first("fir"), "Firefox");
}

/// L2
#[test]
fn what_a_query_learned_applies_to_a_shorter_one() {
    let mut w = World::new(&["Files", "Firefox", "Fish"]);
    w.picks(3, "fi", "Firefox");
    assert_eq!(rows(&mut w, "f")[0], "Firefox");
}

/// L3
#[test]
fn picks_after_a_long_query_teach_its_prefixes() {
    let mut w = World::new(&["Files", "Firefox", "Fish"]);
    // Files is used more, from a query that teaches "fi" nothing.
    w.picks(5, "fles", "Files");
    w.picks(3, "firef", "Firefox");
    assert_eq!(w.first("fi"), "Firefox");
}

/// L4
#[test]
fn it_takes_three_picks_to_learn() {
    let mut w = World::new(&["Files", "Firefox", "Fish"]);
    w.picks(5, "fles", "Files");
    w.picks(2, "fi", "Firefox");
    assert_eq!(w.first("fi"), "Files", "Frecency alone");
    w.pick("fi", "Firefox");
    assert_eq!(w.first("fi"), "Firefox");
}

/// L5, L6
#[test]
fn two_picks_of_something_else_take_over_but_one_does_not() {
    let mut w = World::new(&["Files", "Firefox", "Fish"]);
    w.picks(5, "fles", "Files");
    w.picks(3, "fi", "Firefox");
    w.pick("fi", "Fish");
    assert_eq!(w.first("fi"), "Firefox", "one stray pick");
    w.pick("fi", "Fish");
    assert_eq!(w.first("fi"), "Fish");
}

/// Conflict 3: the two picks are counted since the choice was last picked.
#[test]
fn picking_the_learned_choice_again_resets_the_count_to_take_over() {
    let mut w = World::new(&["Files", "Firefox", "Fish"]);
    w.picks(5, "fles", "Files");
    w.picks(3, "fi", "Firefox");
    w.pick("fi", "Fish");
    w.pick("fi", "Firefox");
    w.pick("fi", "Fish");
    assert_eq!(w.first("fi"), "Firefox");

    w.pick("fi", "Files");
    w.pick("fi", "Fish");
    assert_eq!(
        w.first("fi"),
        "Fish",
        "another pick between doesn't reset it"
    );
}

/// L7
#[test]
fn of_two_prefixes_the_one_picked_more_wins_and_on_a_tie_the_longer() {
    let mut w = World::new(&["Firefox", "Fire Alarm"]);
    w.picks(3, "fi", "Firefox");
    w.picks(3, "fire", "Fire Alarm");
    assert_eq!(w.first("fir"), "Fire Alarm");
    w.pick("fi", "Firefox");
    assert_eq!(w.first("fir"), "Firefox");
}

/// L8
#[test]
fn a_learned_choice_fades_when_it_is_not_used() {
    let mut w = World::new(&["Files", "Firefox", "Fish"]);
    w.picks(3, "fi", "Firefox");
    w.later(180 * DAY);
    w.daily(7, "files", "Files");
    assert_eq!(rows(&mut w, "fi"), ["Files", "Firefox", "Fish"]);
}

// Frecency.

/// R1
#[test]
fn a_few_picks_today_beat_many_a_month_ago() {
    let mut w = World::new(&["Alpha Tool", "Beta Tool"]);
    w.picks(10, "alpha", "Alpha Tool");
    w.later(30 * DAY);
    w.picks(3, "beta", "Beta Tool");
    assert_eq!(w.first("tool"), "Beta Tool");
}

/// R2
#[test]
fn many_picks_a_week_ago_beat_a_few_today() {
    let mut w = World::new(&["Alpha Tool", "Beta Tool"]);
    w.picks(10, "alpha", "Alpha Tool");
    w.later(7 * DAY);
    w.picks(3, "beta", "Beta Tool");
    assert_eq!(w.first("tool"), "Alpha Tool");
}

/// R3
#[test]
fn a_habit_survives_a_week_away() {
    let mut w = World::new(&["Alpha Tool", "Beta Tool"]);
    for day in 0..28 {
        if day % 7 < 5 {
            w.pick("alpha", "Alpha Tool");
        }
        if day == 27 {
            w.picks(2, "beta", "Beta Tool");
        }
        w.later(DAY);
    }
    w.later(7 * DAY);
    assert_eq!(w.first("tool"), "Alpha Tool");
}

/// R4, conflict 4
#[test]
fn one_pick_breaks_a_tie_but_never_beats_a_better_match() {
    let mut w = World::new(&["Files", "Firefox", "Fish", "Fi"]);
    w.pick("firefox", "Firefox");
    w.later(DAY);
    assert_eq!(rows(&mut w, "fi"), ["Fi", "Firefox", "Fish", "Files"]);
}

/// R5
#[test]
fn a_habit_beats_the_last_thing_picked() {
    let mut w = World::new(&["Files", "Firefox", "Fish"]);
    w.daily(100, "firefox", "Firefox");
    w.pick("fish", "Fish");
    w.later(Duration::from_secs(300));
    assert_eq!(w.first("fi"), "Firefox");
}

/// R7
#[test]
fn a_pick_is_never_forgotten_only_weaker() {
    let mut w = World::new(&["Files", "Firefox", "Fish"]);
    w.pick("firefox", "Firefox");
    w.later(365 * DAY);
    assert_eq!(w.first("fi"), "Firefox");
    w.pick("fish", "Fish");
    assert_eq!(w.first("fi"), "Fish");
}

/// R8
#[test]
fn picks_through_an_alias_count_for_the_alias_alone() {
    let mut w = World::with(
        vec![app("Files"), app("Firefox"), app("Fish")],
        "[aliases]\nweb = { extension = \"apps\", item = \"firefox\" }",
    );
    w.daily(10, "web", "Firefox");
    assert_eq!(rows(&mut w, "f"), ["Fish", "Files", "Firefox"]);
}

// Exact matches against what was learned.

/// E1, conflict 2
#[test]
fn heavy_use_cannot_lift_a_match_inside_a_word_above_an_exact_title() {
    let mut w = World::new(&["Encode Tool", "Code"]);
    w.daily(20, "encode", "Encode Tool");
    assert_eq!(rows(&mut w, "code"), ["Code", "Encode Tool"]);
}

/// E2, conflict 1
#[test]
fn a_learned_choice_beats_the_exact_title_it_was_picked_over() {
    let mut w = World::new(&["Encode Tool", "Code"]);
    w.picks(3, "code", "Encode Tool");
    assert_eq!(rows(&mut w, "code"), ["Encode Tool", "Code"]);
}

/// E3, conflict 1
#[test]
fn a_new_exact_title_beats_a_learned_choice_until_it_is_passed_over() {
    let mut w = World::new(&["Files", "Firefox", "Fish"]);
    w.picks(3, "fi", "Firefox");
    w.install(vec![app("Fi")]);
    assert_eq!(w.first("fi"), "Fi");
    w.pick("fi", "Firefox");
    assert_eq!(w.first("fi"), "Firefox");
}

/// E4, conflict 2
#[test]
fn heavy_use_lifts_a_title_starting_with_the_query_above_an_exact_one() {
    let mut w = World::new(&["Files", "Files Backup"]);
    w.daily(10, "backup", "Files Backup");
    assert_eq!(rows(&mut w, "files"), ["Files Backup", "Files"]);
}

/// E5
#[test]
fn heavy_use_beats_a_title_that_starts_with_the_query() {
    let mut w = World::new(&["Code::Blocks", "Visual Studio Code"]);
    w.daily(10, "visual", "Visual Studio Code");
    assert_eq!(w.first("code"), "Visual Studio Code");
}

/// Conflict 2: one tier, not two.
#[test]
fn heavy_use_lifts_a_match_inside_a_word_only_to_a_word_start() {
    let mut w = World::new(&["Terminal", "Determinate"]);
    w.daily(10, "deter", "Determinate");
    assert_eq!(rows(&mut w, "term"), ["Determinate", "Terminal"]);
    let mut w = World::new(&["Fire", "Firefox"]);
    w.daily(10, "firefox", "Firefox");
    assert_eq!(rows(&mut w, "fire"), ["Firefox", "Fire"]);
}

// Aliases.

const FF: &str = "[aliases]\nff = { extension = \"apps\", item = \"firefox\" }";

/// A1
#[test]
fn part_of_an_alias_does_nothing() {
    let mut w = World::with(vec![app("Files"), app("Firefox"), app("Fish")], FF);
    assert_eq!(rows(&mut w, "f"), ["Fish", "Files", "Firefox"]);
}

/// A2
#[test]
fn an_exact_title_wins_a_tie_with_an_alias_and_then_use_decides() {
    let mut w = World::with(vec![app("Firefox"), app("ff")], FF);
    assert_eq!(rows(&mut w, "ff"), ["ff", "Firefox"]);
    w.picks(2, "ff", "Firefox");
    assert_eq!(w.first("ff"), "Firefox");
}

/// A3
#[test]
fn an_alias_beats_a_learned_choice() {
    let mut w = World::with(
        vec![app("Firefox"), app("Fish")],
        "[aliases]\nfi = { extension = \"apps\", item = \"fish\" }",
    );
    w.picks(3, "fi", "Firefox");
    assert_eq!(rows(&mut w, "fi"), ["Fish", "Firefox"]);
}

/// A4
#[test]
fn an_alias_beats_titles_that_start_with_it() {
    let mut w = World::with(
        vec![app("Webcam"), app("WebStorm"), app("Firefox")],
        "[aliases]\nweb = { extension = \"apps\", item = \"firefox\" }",
    );
    assert_eq!(w.first("web"), "Firefox");
}

/// A5
#[test]
fn an_alias_takes_the_rest_of_the_query_as_its_argument() {
    let mut w = World::with(vec![app("Firefox"), app("Private Notes")], FF);
    assert_eq!(w.first("ff private"), "Firefox");
}

// Quicklinks.

const YT: &str = "[quicklinks.yt]\ntitle = \"Search YouTube\"\n\
                  url = \"https://www.youtube.com/results?search_query={query}\"";
const GH: &str = "[quicklinks.gh]\ntitle = \"GitHub\"\nurl = \"https://github.com\"";

/// K1
#[test]
fn a_quicklink_keyword_alone_opens_it_with_an_empty_query() {
    let mut w = World::with(vec![app("Yelp")], YT);
    assert_eq!(w.first("yt"), "Search YouTube");
    w.pick("yt", "Search YouTube");
    assert_eq!(
        *w.h.outputs.1.borrow(),
        [vec![
            "xdg-open".to_owned(),
            "https://www.youtube.com/results?search_query=".into()
        ]]
    );
}

/// K2, K3
#[test]
fn a_quicklinks_row_shows_the_query_it_will_search_for() {
    let mut w = World::with(vec![], YT);
    assert_eq!(w.first("yt cats"), "Search YouTube for “cats”");
}

/// K5
#[test]
fn part_of_a_quicklink_keyword_shows_it_among_the_results() {
    let mut w = World::with(vec![app("Yelp")], YT);
    let rows = rows(&mut w, "y");
    assert!(rows.contains(&"Search YouTube".to_owned()), "{rows:?}");
    assert!(rows.contains(&"Yelp".to_owned()), "{rows:?}");
}

/// K6
#[test]
fn between_a_quicklink_keyword_and_an_app_use_decides() {
    let mut w = World::with(vec![app("GHex")], GH);
    assert_eq!(rows(&mut w, "gh"), ["GitHub", "GHex"]);
    w.daily(7, "ghex", "GHex");
    assert_eq!(w.first("gh"), "GHex");

    let mut w = World::with(vec![app("GHex")], GH);
    w.picks(3, "gh", "GHex");
    assert_eq!(w.first("gh"), "GHex", "a Learned choice picked over it");
}

/// K7
#[test]
fn a_quicklink_without_a_query_ignores_more_words() {
    let mut w = World::with(vec![], GH);
    assert_eq!(w.first("gh kotodama"), "GitHub");
}

/// K9
#[test]
fn frecency_applies_to_quicklinks() {
    let mut w = World::with(vec![app("YouTube Music")], YT);
    w.daily(7, "yt cats", "Search YouTube for “cats”");
    assert_eq!(w.first("you"), "Search YouTube");
}

// What counts as a pick.

/// P1
#[test]
fn any_action_counts_as_a_pick() {
    let mut firefox = app("Firefox");
    firefox.actions.push(Action {
        id: "copy-path".into(),
        title: "Copy path".into(),
        shortcut: None,
        arguments: vec![],
    });
    let mut w = World::with(vec![app("Files"), firefox, app("Fish")], "");
    w.pick_by("firefox", "Firefox", Some("copy-path"));
    assert_eq!(w.first("fi"), "Firefox");
}

/// Z3
#[test]
fn a_pick_counts_at_once() {
    let mut w = World::new(&["Files", "Firefox", "Fish"]);
    w.pick("fi", "Firefox");
    assert_eq!(w.first("fi"), "Firefox");
}

// Invariants, over a desktop's worth of apps and a range of queries.

const DESKTOP: &[&str] = &[
    "Files",
    "Firefox",
    "Fish",
    "Fire Alarm",
    "File Finder",
    "Filezilla",
    "Terminal",
    "Thunar",
    "Telegram",
    "Steam",
    "Settings",
    "Stream Deck",
    "Visual Studio Code",
    "VSCodium",
    "Code",
    "Encode Tool",
    "Color Decoder",
    "Calculator",
    "Calendar",
    "Webcam",
    "WebStorm",
    "Yelp",
    "GHex",
    "Żółw Notes",
    "Disks",
    "System Monitor",
];

const QUERIES: &[&str] = &[
    "f",
    "fi",
    "fir",
    "fire",
    "fil",
    "files",
    "t",
    "te",
    "term",
    "s",
    "st",
    "set",
    "code",
    "vsc",
    "co",
    "cal",
    "web",
    "w",
    "y",
    "gh",
    "zolw",
    "sm",
    "fire alarm",
    "fi ",
    "mon sys",
    "dis",
];

#[test]
fn the_same_query_gives_the_same_rows() {
    let mut w = World::new(DESKTOP);
    for query in QUERIES {
        assert_eq!(rows(&mut w, query), rows(&mut w, query), "{query:?}");
    }
}

#[test]
fn with_nothing_learned_a_better_match_is_never_below_a_worse_one() {
    let mut w = World::new(DESKTOP);
    let mut tiers = Tiers::default();
    for query in QUERIES {
        let folded = Query::new(query);
        let rows = w.h.query(&w.ui, query);
        let ranked: Vec<u8> = rows
            .iter()
            .map(|row| {
                tiers
                    .item(&Haystack::new(&row.item), &row.item, &folded, Matched::ALL)
                    .unwrap_or_else(|| panic!("{:?} doesn't match {query:?}", row.item.title))
                    .coarse()
            })
            .collect();
        assert!(
            ranked.is_sorted_by(|a, b| a >= b),
            "{query:?}: {:?} {ranked:?}",
            titles(&rows)
        );
    }
}

#[test]
fn with_nothing_learned_an_exact_title_is_first() {
    let mut w = World::new(DESKTOP);
    for title in DESKTOP {
        assert_eq!(w.first(&title.to_lowercase()), *title);
    }
}
