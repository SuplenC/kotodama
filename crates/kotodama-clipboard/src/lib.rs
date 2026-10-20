//! The clipboard history Bundled extension, as a library: the history on
//! disk, what a clipboard offer becomes, and the Items and Views the history
//! shows as. `main.rs` speaks the protocol, and `wayland.rs` watches the
//! clipboard.

#![forbid(unsafe_code)]

pub mod copied;
pub mod history;
pub mod items;
pub mod wayland;
