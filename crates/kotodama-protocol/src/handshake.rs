//! The handshake: the first frame each side sends, always in JSON because no
//! other Wire format is agreed yet.

use serde::{Deserialize, Deserializer, Serialize};

use crate::{Error, WireFormat};

/// The protocol version this crate speaks. Both sides of a connection must
/// speak the same one, so a peer that would misread this side's messages is
/// turned away at the handshake rather than failing at some later decode.
///
/// It goes up by one with every change an older peer can't read in some Wire
/// format; see `docs/adr/0012-the-protocol-version-goes-up-with-every-layout-change.md`.
/// The `layout` test fails until it does.
pub const PROTOCOL_VERSION: u32 = 5;

/// What one side sends to open a connection. Both sides send theirs straight
/// away, without waiting for the other's, and each then picks the same Wire format
/// with [`Hello::negotiate`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub version: u32,
    /// The Wire formats this side supports. JSON is required. Names this side
    /// doesn't know are dropped, so a newer peer can offer formats an older one
    /// lacks.
    #[serde(deserialize_with = "known_formats")]
    pub formats: Vec<WireFormat>,
}

impl Hello {
    pub fn new(formats: &[WireFormat]) -> Self {
        Hello {
            version: PROTOCOL_VERSION,
            formats: formats.to_vec(),
        }
    }

    /// The fastest Wire format both sides support. The order of either list
    /// doesn't matter, so both sides arrive at the same answer. Fails if either
    /// side leaves out JSON, which every side must support.
    pub fn negotiate(&self, peer: &Hello) -> Result<WireFormat, Error> {
        if self.version != peer.version {
            return Err(Error::VersionMismatch {
                ours: self.version,
                theirs: peer.version,
            });
        }
        if !self.formats.contains(&WireFormat::Json) || !peer.formats.contains(&WireFormat::Json) {
            return Err(Error::JsonNotOffered);
        }
        Ok(WireFormat::ALL
            .into_iter()
            .find(|format| self.formats.contains(format) && peer.formats.contains(format))
            .unwrap_or(WireFormat::Json))
    }
}

fn known_formats<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<WireFormat>, D::Error> {
    // Telling a known name from an unknown one needs a self-describing format.
    // The handshake is always JSON; this keeps `Hello` decodable everywhere else.
    if !deserializer.is_human_readable() {
        return Vec::deserialize(deserializer);
    }
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MaybeKnown {
        Known(WireFormat),
        Unknown(#[allow(dead_code)] serde::de::IgnoredAny),
    }
    let formats = Vec::<MaybeKnown>::deserialize(deserializer)?;
    Ok(formats
        .into_iter()
        .filter_map(|format| match format {
            MaybeKnown::Known(format) => Some(format),
            MaybeKnown::Unknown(_) => None,
        })
        .collect())
}
