//! The Hyprland Bundled extension's parts: Hyprland's binds as Items and the
//! Shortcuts View ([`binds`]), with the keys of those on a keycode named by
//! the keyboard's keymap ([`keymap`], fetched by `keyboard` on Linux), its open
//! windows ([`windows`]) kept up to date from its events ([`events`]), its two
//! sockets ([`hyprland`]), and the Lua its Lua config dispatches ([`lua`]).
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
#[cfg(target_os = "linux")]
pub mod keyboard;
pub mod keymap;
pub mod lua;
pub mod windows;
