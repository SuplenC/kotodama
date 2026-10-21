//! Apps: every Extension has one, entered through its entry Item, where the
//! query searches that Extension's Items alone, those it keeps out of the
//! search included.

mod common;

use common::*;
use kotodama_protocol::*;
use kotodamad::discovery::Found;

/// The emoji Extension as its Manifest declares it: every Item app only.
fn emoji() -> Found {
    manifest(
        "name = \"emoji\"\ncommand = [\"x\"]\nword = \"emoji\"\ntitle = \"Emoji\"\nicon = \"face-smile\"\nabout = \"Emoji, typed where you were.\"\nexamples = [{ query = \"fire\", result = \"🔥 Fire\" }]\nin_search = false",
    )
}

fn character(id: &str, title: &str, keywords: &[&str]) -> Item {
    let mut item = item(
        id,
        title,
        ItemKind::Namespaced(NamespacedKind::new("emoji.character").unwrap()),
    );
    item.keywords = keywords.iter().map(|&keyword| keyword.to_owned()).collect();
    item
}

fn characters() -> Vec<Item> {
    vec![
        character("smile", "😄 Smile", &["happy", "face"]),
        character("fire", "🔥 Fire", &["flame", "hot"]),
        character("firefighter", "🧑‍🚒 Firefighter", &["fire", "hose"]),
        character("memo", "📝 Memo", &["editor", "note", "pencil"]),
    ]
}

/// A desktop with apps and the emoji, under `toml`, and a Front end.
struct Desk {
    h: Harness,
    ui: FrontEnd,
}

impl Desk {
    fn new(toml: &str) -> Self {
        Self::with(emoji(), toml)
    }

    fn with(emoji: Found, toml: &str) -> Self {
        let mut h = Harness::with_config(vec![plain("apps"), emoji], config(toml));
        let apps = h.up(0);
        h.add(
            &apps,
            vec![app("Editor"), app("Firefox"), app("Emoji Keyboard")],
        );
        let emoji = h.up(1);
        h.add(&emoji, characters());
        let ui = h.front_end();
        Desk { h, ui }
    }

    /// Types `query`: the rows, as extension and title, and the scope.
    fn search(&mut self, query: &str) -> (Vec<(String, String)>, Option<Scope>) {
        let text = query.into();
        self.h
            .front_end_sends(&self.ui, FromFrontEnd::Query { text });
        let (rows, scope) = last_results(self.ui.drain());
        (rows, scope)
    }

    fn titles(&mut self, query: &str) -> Vec<String> {
        let (rows, _) = self.search(query);
        rows.into_iter().map(|(_, title)| title).collect()
    }

    /// Runs the primary Action of the row titled `title` for `query`.
    fn pick(&mut self, query: &str, title: &str) {
        let rows = self.h.query(&self.ui, query);
        let row = rows
            .iter()
            .find(|row| row.item.title == title)
            .unwrap_or_else(|| panic!("no row {title:?} for {query:?}"));
        let (extension, id) = (row.extension.clone(), row.item.id.clone());
        let action = row.item.actions[0].id.clone();
        run(&mut self.h, &self.ui, &extension, &id, &extension, &action);
    }

    /// Enters the app titled `title` from its entry Item, as Tab or Enter on
    /// it does, found by `query`.
    fn enter(&mut self, query: &str, title: &str) {
        self.pick(query, title);
        self.ui.drain();
    }
}

/// The rows of the last Results in `sent`, as extension and title, and the
/// scope.
fn last_results(sent: Vec<ToFrontEnd>) -> (Vec<(String, String)>, Option<Scope>) {
    let (results, scope) = sent
        .into_iter()
        .rev()
        .find_map(|message| match message {
            ToFrontEnd::Results { results, scope, .. } => Some((results, scope)),
            _ => None,
        })
        .expect("a query is always answered");
    let rows = results
        .into_iter()
        .map(|row| (row.extension, row.item.title))
        .collect();
    (rows, scope)
}

fn emoji_scope(intro: bool) -> Option<Scope> {
    Some(Scope {
        word: "emoji".into(),
        title: "Emoji".into(),
        icon: Some("face-smile".into()),
        intro: intro.then(|| Intro {
            about: "Emoji, typed where you were.".into(),
            examples: vec![Example {
                query: "fire".into(),
                result: "🔥 Fire".into(),
            }],
            note: None,
        }),
    })
}

