use super::*;

fn table(pairs: &[(&str, Value)]) -> Table {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

fn string_value(text: &str) -> Value {
    Value::String(text.into())
}

#[test]
fn a_command_is_a_list_of_strings_that_isnt_empty() {
    let config = table(&[
        ("good", Value::Array(vec![string_value("foot"), string_value("-e")])),
        ("empty", Value::Array(Vec::new())),
        ("mixed", Value::Array(vec![string_value("foot"), Value::Integer(1)])),
        ("word", string_value("foot")),
    ]);
    assert_eq!(command(&config, "good"), Some(vec!["foot".into(), "-e".into()]));
    assert_eq!(command(&config, "empty"), None);
    assert_eq!(command(&config, "mixed"), None);
    assert_eq!(command(&config, "word"), None);
    assert_eq!(command(&config, "missing"), None);
}

#[test]
fn a_setting_of_the_wrong_shape_reads_as_missing() {
    let config = table(&[
        ("url", string_value("https://example.com")),
        ("on", Value::Bool(true)),
        ("number", Value::Integer(3)),
    ]);
    assert_eq!(string(&config, "url"), Some("https://example.com"));
    assert_eq!(string(&config, "number"), None);
    assert_eq!(boolean(&config, "on"), Some(true));
    assert_eq!(boolean(&config, "url"), None);
    assert_eq!(boolean(&config, "missing"), None);
}
