use kotodama_protocol::{Channel, Error, Hello, PROTOCOL_VERSION, Version, WireFormat, frame};

use WireFormat::{Json, MessagePack, Postcard, Protobuf};

fn hello(json: &str) -> Hello {
    WireFormat::Json.decode(json.as_bytes()).unwrap()
}

fn on(major: u32, minor: u32, formats: &[WireFormat]) -> Hello {
    Hello {
        version: major,
        minor,
        formats: formats.to_vec(),
    }
}

#[test]
fn a_side_that_only_speaks_json_ends_up_on_json() {
    let daemon = Hello::new(&WireFormat::ALL);
    let shell_script = hello(&format!(
        r#"{{"version": {}, "minor": {}, "formats": ["json"]}}"#,
        PROTOCOL_VERSION.major, PROTOCOL_VERSION.minor
    ));
    assert_eq!(daemon.accept_peer(&shell_script).unwrap(), Json);
    assert_eq!(shell_script.accept_daemon(&daemon).unwrap(), Json);
}

#[test]
fn two_sides_that_both_speak_postcard_end_up_on_postcard() {
    let daemon = Hello::new(&WireFormat::ALL);
    let rust_extension = Hello::new(&[Json, Postcard]);
    assert_eq!(daemon.accept_peer(&rust_extension).unwrap(), Postcard);
    assert_eq!(rust_extension.accept_daemon(&daemon).unwrap(), Postcard);
}

#[test]
fn protobuf_wins_over_messagepack_and_messagepack_over_json_whatever_order_the_lists_are_in() {
    let python = Hello::new(&[Json, MessagePack]);
    let daemon = Hello::new(&[Json, Postcard, MessagePack, Protobuf]);
    assert_eq!(python.negotiate(&daemon).unwrap(), MessagePack);
    let go = Hello::new(&[Json, Protobuf, MessagePack]);
    assert_eq!(go.negotiate(&daemon).unwrap(), Protobuf);
}

#[test]
fn formats_this_side_has_never_heard_of_are_ignored() {
    let future = hello(&format!(
        r#"{{"version": {}, "formats": ["cbor", "msgpack", "json"]}}"#,
        PROTOCOL_VERSION.major
    ));
    let daemon = Hello::new(&WireFormat::ALL);
    assert_eq!(daemon.negotiate(&future).unwrap(), MessagePack);
}

#[test]
fn the_handshake_is_plain_json() {
    let daemon = Hello::new(&WireFormat::ALL);
    let json = String::from_utf8(WireFormat::Json.encode(&daemon).unwrap()).unwrap();
    assert_eq!(
        json,
        format!(
            r#"{{"version":{},"minor":{},"formats":["postcard","protobuf","msgpack","json"]}}"#,
            PROTOCOL_VERSION.major, PROTOCOL_VERSION.minor
        )
    );
}

#[test]
fn a_hello_without_a_minor_version_is_on_minor_version_0() {
    assert_eq!(
        hello(r#"{"version": 1, "formats": ["json"]}"#).protocol(),
        Version::new(1, 0)
    );
}

#[test]
fn a_side_that_leaves_out_json_fails_the_handshake_even_with_a_format_in_common() {
    let daemon = Hello::new(&WireFormat::ALL);
    let no_json = Hello::new(&[Postcard]);
    assert!(matches!(
        daemon.accept_peer(&no_json),
        Err(Error::JsonNotOffered)
    ));
    assert!(matches!(
        no_json.accept_daemon(&daemon),
        Err(Error::JsonNotOffered)
    ));
}

#[test]
fn a_peer_on_an_older_minor_version_is_accepted_on_a_format_that_tolerates_what_it_lacks() {
    let daemon = on(1, 4, &WireFormat::ALL);
    let rust_extension = on(1, 2, &WireFormat::ALL);
    assert_eq!(daemon.accept_peer(&rust_extension).unwrap(), Protobuf);
    assert_eq!(rust_extension.accept_daemon(&daemon).unwrap(), Protobuf);

    let python = on(1, 0, &[Json, MessagePack]);
    assert_eq!(daemon.accept_peer(&python).unwrap(), MessagePack);
}

#[test]
fn a_peer_newer_than_the_daemon_is_turned_away_with_the_version_it_needs() {
    let daemon = Hello::new(&WireFormat::ALL);
    for (major, minor) in [
        (PROTOCOL_VERSION.major, PROTOCOL_VERSION.minor + 1),
        (PROTOCOL_VERSION.major + 1, 0),
    ] {
        let newer = on(major, minor, &WireFormat::ALL);

        let error = daemon
            .accept_peer(&newer)
            .expect_err("a newer peer is refused");

        assert!(
            matches!(error, Error::PeerTooNew { peer, ours } if peer == Version::new(major, minor) && ours == PROTOCOL_VERSION)
        );
        assert_eq!(
            error.to_string(),
            format!(
                "the peer needs protocol {major}.{minor}, newer than this Daemon's {PROTOCOL_VERSION}: update Kotodama"
            )
        );
    }
}

#[test]
fn a_peer_from_before_the_first_release_is_turned_away() {
    let daemon = Hello::new(&WireFormat::ALL);
    let old = hello(r#"{"version": 0, "formats": ["json"]}"#);
    assert!(matches!(
        daemon.accept_peer(&old),
        Err(Error::PeerTooOld { peer, ours }) if peer == Version::new(0, 0) && ours == PROTOCOL_VERSION
    ));
}

#[test]
fn a_daemon_older_than_this_side_is_refused_by_this_side() {
    let daemon = on(1, 1, &WireFormat::ALL);
    let extension = on(1, 2, &WireFormat::ALL);
    let error = extension
        .accept_daemon(&daemon)
        .expect_err("an older Daemon is refused");
    assert_eq!(
        error.to_string(),
        "the Daemon speaks protocol 1.1, older than the 1.2 this side needs: update Kotodama"
    );
}

#[test]
fn a_peer_takes_a_newer_daemon_which_adapts_to_it() {
    let daemon = on(1, 3, &WireFormat::ALL);
    let extension = on(1, 0, &[Json]);
    assert_eq!(extension.accept_daemon(&daemon).unwrap(), Json);
}

#[test]
fn a_peer_built_for_a_newer_major_is_turned_away_at_the_handshake_not_at_a_later_decode() {
    let newer = PROTOCOL_VERSION.major + 1;
    let mut peer_hello = Vec::new();
    frame::write(
        &mut peer_hello,
        format!(r#"{{"version":{newer},"formats":["postcard","protobuf","msgpack","json"]}}"#)
            .as_bytes(),
    )
    .unwrap();

    let error = Channel::accept(
        peer_hello.as_slice(),
        Vec::new(),
        &Hello::new(&WireFormat::ALL),
    )
    .err()
    .expect("a peer on a newer major should fail the handshake");

    assert!(matches!(
        error,
        Error::PeerTooNew { peer, ours } if peer == Version::new(newer, 0) && ours == PROTOCOL_VERSION
    ));
}