#[test]
fn the_search_leaves_out_an_app_only_extensions_items() {
    let mut desk = Desk::new("");

    assert_eq!(desk.titles("editor"), ["Editor"]);
    assert!(!desk.titles("fire").iter().any(|title| title.contains("🔥")));
}

#[test]
fn the_word_typed_alone_finds_its_entry_item_as_a_word_start() {
    let mut desk = Desk::new("");

    let (rows, scope) = desk.search("emoji");

    assert_eq!(
        rows[..2],
        [
            ("apps".to_owned(), "Emoji Keyboard".to_owned()),
            ("kotodama".to_owned(), "Emoji".to_owned()),
        ]
    );
    assert!(rows.iter().all(|(extension, _)| extension != "emoji"));
    assert_eq!(scope, None);
}

#[test]
fn the_entry_item_found_by_the_word_says_what_follows_it() {
    let mut desk = Desk::new("");

    let (rows, scope) = desk.search("emoji fire");

    assert_eq!(
        rows[0],
        ("kotodama".to_owned(), "Search Emoji for “fire”".to_owned())
    );
    assert_eq!(scope, None);
}

#[test]
fn entering_the_app_keeps_what_follows_the_word() {
    let mut desk = Desk::new("");

    desk.pick("emoji fire", "Search Emoji for “fire”");

    let sent = desk.ui.drain();
    assert!(
        sent.contains(&ToFrontEnd::SetQuery {
            text: "emoji fire".into(),
        }),
        "{sent:?}"
    );
    let (rows, scope) = last_results(sent);
    assert_eq!(rows[0].1, "🔥 Fire");
    assert_eq!(scope, emoji_scope(false));
}

#[test]
fn the_entry_item_is_found_by_its_title() {
    let mut desk = Desk::new("");

    assert!(desk.titles("emo").contains(&"Emoji".to_owned()));
}

#[test]
fn every_extension_has_an_entry_item_named_by_default_after_it() {
    let mut desk = Desk::new("");

    let (rows, _) = desk.search("apps");

    assert_eq!(rows[0], ("kotodama".to_owned(), "apps".to_owned()));
}

#[test]
fn typing_the_word_and_more_without_entering_searches_everything() {
    let mut desk = Desk::new("");

    let (rows, scope) = desk.search("emoji keyboard");

    assert_eq!(scope, None);
    assert_eq!(rows[0].1, "Emoji Keyboard");
}

#[test]
fn running_the_entry_item_enters_the_app_with_its_intro_in_the_same_flush() {
    let mut desk = Desk::new("");

    desk.pick("emo", "Emoji");

    let sent = desk.ui.drain();
    let set = sent
        .iter()
        .position(|message| {
            *message
                == ToFrontEnd::SetQuery {
                    text: "emoji ".into(),
                }
        })
        .unwrap_or_else(|| panic!("{sent:?}"));
    let results = sent
        .iter()
        .rposition(|message| matches!(message, ToFrontEnd::Results { .. }))
        .unwrap();
    assert!(set < results, "{sent:?}");
    assert!(!sent.contains(&ToFrontEnd::Close), "the menu stays open");
    let (rows, scope) = last_results(sent);
    assert_eq!(scope, emoji_scope(true));
    let titles: Vec<String> = rows.into_iter().map(|(_, title)| title).collect();
    let sent: Vec<String> = characters().into_iter().map(|item| item.title).collect();
    assert_eq!(titles, sent);
}

#[test]
fn typing_in_the_app_searches_only_its_extensions_items() {
    let mut desk = Desk::new("");
    desk.enter("emoji", "Emoji");

    let (rows, scope) = desk.search("emoji fire");

    assert_eq!(rows[0].1, "🔥 Fire");
    assert!(
        rows.iter().all(|(extension, _)| extension == "emoji"),
        "{rows:?}"
    );
    assert_eq!(scope, emoji_scope(false));
}

#[test]
fn the_word_is_kept_in_any_case() {
    let mut desk = Desk::new("");
    desk.enter("emoji", "Emoji");

    let (rows, scope) = desk.search("Emoji fire");

    assert_eq!(rows[0].1, "🔥 Fire");
    assert_eq!(scope, emoji_scope(false));
}

#[test]
fn a_query_without_the_word_leaves_the_app() {
    let mut desk = Desk::new("");
    desk.enter("emoji", "Emoji");

    assert_eq!(desk.search("").1, None);
    assert_eq!(desk.search("emoji fire").1, None);
}

