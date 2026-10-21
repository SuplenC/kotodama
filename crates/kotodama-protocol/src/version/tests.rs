use serde::Deserialize;
use serde::Serialize;

use super::PROTOCOL_VERSION;
use super::Version;
use super::encoding_for;
use super::older_minor;
use crate::WireFormat;

/// A message as a later minor version might grow it: `added` arrived in
/// minor version 1.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Grown {
    text: String,
    #[serde(default, skip_serializing_if = "crate::version::unread::<1, _>")]
    added: Option<String>,
}

/// The same message as minor version 0 had it.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Original {
    text: String,
}

fn grown() -> Grown {
    Grown {
        text: "fir".into(),
        added: Some("new".into()),
    }
}

#[test]
fn a_field_newer_than_the_peer_is_left_out_of_what_it_is_sent() {
    for format in [WireFormat::Protobuf, WireFormat::MessagePack, WireFormat::Json] {
        let bytes = encoding_for(0, || format.encode(&grown())).expect("encode");

        let original: Original = format.decode(&bytes).expect("an older peer reads it");

        assert_eq!(original.text, "fir", "{format:?}");
    }
}

#[test]
fn a_peer_on_the_minor_version_that_added_a_field_is_sent_it() {
    for format in WireFormat::ALL {
        let bytes = encoding_for(1, || format.encode(&grown())).expect("encode");
        assert_eq!(format.decode::<Grown>(&bytes).expect("decode"), grown(), "{format:?}");
    }
}

#[test]
fn every_field_is_sent_outside_an_encoding_for_an_older_peer() {
    let bytes = WireFormat::Postcard.encode(&grown()).expect("encode");
    assert_eq!(WireFormat::Postcard.decode::<Grown>(&bytes).expect("decode"), grown());
}

#[test]
fn what_an_older_peer_sends_without_a_newer_field_still_decodes() {
    for format in [WireFormat::Protobuf, WireFormat::MessagePack, WireFormat::Json] {
        let bytes = encoding_for(0, || format.encode(&grown())).expect("encode");

        let back: Grown = format.decode(&bytes).expect("decode");

        assert_eq!(back.added, None, "{format:?}");
    }
}

#[test]
fn versions_order_by_major_then_minor() {
    assert!(Version::new(1, 9) < Version::new(2, 0));
    assert!(Version::new(1, 0) < Version::new(1, 1));
    assert_eq!(Version::new(1, 2).to_string(), "1.2");
}

#[test]
fn only_a_peer_on_an_older_minor_version_of_this_major_is_sent_less() {
    let Version { major, minor } = PROTOCOL_VERSION;
    assert!(!older_minor(PROTOCOL_VERSION));
    assert!(!older_minor(Version::new(major, minor + 1)));
    assert!(!older_minor(Version::new(major + 1, 0)));
    assert_eq!(older_minor(Version::new(major, 0)), minor > 0);
}
