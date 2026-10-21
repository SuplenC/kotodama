use super::*;

#[test]
fn the_box_keeps_what_follows_the_prefix_and_its_space() {
    assert_eq!(behind("emoji fire", "emoji"), Some("fire"));
    assert_eq!(behind("emoji ", "emoji"), Some(""));
    assert_eq!(behind("  Emoji\t fire", "emoji"), Some(" fire"));
}

#[test]
fn a_query_that_only_starts_like_the_prefix_isnt_behind_it() {
    assert_eq!(behind("emoji", "emoji"), None);
    assert_eq!(behind("emojis fire", "emoji"), None);
    assert_eq!(behind("fire emoji ", "emoji"), None);
}

#[test]
fn the_daemon_gets_the_prefix_a_space_and_the_box() {
    assert_eq!(full(Some("emoji"), "fire"), "emoji fire");
    assert_eq!(full(Some("emoji"), ""), "emoji ");
    assert_eq!(full(None, "fire"), "fire");
}