#[test]
fn what_is_picked_in_the_app_leads_its_empty_search() {
    let mut desk = Desk::new("");
    desk.enter("emoji", "Emoji");

    desk.pick("emoji memo", "📝 Memo");

    assert_eq!(desk.titles("emoji ")[0], "📝 Memo");
}

#[test]
fn the_empty_query_shows_the_entry_item_picked_but_not_what_was_picked_in_the_app() {
    let mut desk = Desk::new("");
    desk.enter("emo", "Emoji");
    desk.pick("emoji fire", "🔥 Fire");
    desk.pick("edit", "Editor");

    let titles = desk.titles("");

    assert!(titles.contains(&"Emoji".to_owned()), "{titles:?}");
    assert!(titles.contains(&"Editor".to_owned()), "{titles:?}");
    assert!(!titles.contains(&"🔥 Fire".to_owned()), "{titles:?}");
}

#[test]
fn without_a_menu_the_empty_query_lists_the_apps() {
    let mut desk = Desk::new("");

    assert_eq!(desk.titles(""), ["apps", "Emoji"]);
}

#[test]
fn the_apps_follow_the_most_used_on_the_empty_query() {
    let mut desk = Desk::new("");
    desk.pick("edit", "Editor");

    assert_eq!(desk.titles(""), ["Editor", "apps", "Emoji"]);
}

#[test]
fn a_menu_in_the_config_replaces_the_apps_on_the_empty_query() {
    for (toml, titles) in [
        ("[empty_query]\nmenu = []", &[][..]),
        (
            "[empty_query]\nmenu = [{ extension = \"apps\", item = \"firefox\" }]",
            &["Firefox"][..],
        ),
    ] {
        let mut desk = Desk::new(toml);

        assert_eq!(desk.titles(""), titles, "{toml}");
    }
}

#[test]
fn a_near_miss_is_never_an_app_only_item() {
    let mut desk = Desk::new("");

    let titles = desk.titles("memmo");

    assert!(!titles.contains(&"📝 Memo".to_owned()), "{titles:?}");
}

#[test]
fn a_typo_in_the_app_finds_a_near_miss_there() {
    let mut desk = Desk::new("");
    desk.enter("emoji", "Emoji");

    assert_eq!(desk.titles("emoji memmo"), ["📝 Memo"]);
}

#[test]
fn an_app_only_item_shows_only_in_its_app() {
    let mut h = Harness::new(vec![plain("clipboard")]);
    let clipboard = h.up(0);
    let mut history = item(
        "history",
        "Clipboard History",
        ItemKind::Url {
            url: "https://example.org/h".into(),
        },
    );
    history.app_only = true;
    h.add(&clipboard, vec![history, app("Clip Editor")]);
    let ui = h.front_end();

    let found = titles(&h.query(&ui, "clip")).join(", ");
    assert!(!found.contains("Clipboard History"), "{found}");
    assert!(found.contains("Clip Editor"), "{found}");
    let mut desk = Desk { h, ui };
    desk.enter("clipboard", "clipboard");
    let titles = desk.titles("clipboard clip");
    assert!(
        titles.contains(&"Clipboard History".to_owned()),
        "{titles:?}"
    );
    assert!(titles.contains(&"Clip Editor".to_owned()), "{titles:?}");
}

#[test]
fn an_item_moved_into_the_app_leaves_the_search() {
    let mut h = Harness::new(vec![plain("clipboard")]);
    let clipboard = h.up(0);
    h.add(&clipboard, vec![app("Clip Editor")]);
    let ui = h.front_end();
    assert!(titles(&h.query(&ui, "clip editor")).contains(&"Clip Editor"));

    let mut moved = app("Clip Editor");
    moved.app_only = true;
    h.add(&clipboard, vec![moved]);

    assert!(!titles(&h.query(&ui, "clip editor")).contains(&"Clip Editor"));
}

#[test]
fn the_app_lists_its_commands_first() {
    let clipboard = manifest("name = \"clipboard\"\ncommand = [\"x\"]\ncommands = [\"history\"]");
    let mut h = Harness::new(vec![clipboard]);
    let ext = h.up(0);
    let mut history = item(
        "history",
        "Clipboard History",
        ItemKind::Url {
            url: "https://example.org/h".into(),
        },
    );
    history.app_only = true;
    h.add(&ext, vec![app("Alpha"), app("Beta"), history]);
    let ui = h.front_end();
    let mut desk = Desk { h, ui };

    desk.enter("clipboard", "clipboard");

    assert_eq!(
        desk.titles("clipboard "),
        ["Clipboard History", "Alpha", "Beta"]
    );
}

