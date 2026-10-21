use super::*;

#[test]
fn a_header_sets_a_name_with_spaces_the_terminal_and_a_prompt() {
    let header =
        Header::parse("#!/usr/bin/env bash\n# kotodama: name=Deploy staging terminal=false args=prompt\necho\n");
    assert_eq!(
        header,
        Header {
            name: Some("Deploy staging".into()),
            terminal: false,
            prompt: true,
        }
    );
}

#[test]
fn without_a_header_a_script_runs_in_a_terminal_without_asking() {
    assert_eq!(Header::parse("#!/bin/sh\necho kotodama: name=x\n"), Header::default());
}

#[test]
fn other_comment_styles_and_quotes_work_too() {
    assert_eq!(
        Header::parse("#!/usr/bin/env node\n// kotodama: name=\"Say hi\"\n").name,
        Some("Say hi".into())
    );
    assert!(!Header::parse("-- kotodama: terminal=false\n").terminal);
    assert!(Header::parse("## kotodama: args=prompt").prompt);
}

#[test]
fn unknown_options_are_skipped_and_the_rest_apply() {
    let header = Header::parse("# kotodama: colour=red terminal=false name=x");
    assert!(!header.terminal);
    assert_eq!(header.name.as_deref(), Some("x"));
}

#[test]
fn a_header_past_the_first_lines_is_not_one() {
    let text = format!("{}# kotodama: name=late\n", "#\n".repeat(HEADER_LINES));
    assert_eq!(Header::parse(&text).name, None);
}
