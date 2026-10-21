use super::*;

#[test]
fn the_last_line_that_isnt_blank_is_kept_and_cut() {
    assert_eq!(last_line_of(&b"one\ntwo\n\n  \n"[..]), Some("two".into()));
    assert_eq!(last_line_of(&b"no end"[..]), Some("no end".into()));
    assert_eq!(last_line_of(&b""[..]), None);
    let long = "x".repeat(10_000);
    assert_eq!(last_line_of(long.as_bytes()).map(|line| line.len()), Some(LAST_LINE));
}
