//! The functions Qt looks up to load the module as a plugin. moc defines them
//! in C++ under these names, but a Rust cdylib exports only symbols Rust
//! defines, so `build.rs` renames moc's and these pass them on.
//!
//! # Owns
//!
//! - `qt_plugin_query_metadata_v2` and `qt_plugin_instance`, the symbols Qt loads the plugin by.
//!
//! # Not here
//!
//! - Renaming moc's functions: `build.rs`.
//! - The QML types the plugin registers: `bridge`.

use std::ffi::c_void;

/// Qt's `QPluginMetaData`.
#[repr(C)]
pub struct PluginMetaData {
    data: *const u8,
    size: usize,
}

unsafe extern "C" {
    fn kotodama_qt_plugin_query_metadata_v2() -> PluginMetaData;
    fn kotodama_qt_plugin_instance() -> *mut c_void;
}

#[unsafe(no_mangle)]
pub extern "C" fn qt_plugin_query_metadata_v2() -> PluginMetaData {
    // SAFETY: moc's function takes nothing and returns static data.
    unsafe { kotodama_qt_plugin_query_metadata_v2() }
}

#[unsafe(no_mangle)]
pub extern "C" fn qt_plugin_instance() -> *mut c_void {
    // SAFETY: moc's function takes nothing and returns the plugin's QObject,
    // which Qt owns.
    unsafe { kotodama_qt_plugin_instance() }
}
