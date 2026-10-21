//! Wayland for the Daemon and the clipboard Extension, which both need it
//! (feature `wayland`, Linux only): [`Display`] finds the compositor, and
//! [`DataControl`] sets and watches the clipboard through ext-data-control,
//! or wlr-data-control on a compositor that has only that.
//!
//! # Owns
//!
//! Finding the compositor and the clipboard's protocol objects.
//!
//! # Not here
//!
//! What is done with the clipboard: its history (the clipboard Extension),
//! copying an Action's text (the Daemon).

mod data_control;
mod display;

pub use data_control::DataControl;
pub use data_control::DataControlHandler;
pub use data_control::Offer;
pub use data_control::OfferMimes;
pub use data_control::SourceData;
pub use display::Display;

/// The Wayland crates [`delegate_data_control!`](crate::delegate_data_control)
/// names, so a user's own versions can't differ.
#[doc(hidden)]
pub mod reexports {
    pub use wayland_client;
    pub use wayland_protocols;
    pub use wayland_protocols_wlr;
}
