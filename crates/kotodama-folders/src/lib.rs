//! The Folder source Bundled extension, as a library: finding projects and
//! scripts in the folders the config names, watching them, the Items they
//! become, and starting what their Actions run. `main.rs` speaks the
//! protocol.

pub mod items;
pub mod launch;
pub mod projects;
pub mod scan;
pub mod scripts;
pub mod settings;
