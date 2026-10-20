//! Wayland for the Daemon and the clipboard Extension, which both need it
//! (feature `wayland`, Linux only): [`Display`] finds the compositor, and
//! [`DataControl`] sets and watches the clipboard through ext-data-control,
//! or wlr-data-control on a compositor that has only that.

mod data_control;
mod display;

pub use data_control::{DataControl, DataControlHandler, Offer, OfferMimes, SourceData};
pub use display::Display;

/// The Wayland crates [`delegate_data_control!`](crate::delegate_data_control)
/// names, so a user's own versions can't differ.
#[doc(hidden)]
pub mod reexports {
    pub use wayland_client;
    pub use wayland_protocols;
    pub use wayland_protocols_wlr;
}