#[test]
fn in_search_true_in_the_config_puts_an_app_only_extensions_items_in_the_search() {
    let mut desk = Desk::new("[extensions.emoji]\nin_search = true");

    assert_eq!(desk.titles("fire")[0], "🔥 Fire");
}

#[test]
fn in_search_false_in_the_config_puts_the_whole_extension_behind_its_app() {
    let mut desk = Desk::new("[extensions.apps]\nin_search = false");

    assert!(!desk.titles("editor").contains(&"Editor".to_owned()));
    desk.enter("apps", "apps");
    assert_eq!(desk.titles("apps editor"), ["Editor"]);
}

#[test]
fn the_config_gives_an_extension_a_word_of_its_own() {
    let mut desk = Desk::with(plain("emoji"), "[extensions.emoji]\nword = \"em\"");

    assert_eq!(desk.titles("fire")[0], "🔥 Fire");
    desk.enter("em", "emoji");
    let (rows, scope) = desk.search("em fire");
    assert_eq!(rows[0].1, "🔥 Fire");
    assert_eq!(
        scope,
        Some(Scope {
            word: "em".into(),
            title: "emoji".into(),
            icon: None,
            intro: None,
        })
    );
}

#[test]
fn the_config_overrides_the_manifests_word() {
    let mut desk = Desk::new("[extensions.emoji]\nword = \"e\"");

    desk.enter("emoji", "Emoji");

    assert_eq!(desk.search("e fire").0[0].1, "🔥 Fire");
}

#[test]
fn opening_on_an_apps_word_enters_it() {
    let mut desk = Desk::new("");
    let cli = desk.h.front_end();

    desk.h.front_end_sends(
        &cli,
        FromFrontEnd::Open {
            query: "emoji ".into(),
            view: None,
        },
    );

    assert_eq!(desk.search("emoji fire").1, emoji_scope(false));
}

/// The Live queries in `sent`, and the prefix each was routed by.
fn live_queries(sent: Vec<ToExtension>) -> Vec<(String, Option<String>)> {
    sent.into_iter()
        .filter_map(|message| match message {
            ToExtension::LiveQuery { query, prefix, .. } => Some((query, prefix)),
            _ => None,
        })
        .collect()
}

