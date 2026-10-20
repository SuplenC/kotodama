//! The Hyprland Bundled extension's parts: Hyprland's binds as Items and the
//! Shortcuts View ([`binds`]), its open windows ([`windows`]) kept up to date
//! from its events ([`events`]), and its two sockets ([`hyprland`]).

pub mod binds;
pub mod events;
pub mod hyprland;
pub mod windows;
