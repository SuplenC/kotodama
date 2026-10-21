//! The handshake: the first frame each side sends, always in JSON because no
//! other Wire format is agreed yet.
//!
//! The Daemon talks to a peer on its own version or an older one, and turns
//! away a newer peer with an error naming the version the peer needs. A peer
//! talks to a Daemon on its version or a newer one, which adapts to it.
//!
//! # Owns
//!
//! - [`Hello`], and which Wire format two hellos agree on.
//!
//! # Not here
//!
//! - Sending and reading the hellos: `channel.rs`. The version rules: `version.rs`.

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

use crate::Error;
use crate::WireFormat;
use crate::version::FIRST_VERSION;
use crate::version::PROTOCOL_VERSION;
use crate::version::Version;

/// What one side sends to open a connection. Both sides send theirs straight
/// away, without waiting for the other's. The Daemon then checks the peer's
/// with [`Hello::accept_peer`], the peer the Daemon's with
/// [`Hello::accept_daemon`], and both arrive at the same Wire format.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// The major protocol version.
    pub version: u32,
    /// The minor protocol version; 0 when left out.
    #[serde(default)]
    pub minor: u32,
    /// The Wire formats this side supports. JSON is required. Names this side
    /// doesn't know are dropped, so a newer peer can offer formats an older one
    /// lacks.
    #[serde(deserialize_with = "known_formats")]
    pub formats: Vec<WireFormat>,
}

impl Hello {
    /// This side's hello: [`PROTOCOL_VERSION`] and `formats`.
    #[must_use]
    pub fn new(formats: &[WireFormat]) -> Self {
        Hello {
            version: PROTOCOL_VERSION.major,
            minor: PROTOCOL_VERSION.minor,
            formats: formats.to_vec(),
        }
    }

    /// The protocol version this hello names.
    pub fn protocol(&self) -> Version {
        Version::new(self.version, self.minor)
    }

    /// The Daemon's side: the Wire format for `peer`, an Extension or a Front
    /// end, on this version or an older one.
    ///
    /// # Errors
    ///
    /// [`Error::PeerTooNew`] for a peer on a newer version than this side,
    /// [`Error::PeerTooOld`] for one older than any released version, and
    /// [`Error::JsonNotOffered`] as for [`Hello::negotiate`].
    pub fn accept_peer(&self, peer: &Hello) -> Result<WireFormat, Error> {
        let (ours, theirs) = (self.protocol(), peer.protocol());
        if theirs > ours {
            return Err(Error::PeerTooNew { peer: theirs, ours });
        }
        if theirs < FIRST_VERSION || theirs.major < ours.major {
            // Every major version is kept: an older one would be spoken in its
            // own frozen types, and there is none before this one.
            return Err(Error::PeerTooOld { peer: theirs, ours });
        }
        self.negotiate(peer)
    }

    /// An Extension's or a Front end's side: the Wire format for the Daemon,
    /// which speaks this version or a newer one.
    ///
    /// # Errors
    ///
    /// [`Error::DaemonTooOld`] for a Daemon on an older version than this
    /// side, and [`Error::JsonNotOffered`] as for [`Hello::negotiate`].
    pub fn accept_daemon(&self, daemon: &Hello) -> Result<WireFormat, Error> {
        let (ours, theirs) = (self.protocol(), daemon.protocol());
        if theirs < ours {
            return Err(Error::DaemonTooOld { daemon: theirs, ours });
        }
        self.negotiate(daemon)
    }

    /// The fastest Wire format both sides support, leaving out postcard unless
    /// both are on the same version: postcard has no room for a field one side
    /// lacks. The order of either list doesn't matter, so both sides arrive at
    /// the same answer. This checks no versions; [`Hello::accept_peer`] and
    /// [`Hello::accept_daemon`] do.
    ///
    /// # Errors
    ///
    /// [`Error::JsonNotOffered`] if either side leaves out JSON, which every
    /// side must support.
    pub fn negotiate(&self, peer: &Hello) -> Result<WireFormat, Error> {
        if !self.formats.contains(&WireFormat::Json) || !peer.formats.contains(&WireFormat::Json) {
            return Err(Error::JsonNotOffered);
        }
        let same_version = self.protocol() == peer.protocol();
        Ok(WireFormat::ALL
            .into_iter()
            .filter(|&format| format != WireFormat::Postcard || same_version)
            .find(|format| self.formats.contains(format) && peer.formats.contains(format))
            .unwrap_or(WireFormat::Json))
    }
}

fn known_formats<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<WireFormat>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MaybeKnown {
        Known(WireFormat),
        Unknown(serde::de::IgnoredAny),
    }
    // Telling a known name from an unknown one needs a self-describing format.
    // The handshake is always JSON; this keeps `Hello` decodable everywhere else.
    if !deserializer.is_human_readable() {
        return Vec::deserialize(deserializer);
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
