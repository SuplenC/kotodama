//! The Folder source Bundled extension, as a library: finding projects and
//! scripts in the folders the config names, watching them, and the Items
//! they become. `main.rs` speaks the protocol and starts what their Actions
//! run.

#![forbid(unsafe_code)]

pub mod items;
pub mod projects;
pub mod scan;
pub mod scripts;
pub mod settings;
