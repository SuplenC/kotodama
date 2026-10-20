//! Searching emoji and symbols through the real Daemon.

mod menu;

use menu::Menu;

#[test]
fn thumbs_finds_thumbs_up_first() {
    let mut menu = Menu::start("");
    let results = menu.search("thumbs");
    assert_eq!(results[0].item.id, "👍", "{:?}", menu.ids("thumbs"));
    assert!(menu.ids("thumbs").contains(&"👎".to_owned()));
}

// Regression: #121.
#[test]
fn the_emoji_named_as_the_query_comes_first() {
    let mut menu = Menu::start("");
    for (query, emoji) in [("fire", "🔥"), ("bird", "🐦")] {
        let ids = menu.ids(query);
        assert_eq!(ids[0], emoji, "{query}: {ids:?}");
    }
}

#[test]
fn arrow_right_finds_the_right_arrow() {
    let mut menu = Menu::start("");
    let ids = menu.ids("arrow right");
    assert!(ids.iter().take(5).any(|id| id == "→"), "{ids:?}");
}

#[test]
fn keyboard_symbols_are_found_by_their_key() {
    let mut menu = Menu::start("");
    for (query, symbol) in [("command", "⌘"), ("option", "⌥"), ("shift", "⇧")] {
        let ids = menu.ids(query);
        assert!(
            ids.iter().take(3).any(|id| id == symbol),
            "{query}: {ids:?}"
        );
    }
}

#[test]
fn a_cldr_keyword_finds_its_emoji() {
    let mut menu = Menu::start("");
    // "like" is one of 👍's CLDR keywords, not a word of its name.
    let ids = menu.ids("like");
    assert!(ids.iter().any(|id| id == "👍"), "{ids:?}");
}

#[test]
fn the_emoji_picked_most_ranks_first() {
    let mut menu = Menu::start("");
    let before = menu.ids("heart");
    let picked = before.get(3).expect("more than three hearts").clone();
    menu.pick(&picked);
    menu.pick(&picked);
    assert_eq!(menu.ids("heart")[0], picked, "before the picks: {before:?}");
}
