//! The Folder source Bundled extension, as a library: finding projects and scripts in the folders
//! the config names, watching them, and the Items they become.
//!
//! # Owns
//!
//! - The config table: `settings`.
//! - Finding projects (`projects`) and scripts (`scripts`), and searching again when the folders
//!   change (`scan`).
//! - The Items they become, and their Action ids: `items`.
//!
//! # Not here
//!
//! - The protocol loop, and starting what an Action runs: `main.rs`.
//! - The file watcher and the launcher: `kotodama_extension::watch` and
//!   `kotodama_extension::launch`.

#![forbid(unsafe_code)]

pub mod items;
pub mod projects;
pub mod scan;
pub mod scripts;
pub mod settings;
