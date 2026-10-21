//! Waiting for something to turn up in `$XDG_RUNTIME_DIR`, such as a
//! compositor's socket (Linux only).
//!
//! With socket activation, the Daemon and its Extensions can start before
//! the compositor has put its variables (`WAYLAND_DISPLAY`,
//! `HYPRLAND_INSTANCE_SIGNATURE`) in the service manager's environment, and a
//! process never sees its environment change. So whatever needs the
//! compositor finds its sockets in the runtime directory instead, where every
//! compositor makes them, and waits there with [`wait_for`] until they are
//! made: an inotify watch wakes it the moment they are, so waiting costs
//! nothing.
//!
//! # Owns
//!
//! The runtime directory, the compositors' sockets in it, and waiting for
//! something there with an inotify watch and a backoff.
//!
//! # Not here
//!
//! Connecting to what turned up: Wayland (`crate::wayland`), Hyprland (its
//! Extension).

use std::ffi::CString;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// The first wait between tries when nothing in the watched directories
/// changes; it doubles up to [`MAX_BACKOFF`]. Mostly a change ends the wait
/// at once.
const MIN_BACKOFF: Duration = Duration::from_millis(100);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// `$XDG_RUNTIME_DIR`.
pub fn dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

/// The `wayland-N` sockets in `dir`, lowest `N` first. A socket linked in
/// counts.
pub fn wayland_sockets(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = dir.read_dir() else {
        return Vec::new();
    };
    let mut found: Vec<(u32, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let number = entry.file_name().to_str()?.strip_prefix("wayland-")?.parse().ok()?;
            let path = entry.path();
            // Follows a link.
            path.metadata()
                .ok()
                .filter(|meta| meta.file_type().is_socket())
                .map(|_| (number, path))
        })
        .collect();
    found.sort_unstable();
    found.into_iter().map(|(_, path)| path).collect()
}

/// Gives a program about to start the `WAYLAND_DISPLAY` this process lacks,
/// when it started before the compositor's environment reached it, so
/// programs launched from the menu still find the compositor.
pub fn pass_wayland_display(command: &mut Command) {
    let runtime = dir();
    if let Some(display) = missing_wayland_display(std::env::var_os("WAYLAND_DISPLAY").as_deref(), runtime.as_deref()) {
        command.env("WAYLAND_DISPLAY", display);
    }
}

/// The display to pass on: none if this process has one, else the first
/// `wayland-N` socket in the runtime directory.
fn missing_wayland_display(own: Option<&OsStr>, runtime: Option<&Path>) -> Option<OsString> {
    if own.is_some_and(|own| !own.is_empty()) {
        return None;
    }
    let first = wayland_sockets(runtime?).into_iter().next()?;
    first.file_name().map(OsStr::to_owned)
}

/// Runs `attempt` until it succeeds. Between tries it waits until something
/// is made, moved in or changed in one of `watched` (directories that don't
/// exist are left out), or failing that, a while that doubles each time.
/// `waiting` hears why the first try failed, so the wait is logged once.
pub fn wait_for<T>(watched: &[&Path], mut attempt: impl FnMut() -> Result<T, String>, waiting: impl FnOnce(&str)) -> T {
    let mut waiting = Some(waiting);
    let mut backoff = MIN_BACKOFF;
    loop {
        // Watching before trying, so a change between the two still ends the
        // wait.
        let watch = Changes::watch(watched);
        match attempt() {
            Ok(done) => return done,
            Err(err) => {
                if let Some(waiting) = waiting.take() {
                    waiting(&err);
                }
            }
        }
        let changed = if let Some(watch) = watch {
            watch.wait(backoff)
        } else {
            std::thread::sleep(backoff);
            false
        };
        // After a change, what turned up may need a moment more, such as a
        // socket made just before its compositor listens on it.
        backoff = if changed {
            MIN_BACKOFF
        } else {
            (backoff * 2).min(MAX_BACKOFF)
        };
    }
}

/// An inotify watch on some directories.
struct Changes(OwnedFd);

impl Changes {
    /// `None` when none of `dirs` can be watched.
    fn watch(dirs: &[&Path]) -> Option<Changes> {
        // SAFETY: inotify_init1 takes no pointers; a valid fd is ours to own.
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if fd < 0 {
            return None;
        }
        // SAFETY: fd was just opened and nothing else owns it.
        let changes = Changes(unsafe { OwnedFd::from_raw_fd(fd) });
        let mask = libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_ATTRIB;
        let mut any = false;
        for dir in dirs {
            let Ok(path) = CString::new(dir.as_os_str().as_bytes()) else {
                continue;
            };
            // SAFETY: path is a NUL-terminated string that outlives the call.
            any |= unsafe { libc::inotify_add_watch(fd, path.as_ptr(), mask) } >= 0;
        }
        any.then_some(changes)
    }

    /// Waits until something changes, or `timeout`. True if something did.
    fn wait(&self, timeout: Duration) -> bool {
        let mut poll = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
        // SAFETY: one valid pollfd, for the length given.
        unsafe { libc::poll(&raw mut poll, 1, millis) > 0 }
    }
}

#[cfg(test)]
mod tests;
