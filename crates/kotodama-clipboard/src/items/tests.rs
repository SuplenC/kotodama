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

fn action_ids(item: &Item) -> Vec<&str> {
    item.actions.iter().map(|action| action.id.as_str()).collect()
}

#[test]
fn the_pause_item_pauses_while_copies_are_kept_and_resumes_while_paused() {
    const NOW: u64 = 1_800_000_000;
    let off = pause_item(Pause::Off, NOW);
    let paused = pause_item(Pause::Until(NOW + 60), NOW);
    let over = pause_item(Pause::Until(NOW + 60), NOW + 60);

    assert_eq!(off.title, "Pause Clipboard History");
    assert_eq!(action_ids(&off), [PAUSE_5_MINUTES, PAUSE_HOUR, PAUSE_UNTIL_RESUMED]);
    assert_eq!(paused.title, "Resume Clipboard History");
    assert_eq!(action_ids(&paused), [RESUME]);
    assert_eq!(over, off);
}

#[test]
fn each_pause_action_pauses_for_its_time_and_resume_ends_it() {
    const NOW: u64 = 1_800_000_000;

    assert_eq!(paused_by(PAUSE_5_MINUTES, NOW), Some(Pause::Until(NOW + 300)));
    assert_eq!(paused_by(PAUSE_HOUR, NOW), Some(Pause::Until(NOW + 3600)));
    assert_eq!(paused_by(PAUSE_UNTIL_RESUMED, NOW), Some(Pause::UntilResumed));
    assert_eq!(paused_by(RESUME, NOW), Some(Pause::Off));
    assert_eq!(paused_by(CLEAR, NOW), None);
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
