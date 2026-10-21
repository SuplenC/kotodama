use super::*;

#[test]
fn markdown_shows_the_text_as_it_is_but_keeps_code_spans() {
    assert_eq!(
        escape("history.half_life_days in ~/a_b.toml: `word_x` *x*"),
        "history.half\\_life\\_days in ~/a\\_b.toml: `word_x` \\*x\\*"
    );
}

#[test]
fn front_ends_name_the_item_and_its_action_as_the_daemon_does() {
    assert_eq!(Problem::list().id, PROBLEMS);
    assert_eq!(Problem::show().id, SHOW);
    assert_eq!(Problem::list().extension, crate::daemon::OWN);
}

#[test]
fn the_item_counts_the_problems() {
    assert_eq!(item(1).title, "1 problem");
    assert_eq!(item(3).title, "3 problems");
}
