//! A Prefix: one Extension's Items searched only behind a keyword, such as
//! `emoji fire`, with an entry Item in the search that enters it.

mod common;

use common::*;
use kotodama_protocol::*;
use kotodamad::discovery::Found;

/// The emoji Extension as its Manifest declares it, behind `emoji`.
fn emoji() -> Found {
    manifest(
        "name = \"emoji\"\ncommand = [\"x\"]\nprefix = \"emoji\"\ntitle = \"Emoji\"\nicon = \"face-smile\"",
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
        let (results, scope) = self
            .ui
            .drain()
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
}

fn emoji_scope() -> Option<Scope> {
    Some(Scope {
        prefix: "emoji".into(),
        title: "Emoji".into(),
        icon: Some("face-smile".into()),
    })
}

#[test]
fn the_search_leaves_out_the_items_behind_a_prefix() {
    let mut desk = Desk::new("");

    assert_eq!(desk.titles("editor"), ["Editor"]);
    assert!(!desk.titles("fire").iter().any(|title| title.contains("🔥")));
}

#[test]
fn the_prefix_typed_alone_ranks_its_entry_item_first() {
    let mut desk = Desk::new("");

    let (rows, scope) = desk.search("emoji");

    assert_eq!(rows[0], ("kotodama".to_owned(), "Emoji".to_owned()));
    assert!(rows.iter().all(|(extension, _)| extension != "emoji"));
    assert_eq!(scope, None);
}

#[test]
fn the_entry_item_is_found_by_its_title() {
    let mut desk = Desk::new("");

    assert!(desk.titles("emo").contains(&"Emoji".to_owned()));
}

#[test]
fn typing_after_the_prefix_searches_only_its_extensions_items() {
    let mut desk = Desk::new("");

    let (rows, scope) = desk.search("emoji fire");

    assert_eq!(rows[0].1, "🔥 Fire");
    assert!(
        rows.iter().all(|(extension, _)| extension == "emoji"),
        "{rows:?}"
    );
    assert_eq!(scope, emoji_scope());
}

#[test]
fn the_prefix_is_typed_in_any_case() {
    let mut desk = Desk::new("");

    let (rows, scope) = desk.search("Emoji fire");

    assert_eq!(rows[0].1, "🔥 Fire");
    assert_eq!(scope, emoji_scope());
}

#[test]
fn the_prefix_and_a_space_show_the_extensions_items_in_the_order_it_sent_them() {
    let mut desk = Desk::new("");

    let (rows, scope) = desk.search("emoji ");

    let sent: Vec<String> = characters().into_iter().map(|item| item.title).collect();
    let titles: Vec<String> = rows.into_iter().map(|(_, title)| title).collect();
    assert_eq!(titles, sent);
    assert_eq!(scope, emoji_scope());
}

#[test]
fn running_the_entry_item_types_the_prefix_and_a_space() {
    let mut desk = Desk::new("");

    desk.pick("emo", "Emoji");

    let sent = desk.ui.drain();
    assert!(
        sent.contains(&ToFrontEnd::SetQuery {
            text: "emoji ".into()
        }),
        "{sent:?}"
    );
    assert!(!sent.contains(&ToFrontEnd::Close), "the menu stays open");
}

#[test]
fn what_is_picked_behind_the_prefix_leads_its_empty_search() {
    let mut desk = Desk::new("");

    desk.pick("emoji memo", "📝 Memo");

    assert_eq!(desk.titles("emoji ")[0], "📝 Memo");
}

#[test]
fn the_empty_query_shows_the_entry_item_picked_but_not_what_was_picked_behind_it() {
    let mut desk = Desk::new("");
    desk.pick("emo", "Emoji");
    desk.pick("emoji fire", "🔥 Fire");
    desk.pick("edit", "Editor");

    let titles = desk.titles("");

    assert!(titles.contains(&"Emoji".to_owned()), "{titles:?}");
    assert!(titles.contains(&"Editor".to_owned()), "{titles:?}");
    assert!(!titles.contains(&"🔥 Fire".to_owned()), "{titles:?}");
}

#[test]
fn a_near_miss_is_never_behind_a_prefix() {
    let mut desk = Desk::new("");

    let titles = desk.titles("memmo");

    assert!(!titles.contains(&"📝 Memo".to_owned()), "{titles:?}");
}

#[test]
fn a_typo_behind_the_prefix_finds_a_near_miss_there() {
    let mut desk = Desk::new("");

    assert_eq!(desk.titles("emoji memmo"), ["📝 Memo"]);
}

#[test]
fn an_empty_prefix_in_the_config_puts_the_items_back_in_the_search() {
    let mut desk = Desk::new("[extensions.emoji]\nprefix = \"\"");

    assert_eq!(desk.titles("fire")[0], "🔥 Fire");
    let (rows, scope) = desk.search("emoji fire");
    assert_eq!(scope, None);
    assert!(rows.iter().all(|(_, title)| title != "Emoji"), "{rows:?}");
    assert!(!desk.titles("emo").contains(&"Emoji".to_owned()));
}

#[test]
fn the_config_gives_an_extension_a_prefix_of_its_own() {
    let mut desk = Desk::with(plain("emoji"), "[extensions.emoji]\nprefix = \"em\"");

    assert!(!desk.titles("fire").contains(&"🔥 Fire".to_owned()));
    let (rows, scope) = desk.search("em fire");
    assert_eq!(rows[0].1, "🔥 Fire");
    assert_eq!(
        scope,
        Some(Scope {
            prefix: "em".into(),
            title: "emoji".into(),
            icon: None,
        })
    );
}

#[test]
fn the_config_overrides_the_manifests_prefix() {
    let mut desk = Desk::new("[extensions.emoji]\nprefix = \"e\"");

    assert_eq!(desk.search("emoji fire").1, None);
    assert_eq!(desk.search("e fire").0[0].1, "🔥 Fire");
}

#[test]
fn a_live_query_extension_behind_a_prefix_is_asked_only_behind_it() {
    let calc = manifest(
        "name = \"calc\"\ncommand = [\"x\"]\nprefix = \"calc\"\n[live_query]\ntriggers = ['^[0-9]']\ndebounce_ms = 0",
    );
    let mut h = Harness::new(vec![plain("apps"), calc]);
    h.up(0);
    let calc = h.up(1);
    let ui = h.front_end();
    calc.drain();

    h.query(&ui, "1+1");
    let asked_in_the_search = calc.drain();
    h.query(&ui, "calc 1+1");
    let asked_behind_the_prefix = calc.drain();

    assert!(
        !asked_in_the_search
            .iter()
            .any(|message| matches!(message, ToExtension::LiveQuery { .. })),
        "{asked_in_the_search:?}"
    );
    assert!(
        asked_behind_the_prefix.iter().any(|message| matches!(
            message,
            ToExtension::LiveQuery { query, .. } if query == "1+1"
        )),
        "{asked_behind_the_prefix:?}"
    );
}
