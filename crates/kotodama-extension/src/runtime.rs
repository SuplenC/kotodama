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

use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
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

/// Runs `attempt` until it succeeds. Between tries it waits until something
/// is made, moved in or changed in one of `watched` (directories that don't
/// exist are left out), or failing that, a while that doubles each time.
/// `waiting` hears why the first try failed, so the wait is logged once.
pub fn wait_for<T>(
    watched: &[&Path],
    mut attempt: impl FnMut() -> Result<T, String>,
    waiting: impl FnOnce(&str),
) -> T {
    let mut waiting = Some(waiting);
    let mut backoff = MIN_BACKOFF;
    loop {
        // Watching before trying, so a change between the two still ends the
        // wait.
        let changes = Changes::watch(watched);
        match attempt() {
            Ok(done) => return done,
            Err(err) => {
                if let Some(waiting) = waiting.take() {
                    waiting(&err);
                }
            }
        }
        match changes {
            Some(changes) => changes.wait(backoff),
            None => std::thread::sleep(backoff),
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
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

    /// Waits until something changes, or `timeout`.
    fn wait(&self, timeout: Duration) {
        let mut poll = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
        // SAFETY: one valid pollfd, for the length given.
        unsafe { libc::poll(&mut poll, 1, millis) };
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::wait_for;

    #[test]
    fn waiting_ends_once_something_is_made_where_it_watches() {
        let runtime = tempfile::tempdir().unwrap();
        let socket = runtime.path().join("compositor");
        let (done, waited) = mpsc::channel();
        let dir = runtime.path().to_owned();
        let looked_for = socket.clone();
        thread::spawn(move || {
            let mut said = Vec::new();
            wait_for(
                &[&dir],
                || {
                    if looked_for.exists() {
                        Ok(())
                    } else {
                        Err("not there".into())
                    }
                },
                |err| said.push(err.to_owned()),
            );
            let _ = done.send(said);
        });
        // Long enough that the wait between tries has grown past what the
        // test allows below, so only the watch can end it in time.
        thread::sleep(Duration::from_millis(1600));
        assert!(waited.try_recv().is_err(), "found nothing");

        let made = Instant::now();
        std::fs::write(&socket, "").unwrap();
        let said = waited.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            made.elapsed() < Duration::from_millis(500),
            "{:?}",
            made.elapsed()
        );
        assert_eq!(said, ["not there"], "says it waits once");
    }

    #[test]
    fn a_missing_directory_is_tried_again_after_a_while() {
        let runtime = tempfile::tempdir().unwrap();
        let missing = runtime.path().join("hypr");
        let mut tries = 0;
        wait_for(
            &[&missing],
            || {
                tries += 1;
                if tries < 3 { Err("no".into()) } else { Ok(()) }
            },
            |_| {},
        );
        assert_eq!(tries, 3);
    }
}
