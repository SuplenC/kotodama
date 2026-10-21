//! Watching the Wayland clipboard, and setting it, through
//! `kotodama_extension::wayland` (ext-data-control, or wlr-data-control on a
//! compositor that has only that).
//!
//! The connection lives on a thread of its own, which first waits for the
//! compositor: the Extension may start before the compositor does, or before
//! `WAYLAND_DISPLAY` reaches it (see `kotodama_extension::wayland::Display`).
//! What each new selection offers is checked before anything is read: an
//! offer a password manager marked sensitive is never read at all (see
//! [`choose`](crate::copied::choose)). The rest are read one at a time on a
//! second thread, which also decodes images for their thumbnails, so a slow
//! source or a large image never holds up the clipboard or the Daemon.
//!
//! # Owns
//!
//! - The Wayland connection and its threads, on Linux in `wayland/linux.rs`.
//! - Reading an offer off its source, with a size cap and a timeout.
//!
//! # Not here
//!
//! - Which MIME type to read, and what the bytes become: `copied.rs`.
//! - Finding the compositor and binding data-control: `kotodama_extension::wayland`.

use std::io;

use crate::copied::Copied;

/// What the watching thread reports.
#[derive(Debug)]
pub enum Change {
    /// Something was copied, and read.
    Copied(Copied),
    /// The connection to the compositor is gone.
    Lost,
}

#[cfg(target_os = "linux")]
pub use linux::Clipboard;

/// Without Linux there is no Wayland clipboard to watch.
#[cfg(not(target_os = "linux"))]
#[derive(Debug)]
pub struct Clipboard;

#[cfg(not(target_os = "linux"))]
impl Clipboard {
    /// Fails: there is no Wayland clipboard to watch.
    ///
    /// # Errors
    ///
    /// Always [`WatchError::NoWayland`].
    pub fn watch(_changed: impl Fn(Change) + Send + Sync + 'static) -> Result<Clipboard, WatchError> {
        Err(WatchError::NoWayland)
    }

    /// Fails: there is no Wayland clipboard to set.
    ///
    /// # Errors
    ///
    /// Always [`SetError::NoWayland`].
    pub fn set(&self, _mimes: Vec<String>, _data: Vec<u8>) -> Result<(), SetError> {
        Err(SetError::NoWayland)
    }
}

/// Why [`Clipboard::watch`] couldn't start watching.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    /// A thread couldn't start; its error is the message as it is.
    #[error(transparent)]
    Thread(io::Error),
    /// The pipe that wakes the Wayland thread couldn't be made; its error is
    /// the message as it is.
    #[error(transparent)]
    Pipe(io::Error),
    #[error("the clipboard needs Wayland, which only Linux has")]
    NoWayland,
}

/// Why [`Clipboard::set`] didn't put the data on the clipboard.
#[derive(Debug, thiserror::Error)]
pub enum SetError {
    #[error("lost the connection to the compositor")]
    Lost,
    #[error("the compositor did not answer")]
    NoAnswer,
    #[error("the clipboard needs Wayland, which only Linux has")]
    NoWayland,
}

#[cfg(target_os = "linux")]
mod linux;
