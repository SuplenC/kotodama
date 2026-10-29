//! What a user expects to see first, as scenarios: what is installed, what was
//! picked before and when, what is typed, and the rows expected. Each test is
//! named after the expectation it checks, with its id in the ranking
//! questionnaire (M1, L3, ...) in its doc comment. The invariants at the end
//! hold for any query.

#[path = "support/harness.rs"]
#[expect(dead_code, reason = "each test uses part of the harness")]
mod harness;

use std::time::Duration;

use harness::*;
use kotodama_protocol::*;
use kotodamad::daemon::Event;
use kotodamad::history::History;
use kotodamad::rank::Haystack;
use kotodamad::rank::Matched;
use kotodamad::rank::Query;
use kotodamad::rank::Tier;
use kotodamad::rank::Tiers;

const DAY: Duration = Duration::from_hours(24);

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
        Self::with_history(items, toml, History::default())
    }

    fn with_history(items: Vec<Item>, toml: &str, history: History) -> Self {
        let mut h = Harness::with_history(vec![plain("apps")], config(toml), history);
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

    /// Waits for the next `hour` o'clock.
    fn at(&mut self, hour: u64) {
        self.h.advance_to(hour);
    }

    /// Waits for the next `hour` o'clock on the day of the week `day`, 0 for
    /// Monday to 6 for Sunday.
    fn on(&mut self, day: u8, hour: u64) {
        self.h.advance_to_on(day, hour);
    }

    /// Replaces the config, as when the user edits it.
    fn reconfigure(&mut self, toml: &str) {
        self.h.event(Event::Config(config(toml)));
        self.ui.drain();
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

/// M4: an exact keyword counts as a word start, and the shorter title wins
/// the tie.
#[test]
fn an_exact_keyword_ties_with_a_title_that_starts_with_the_query() {
    let mut w = World::with(
        vec![
            app("Browser History Viewer"),
            with_keywords(app("Firefox"), &["browser", "web"]),
        ],
        "",
    );
    assert_eq!(rows(&mut w, "browser"), ["Firefox", "Browser History Viewer"]);

    let mut w = World::with(
        vec![
            with_keywords(app("Mozilla Firefox"), &["browser", "web"]),
            app("Browser Tabs"),
        ],
        "",
    );
    assert_eq!(rows(&mut w, "browser"), ["Browser Tabs", "Mozilla Firefox"]);
}

/// M5
#[test]
fn subtitles_are_searched_by_their_words() {
    let mut w = World::with(vec![with_subtitle(app("Firefox"), "Web Browser"), app("Files")], "");
    assert_eq!(rows(&mut w, "web browser"), ["Firefox"]);
    assert_eq!(rows(&mut w, "eb"), Vec::<String>::new(), "only at the start of a word");
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

// The kind of Result.

fn file(title: &str) -> Item {
    item(
        title,
        title,
        ItemKind::File {
            path: format!("/home/me/{title}").into(),
        },
    )
}

fn bookmark(title: &str) -> Item {
    let id = title.to_lowercase().replace(' ', "-");
    item(
        &id,
        title,
        ItemKind::Url {
            url: format!("https://{id}.example"),
        },
    )
}

fn named_project(title: &str) -> Item {
    let mut item = project(title);
    item.title = title.into();
    item
}

/// M9: though the shorter title would win a tie.
#[test]
fn on_an_equal_match_apps_come_before_projects_bookmarks_and_files() {
    let mut w = World::with(
        vec![
            file("notes.txt"),
            bookmark("Notes Online"),
            named_project("notebook-tools"),
            app("Notebook Organizer"),
        ],
        "",
    );
    assert_eq!(
        rows(&mut w, "note"),
        ["Notebook Organizer", "notebook-tools", "Notes Online", "notes.txt"]
    );
}

/// M9: the kind order applies within a Tier.
#[test]
fn the_kind_of_result_never_beats_a_better_match() {
    let mut w = World::with(vec![app("Notebook"), file("note")], "");
    assert_eq!(rows(&mut w, "note"), ["note", "Notebook"]);
}

/// M10
#[test]
fn a_well_used_file_beats_a_never_used_app() {
    let mut w = World::with(vec![app("Notebook"), file("notes.txt")], "");
    // Through a query that teaches `note` no Learned choice, and too little to
    // lift it a Tier: only its Frecency counts.
    w.daily(3, "txt", "notes.txt");
    assert_eq!(rows(&mut w, "note"), ["notes.txt", "Notebook"]);
}

/// M9: the order is a setting, and `*` places the kinds it doesn't name.
#[test]
fn the_kind_order_is_a_setting() {
    let items = || {
        vec![
            app("Notebook Organizer"),
            file("notes.txt"),
            item(
                "n",
                "Notes Clip",
                ItemKind::Namespaced(NamespacedKind::new("apps.clip").unwrap()),
            ),
        ]
    };
    let mut w = World::with(items(), "");
    assert_eq!(
        rows(&mut w, "note"),
        ["Notebook Organizer", "Notes Clip", "notes.txt"],
        "by default, other kinds come between bookmarks and files"
    );
    let mut w = World::with(items(), "[ranking]\nkinds = [\"file\", \"*\", \"app\"]");
    assert_eq!(rows(&mut w, "note"), ["notes.txt", "Notes Clip", "Notebook Organizer"]);
    w.reconfigure("[ranking]\nkinds = [\"apps.clip\", \"app\"]");
    assert_eq!(
        rows(&mut w, "note"),
        ["Notes Clip", "Notebook Organizer", "notes.txt"],
        "without `*`, the kinds not named come last; and the order reloads with the config"
    );
}

/// M9: a kind that isn't one is left out, and a list that isn't one keeps the
/// default.
#[test]
fn a_kind_order_that_does_not_parse_keeps_what_does() {
    let items = || vec![app("Notebook Organizer"), file("notes.txt")];
    let mut w = World::with(items(), "[ranking]\nkinds = [\"bogus\", 3, \"file\"]");
    assert_eq!(rows(&mut w, "note"), ["notes.txt", "Notebook Organizer"]);
    let mut w = World::with(items(), "[ranking]\nkinds = \"file\"");
    assert_eq!(rows(&mut w, "note"), ["Notebook Organizer", "notes.txt"]);
}

/// D1: of the Results of one thing, the one picked through most still leads.
#[test]
fn the_kind_order_does_not_decide_between_results_of_one_thing() {
    let mut firefox = app("Firefox");
    firefox.identity = Some("app:firefox.desktop".into());
    let mut desktop_file = file("firefox.desktop");
    desktop_file.identity = Some("app:firefox.desktop".into());
    let mut w = World::with(vec![firefox, desktop_file], "");
    assert_eq!(rows(&mut w, "fire"), ["Firefox", "firefox.desktop"]);
    w.pick("fire", "firefox.desktop");
    assert_eq!(rows(&mut w, "fire"), ["firefox.desktop", "Firefox"]);
}

/// M1: though the shorter title would win a tie.
#[test]
fn an_exact_title_beats_an_exact_keyword() {
    let mut w = World::with(vec![with_keywords(app("Ash"), &["fire"]), app("Fire")], "");
    assert_eq!(rows(&mut w, "fire"), ["Fire", "Ash"]);
}

/// An Extension's Item about an app, as "Remove Google Chrome" is about Google Chrome.
fn about(title: &str, keywords: &[&str]) -> Item {
    let mut about = item(
        &title.to_lowercase().replace(' ', "-"),
        title,
        ItemKind::Namespaced(NamespacedKind::new("apps.package").expect("a valid kind")),
    );
    about.actions = vec![Action::new("remove", "Remove").confirm()];
    with_keywords(about, keywords)
}

// Regression: #245.
#[test]
fn an_installed_app_beats_an_item_about_it_that_lists_its_name_as_a_keyword() {
    let mut w = World::with(
        vec![
            about("Remove Google Chrome", &["chrome"]),
            about("Install Google Chrome", &["chrome", "google"]),
            app("Google Chrome"),
        ],
        "",
    );
    assert_eq!(w.first("chrome"), "Google Chrome");
}

#[test]
fn an_items_own_keyword_still_ranks_as_an_exact_match() {
    let mut remove = about("Remove Google Chrome", &[]);
    remove.keyword = Some("chrome".into());
    let mut w = World::with(vec![app("Google Chrome"), remove], "");
    assert_eq!(rows(&mut w, "chrome"), ["Remove Google Chrome", "Google Chrome"]);
}

/// An app whose desktop id, as a Chrome web app's is, names the browser it
/// runs in.
fn web_app(title: &str, desktop_id: &str) -> Item {
    let mut web_app = app(title);
    web_app.weak_keywords = vec![desktop_id.into()];
    web_app
}

// Regression: #246.
#[test]
fn an_app_found_by_its_desktop_id_alone_never_passes_an_app_whose_title_matches() {
    let mut w = World::with(
        vec![
            app("Google Chrome"),
            app("Visual Studio Code"),
            web_app("Google Chat", "chrome-abcdef-Default"),
        ],
        "",
    );
    w.daily(100, "google chat", "Google Chat");

    assert_eq!(rows(&mut w, "chrome"), ["Google Chrome", "Google Chat"]);
    assert_eq!(w.first("chat"), "Google Chat");
    assert_eq!(
        w.first("code"),
        "Visual Studio Code",
        "`code` is only scattered in the id"
    );
}

/// The rows for `query` of a desktop with `apps` and the emoji Extension's
/// real Items.
fn with_emoji(apps: &[&str], query: &str) -> Vec<String> {
    let mut h = Harness::with_config(vec![plain("apps"), plain("emoji")], config(""));
    let (apps_ext, emoji) = (h.up(0), h.up(1));
    h.add(&apps_ext, apps.iter().map(|title| app(title)).collect());
    h.add(&emoji, kotodama_emoji::items());
    let ui = h.front_end();
    titles(&h.query(&ui, query)).into_iter().map(str::to_owned).collect()
}

#[test]
fn the_emoji_named_as_the_query_comes_first_among_emoji() {
    let rows = with_emoji(&[], "fire");
    assert_eq!(rows[0], "🔥 Fire", "{rows:?}");
}

#[test]
fn an_app_named_as_the_query_comes_before_the_emoji_of_that_name() {
    let rows = with_emoji(&["Fire"], "fire");
    assert_eq!(rows[..2], ["Fire", "🔥 Fire"], "{rows:?}");
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

/// T5
#[test]
fn two_letters_swapped_still_find_it() {
    let mut w = World::new(&["Firefox"]);
    assert_eq!(rows(&mut w, "fierfox"), ["Firefox"]);
}

/// T6
#[test]
fn a_wrong_letter_still_finds_it() {
    let mut w = World::new(&["Firefox", "Calculator"]);
    assert_eq!(rows(&mut w, "calculater"), ["Calculator"]);
}

/// T7
#[test]
fn an_extra_letter_still_finds_it() {
    let mut w = World::new(&["Firefox"]);
    assert_eq!(rows(&mut w, "firefoxx"), ["Firefox"]);
}

/// T8
#[test]
fn a_real_match_comes_first() {
    let mut w = World::new(&["Steam", "Stream Deck"]);
    assert_eq!(rows(&mut w, "stream"), ["Stream Deck", "Steam"]);
}

/// T8: however much the near miss is used.
#[test]
fn use_never_lifts_a_near_miss_above_a_real_match() {
    let mut w = World::new(&["Steam", "Stream Deck"]);
    w.daily(30, "steam", "Steam");
    assert_eq!(rows(&mut w, "stream"), ["Stream Deck", "Steam"]);
}

/// T9
#[test]
fn a_query_has_at_most_one_typo_per_four_letters() {
    let mut w = World::new(&["Firefox", "Calculator"]);
    assert_eq!(rows(&mut w, "fri"), Vec::<String>::new(), "3 letters, no typo");
    assert_eq!(rows(&mut w, "frie"), ["Firefox"], "4 letters, one typo");
    assert_eq!(rows(&mut w, "cakcuka"), Vec::<String>::new(), "7 letters, one typo");
    assert_eq!(rows(&mut w, "cakcukat"), ["Calculator"], "8 letters, two");
}

/// T9: each word of the query has its own typos.
#[test]
fn every_word_may_have_its_typos() {
    let mut w = World::new(&["Fire Alarm", "Firefox"]);
    assert_eq!(rows(&mut w, "frie alram"), ["Fire Alarm"]);
    assert_eq!(
        rows(&mut w, "fire arl"),
        Vec::<String>::new(),
        "3 letters of a word, no typo, however long the query"
    );
}

/// T9: people rarely mistype the first letter, and holding to it keeps a
/// short query from nearly matching most words.
#[test]
fn the_first_letter_is_never_a_typo() {
    let mut w = World::new(&["Firefox"]);
    assert_eq!(rows(&mut w, "virefox"), Vec::<String>::new());
    assert_eq!(rows(&mut w, "ifrefox"), Vec::<String>::new());
}

/// Near misses rank below every real match, so they aren't looked for once
/// real matches fill the rows on screen.
#[test]
fn near_misses_only_fill_rows_that_real_matches_leave_empty() {
    let mut w = World::new(&["Steam", "Stream Deck"]);
    w.h.front_end_sends(&w.ui, FromFrontEnd::Viewport { top: 0, rows: 1 });
    w.ui.drain();
    assert_eq!(rows(&mut w, "stream"), ["Stream Deck"]);
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
    go.actions[0].arguments = vec![Argument::new("version")];
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
    assert_eq!(w.first("fi"), "Fish", "another pick between doesn't reset it");
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

/// R6: in the morning, the tool used every morning comes first, though the
/// one used every evening was picked more recently.
#[test]
fn a_morning_habit_ranks_higher_in_the_morning() {
    let mut w = World::new(&["Alpha Tool", "Beta Tool"]);
    for _ in 0..10 {
        w.at(8);
        w.pick("alpha", "Alpha Tool");
        w.at(20);
        w.pick("beta", "Beta Tool");
    }
    w.at(8);
    assert_eq!(w.first("tool"), "Alpha Tool", "in the morning");
    w.at(20);
    assert_eq!(w.first("tool"), "Beta Tool", "in the evening");
    w.at(14);
    assert_eq!(w.first("tool"), "Beta Tool", "at neither, the more recent");
}

const MONDAY: u8 = 0;
const TUESDAY: u8 = 1;
const THURSDAY: u8 = 3;
const SATURDAY: u8 = 5;
const SUNDAY: u8 = 6;

/// Ten weeks of a tool used two weekday mornings a week, and another used
/// both weekend mornings, the weekday tool picked last.
fn weekly_habits(w: &mut World) {
    for _ in 0..10 {
        w.on(SATURDAY, 9);
        w.pick("play", "Play Tool");
        w.on(SUNDAY, 9);
        w.pick("play", "Play Tool");
        w.on(TUESDAY, 9);
        w.pick("work", "Work Tool");
        w.on(THURSDAY, 9);
        w.pick("work", "Work Tool");
    }
}

/// R6: on a Saturday morning, the tool used every weekend morning comes
/// first, though the one used on weekday mornings was picked more recently.
#[test]
fn a_weekday_habit_ranks_higher_on_a_weekday() {
    let mut w = World::new(&["Work Tool", "Play Tool"]);
    weekly_habits(&mut w);
    w.on(SATURDAY, 9);
    assert_eq!(w.first("tool"), "Play Tool", "on a Saturday");
    w.on(TUESDAY, 9);
    assert_eq!(w.first("tool"), "Work Tool", "on a Tuesday");
}

/// R6: on a Monday morning the Empty query shows what was used over the
/// weekend when nothing else was.
#[test]
fn on_a_weekday_the_empty_query_shows_what_is_used_on_weekdays_first() {
    let mut w = World::new(&["Work Tool", "Play Tool", "Mail"]);
    weekly_habits(&mut w);
    w.on(SATURDAY, 9);
    assert_eq!(rows(&mut w, ""), ["Play Tool", "Work Tool", "apps"]);
    w.on(MONDAY, 9);
    assert_eq!(rows(&mut w, ""), ["Work Tool", "Play Tool", "apps"]);

    let mut w = World::new(&["Work Tool", "Play Tool", "Mail"]);
    for _ in 0..10 {
        w.on(SATURDAY, 9);
        w.pick("play", "Play Tool");
    }
    w.on(MONDAY, 9);
    assert_eq!(rows(&mut w, ""), ["Play Tool", "apps"], "nothing else is used");
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

/// Habit climbs Tiers as the evidence grows: a week of daily use is worth
/// one, and each further Tier takes six times as much.
#[test]
fn of_two_heavily_used_matches_inside_a_word_the_one_used_more_comes_first() {
    let mut w = World::new(&["Determinate", "Waterm"]);
    w.picks(300, "determinate", "Determinate");
    w.picks(60, "waterm", "Waterm");

    assert_eq!(rows(&mut w, "term"), ["Determinate", "Waterm"]);
}

#[test]
fn habit_climbs_tiers_as_picks_accumulate() {
    let mut w = World::new(&["Terminal", "Determinate", "Tuner Remote Monitor"]);
    assert_eq!(
        rows(&mut w, "term"),
        ["Terminal", "Determinate", "Tuner Remote Monitor"],
        "a word start, a match inside a word, letters scattered"
    );

    w.daily(7, "tuner", "Tuner Remote Monitor");
    assert_eq!(
        rows(&mut w, "term"),
        ["Terminal", "Tuner Remote Monitor", "Determinate"],
        "a week of daily use: one Tier"
    );

    for _ in 0..30 {
        w.picks(3, "tuner", "Tuner Remote Monitor");
        w.later(DAY);
    }
    assert_eq!(
        rows(&mut w, "term"),
        ["Tuner Remote Monitor", "Terminal", "Determinate"],
        "a month of use three times a day: two Tiers"
    );
}

/// However much it is used, a match inside a word or scattered stops
/// below an exact title.
#[test]
fn no_amount_of_use_lifts_a_scattered_match_past_an_exact_title() {
    let mut w = World::with(
        vec![app("Term"), app("Terminal"), app("Tuner Remote Monitor")],
        "[history]\nlift_threshold = 0.1",
    );
    w.daily(30, "tuner", "Tuner Remote Monitor");
    assert_eq!(rows(&mut w, "term"), ["Term", "Tuner Remote Monitor", "Terminal"]);
}

/// However much it is used, a near miss stays below letters scattered.
#[test]
fn no_amount_of_use_lifts_a_near_miss_above_letters_scattered() {
    let mut w = World::with(
        vec![app("Steam"), app("Sticky Remote Editor Amp Monitor")],
        "[history]\nlift_threshold = 0.1",
    );
    w.daily(30, "steam", "Steam");
    assert_eq!(rows(&mut w, "stream"), ["Sticky Remote Editor Amp Monitor", "Steam"]);
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

/// K4
#[test]
fn a_quicklink_is_found_by_its_title() {
    let mut w = World::with(vec![app("Yelp")], YT);
    assert_eq!(w.first("youtube"), "Search YouTube");
}

/// K4
#[test]
fn a_quicklinks_title_ranks_by_its_tier_like_any_title() {
    let mut w = World::with(vec![app("YouTube")], YT);
    assert_eq!(rows(&mut w, "youtube"), ["YouTube", "Search YouTube"]);
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

// Apps' entry Items.

/// The rows for `query`, as Extension and title, of a desktop with the
/// applications `titles` and an Extension whose app is titled `title` and
/// named by `word`.
fn beside_an_app(titles: &[&str], title: &str, word: &str, query: &str) -> Vec<(String, String)> {
    let other = manifest(&format!(
        "name = \"other\"\ncommand = [\"x\"]\nword = \"{word}\"\ntitle = \"{title}\""
    ));
    let mut h = Harness::with_config(vec![plain("apps"), other], config(""));
    let apps = h.up(0);
    let other = h.up(1);
    // It sends no Items, so its app's entry Item shows once it says it can
    // do something.
    h.send(
        &other,
        FromExtension::Status {
            usable: true,
            note: None,
        },
    );
    h.add(&apps, titles.iter().map(|title| app(title)).collect());
    let ui = h.front_end();
    h.query(&ui, query)
        .into_iter()
        .map(|row| (row.extension, row.item.title))
        .collect()
}

#[test]
fn an_application_ranks_above_the_app_whose_word_is_typed() {
    let rows = beside_an_app(&["Calculator"], "Calculator", "calc", "calc");
    assert_eq!(
        rows,
        [
            ("apps".to_owned(), "Calculator".to_owned()),
            ("kotodama".to_owned(), "Calculator".to_owned()),
        ]
    );
}

#[test]
fn applications_starting_with_an_apps_title_rank_above_it() {
    let rows = beside_an_app(&["System Monitor", "System Settings"], "System", "system", "system");
    let titles: Vec<&str> = rows.iter().map(|(_, title)| title.as_str()).collect();
    assert_eq!(titles, ["System Monitor", "System Settings", "System"]);
}

#[test]
fn the_apps_entry_item_is_still_first_when_nothing_else_matches() {
    let rows = beside_an_app(&["Firefox"], "Calculator", "calc", "calc");
    assert_eq!(rows[0], ("kotodama".to_owned(), "Calculator".to_owned()));
}

// What counts as a pick.

/// P1
#[test]
fn any_action_counts_as_a_pick() {
    let mut firefox = app("Firefox");
    firefox.actions.push(Action::new("copy-path", "Copy path"));
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

// The Empty query.

const HABITS: &[&str] = &[
    "Mail", "News", "Calendar", "Music", "Editor", "Games", "Files", "Terminal",
];

/// Ten days of picks: three habits each morning, three each evening.
fn habits(w: &mut World) {
    for _ in 0..10 {
        w.at(8);
        w.picks(3, "mail", "Mail");
        w.picks(2, "news", "News");
        w.pick("calendar", "Calendar");
        w.at(20);
        w.picks(3, "music", "Music");
        w.picks(2, "editor", "Editor");
        w.pick("games", "Games");
    }
}

const MENU: &str = "[empty_query]\nmenu = [\n\
                    { extension = \"apps\", item = \"files\" },\n\
                    { extension = \"kotodama\", item = \"clear-history\" },\n\
                    { extension = \"apps\", item = \"mail\" },\n]";

/// Z1
#[test]
fn the_empty_query_shows_the_three_most_used_at_this_time_of_day_then_the_menu() {
    let mut w = World::with(HABITS.iter().map(|title| app(title)).collect(), MENU);
    habits(&mut w);
    w.at(8);
    assert_eq!(
        rows(&mut w, ""),
        ["Mail", "News", "Calendar", "Files", "Clear history"],
        "in the morning"
    );
    w.at(20);
    assert_eq!(
        rows(&mut w, ""),
        ["Music", "Editor", "Games", "Files", "Clear history", "Mail"],
        "in the evening"
    );
}

/// Z1: however the menu opens, it shows what is used at that time of day.
#[test]
fn showing_the_menu_in_another_part_of_the_day_ranks_the_empty_query_again() {
    let mut w = World::new(HABITS);
    habits(&mut w);
    w.at(8);
    assert_eq!(rows(&mut w, ""), ["Mail", "News", "Calendar", "apps"]);

    w.later(Duration::from_mins(30));
    w.h.front_end_sends(&w.ui, FromFrontEnd::Shown);
    assert!(w.ui.results().is_none(), "shown in the same part of the day");

    w.at(20);
    assert!(w.ui.results().is_none(), "nothing is ranked while hidden");
    w.h.front_end_sends(&w.ui, FromFrontEnd::Shown);
    let evening = w.ui.results().expect("shown in the evening");
    assert_eq!(titles(&evening), ["Music", "Editor", "Games", "apps"]);

    w.later(Duration::from_mins(50));
    w.h.front_end_sends(&w.ui, FromFrontEnd::Shown);
    assert!(w.ui.results().is_none(), "shown again in the same part");

    w.later(DAY);
    w.h.front_end_sends(&w.ui, FromFrontEnd::Shown);
    assert!(w.ui.results().is_some(), "shown in the same part a day later");
}

/// Z1: showing the menu leaves a typed query's Results alone.
#[test]
fn showing_the_menu_on_a_typed_query_ranks_nothing_again() {
    let mut w = World::new(HABITS);
    habits(&mut w);
    w.at(8);
    rows(&mut w, "m");

    w.at(20);
    w.h.front_end_sends(&w.ui, FromFrontEnd::Shown);
    assert!(w.ui.results().is_none());
}

/// Z1: at a time of day nothing is picked at, the most used overall.
#[test]
fn at_a_time_of_day_nothing_is_used_at_the_empty_query_shows_the_most_used() {
    let mut w = World::new(HABITS);
    habits(&mut w);
    w.at(14);
    assert_eq!(rows(&mut w, ""), ["Music", "Mail", "Editor", "apps"]);
}

/// Z1
#[test]
fn with_nothing_picked_the_empty_query_shows_the_menu() {
    let mut w = World::with(HABITS.iter().map(|title| app(title)).collect(), MENU);
    assert_eq!(rows(&mut w, ""), ["Files", "Clear history", "Mail"]);
    let mut w = World::new(HABITS);
    assert_eq!(rows(&mut w, ""), ["apps"], "and without one, the apps");
}

/// Z1
#[test]
fn how_many_of_the_most_used_come_first_is_a_setting() {
    let mut w = World::with(
        HABITS.iter().map(|title| app(title)).collect(),
        "[empty_query]\nmost_used = 1",
    );
    habits(&mut w);
    w.at(8);
    assert_eq!(rows(&mut w, ""), ["Mail", "apps"]);
}

/// Z1: an Alias's picks count for the Result it names.
#[test]
fn picks_through_an_alias_count_towards_the_most_used() {
    let mut w = World::with(
        vec![app("Files"), app("Firefox"), app("Fish")],
        "[aliases]\nweb = { extension = \"apps\", item = \"firefox\" }",
    );
    w.picks(2, "fish", "Fish");
    w.picks(3, "web", "Firefox");
    assert_eq!(rows(&mut w, ""), ["Firefox", "Fish", "apps"]);
}

/// Z2
#[test]
fn the_same_history_gives_the_same_rows() {
    let mut w = World::new(HABITS);
    habits(&mut w);
    w.picks(2, "files", "Files");
    w.at(8);
    let expected = rows(&mut w, "");
    assert_eq!(rows(&mut w, ""), expected);

    let history = w.h.saved.0.borrow().clone().unwrap();
    let mut h = Harness::with_history(vec![plain("apps")], config(""), history);
    let apps = h.up(0);
    h.add(&apps, HABITS.iter().map(|title| app(title)).collect());
    h.advance(w.h.elapsed());
    let ui = h.front_end();
    assert_eq!(titles(&h.query(&ui, "")), expected);
}

/// Z3
#[test]
fn a_pick_from_the_empty_query_counts_at_once() {
    let mut w = World::new(HABITS);
    w.pick("term", "Terminal");
    assert_eq!(rows(&mut w, ""), ["Terminal", "apps"]);
    w.pick("files", "Files");
    w.pick("", "Files");
    assert_eq!(rows(&mut w, ""), ["Files", "Terminal", "apps"]);
}

// The History's settings, from the `[history]` table of the config.

/// H4
#[test]
fn the_picks_to_learn_are_a_setting() {
    let mut w = World::with(
        vec![app("Files"), app("Firefox"), app("Fish")],
        "[history]\npicks_to_learn = 2",
    );
    w.picks(5, "fles", "Files");
    w.picks(2, "fi", "Firefox");
    assert_eq!(w.first("fi"), "Firefox");
}

/// H4
#[test]
fn the_picks_to_take_over_are_a_setting() {
    let mut w = World::with(
        vec![app("Files"), app("Firefox"), app("Fish")],
        "[history]\npicks_to_take_over = 1",
    );
    w.picks(3, "fi", "Firefox");
    w.pick("fi", "Fish");
    assert_eq!(w.first("fi"), "Fish");
}

/// H4
#[test]
fn the_half_life_is_a_setting() {
    let mut w = World::with(
        vec![app("Alpha Tool"), app("Beta Tool")],
        "[history]\nhalf_life_days = 1",
    );
    w.picks(10, "alpha", "Alpha Tool");
    w.later(7 * DAY);
    w.picks(3, "beta", "Beta Tool");
    assert_eq!(w.first("tool"), "Beta Tool");
}

/// H4
#[test]
fn the_frecency_that_lifts_a_result_a_tier_is_a_setting() {
    let mut w = World::with(
        vec![app("Files"), app("Firefox"), app("Fish"), app("Fi")],
        "[history]\nlift_threshold = 0.5",
    );
    w.pick("firefox", "Firefox");
    w.later(DAY);
    assert_eq!(rows(&mut w, "fi"), ["Firefox", "Fi", "Fish", "Files"]);
}

/// H4
#[test]
fn a_setting_that_does_not_parse_keeps_its_default() {
    let mut w = World::with(
        vec![app("Files"), app("Firefox"), app("Fish")],
        "[history]\npicks_to_learn = 0\nhalf_life_days = \"two weeks\"",
    );
    w.picks(5, "fles", "Files");
    w.picks(2, "fi", "Firefox");
    assert_eq!(w.first("fi"), "Files");
    w.pick("fi", "Firefox");
    assert_eq!(w.first("fi"), "Firefox");
}

/// H4: a threshold of 0 would lift every Result, picked or not.
#[test]
fn a_lift_threshold_of_zero_keeps_the_default() {
    let mut w = World::with(
        vec![app("Files"), app("Firefox"), app("Fish"), app("Fi")],
        "[history]\nlift_threshold = 0",
    );
    w.pick("firefox", "Firefox");
    w.later(DAY);
    assert_eq!(rows(&mut w, "fi"), ["Fi", "Firefox", "Fish", "Files"]);
}

/// H4
#[test]
fn how_much_the_time_of_day_counts_is_a_setting() {
    let mut w = World::with(vec![app("Alpha Tool"), app("Beta Tool")], "[history]\ntime_of_day = 0");
    for _ in 0..10 {
        w.at(8);
        w.pick("alpha", "Alpha Tool");
        w.at(20);
        w.pick("beta", "Beta Tool");
    }
    w.at(8);
    assert_eq!(w.first("tool"), "Beta Tool", "the more recent");
}

/// H4
#[test]
fn how_much_the_day_of_the_week_counts_is_a_setting() {
    let mut w = World::new(&["Work Tool", "Play Tool"]);
    weekly_habits(&mut w);
    w.on(SATURDAY, 9);
    assert_eq!(w.first("tool"), "Play Tool");

    w.reconfigure("[history]\nday_of_week = 0");
    assert_eq!(w.first("tool"), "Work Tool", "the more recent");
}

/// H4: a History kept before the day of the week was counts in full on
/// either kind of day.
#[test]
fn a_history_with_no_weekday_shares_ranks_as_before() {
    let mut w = World::new(&["Work Tool", "Play Tool"]);
    weekly_habits(&mut w);
    let mut kept = serde_json::to_value(w.h.saved.0.borrow().as_ref().expect("the History was saved")).unwrap();
    for (_, picks) in kept["picks"]["apps"].as_object_mut().unwrap() {
        let picks = picks.as_object_mut().unwrap();
        // A kind of day with no share is left out of the file.
        let shares = |kind: &str| -> Vec<u64> {
            picks.get(kind).map_or(vec![0; 8], |shares| {
                shares.as_array().unwrap().iter().map(|n| n.as_u64().unwrap()).collect()
            })
        };
        let parts: Vec<u64> = shares("weekdays")
            .iter()
            .zip(shares("weekends"))
            .map(|(a, b)| a + b)
            .collect();
        picks.remove("weekdays");
        picks.remove("weekends");
        picks.insert("parts".into(), parts.into());
    }
    let old = serde_json::from_value(kept).expect("an old History decodes");
    let elapsed = w.h.elapsed();
    let mut w = World::with_history(vec![app("Work Tool"), app("Play Tool")], "", old);
    w.later(elapsed);

    w.on(SATURDAY, 9);
    assert_eq!(w.first("tool"), "Work Tool", "the more recent, on a Saturday too");
}

/// H4
#[test]
fn the_settings_reload_with_the_config() {
    let mut w = World::new(&["Files", "Firefox", "Fish"]);
    w.picks(5, "fles", "Files");
    w.reconfigure("[history]\npicks_to_learn = 2");
    w.picks(2, "fi", "Firefox");
    assert_eq!(w.first("fi"), "Firefox");

    w.reconfigure("[history]\npicks_to_take_over = 3");
    w.picks(2, "fi", "Fish");
    assert_eq!(w.first("fi"), "Firefox");
    w.pick("fi", "Fish");
    assert_eq!(w.first("fi"), "Fish");
}

// The calculator, with its real Manifest and its real answers.

/// Apps, and the calculator Bundled extension answering what it is sent.
struct Calculating {
    h: Harness,
    calc: Ext,
    ui: FrontEnd,
    calculator: kotodama_calculator::Calculator,
}

impl Calculating {
    fn new(titles: &[&str]) -> Self {
        let calculator = manifest(include_str!("../../kotodama-calculator/manifest.toml"));
        let mut h = Harness::new(vec![plain("apps"), calculator]);
        let apps = h.up(0);
        h.add(&apps, titles.iter().map(|title| app(title)).collect());
        let calc = h.up(1);
        calc.drain();
        let ui = h.front_end();
        Calculating {
            h,
            calc,
            ui,
            calculator: kotodama_calculator::Calculator::new(kotodama_calculator::Rates::default()),
        }
    }

    /// The rows for `query`, once the calculator has answered.
    fn rows(&mut self, query: &str) -> Vec<String> {
        let mut rows = self.h.query(&self.ui, query);
        for message in self.calc.drain() {
            let ToExtension::LiveQuery { id, query, prefix } = message else {
                continue;
            };
            let now = kotodama_calculator::Zoned::now();
            let results = match self.calculator.evaluate(&query, prefix.is_some(), &now) {
                kotodama_calculator::Evaluation::Answer(answer) => vec![answer.live_result()],
                _ => vec![],
            };
            self.h.send(
                &self.calc,
                FromExtension::LiveResults {
                    live_query: id,
                    results,
                    done: true,
                },
            );
            rows = self.ui.results().unwrap_or(rows);
        }
        titles(&rows).into_iter().map(str::to_owned).collect()
    }
}

/// V1, V2, V6
#[test]
fn a_bare_number_gives_no_answer_but_an_expression_or_conversion_takes_the_top_row() {
    let mut w = Calculating::new(&["42 Lab", "2048", "Kilometre Tracker"]);
    assert_eq!(w.rows("42"), ["42 Lab"]);
    assert_eq!(w.rows("42*2"), ["84"]);
    assert_eq!(w.rows("42 km to mi"), ["26.097590074 mi"]);
    assert_eq!(w.rows("2048"), ["2048"]);
    assert_eq!(w.rows("2048/2")[0], "1024");
}

// Web search, with its real Manifest and its real rows: a fallback, asked
// only what real matches leave rows empty for, ranked after every real match.

/// Apps, and the web search Bundled extension, searching Google and then
/// `DuckDuckGo`, answering what it is sent.
struct Searching {
    h: Harness,
    web: Ext,
    ui: FrontEnd,
    engines: Vec<kotodama_web_search::Engine>,
    /// The queries web search was asked, in order.
    asked: Vec<String>,
}

impl Searching {
    fn new(titles: &[&str]) -> Self {
        let web_search = manifest(include_str!("../../kotodama-web-search/manifest.toml"));
        let mut h = Harness::new(vec![plain("apps"), web_search]);
        let apps = h.up(0);
        h.add(&apps, titles.iter().map(|title| app(title)).collect());
        let web = h.up(1);
        web.drain();
        let ui = h.front_end();
        let engines = kotodama_web_search::engines(&config(
            "engines = [\n\
             { name = \"Google\", url = \"https://www.google.com/search?q={query}\" },\n\
             { name = \"DuckDuckGo\", url = \"https://duckduckgo.com/?q={query}\" },\n\
             ]",
        ));
        Searching {
            h,
            web,
            ui,
            engines,
            asked: Vec::new(),
        }
    }

    fn shown(&mut self, query: &str) -> Vec<ShownResult> {
        let mut rows = self.h.query(&self.ui, query);
        for message in self.web.drain() {
            let ToExtension::LiveQuery { id, query, .. } = message else {
                continue;
            };
            let results = kotodama_web_search::results(&query, &self.engines);
            self.asked.push(query);
            self.h.send(
                &self.web,
                FromExtension::LiveResults {
                    live_query: id,
                    results,
                    done: true,
                },
            );
            rows = self.ui.results().unwrap_or(rows);
        }
        rows
    }

    fn rows(&mut self, query: &str) -> Vec<String> {
        titles(&self.shown(query)).into_iter().map(str::to_owned).collect()
    }

    fn pick(&mut self, query: &str, title: &str) {
        let rows = self.shown(query);
        let row = rows
            .iter()
            .find(|row| row.item.title == title)
            .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(&rows)));
        let id = row.item.id.clone();
        run(&mut self.h, &self.ui, "web-search", &id, "web-search", "open");
        self.ui.drain();
        self.web.drain();
    }
}

/// V3
#[test]
fn web_search_is_offered_below_the_real_matches_when_they_leave_rows_empty() {
    let mut w = Searching::new(&["Firefox", "Terminal"]);
    assert_eq!(
        w.rows("fire"),
        ["Firefox", "Search Google for fire", "Search DuckDuckGo for fire"]
    );
}

/// V3
#[test]
fn web_search_is_offered_for_a_query_nothing_matches() {
    let mut w = Searching::new(&["Firefox"]);
    assert_eq!(
        w.rows("weather"),
        ["Search Google for weather", "Search DuckDuckGo for weather"]
    );
}

#[test]
fn web_search_ranks_below_a_near_miss() {
    let mut w = Searching::new(&["Firefox"]);
    assert_eq!(
        w.rows("fierfox"),
        ["Firefox", "Search Google for fierfox", "Search DuckDuckGo for fierfox"]
    );
}

/// V3
#[test]
fn web_search_picked_every_time_still_ranks_below_every_real_match() {
    let mut w = Searching::new(&["Firefox", "Fire Alarm"]);
    for _ in 0..20 {
        w.pick("fire", "Search DuckDuckGo for fire");
        w.h.advance(Duration::from_secs(60));
    }
    let rows = w.rows("fire");
    let mut real = rows[..2].to_vec();
    real.sort();
    assert_eq!(real, ["Fire Alarm", "Firefox"]);
    // Among themselves, the engine used goes first.
    assert_eq!(
        rows[2..],
        ["Search DuckDuckGo for fire", "Search Google for fire"].map(String::from)
    );
    let rows = w.rows("fierfox");
    assert_eq!(rows[0], "Firefox", "{rows:?}");
}

/// V3: the fallback costs nothing while the real matches fill the rows.
#[test]
fn web_search_is_not_asked_while_real_matches_fill_the_rows() {
    let titles: Vec<String> = (0..12).map(|n| format!("Note {n}")).collect();
    let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
    let mut w = Searching::new(&titles);
    let rows = w.rows("note");
    assert!(rows.iter().all(|row| row.starts_with("Note")), "{rows:?}");
    assert!(w.asked.is_empty(), "{:?}", w.asked);
    w.rows("nothing");
    assert_eq!(w.asked, ["nothing"]);
}

/// V3: web search back up after a restart is asked what is typed already.
#[test]
fn web_search_up_after_the_query_is_typed_is_asked_it() {
    let mut w = Searching::new(&["Firefox"]);
    w.h.down(&w.web);
    w.h.advance(Duration::from_millis(100));
    assert_eq!(w.rows("weather"), Vec::<String>::new());
    w.web = w.h.up(1);
    let asked: Vec<String> = w
        .web
        .drain()
        .into_iter()
        .filter_map(|message| match message {
            ToExtension::LiveQuery { query, .. } => Some(query),
            _ => None,
        })
        .collect();
    assert_eq!(asked, ["weather"]);
}

/// V3: a real match arriving late, from a slower Live query, goes
/// above the searches already on screen.
#[test]
fn a_late_real_match_goes_above_web_search_on_screen() {
    let notes = manifest(
        "name = \"notes\"\ncommand = [\"x\"]\n\
         [live_query]\ntriggers = ['.']\ndebounce_ms = 50",
    );
    let web_search = manifest(include_str!("../../kotodama-web-search/manifest.toml"));
    let mut h = Harness::new(vec![notes, web_search]);
    let notes = h.up(0);
    let web = h.up(1);
    let ui = h.front_end();
    h.query(&ui, "zebra");
    let engines = kotodama_web_search::default_engines();
    for message in web.drain() {
        if let ToExtension::LiveQuery { id, query, .. } = message {
            let results = kotodama_web_search::results(&query, &engines[..1]);
            let done = true;
            h.send(
                &web,
                FromExtension::LiveResults {
                    live_query: id,
                    results,
                    done,
                },
            );
        }
    }
    let rows = ui.results().unwrap();
    assert_eq!(titles(&rows), ["Search DuckDuckGo for zebra"]);

    h.advance(Duration::from_millis(50));
    let Some(ToExtension::LiveQuery { id, .. }) = notes.drain().pop() else {
        panic!("notes wasn't asked");
    };
    let note = LiveResult {
        item: item("zebra", "Zebra crossing", ItemKind::Url { url: "x".into() }),
        score: Fraction::new(1.0).unwrap(),
        answer: false,
    };
    let (results, done) = (vec![note], true);
    h.send(
        &notes,
        FromExtension::LiveResults {
            live_query: id,
            results,
            done,
        },
    );
    let rows = ui.results().unwrap();
    assert_eq!(titles(&rows), ["Zebra crossing", "Search DuckDuckGo for zebra"]);
}

/// V3: picking a search for a query never stops what the user learned
/// among the real matches from applying.
#[test]
fn a_picked_search_leaves_the_learned_choice_among_real_matches_alone() {
    let mut w = Searching::new(&["Fox", "Firefox"]);
    let firefox = |w: &mut Searching| w.rows("fox").iter().position(|row| row == "Firefox");
    let before = firefox(&mut w);
    for _ in 0..3 {
        let rows = w.shown("fox");
        let id = rows
            .iter()
            .find(|row| row.item.title == "Firefox")
            .unwrap()
            .item
            .id
            .clone();
        run(&mut w.h, &w.ui, "apps", &id, "apps", "open");
        w.ui.drain();
    }
    assert_eq!(firefox(&mut w), Some(0), "learned, from {before:?}");
    for _ in 0..5 {
        w.pick("fox", "Search Google for fox");
    }
    assert_eq!(firefox(&mut w), Some(0));
}

/// V3: what looks like an address opens first, whatever else matches.
#[test]
fn an_address_opens_in_the_first_row_with_its_searches_last() {
    let mut w = Searching::new(&["Example Notes"]);
    assert_eq!(
        w.rows("example.com"),
        [
            "Open example.com",
            "Search Google for example.com",
            "Search DuckDuckGo for example.com"
        ]
    );
    let titles: Vec<String> = (0..12).map(|n| format!("Example.com {n}")).collect();
    let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
    let mut w = Searching::new(&titles);
    let rows = w.rows("example.com");
    assert_eq!(rows[0], "Open example.com");
    // The rows are full, so no search is offered.
    assert!(rows[1..].iter().all(|row| row.starts_with("Example.com")), "{rows:?}");
}

// Boost, from an Extension's `[extensions.<name>]` table.

/// The rows for `query` of a desktop with two Extensions, `first` and
/// `second`, holding the apps titled `firsts` and `seconds`, under `toml`.
fn boosted(toml: &str, firsts: &[&str], seconds: &[&str], query: &str) -> Vec<String> {
    let mut h = Harness::with_config(vec![plain("first"), plain("second")], config(toml));
    let (first, second) = (h.up(0), h.up(1));
    h.add(&first, firsts.iter().map(|title| app(title)).collect());
    h.add(&second, seconds.iter().map(|title| app(title)).collect());
    let ui = h.front_end();
    titles(&h.query(&ui, query)).into_iter().map(str::to_owned).collect()
}

#[test]
fn a_boost_above_1_puts_an_extensions_results_above_equal_matches() {
    assert_eq!(
        boosted("", &["Terminal"], &["Terminus"], "termin"),
        ["Terminal", "Terminus"]
    );
    assert_eq!(
        boosted(
            "[extensions.second]\nboost = 1.5\n",
            &["Terminal"],
            &["Terminus"],
            "termin"
        ),
        ["Terminus", "Terminal"]
    );
}

#[test]
fn a_boost_below_1_puts_an_extensions_results_below_equal_matches() {
    assert_eq!(
        boosted(
            "[extensions.first]\nboost = 0.5\n",
            &["Terminal"],
            &["Terminus"],
            "termin"
        ),
        ["Terminus", "Terminal"]
    );
}

#[test]
fn no_boost_lifts_a_result_past_an_exact_title() {
    let toml = "[extensions.second]\nboost = 1000\n";

    assert_eq!(boosted(toml, &["Term"], &["Terminal"], "term"), ["Term", "Terminal"]);
    assert_eq!(
        boosted(toml, &["Term"], &["Determinate"], "term"),
        ["Term", "Determinate"]
    );
}

#[test]
fn no_boost_takes_a_real_match_below_a_near_miss() {
    let toml = "[extensions.first]\nboost = 0.001\n[extensions.second]\nboost = 1000\n";

    assert_eq!(
        boosted(toml, &["Streamer"], &["Steam"], "stream"),
        ["Streamer", "Steam"]
    );
}

#[test]
fn a_boost_counts_for_live_query_results() {
    let rows = |toml: &str| {
        let notes = manifest(
            "name = \"notes\"\ncommand = [\"x\"]\n\
             [live_query]\ntriggers = ['.']\ndebounce_ms = 50",
        );
        let mut h = Harness::with_config(vec![plain("apps"), notes], config(toml));
        let (apps, notes) = (h.up(0), h.up(1));
        h.add(&apps, vec![app("Zebra one")]);
        let ui = h.front_end();
        h.query(&ui, "zebra");
        h.advance(Duration::from_millis(50));
        let Some(ToExtension::LiveQuery { id, .. }) = notes.drain().pop() else {
            panic!("notes wasn't asked");
        };
        let note = LiveResult {
            item: item("zebra", "Zebra two", ItemKind::Url { url: "x".into() }),
            score: Fraction::new(1.0).unwrap(),
            answer: false,
        };
        let (results, done) = (vec![note], true);
        h.send(
            &notes,
            FromExtension::LiveResults {
                live_query: id,
                results,
                done,
            },
        );
        titles(&ui.results().unwrap())
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };

    assert_eq!(rows(""), ["Zebra one", "Zebra two"]);
    assert_eq!(rows("[extensions.notes]\nboost = 2\n"), ["Zebra two", "Zebra one"]);
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
    "firfox",
    "fierfox",
    "calculater",
    "stream",
    "termnal",
    "setings",
    "sistem monitr",
];

/// A one-letter query only matches at a word start; the next letter may
/// match inside a word.
#[test]
fn typing_on_from_one_letter_finds_matches_inside_words() {
    let mut w = World::new(&["Terminal", "Steam"]);
    assert_eq!(rows(&mut w, "t"), ["Terminal"]);
    assert_eq!(rows(&mut w, "te"), World::new(&["Terminal", "Steam"]).rows("te"));
    assert_eq!(rows(&mut w, "te"), ["Terminal", "Steam"]);
}

#[test]
fn typing_a_query_letter_by_letter_gives_the_rows_typing_it_at_once_does() {
    let mut typed = World::new(DESKTOP);
    for query in QUERIES {
        let mut letters = String::new();
        for c in query.chars() {
            letters.push(c);
            let rows = rows(&mut typed, &letters);
            assert_eq!(rows, World::new(DESKTOP).rows(&letters), "{letters:?}");
        }
    }
}

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
                let haystack = Haystack::new(&row.item);
                tiers
                    .item(&haystack, &row.item, &folded, Matched::ALL)
                    .or_else(|| tiers.near_miss(&haystack, &row.item, &folded).then_some(Tier::NearMiss))
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
