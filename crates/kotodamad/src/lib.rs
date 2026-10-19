//! The Kotodama Daemon. It owns the index and the ranking, runs the Extensions,
//! and serves Front ends on a Unix socket.

pub mod config;
pub mod daemon;
pub mod discovery;
mod index;
