//! The crate's version names the protocol version it speaks, as
//! `kotodama-protocol`'s does, so an Extension asks for the same
//! `major.minor` of both.

use kotodama_protocol::PROTOCOL_VERSION;

#[test]
fn the_crates_major_and_minor_version_are_the_protocols() {
    let version = format!(
        "{}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR")
    );

    assert_eq!(version, PROTOCOL_VERSION.to_string());
}

#[test]
fn it_asks_for_the_kotodama_protocol_of_its_own_protocol_version() {
    let manifest = include_str!("../Cargo.toml");
    let wanted = format!(r#"kotodama-protocol = {{ path = "../kotodama-protocol", version = "{PROTOCOL_VERSION}" }}"#);

    assert!(
        manifest.lines().any(|line| line == wanted),
        "Cargo.toml lacks: {wanted}"
    );
}
