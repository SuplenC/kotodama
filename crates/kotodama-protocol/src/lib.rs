//! The protocol the Kotodama Daemon, Front ends and Extensions share: the
//! messages, how they are framed, the handshake that picks a Wire format, and the
//! `manifest.toml` schema.
//!
//! A connection opens with each side sending a [`Hello`] in JSON. Both then use
//! the fastest [`WireFormat`] they share for every later message, each sent as
//! one [`frame`]. [`Channel`] does all of this over any pair of byte streams.
//!
//! Between the Daemon and an Extension, the Daemon sends [`ToExtension`] and the
//! Extension sends [`FromExtension`]. The first message after the handshake is
//! always the Extension's [`ToExtension::Config`].
//!
//! Every type decodes from all three Wire formats, so none uses serde features
//! postcard can't decode: no internally tagged or untagged enums, no flattening,
//! and no skipped fields.

mod channel;
pub mod frame;
mod handshake;
mod item;
mod manifest;
mod message;
mod value;
mod view;
mod wire;

pub use channel::Channel;
pub use handshake::{Hello, PROTOCOL_VERSION};
pub use item::{
    Action, Argument, Item, ItemKind, ItemRef, KindName, NamespacedKind, SHARED_KINDS,
    is_valid_name,
};
pub use manifest::{ContributedAction, LiveQueries, Manifest};
pub use message::{CatalogEntry, Clipboard, FromExtension, LiveResult, Outcome, ToExtension};
pub use value::{Bytes, Fraction, Table, Value};
pub use view::{
    FormField, FormInput, Job, JobState, MetadataEntry, SelectOption, View, ViewContent,
};
pub use wire::WireFormat;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("a frame header must be a decimal length followed by a newline")]
    BadFrameHeader,
    #[error("a frame of {0} bytes is longer than the limit of {max}", max = frame::MAX_FRAME_LEN)]
    FrameTooLong(usize),
    #[error("couldn't encode a message as {format:?}")]
    Encode {
        format: WireFormat,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("couldn't decode a message as {format:?}")]
    Decode {
        format: WireFormat,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("the peer speaks protocol version {theirs}, but this side speaks {ours}")]
    VersionMismatch { ours: u32, theirs: u32 },
    #[error("a side's hello leaves out JSON, which every side must support")]
    JsonNotOffered,
    #[error("invalid manifest: {0}")]
    Manifest(String),
}
