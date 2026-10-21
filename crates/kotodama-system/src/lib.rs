//! The system Bundled extension's parts: power and session Items
//! ([`power`]) run through logind ([`logind`]), and the open windows
//! ([`windows`]) of any compositor with wlr-foreign-toplevel-management
//! ([`toplevels`], Linux only).
//!
//! # Owns
//!
//! - Everything about power, the session and windows the Extension knows, in the modules above.
//!
//! # Not here
//!
//! - The protocol loop and when Items are sent: `main.rs`.

#![forbid(unsafe_code)]

pub mod logind;
pub mod power;
#[cfg(target_os = "linux")]
pub mod toplevels;
pub mod windows;
