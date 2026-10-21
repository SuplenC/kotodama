//! Searching emoji and symbols through the real Daemon.

#[path = "support/menu.rs"]
mod menu;

use menu::Menu;

#[test]
fn thumbs_finds_thumbs_up_first() {
    let mut menu = Menu::start("");
    let results = menu.search("emoji thumbs");
    assert_eq!(results[0].item.id, "👍", "{:?}", menu.ids("emoji thumbs"));
    assert!(menu.ids("emoji thumbs").contains(&"👎".to_owned()));
}

// Regression: #121.
#[test]
fn the_emoji_named_as_the_query_comes_first() {
    let mut menu = Menu::start("");
    for (query, emoji) in [("emoji fire", "🔥"), ("emoji bird", "🐦")] {
        let ids = menu.ids(query);
        assert_eq!(ids[0], emoji, "{query}: {ids:?}");
    }
}

#[test]
fn arrow_right_finds_the_right_arrow() {
    let mut menu = Menu::start("");
    let ids = menu.ids("emoji arrow right");
    assert!(ids.iter().take(5).any(|id| id == "→"), "{ids:?}");
}

#[test]
fn keyboard_symbols_are_found_by_their_key() {
    let mut menu = Menu::start("");
    for (query, symbol) in [("emoji command", "⌘"), ("emoji option", "⌥"), ("emoji shift", "⇧")] {
        let ids = menu.ids(query);
        assert!(ids.iter().take(3).any(|id| id == symbol), "{query}: {ids:?}");
    }
}

#[test]
fn a_cldr_keyword_finds_its_emoji() {
    let mut menu = Menu::start("");
    // "like" is one of 👍's CLDR keywords, not a word of its name.
    let ids = menu.ids("emoji like");
    assert!(ids.iter().any(|id| id == "👍"), "{ids:?}");
}

#[test]
fn the_emoji_picked_most_ranks_first() {
    let mut menu = Menu::start("");
    let before = menu.ids("emoji heart");
    let picked = before.get(3).expect("more than three hearts").clone();
    menu.pick(&picked);
    menu.pick(&picked);
    assert_eq!(menu.ids("emoji heart")[0], picked, "before the picks: {before:?}");
}

#[test]
fn the_emoji_are_searched_behind_their_prefix() {
    let mut menu = Menu::start("");

    assert!(!menu.ids("fire").contains(&"🔥".to_owned()));
    assert_eq!(menu.ids("emoji")[0], "prefix:emoji");
}

#[test]
fn an_empty_prefix_puts_the_emoji_back_in_the_search() {
    let mut menu = Menu::start_with("[extensions.emoji]\nprefix = \"\"\n", "thumbs");

    assert_eq!(menu.ids("fire")[0], "🔥");
}
