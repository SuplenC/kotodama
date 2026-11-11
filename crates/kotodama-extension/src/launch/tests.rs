use super::*;

/// The lines `output` passes on.
fn lines(output: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    each_line(output, |line| lines.push(line));
    lines
}

#[test]
fn each_line_that_isnt_blank_is_passed_on_trimmed_and_cut() {
    assert_eq!(lines(b"one\n two \n\n  \n"), ["one", "two"]);
    assert_eq!(lines(b"no end"), ["no end"]);
    assert!(lines(b"").is_empty());
    let long = "x".repeat(10_000);
    assert_eq!(lines(long.as_bytes())[0].len(), LAST_LINE);
}
