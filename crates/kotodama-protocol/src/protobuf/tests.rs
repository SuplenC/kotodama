use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{from_slice, to_vec, unzigzag, varint_len, zigzag};

fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    to_vec(value, &mut buf).expect("encode");
    buf
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Query {
    text: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Message {
    Query { text: String },
    Closed,
    Moved(u32),
    Batch(Vec<String>),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Everything {
    flag: bool,
    count: u32,
    delta: i64,
    ratio: f32,
    precise: f64,
    name: String,
    maybe: Option<String>,
    numbers: Vec<u32>,
    names: Vec<String>,
    nested: Query,
    table: BTreeMap<String, Vec<u32>>,
    message: Message,
    messages: Vec<Message>,
}

fn everything() -> Everything {
    Everything {
        flag: true,
        count: 300,
        delta: -2,
        ratio: 0.5,
        precise: -1.25,
        name: "fir".into(),
        maybe: Some("x".into()),
        numbers: vec![1, 150, 3],
        names: vec!["a".into(), String::new()],
        nested: Query { text: "go".into() },
        table: BTreeMap::from([("k".into(), vec![7, 8]), ("empty".into(), vec![])]),
        message: Message::Batch(vec!["b".into()]),
        messages: vec![Message::Closed, Message::Moved(2)],
    }
}

#[test]
fn a_struct_is_its_fields_numbered_from_one() {
    assert_eq!(
        encode(&Query { text: "fir".into() }),
        [0x0a, 3, b'f', b'i', b'r']
    );
}

#[test]
fn an_enum_is_a_oneof_of_its_variants() {
    assert_eq!(
        encode(&Message::Query { text: "fir".into() }),
        [0x0a, 5, 0x0a, 3, b'f', b'i', b'r']
    );
    assert_eq!(encode(&Message::Closed), [0x12, 0]);
    assert_eq!(encode(&Message::Moved(150)), [0x18, 0x96, 0x01]);
    assert_eq!(
        encode(&Message::Batch(vec!["a".into()])),
        [0x22, 3, 0x0a, 1, b'a']
    );
}

#[test]
fn numbers_in_a_list_are_packed() {
    #[derive(Serialize)]
    struct Highlights {
        at: Vec<u32>,
    }
    assert_eq!(
        encode(&Highlights { at: vec![3, 270] }),
        [0x0a, 3, 3, 0x8e, 0x02]
    );
}

#[test]
fn every_shape_survives_a_round_trip() {
    let bytes = encode(&everything());
    assert_eq!(
        from_slice::<Everything>(&bytes).expect("decode"),
        everything()
    );
}

#[test]
fn a_long_body_gets_a_longer_length() {
    let text = "x".repeat(20_000);
    let message = Message::Query { text: text.clone() };
    let bytes = encode(&message);
    assert_eq!(from_slice::<Message>(&bytes).expect("decode"), message);
    assert_eq!(bytes.len(), 1 + 3 + 1 + 3 + text.len());
}

#[test]
fn fields_may_arrive_in_any_order_and_a_list_may_be_split() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct Split {
        text: String,
        names: Vec<String>,
    }
    // Field 2 (`names`), then field 1 (`text`), then `names` again.
    let bytes = [0x12, 1, b'a', 0x0a, 1, b't', 0x12, 1, b'b'];
    assert_eq!(
        from_slice::<Split>(&bytes).expect("decode"),
        Split {
            text: "t".into(),
            names: vec!["a".into(), "b".into()],
        }
    );
}

#[test]
fn numbers_not_packed_are_read_too() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct Highlights {
        at: Vec<u32>,
    }
    assert_eq!(
        from_slice::<Highlights>(&[0x08, 3, 0x08, 4])
            .expect("decode")
            .at,
        [3, 4]
    );
}

#[test]
fn an_unknown_field_is_skipped_and_a_missing_one_takes_its_default() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct Older {
        text: String,
        count: u32,
        names: Vec<String>,
        maybe: Option<String>,
        nested: Query,
    }
    // `text`, then field 9, which this side doesn't know.
    let bytes = [0x0a, 1, b't', 0x4a, 2, b'?', b'?'];
    assert_eq!(
        from_slice::<Older>(&bytes).expect("decode"),
        Older {
            text: "t".into(),
            count: 0,
            names: vec![],
            maybe: None,
            nested: Query {
                text: String::new()
            },
        }
    );
}

#[test]
fn a_missing_enum_is_an_error() {
    #[derive(Debug, Deserialize)]
    struct Holder {
        _message: Message,
    }
    assert!(from_slice::<Holder>(&[]).is_err());
}

#[test]
fn a_message_cut_inside_a_field_is_an_error() {
    let bytes = encode(&everything());
    assert!(from_slice::<Everything>(&bytes[..bytes.len() - 1]).is_err());
    assert!(from_slice::<Query>(&[0x0a, 3, b'f']).is_err());
    assert!(from_slice::<Query>(&[0x0a]).is_err());
}

#[test]
fn zigzag_and_varint_lengths() {
    for value in [0, 1, -1, 63, -64, i64::MAX, i64::MIN] {
        assert_eq!(unzigzag(zigzag(value)), value);
    }
    assert_eq!(varint_len(0), 1);
    assert_eq!(varint_len(127), 1);
    assert_eq!(varint_len(128), 2);
    assert_eq!(varint_len(u64::MAX), 10);
}
