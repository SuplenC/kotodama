use super::string;

#[test]
fn plain_text_is_quoted_as_it_is() {
    assert_eq!(string("address:0x55d0c6a1b2c0"), r#""address:0x55d0c6a1b2c0""#);
    assert_eq!(string("name:Café"), r#""name:Café""#);
}

#[test]
fn a_quote_or_a_backslash_cant_end_the_literal() {
    assert_eq!(string(r#"a"b\c"#), r#""a\"b\\c""#);
    assert_eq!(string(r#"") hl.exec_cmd("x"#), r#""\") hl.exec_cmd(\"x""#);
}

#[test]
fn a_control_character_is_a_three_digit_escape() {
    assert_eq!(string("a\nb"), r#""a\010b""#);
    assert_eq!(string("\u{0}1"), r#""\0001""#);
}
