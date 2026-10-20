//! Finding the compositor's socket, and waiting for it.
//!
//! A Daemon or Extension that started before the compositor's environment
//! reached the service manager has no `WAYLAND_DISPLAY`, and can't read the
//! fresh one (`systemctl --user show-environment` isn't there for every
//! service manager). It doesn't need it: without `WAYLAND_DISPLAY`, the
//! compositor is the `wayland-N` socket in `$XDG_RUNTIME_DIR`, which every
//! compositor makes there (see [`crate::runtime`]).

use std::ffi::{OsStr, OsString};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use wayland_client::Connection;

/// Where the compositor listens: `WAYLAND_DISPLAY`, a name in
/// `XDG_RUNTIME_DIR` or a path, or without it, the `wayland-N` sockets in
/// `XDG_RUNTIME_DIR`, lowest `N` first.
pub struct Display {
    name: Option<OsString>,
    runtime: Option<PathBuf>,
}

impl Display {
    /// The display in this process's `WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR`.
    pub fn from_env() -> Display {
        Display::new(std::env::var_os("WAYLAND_DISPLAY"), crate::runtime::dir())
    }

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
        let Some(entries) = self.runtime.as_ref().and_then(|dir| dir.read_dir().ok()) else {
            return Vec::new();
        };
        let mut found: Vec<(u32, PathBuf)> = entries
            .flatten()
            .filter_map(|entry| {
                let number = display_number(&entry.file_name())?;
                // Follows a link, so a socket linked in counts.
                let path = entry.path();
                path.metadata()
                    .ok()
                    .filter(|meta| meta.file_type().is_socket())
                    .map(|_| (number, path))
            })
            .collect();
        found.sort_unstable();
        found.into_iter().map(|(_, path)| path).collect()
    }

    /// Connects to the first socket that takes the connection.
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
            (None, None) => {
                "no Wayland compositor: neither WAYLAND_DISPLAY nor XDG_RUNTIME_DIR is set".into()
            }
        }))
    }

    /// Connects, and hands the connection to `attempt`, until it succeeds,
    /// waiting between tries for the socket's directory to change
    /// ([`runtime::wait_for`](crate::runtime::wait_for)): it costs nothing
    /// while there is no compositor. `waiting` hears why the first try
    /// failed, so the wait is logged once.
    pub fn wait<T>(
        &self,
        mut attempt: impl FnMut(Connection) -> Result<T, String>,
        waiting: impl FnOnce(&str),
    ) -> T {
        let dir = match &self.name {
            Some(name) if Path::new(name).is_absolute() => Path::new(name).parent(),
            _ => self.runtime.as_deref(),
        };
        crate::runtime::wait_for(
            dir.as_slice(),
            || self.connect().and_then(&mut attempt),
            waiting,
        )
    }
}

/// `N` of a `wayland-N` socket.
fn display_number(name: &OsStr) -> Option<u32> {
    name.to_str()?.strip_prefix("wayland-")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::Display;

    fn display(name: Option<&str>, runtime: &Path) -> Display {
        Display::new(name.map(Into::into), Some(runtime.to_owned()))
    }

    #[test]
    fn a_named_display_is_its_socket_in_the_runtime_dir() {
        let runtime = tempfile::tempdir().unwrap();
        assert_eq!(
            display(Some("wayland-5"), runtime.path()).sockets(),
            [runtime.path().join("wayland-5")]
        );
    }

    #[test]
    fn a_display_named_by_its_path_is_that_path() {
        let runtime = tempfile::tempdir().unwrap();
        assert_eq!(
            display(Some("/run/elsewhere/wayland-0"), runtime.path()).sockets(),
            [Path::new("/run/elsewhere/wayland-0")]
        );
    }

    #[test]
    fn without_a_name_every_wayland_socket_in_the_runtime_dir_lowest_number_first() {
        let runtime = tempfile::tempdir().unwrap();
        let dir = runtime.path();
        let _ten = UnixListener::bind(dir.join("wayland-10")).unwrap();
        let _two = UnixListener::bind(dir.join("wayland-2")).unwrap();
        std::fs::write(dir.join("wayland-2.lock"), "").unwrap();
        std::fs::write(dir.join("wayland-3"), "not a socket").unwrap();
        let _other = UnixListener::bind(dir.join("pipewire-0")).unwrap();
        // A socket linked in from elsewhere counts too.
        let elsewhere = tempfile::tempdir().unwrap();
        let _linked = UnixListener::bind(elsewhere.path().join("socket")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path().join("socket"), dir.join("wayland-7")).unwrap();

        assert_eq!(
            display(None, dir).sockets(),
            [
                dir.join("wayland-2"),
                dir.join("wayland-7"),
                dir.join("wayland-10")
            ]
        );
    }

    #[test]
    fn an_empty_name_is_no_name() {
        let runtime = tempfile::tempdir().unwrap();
        let _one = UnixListener::bind(runtime.path().join("wayland-1")).unwrap();
        assert_eq!(
            display(Some(""), runtime.path()).sockets(),
            [runtime.path().join("wayland-1")]
        );
    }

    #[test]
    fn connecting_without_a_compositor_says_so() {
        let runtime = tempfile::tempdir().unwrap();
        let err = match display(None, runtime.path()).connect() {
            Ok(_) => panic!("connected to nothing"),
            Err(err) => err,
        };
        assert!(err.contains("no Wayland compositor"), "{err}");
    }

    /// Runs `wait` on a thread, so a test that fails doesn't hang.
    fn waits(display: Display, mut failures: usize) -> mpsc::Receiver<Vec<String>> {
        let (done, waited) = mpsc::channel();
        thread::spawn(move || {
            let mut said = Vec::new();
            display.wait(
                |_| {
                    if failures == 0 {
                        Ok(())
                    } else {
                        failures -= 1;
                        Err("not yet".into())
                    }
                },
                |err| said.push(err.to_owned()),
            );
            let _ = done.send(said);
        });
        waited
    }

    #[test]
    fn waiting_ends_once_the_compositor_makes_its_socket() {
        let runtime = tempfile::tempdir().unwrap();
        let waited = waits(display(None, runtime.path()), 0);
        thread::sleep(Duration::from_millis(300));
        assert!(waited.try_recv().is_err(), "connected to nothing");

        let _compositor = UnixListener::bind(runtime.path().join("wayland-1")).unwrap();
        let said = waited.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(said.len(), 1, "says it waits once: {said:?}");
    }

    #[test]
    fn waiting_for_a_named_display_ignores_other_sockets() {
        let runtime = tempfile::tempdir().unwrap();
        let waited = waits(display(Some("wayland-2"), runtime.path()), 0);
        let _other = UnixListener::bind(runtime.path().join("wayland-1")).unwrap();
        thread::sleep(Duration::from_millis(300));
        assert!(waited.try_recv().is_err(), "connected to the wrong display");

        let _named = UnixListener::bind(runtime.path().join("wayland-2")).unwrap();
        waited.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn a_connection_that_fails_is_tried_again() {
        let runtime = tempfile::tempdir().unwrap();
        let _compositor = UnixListener::bind(runtime.path().join("wayland-0")).unwrap();
        let said = waits(display(None, runtime.path()), 2)
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert_eq!(said, ["not yet"]);
    }
}
