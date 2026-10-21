use super::*;

#[test]
fn a_code_block_fence_outlasts_the_backticks_inside() {
    assert_eq!(code_block("a ``` b", 7, 7), "````\na ``` b\n````\n");
    assert_eq!(code_block("plain\n", 6, 6), "```\nplain\n```\n");
}

#[test]
fn a_long_text_is_cut_with_a_note() {
    let text = "line\n".repeat(100);
    let block = code_block(&text, 500, 500);
    assert_eq!(block.matches("line").count(), DETAIL_LINES);
    assert!(block.ends_with("pasting gives all 500 characters."), "{block}");
}

#[test]
fn a_file_url_escapes_what_markdown_would_split_on() {
    assert_eq!(
        file_url(Path::new("/home/a b/thumbs/1.png")),
        "file:///home/a%20b/thumbs/1.png"
    );
}

#[test]
fn numbers_read_as_words() {
    assert_eq!(count(1, "character"), "1 character");
    assert_eq!(count(1_234_567, "character"), "1,234,567 characters");
    assert_eq!(bytes(2048), "2 KB");
    assert_eq!(ago(30), "just now");
    assert_eq!(ago(3 * 3600 + 5), "3 hours ago");
}

#[test]
fn clearing_the_history_asks_first_and_showing_it_does_not() {
    let item = history_item();
    let confirm: Vec<(&str, bool)> = item
        .actions
        .iter()
        .map(|action| (action.id.as_str(), action.confirm))
        .collect();

    assert_eq!(confirm, [(OPEN, false), (CLEAR, true)]);
}
