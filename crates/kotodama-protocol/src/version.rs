//! Protocol versions, and what the Daemon leaves out for a peer on an older
//! one.
//!
//! A version is a major and a minor. A minor version only adds to the one
//! before it: optional fields at the end of a type, new messages, new enum
//! variants. So the Daemon talks to a peer on an older minor version of its
//! major by sending it nothing that version lacks: a message, or a variant
//! inside one, newer than the peer is not sent ([`Since`]), and a field newer
//! than the peer is left out of what is sent ([`unread`]).
//!
//! # Owns
//! [`Version`], [`PROTOCOL_VERSION`], [`Since`] and the fields left out for an
//! older peer.
//!
//! # Not here
//! Which peers the handshake accepts: `handshake.rs`.

use std::cell::Cell;
use std::fmt;

use crate::FormInput;
use crate::Item;
use crate::ItemKind;
use crate::Job;
use crate::JobState;
use crate::ToExtension;
use crate::ToFrontEnd;
use crate::View;
use crate::ViewContent;

/// A protocol version: `major.minor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Goes up with a change an older peer can't read: a field removed,
    /// renamed or made required, a variant removed. The Daemon keeps speaking
    /// every major version from [`FIRST_VERSION`] on.
    pub major: u32,
    /// Goes up with a change that only adds: an optional field at the end of
    /// a type, a message, an enum variant.
    pub minor: u32,
}

impl Version {
    #[must_use]
    pub const fn new(major: u32, minor: u32) -> Self {
        Version { major, minor }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// The protocol version this crate speaks.
///
/// A change to a message, or to anything a message carries, needs a new
/// version, and the `layout` test fails until it has one: a new minor version
/// for a change that only adds, a new major one for any other. Every version
/// released is kept, and the Daemon goes on talking to its peers.
pub const PROTOCOL_VERSION: Version = Version::new(1, 0);

/// The oldest version the Daemon speaks: the protocol frozen at Kotodama's
/// first release. The versions before it were never released.
pub const FIRST_VERSION: Version = Version::new(1, 0);

/// The minor version of [`PROTOCOL_VERSION`]'s major that first has a value,
/// which a peer on an older minor version can't read and is never sent.
///
/// The matches below list every variant, so a new one doesn't compile until
/// it says which minor version added it.
pub trait Since {
    fn since(&self) -> u32;
}

impl Since for ToExtension {
    fn since(&self) -> u32 {
        match self {
            ToExtension::RunAction { item, .. } => item.kind.since(),
            ToExtension::Config(_)
            | ToExtension::LiveQuery { .. }
            | ToExtension::CancelLiveQuery { .. }
            | ToExtension::SubmitForm { .. }
            | ToExtension::ViewPopped { .. }
            | ToExtension::FilterView { .. }
            | ToExtension::ActionCatalog { .. }
            | ToExtension::Rejected { .. } => 0,
        }
    }
}

impl Since for ToFrontEnd {
    fn since(&self) -> u32 {
        match self {
            ToFrontEnd::Results { results, .. } => results.iter().map(|result| result.item.since()).max().unwrap_or(0),
            ToFrontEnd::PushView { view, .. } | ToFrontEnd::UpdateView { view, .. } => view.since(),
            ToFrontEnd::ReplaceView { with, .. } => with.since(),
            ToFrontEnd::JobProgress { job, .. } => job.since(),
            ToFrontEnd::Status { .. }
            | ToFrontEnd::Close
            | ToFrontEnd::ActionFailed { .. }
            | ToFrontEnd::PopView { .. }
            | ToFrontEnd::FilteredView { .. }
            | ToFrontEnd::Open { .. }
            | ToFrontEnd::Toggle
            | ToFrontEnd::OfferUndo { .. }
            | ToFrontEnd::WithdrawUndo { .. }
            | ToFrontEnd::SetQuery { .. }
            | ToFrontEnd::Reached { .. } => 0,
        }
    }
}

impl Since for Item {
    fn since(&self) -> u32 {
        self.kind.since()
    }
}

impl Since for ItemKind {
    fn since(&self) -> u32 {
        match self {
            ItemKind::App { .. }
            | ItemKind::File { .. }
            | ItemKind::Folder { .. }
            | ItemKind::Project { .. }
            | ItemKind::Window { .. }
            | ItemKind::Url { .. }
            | ItemKind::Namespaced(_) => 0,
        }
    }
}

impl Since for View {
    fn since(&self) -> u32 {
        self.content.since()
    }
}

impl Since for ViewContent {
    fn since(&self) -> u32 {
        match self {
            ViewContent::List { items, .. } => items.iter().map(Since::since).max().unwrap_or(0),
            ViewContent::Form { fields, .. } => fields.iter().map(|field| field.input.since()).max().unwrap_or(0),
            ViewContent::Detail { .. } | ViewContent::Progress { .. } => 0,
        }
    }
}

impl Since for FormInput {
    fn since(&self) -> u32 {
        match self {
            FormInput::Text { .. } | FormInput::Checkbox { .. } | FormInput::Select { .. } => 0,
        }
    }
}

impl Since for Job {
    fn since(&self) -> u32 {
        self.state.since()
    }
}

impl Since for JobState {
    fn since(&self) -> u32 {
        match self {
            JobState::Running | JobState::Succeeded | JobState::Failed => 0,
        }
    }
}

/// Whether `peer` is on an older minor version of this major, and so is sent
/// less: nothing newer than its minor version.
pub(crate) fn older_minor(peer: Version) -> bool {
    peer.major == PROTOCOL_VERSION.major && peer < PROTOCOL_VERSION
}

thread_local! {
    /// The minor version of the peer a message is being encoded for, while
    /// [`encoding_for`] runs; otherwise every field is sent.
    static PEER_MINOR: Cell<u32> = const { Cell::new(u32::MAX) };
}

/// Runs `encode` for a peer on minor version `minor` of this major, so that
/// [`unread`] leaves out the fields that version lacks.
pub(crate) fn encoding_for<R>(minor: u32, encode: impl FnOnce() -> R) -> R {
    let before = PEER_MINOR.replace(minor);
    let encoded = encode();
    PEER_MINOR.set(before);
    encoded
}

/// Whether the peer a message is being encoded for is older than minor
/// version `MINOR`, which added the field: the field's
/// `#[serde(skip_serializing_if = "crate::version::unread::<MINOR, _>")]`.
/// The field must also be `#[serde(default)]`, since an older peer never
/// sends it.
///
/// Postcard is only spoken between peers on the same version, so it never
/// sees a field left out.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "no field of the current major version is newer than its first minor version"
    )
)]
pub(crate) fn unread<const MINOR: u32, T: ?Sized>(_: &T) -> bool {
    PEER_MINOR.get() < MINOR
}

#[cfg(test)]
mod tests;
