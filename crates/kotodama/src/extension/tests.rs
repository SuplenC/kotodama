use super::*;

fn args(line: &str) -> Vec<String> {
    line.split(' ').map(String::from).collect()
}

#[test]
fn check_takes_a_directory_then_a_query_and_an_item_to_run() {
    let mut options = check::Options::new("ext");
    options.query = Some("fir".into());
    options.run = Some("a".into());
    assert_eq!(
        parse(&args("check ext --run a --query fir")),
        Ok(Command::Check(options))
    );
    assert_eq!(
        parse(&args("check ext")),
        Ok(Command::Check(check::Options::new("ext")))
    );
}

#[test]
fn new_writes_python_in_the_current_directory_unless_told_otherwise() {
    let options = scaffold::Options::new("notes");
    assert_eq!(options.language, scaffold::Language::Python);
    assert_eq!(options.parent, PathBuf::from("."));
    assert_eq!(parse(&args("new notes")), Ok(Command::New(options)));

    let mut options = scaffold::Options::new("notes");
    options.language = scaffold::Language::Rust;
    options.parent = PathBuf::from("/tmp");
    assert_eq!(
        parse(&args("new notes --dir /tmp --lang rust")),
        Ok(Command::New(options))
    );
}

#[test]
fn a_missing_argument_an_unknown_flag_or_language_is_an_error() {
    assert_eq!(parse(&[]), Err(ParseError::NoCommand));
    assert_eq!(
        parse(&args("check")),
        Err(ParseError::Missing("check", "the Extension's directory"))
    );
    assert_eq!(
        parse(&args("check ext --query")),
        Err(ParseError::NoValue("--query".into()))
    );
    assert_eq!(
        parse(&args("check ext --size 3")),
        Err(ParseError::UnknownOption("--size".into()))
    );
    assert_eq!(
        parse(&args("new notes --lang cobol")),
        Err(ParseError::UnknownLanguage("cobol".into()))
    );
    assert_eq!(parse(&args("list")), Err(ParseError::UnknownCommand("list".into())));
}
