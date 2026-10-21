//! The clipboard history Bundled extension, as a library: the history on
//! disk, what a clipboard offer becomes, and the Items and Views the history
//! shows as. `main.rs` speaks the protocol, and `wayland.rs` watches the
//! clipboard.
//!
//! # Owns
//!
//! - What the binary, its tests and its bench share: `copied`, `history`, `items` and `wayland`.
//!
//! # Not here
//!
//! - The protocol loop and the Actions: `main.rs`.

#![forbid(unsafe_code)]

pub mod copied;
pub mod history;
pub mod items;
pub mod wayland;
