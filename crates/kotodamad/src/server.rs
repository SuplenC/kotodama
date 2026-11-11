//! The Unix socket Front ends connect to.
//!
//! # Owns
//!
//! - [`take_activation`]: reading socket activation, and keeping its variables from the processes
//!   the Daemon starts.
//! - [`listen`] and [`serve`]: the socket, and a Front end's handshake.
//!
//! # Not here
//!
//! - The connection after the handshake: `link.rs`.
//! - What a Front end sends: `daemon/front_ends.rs`.

use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::mpsc::Sender;

use kotodama_protocol::Channel;
use kotodama_protocol::Error;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::Hello;
use kotodama_protocol::WireFormat;

use crate::daemon::Event;
use crate::daemon::FrontEndId;
use crate::daemon::HANDSHAKE_TIMEOUT;
use crate::host::spawn;
use crate::link::pump;

/// The socket to serve: the one systemd hands over under socket activation,
/// or else `path`, bound here.
pub fn listen(path: &Path) -> io::Result<UnixListener> {
    match activated() {
        Some(listener) => {
            log::info!("serving the socket systemd passed in");
            Ok(listener)
        }
        None => bind(path),
    }
}

/// The variables socket activation sets (`sd_listen_fds(3)`).
const ACTIVATION_VARS: [&str; 4] = ["LISTEN_PID", "LISTEN_FDS", "LISTEN_FDNAMES", "LISTEN_PIDFDID"];

/// Whether systemd handed this process a socket, as [`take_activation`] read
/// it. Unset when it never ran, and then the Daemon binds its own socket.
static HANDED_A_SOCKET: OnceLock<bool> = OnceLock::new();

/// Reads whether systemd handed this process a socket, then takes the
/// activation variables out of the environment, as
/// `sd_listen_fds(unset_environment=1)` does. Left in, every Extension and
/// every program the Daemon starts would inherit them, and one that does
/// socket activation itself would take its own descriptor 3 for a socket.
/// Removing them here, rather than at each spawn, also covers a spawn added
/// later.
///
/// # Safety
///
/// No other thread may be running: one could be reading the environment.
pub unsafe fn take_activation() {
    let number = |name| std::env::var(name).ok()?.parse::<u32>().ok();
    let handed = number("LISTEN_PID") == Some(std::process::id()) && number("LISTEN_FDS").is_some_and(|fds| fds > 0);
    let _ = HANDED_A_SOCKET.set(handed);
    for name in ACTIVATION_VARS {
        // SAFETY: the caller guarantees no other thread is running.
        unsafe { std::env::remove_var(name) };
    }
}

/// The listening socket systemd passed in as file descriptor 3, if
/// [`take_activation`] found `LISTEN_PID` naming this process and `LISTEN_FDS`
/// counting at least one. systemd owns its socket file and keeps it while the
/// Daemon is stopped, so connections made meanwhile wait for the next Daemon.
#[cfg(target_os = "linux")]
fn activated() -> Option<UnixListener> {
    use std::os::fd::BorrowedFd;
    use std::os::fd::FromRawFd;
    use std::os::fd::RawFd;

    const SD_LISTEN_FDS_START: RawFd = 3;
    if HANDED_A_SOCKET.get() != Some(&true) {
        return None;
    }
    // Not passed on to Extensions or the programs the Daemon starts. This also
    // checks the descriptor is open before anything takes ownership of it.
    // SAFETY: only borrowed for the call; fcntl fails on a closed descriptor.
    let fd = unsafe { BorrowedFd::borrow_raw(SD_LISTEN_FDS_START) };
    if let Err(err) = rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::CLOEXEC) {
        log::warn!("LISTEN_FDS is set, but descriptor 3 isn't open: {err}");
        return None;
    }
    // SAFETY: open, as checked above, and sd_listen_fds(3) hands it to this
    // process alone.
    let listener = unsafe { UnixListener::from_raw_fd(SD_LISTEN_FDS_START) };
    // Anything but a Unix socket has no Unix address.
    if let Err(err) = listener.local_addr() {
        log::warn!("LISTEN_FDS is set, but descriptor 3 isn't a Unix socket: {err}");
        return None;
    }
    Some(listener)
}

/// systemd, and so socket activation, is Linux only.
#[cfg(not(target_os = "linux"))]
fn activated() -> Option<UnixListener> {
    None
}

/// Binds the socket. A socket file left by a Daemon that died is replaced; one
/// that a running Daemon answers on is an error.
fn bind(path: &Path) -> io::Result<UnixListener> {
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("another Daemon is serving {}", path.display()),
            ));
        }
        std::fs::remove_file(path)?;
    }
    UnixListener::bind(path)
}

/// Accepts Front ends on a thread of its own, each served by a reading and a
/// writing thread.
pub fn serve(listener: UnixListener, events: Sender<Event>) {
    spawn("accept".into(), move || {
        for (n, stream) in listener.incoming().enumerate() {
            match stream {
                Ok(stream) => {
                    let events = events.clone();
                    let id = FrontEndId(n as u64);
                    spawn(format!("front-end-{n}"), move || {
                        if let Err(err) = connection(id, stream, &events) {
                            log::warn!("front end {n}: {err}");
                        }
                        let _ = events.send(Event::FrontEndDown { front_end: id });
                    });
                }
                Err(err) => log::warn!("couldn't accept a Front end: {err}"),
            }
        }
    });
}

fn connection(id: FrontEndId, stream: UnixStream, events: &Sender<Event>) -> Result<(), Error> {
    // A Front end that connects and never says hello mustn't hold a thread.
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT)).map_err(Error::Io)?;
    let channel = Channel::accept(
        stream.try_clone().map_err(Error::Io)?,
        stream.try_clone().map_err(Error::Io)?,
        &Hello::new(&WireFormat::ALL),
    )?;
    stream.set_read_timeout(None).map_err(Error::Io)?;
    pump(
        &format!("front-end-{}", id.0),
        channel,
        |sender| {
            let _ = events.send(Event::FrontEndUp {
                front_end: id,
                sender,
                // Unblocks both threads, so a Front end that stopped reading
                // can be dropped.
                disconnect: Box::new(move || {
                    let _ = stream.shutdown(Shutdown::Both);
                }),
            });
        },
        |message: FromFrontEnd| {
            let _ = events.send(Event::FromFrontEnd { front_end: id, message });
        },
        None,
    )
}
