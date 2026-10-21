//! The Wire formats, and encoding any protocol message in one of them.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::version::{self, Version};

/// An encoding the two ends of a connection agree on at the handshake. Every
/// protocol type goes through serde, so one set of types serves all four.
///
/// The declaration order is the order of preference: fastest first. Postcard
/// encodes fields by position with nothing to mark where one ends, so it is
/// only used between peers on the same protocol version; the others tolerate a
/// field one side lacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WireFormat {
    /// Compact and fastest, but its messages can only be read by serde, so in
    /// practice only Rust peers offer it.
    Postcard,
    /// Protocol Buffers: binary and tolerant of added fields, with generated
    /// code in most languages from `kotodama.proto`. The fastest format
    /// between peers on different protocol versions.
    Protobuf,
    /// Self-describing and binary, with a library in most languages. Structs are
    /// encoded as maps with named fields, so other languages see the same shape as
    /// JSON.
    #[serde(rename = "msgpack")]
    MessagePack,
    /// The baseline every peer must support, and the format of the handshake.
    Json,
}

impl WireFormat {
    /// Every format, in order of preference.
    pub const ALL: [WireFormat; 4] = [
        WireFormat::Postcard,
        WireFormat::Protobuf,
        WireFormat::MessagePack,
        WireFormat::Json,
    ];

    pub fn encode<T: Serialize + ?Sized>(self, value: &T) -> Result<Vec<u8>, Error> {
        let mut buf = Vec::new();
        self.encode_into(value, &mut buf)?;
        Ok(buf)
    }

    /// Appends the encoded `value` to `buf`, so a caller can reuse one buffer.
    pub fn encode_into<T: Serialize + ?Sized>(
        self,
        value: &T,
        buf: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let encoded = match self {
            WireFormat::Postcard => postcard::to_io(value, &mut *buf)
                .map(drop)
                .map_err(Box::from),
            WireFormat::Protobuf => crate::protobuf::to_vec(value, buf).map_err(Box::from),
            WireFormat::MessagePack => {
                let mut serializer = rmp_serde::Serializer::new(&mut *buf)
                    .with_struct_map()
                    .with_human_readable();
                value.serialize(&mut serializer).map_err(Box::from)
            }
            WireFormat::Json => serde_json::to_writer(&mut *buf, value).map_err(Box::from),
        };
        encoded.map_err(|source| Error::Encode {
            format: self,
            source,
        })
    }

    /// Encodes `value` for a peer on protocol version `peer`: for one on an
    /// older minor version of this major, the fields that version lacks are
    /// left out.
    pub fn encode_for<T: Serialize + ?Sized>(
        self,
        peer: Version,
        value: &T,
    ) -> Result<Vec<u8>, Error> {
        let mut buf = Vec::new();
        self.encode_for_into(peer, value, &mut buf)?;
        Ok(buf)
    }

    /// [`WireFormat::encode_for`], appending to `buf`.
    pub fn encode_for_into<T: Serialize + ?Sized>(
        self,
        peer: Version,
        value: &T,
        buf: &mut Vec<u8>,
    ) -> Result<(), Error> {
        if version::older_minor(peer) {
            return version::encoding_for(peer.minor, || self.encode_into(value, buf));
        }
        self.encode_into(value, buf)
    }

    /// Decodes one whole message; bytes left over after it are an error.
    pub fn decode<T: DeserializeOwned>(self, bytes: &[u8]) -> Result<T, Error> {
        let decoded = match self {
            WireFormat::Postcard => postcard::take_from_bytes(bytes)
                .map_err(Box::from)
                .and_then(|(value, rest)| ensure_consumed(value, rest)),
            WireFormat::Protobuf => crate::protobuf::from_slice(bytes).map_err(Box::from),
            WireFormat::MessagePack => {
                let mut rest = bytes;
                let mut deserializer =
                    rmp_serde::Deserializer::new(&mut rest).with_human_readable();
                T::deserialize(&mut deserializer)
                    .map_err(Box::from)
                    .and_then(|value| ensure_consumed(value, rest))
            }
            WireFormat::Json => serde_json::from_slice(bytes).map_err(Box::from),
        };
        decoded.map_err(|source| Error::Decode {
            format: self,
            source,
        })
    }
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

fn ensure_consumed<T>(value: T, rest: &[u8]) -> Result<T, BoxError> {
    if rest.is_empty() {
        Ok(value)
    } else {
        Err(format!("{} bytes left over after the message", rest.len()).into())
    }
}
