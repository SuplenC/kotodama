use kotodama_protocol::Table;
use kotodama_protocol::Value;

use super::*;

#[test]
fn it_is_off_until_a_key_or_an_endpoint_is_configured() {
    let key = Table::from([(
        "api_key_command".into(),
        Value::Array(vec![Value::String("pass".into())]),
    )]);
    let local = Table::from([(
        "url".into(),
        Value::String("http://localhost:11434/v1/messages".into()),
    )]);

    assert!(!Settings::from_config(&Table::new()).enabled());
    assert!(Settings::from_config(&key).enabled());
    assert!(Settings::from_config(&local).enabled());
}

#[test]
fn an_empty_effort_is_left_out() {
    let table = Table::from([("effort".into(), Value::String(String::new()))]);

    assert_eq!(Settings::from_config(&table).effort, None);
}

#[test]
fn a_negative_debounce_keeps_the_default() {
    let table = Table::from([("debounce_ms".into(), Value::Integer(-1))]);

    assert_eq!(Settings::from_config(&table).debounce, DEFAULT_DEBOUNCE);
}
