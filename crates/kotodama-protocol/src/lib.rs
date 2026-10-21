//! The protocol the Kotodama Daemon, Front ends and Extensions share: the
//! messages, how they are framed, the handshake that picks a Wire format, and the
//! `manifest.toml` schema. It also builds the [`launch_command`] that the Daemon
//! and Extensions start the user's programs with.
//!
//! A connection opens with each side sending a [`Hello`] in JSON, naming its
//! protocol [`Version`]. Both then use the fastest [`WireFormat`] they share
//! for every later message, each sent as one [`frame`]. [`Channel`] does all
//! of this over any pair of byte streams.
//!
//! Between the Daemon and an Extension, the Daemon sends [`ToExtension`] and the
//! Extension sends [`FromExtension`]. The first message after the handshake is
//! always the Extension's [`ToExtension::Config`]. Between the Daemon and a
//! Front end, the Front end sends [`FromFrontEnd`] and the Daemon sends
//! [`ToFrontEnd`].
//!
//! Every type decodes from all four Wire formats, so none uses serde features
//! postcard or protobuf can't decode: no internally tagged or untagged enums,
//! no flattening, and no field skipped but one a newer minor version added,
//! which an older peer is never sent.
//!
//! Any change to how a message encodes, in any Wire format, needs a new
//! [`PROTOCOL_VERSION`]: a minor one for a change that only adds, a major one
//! for any other. The Daemon talks to every peer on its version or an older
//! one, and turns away a newer one at the handshake. `tests/layout.rs` keeps
//! the layout of every version and fails until the version goes up.

#![forbid(unsafe_code)]

mod channel;
pub mod frame;
mod front_end;
mod handshake;
mod item;
mod launch;
mod manifest;
mod message;
mod protobuf;
mod shortcut;
mod value;
mod version;
mod view;
mod wire;

pub use channel::{Channel, ChannelReader, ChannelWriter};
pub use front_end::{
    APP_ENTRY_KIND, ActionRef, Example, FromFrontEnd, Intro, ListMatch, OfferedAction, ResultRef,
    Scope, ShownResult, ToFrontEnd, socket_path,
};
pub use handshake::Hello;
pub use item::{
    Action, Argument, Item, ItemKind, ItemRef, KindName, NamespacedKind, SHARED_KINDS,
    is_valid_name,
};
pub use launch::{LAUNCH_PREFIX, LAUNCH_SCOPED, SCOPE_PREFIX, launch_command, launch_prefix};
pub use manifest::{ContributedAction, LiveQueries, Manifest, is_valid_word};
pub use message::{CatalogEntry, Clipboard, FromExtension, LiveResult, Outcome, ToExtension};
pub use shortcut::{FORGET_SHORTCUT, RESERVED_SHORTCUTS, UNDO_SHORTCUT, is_reserved_shortcut};
pub use value::{Bytes, Fraction, Table, Value};
pub use version::{FIRST_VERSION, PROTOCOL_VERSION, Since, Version};
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
    /// The Daemon's side: the peer was built for a newer protocol than the
    /// Daemon speaks.
    #[error("the peer needs protocol {peer}, newer than this Daemon's {ours}: update Kotodama")]
    PeerTooNew { peer: Version, ours: Version },
    /// The Daemon's side: the peer speaks a protocol from before any release.
    #[error("the peer speaks protocol {peer}, which no release of Kotodama speaks; this Daemon speaks {first} to {ours}", first = FIRST_VERSION)]
    PeerTooOld { peer: Version, ours: Version },
    /// An Extension's or a Front end's side: the Daemon is older than the
    /// protocol this side was built for.
    #[error(
        "the Daemon speaks protocol {daemon}, older than the {ours} this side needs: update Kotodama"
    )]
    DaemonTooOld { daemon: Version, ours: Version },
    #[error("a side's hello leaves out JSON, which every side must support")]
    JsonNotOffered,
    #[error("invalid manifest: {0}")]
    Manifest(String),
}
