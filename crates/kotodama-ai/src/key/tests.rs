use super::*;

fn command(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

#[test]
fn a_secret_never_prints() {
    assert_eq!(format!("{:?}", Secret::new("sk-1")), "Secret(<redacted>)");
}

#[test]
fn the_key_is_the_first_line_the_command_prints() {
    let key = read_key(&command(&["printf", "sk-1\nsomething else\n"])).expect("read the key");

    assert_eq!(key, Secret::new("sk-1"));
}

#[test]
fn a_command_that_fails_or_prints_nothing_gives_no_key() {
    assert!(matches!(read_key(&command(&["false"])), Err(KeyError::Failed(_))));
    assert!(matches!(read_key(&command(&["true"])), Err(KeyError::NoKey)));
    assert!(matches!(read_key(&[]), Err(KeyError::Empty)));
}
