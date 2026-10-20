//! The Hyprland Bundled extension's parts: Hyprland's binds as Items and the
//! Shortcuts View ([`binds`]), its open windows ([`windows`]) kept up to date
//! from its events ([`events`]), and its two sockets ([`hyprland`]).
//!
//! # Owns
//!
//! - Everything about Hyprland the Extension knows, in the modules above.
//!
//! # Not here
//!
//! - The protocol loop, the event thread and when Items are sent: `main.rs`.

#![forbid(unsafe_code)]

pub mod binds;
pub mod events;
pub mod hyprland;
pub mod windows;
