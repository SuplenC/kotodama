use kotodama_protocol::{Channel, Error, Hello, PROTOCOL_VERSION, WireFormat, frame};

use WireFormat::{Json, MessagePack, Postcard};

fn hello(json: &str) -> Hello {
    WireFormat::Json.decode(json.as_bytes()).unwrap()
}

#[test]
fn a_side_that_only_speaks_json_ends_up_on_json() {
    let daemon = Hello::new(&[Postcard, MessagePack, Json]);
    let shell_script = hello(&format!(
        r#"{{"version": {PROTOCOL_VERSION}, "formats": ["json"]}}"#
    ));
    assert_eq!(daemon.negotiate(&shell_script).unwrap(), Json);
    assert_eq!(shell_script.negotiate(&daemon).unwrap(), Json);
}

#[test]
fn two_sides_that_both_speak_postcard_end_up_on_postcard() {
    let daemon = Hello::new(&[Postcard, MessagePack, Json]);
    let rust_extension = Hello::new(&[Json, Postcard]);
    assert_eq!(daemon.negotiate(&rust_extension).unwrap(), Postcard);
    assert_eq!(rust_extension.negotiate(&daemon).unwrap(), Postcard);
}

#[test]
fn messagepack_wins_over_json_whatever_order_the_lists_are_in() {
    let python = Hello::new(&[Json, MessagePack]);
    let daemon = Hello::new(&[Json, Postcard, MessagePack]);
    assert_eq!(python.negotiate(&daemon).unwrap(), MessagePack);
}

#[test]
fn formats_this_side_has_never_heard_of_are_ignored() {
    let future = hello(&format!(
        r#"{{"version": {PROTOCOL_VERSION}, "formats": ["cbor", "msgpack", "json"]}}"#
    ));
    let daemon = Hello::new(&[Postcard, MessagePack, Json]);
    assert_eq!(daemon.negotiate(&future).unwrap(), MessagePack);
}

#[test]
fn the_handshake_is_plain_json() {
    let daemon = Hello::new(&[Postcard, MessagePack, Json]);
    let json = String::from_utf8(WireFormat::Json.encode(&daemon).unwrap()).unwrap();
    assert_eq!(
        json,
        format!(r#"{{"version":{PROTOCOL_VERSION},"formats":["postcard","msgpack","json"]}}"#)
    );
}

#[test]
fn a_side_that_leaves_out_json_fails_the_handshake_even_with_a_format_in_common() {
    let daemon = Hello::new(&[Postcard, MessagePack, Json]);
    let no_json = Hello::new(&[Postcard]);
    assert!(matches!(
        daemon.negotiate(&no_json),
        Err(Error::JsonNotOffered)
    ));
    assert!(matches!(
        no_json.negotiate(&daemon),
        Err(Error::JsonNotOffered)
    ));
}

#[test]
fn sides_on_different_protocol_versions_fail_the_handshake() {
    let daemon = Hello::new(&[Json]);
    let old = hello(r#"{"version": 0, "formats": ["json"]}"#);
    assert!(matches!(
        daemon.negotiate(&old),
        Err(Error::VersionMismatch {
            ours: PROTOCOL_VERSION,
            theirs: 0
        })
    ));
}

#[test]
fn a_peer_built_for_version_1_is_turned_away_at_the_handshake_not_at_a_later_decode() {
    // Version 1 predates the Identity of Items and the copy Outcome, so a postcard
    // peer on it would misread every Item it was sent.
    let mut peer_hello = Vec::new();
    frame::write(
        &mut peer_hello,
        br#"{"version":1,"formats":["postcard","msgpack","json"]}"#,
    )
    .unwrap();

    let error = Channel::handshake(
        peer_hello.as_slice(),
        Vec::new(),
        &Hello::new(&WireFormat::ALL),
    )
    .err()
    .expect("a version 1 peer should fail the handshake");

    assert!(matches!(
        error,
        Error::VersionMismatch {
            ours: PROTOCOL_VERSION,
            theirs: 1
        }
    ));
    assert_eq!(
        error.to_string(),
        format!("the peer speaks protocol version 1, but this side speaks {PROTOCOL_VERSION}")
    );
}