#[test]
fn in_its_app_an_extension_is_asked_every_query_triggered_or_not() {
    let calc = manifest(
        "name = \"calc\"\ncommand = [\"x\"]\n[live_query]\nprefixes = [\"=\"]\ntriggers = ['^[0-9]']\ndebounce_ms = 0",
    );
    let mut h = Harness::new(vec![plain("apps"), calc]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Editor")]);
    let calc = h.up(1);
    let ui = h.front_end();
    calc.drain();

    h.query(&ui, "1+1");
    let triggered = live_queries(calc.drain());
    h.query(&ui, "pi");
    let untriggered = live_queries(calc.drain());
    let mut desk = Desk { h, ui };
    desk.enter("calc", "calc");
    desk.search("calc pi");
    let in_the_app = live_queries(calc.drain());

    assert_eq!(triggered, [("1+1".to_owned(), None)]);
    assert!(untriggered.is_empty(), "{untriggered:?}");
    assert_eq!(in_the_app, [("pi".to_owned(), Some("=".to_owned()))]);
}

#[test]
fn an_extension_behind_its_app_is_asked_nothing_by_the_search() {
    let calc = manifest(
        "name = \"calc\"\ncommand = [\"x\"]\nin_search = false\n[live_query]\ntriggers = ['^[0-9]']\ndebounce_ms = 0",
    );
    let mut h = Harness::new(vec![plain("apps"), calc]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Editor")]);
    let calc = h.up(1);
    let ui = h.front_end();
    calc.drain();

    h.query(&ui, "1+1");

    assert!(live_queries(calc.drain()).is_empty());
}

#[test]
fn another_apps_live_queries_are_not_asked_in_one() {
    let calc = manifest(
        "name = \"calc\"\ncommand = [\"x\"]\n[live_query]\ntriggers = ['^[0-9]']\ndebounce_ms = 0",
    );
    let mut h = Harness::new(vec![plain("apps"), calc]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Editor")]);
    let calc = h.up(1);
    let ui = h.front_end();
    let mut desk = Desk { h, ui };
    desk.enter("apps", "apps");
    calc.drain();

    desk.search("apps 1+1");

    assert!(live_queries(calc.drain()).is_empty());
}

fn status(usable: bool, note: Option<&str>) -> FromExtension {
    FromExtension::Status {
        usable,
        note: note.map(str::to_owned),
    }
}

#[test]
fn an_extension_that_cant_do_anything_has_no_entry_item_until_it_can() {
    let hyprland = manifest("name = \"hyprland\"\ncommand = [\"x\"]\ntitle = \"Hyprland\"");
    let mut h = Harness::new(vec![plain("apps"), hyprland]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Editor")]);
    let ext = h.up(1);
    let ui = h.front_end();
    let mut desk = Desk { h, ui };

    desk.h
        .send(&ext, status(false, Some("Hyprland isn't running")));
    let hidden = (desk.titles("hyprland"), desk.titles(""));
    desk.h.send(&ext, status(true, None));
    let shown = (desk.titles("hyprland"), desk.titles(""));

    assert!(!hidden.0.contains(&"Hyprland".to_owned()), "{hidden:?}");
    assert_eq!(hidden.1, ["apps"]);
    assert!(shown.0.contains(&"Hyprland".to_owned()), "{shown:?}");
    assert_eq!(shown.1, ["apps", "Hyprland"]);
}

#[test]
fn an_extension_that_never_says_has_an_entry_item_once_it_sends_items() {
    let mut h = Harness::new(vec![plain("apps"), plain("notes")]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Editor")]);
    let notes = h.up(1);
    let ui = h.front_end();

    let before = titles(&h.query(&ui, "")).join(", ");
    h.add(&notes, vec![app("Groceries")]);
    let after = titles(&h.query(&ui, "")).join(", ");

    assert_eq!(before, "apps");
    assert_eq!(after, "apps, notes");
}

#[test]
fn an_extension_with_live_queries_has_an_entry_item_without_items() {
    let calc = manifest(
        "name = \"calc\"\ncommand = [\"x\"]\n[live_query]\ntriggers = ['^[0-9]']\ndebounce_ms = 0",
    );
    let mut h = Harness::new(vec![calc]);
    h.up(0);
    let ui = h.front_end();

    assert_eq!(titles(&h.query(&ui, "")), ["calc"]);
}

#[test]
fn what_an_extension_says_goes_before_its_live_queries_and_items() {
    let ai = manifest(
        "name = \"ai\"\ncommand = [\"x\"]\n[live_query]\nprefixes = [\"?\"]\ndebounce_ms = 0",
    );
    let mut h = Harness::new(vec![ai]);
    let ext = h.up(0);
    h.send(&ext, status(false, Some("Set `api_key_command` or `url`")));
    h.add(&ext, vec![app("Ask")]);
    let ui = h.front_end();

    assert!(!titles(&h.query(&ui, "")).contains(&"ai"));
}

#[test]
fn an_extension_that_stopped_has_no_entry_item_until_it_says_again() {
    let mut h = Harness::new(vec![plain("notes")]);
    let ext = h.up(0);
    h.send(&ext, status(true, None));
    let ui = h.front_end();
    assert_eq!(titles(&h.query(&ui, "")), ["notes"]);

    h.down(&ext);

    assert!(titles(&h.query(&ui, "")).is_empty());
}

#[test]
fn opening_an_app_that_cant_do_anything_shows_why_on_its_intro_card() {
    let mut h = Harness::new(vec![manifest(
        "name = \"hyprland\"\ncommand = [\"x\"]\nabout = \"Windows and binds.\"",
    )]);
    let ext = h.up(0);
    h.send(&ext, status(false, Some("Hyprland isn't running")));
    let ui = h.front_end();
    let cli = h.front_end();
    h.query(&ui, "");

    h.front_end_sends(
        &cli,
        FromFrontEnd::Open {
            query: "hyprland ".into(),
            view: None,
        },
    );
    h.front_end_sends(
        &ui,
        FromFrontEnd::Query {
            text: "hyprland ".into(),
        },
    );

    let (_, scope) = last_results(ui.drain());
    let intro = scope.and_then(|scope| scope.intro).expect("the intro card");
    assert_eq!(intro.about, "Windows and binds.");
    assert_eq!(intro.note.as_deref(), Some("Hyprland isn't running"));
}
