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
//!
//! # Owns
//!
//! - The protocol's types, re-exported here, and [`Error`], what every fallible operation on a
//!   connection returns.
//!
//! # Not here
//!
//! - What the Daemon does with a message: `kotodamad`.
//! - The runtime a Rust Extension is built on (the reader thread, sending only changed Items):
//!   `kotodama-extension`.
//! - The protobuf schema for other languages: `kotodama.proto`, written by `tests/layout.rs`.

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

pub use channel::Channel;
pub use channel::ChannelReader;
pub use channel::ChannelWriter;
pub use front_end::APP_ENTRY_KIND;
pub use front_end::ActionRef;
pub use front_end::Example;
pub use front_end::FieldError;
pub use front_end::FromFrontEnd;
pub use front_end::Intro;
pub use front_end::ListMatch;
pub use front_end::OfferedAction;
pub use front_end::PathCompletion;
pub use front_end::Problem;
pub use front_end::ResultRef;
pub use front_end::Scope;
pub use front_end::ShownResult;
pub use front_end::ToFrontEnd;
pub use front_end::socket_path;
pub use handshake::Hello;
pub use item::Action;
pub use item::ActionRole;
pub use item::Argument;
pub use item::ArgumentKind;
pub use item::ArgumentKindError;
pub use item::Item;
pub use item::ItemKind;
pub use item::ItemRef;
pub use item::KindError;
pub use item::KindName;
pub use item::NamespacedKind;
pub use item::SHARED_KINDS;
pub use item::is_valid_name;
pub use launch::LAUNCH_PREFIX;
pub use launch::LAUNCH_SCOPED;
pub use launch::SCOPE_PREFIX;
pub use launch::launch_command;
pub use launch::launch_prefix;
pub use manifest::ContributedAction;
pub use manifest::ContributedTo;
pub use manifest::LiveQueries;
pub use manifest::Manifest;
pub use manifest::is_valid_word;
pub use message::CatalogEntry;
pub use message::Clipboard;
pub use message::FromExtension;
pub use message::LiveResult;
pub use message::Outcome;
pub use message::ToExtension;
pub use shortcut::FORGET_SHORTCUT;
pub use shortcut::RESERVED_SHORTCUTS;
pub use shortcut::UNDO_SHORTCUT;
pub use shortcut::is_reserved_shortcut;
pub use value::Bytes;
pub use value::Fraction;
pub use value::FractionError;
pub use value::Table;
pub use value::Value;
pub use version::FIRST_VERSION;
pub use version::PROTOCOL_VERSION;
pub use version::Since;
pub use version::Version;
pub use view::FormField;
pub use view::FormInput;
pub use view::Job;
pub use view::JobState;
pub use view::MetadataEntry;
pub use view::SelectOption;
pub use view::View;
pub use view::ViewContent;
pub use wire::WireFormat;

/// Why a connection, a message or a Manifest failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Reading or writing the byte stream failed, or it ended inside a frame.
    #[error(transparent)]
    Io(std::io::Error),
    /// A frame's header isn't a length and a newline.
    #[error("a frame header must be a decimal length followed by a newline")]
    BadFrameHeader,
    /// A frame announced more than [`frame::MAX_FRAME_LEN`] bytes.
    #[error("a frame of {0} bytes is longer than the limit of {max}", max = frame::MAX_FRAME_LEN)]
    FrameTooLong(usize),
    /// A message couldn't be written in the agreed Wire format.
    #[error("couldn't encode a message as {format:?}")]
    Encode {
        format: WireFormat,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// A frame's payload isn't a message in the agreed Wire format. The reader
    /// is left at the next frame.
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
    #[error("the Daemon speaks protocol {daemon}, older than the {ours} this side needs: update Kotodama")]
    DaemonTooOld { daemon: Version, ours: Version },
    /// A side's [`Hello`] leaves out JSON.
    #[error("a side's hello leaves out JSON, which every side must support")]
    JsonNotOffered,
    /// A `manifest.toml` doesn't parse, or breaks one of its rules. The text
    /// is the TOML parser's, which points at the line.
    #[error("invalid manifest: {0}")]
    Manifest(String),
}
