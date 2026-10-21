//! Finding the compositor's socket, and waiting for it.
//!
//! A Daemon or Extension that started before the compositor's environment
//! reached the service manager has no `WAYLAND_DISPLAY`, and can't read the
//! fresh one (`systemctl --user show-environment` isn't there for every
//! service manager). It doesn't need it: without `WAYLAND_DISPLAY`, the
//! compositor is the `wayland-N` socket in `$XDG_RUNTIME_DIR`, which every
//! compositor makes there (see [`crate::runtime`]).
//!
//! # Owns
//!
//! Which sockets are the compositor's, in order, and connecting to the first
//! that answers, or waiting for one.
//!
//! # Not here
//!
//! The runtime directory and the wait itself (`crate::runtime`); what is
//! done over the connection.

use std::ffi::OsString;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;

use wayland_client::Connection;

/// Where the compositor listens: `WAYLAND_DISPLAY`, a name in
/// `XDG_RUNTIME_DIR` or a path, or without it, the `wayland-N` sockets in
/// `XDG_RUNTIME_DIR`, lowest `N` first.
#[derive(Debug)]
pub struct Display {
    name: Option<OsString>,
    runtime: Option<PathBuf>,
}

impl Display {
    /// The display in this process's `WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR`.
    #[must_use]
    pub fn from_env() -> Display {
        Display::new(std::env::var_os("WAYLAND_DISPLAY"), crate::runtime::dir())
    }

    /// The display named `name` (`WAYLAND_DISPLAY`; empty is none), with
    /// `runtime` as `XDG_RUNTIME_DIR`.
    #[must_use]
    pub fn new(name: Option<OsString>, runtime: Option<PathBuf>) -> Display {
        Display {
            name: name.filter(|name| !name.is_empty()),
            runtime,
        }
    }

    /// The sockets to try, in order.
    pub fn sockets(&self) -> Vec<PathBuf> {
        if let Some(name) = &self.name {
            let path = Path::new(name);
            return if path.is_absolute() {
                vec![path.to_owned()]
            } else {
                self.runtime.iter().map(|dir| dir.join(path)).collect()
            };
        }
        self.runtime
            .as_deref()
            .map(crate::runtime::wayland_sockets)
            .unwrap_or_default()
    }

    /// Connects to the first socket that takes the connection.
    ///
    /// # Errors
    ///
    /// When none does, with a message for the log naming the last socket
    /// tried, or where none was found.
    pub fn connect(&self) -> Result<Connection, String> {
        let sockets = self.sockets();
        let mut last = None;
        for socket in &sockets {
            match UnixStream::connect(socket) {
                Ok(stream) => {
                    return Connection::from_socket(stream).map_err(|err| err.to_string());
                }
                Err(err) => last = Some(format!("{}: {err}", socket.display())),
            }
        }
        Err(last.unwrap_or_else(|| match (&self.name, &self.runtime) {
            (Some(name), _) => format!("no Wayland compositor at {}", name.display()),
            (None, Some(dir)) => format!("no Wayland compositor in {}", dir.display()),
            (None, None) => "no Wayland compositor: neither WAYLAND_DISPLAY nor XDG_RUNTIME_DIR is set".into(),
        }))
    }

    /// Connects, and hands the connection to `attempt`, until it succeeds,
    /// waiting between tries for the socket's directory to change
    /// ([`runtime::wait_for`](crate::runtime::wait_for)): it costs nothing
    /// while there is no compositor. `waiting` hears why the first try
    /// failed, so the wait is logged once.
    pub fn wait<T>(&self, mut attempt: impl FnMut(Connection) -> Result<T, String>, waiting: impl FnOnce(&str)) -> T {
        let dir = match &self.name {
            Some(name) if Path::new(name).is_absolute() => Path::new(name).parent(),
            _ => self.runtime.as_deref(),
        };
        crate::runtime::wait_for(dir.as_slice(), || self.connect().and_then(&mut attempt), waiting)
    }
}

#[cfg(test)]
mod tests;
