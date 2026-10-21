//! The Unix socket Front ends connect to.

use std::io;
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::Sender;

use kotodama_protocol::{Channel, Error, FromFrontEnd, Hello, WireFormat};

use crate::daemon::{Event, FrontEndId, HANDSHAKE_TIMEOUT};
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

/// The listening socket systemd passed in as file descriptor 3, if `LISTEN_PID`
/// names this process and `LISTEN_FDS` counts at least one (sd_listen_fds(3)).
/// systemd owns its socket file and keeps it while the Daemon is stopped, so
/// connections made meanwhile wait for the next Daemon.
#[cfg(target_os = "linux")]
fn activated() -> Option<UnixListener> {
    use std::env;
    use std::os::fd::{BorrowedFd, FromRawFd, RawFd};

    const SD_LISTEN_FDS_START: RawFd = 3;
    let pid: u32 = env::var("LISTEN_PID").ok()?.parse().ok()?;
    let fds: u32 = env::var("LISTEN_FDS").ok()?.parse().ok()?;
    if pid != std::process::id() || fds == 0 {
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
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let channel = Channel::accept(
        stream.try_clone()?,
        stream.try_clone()?,
        &Hello::new(&WireFormat::ALL),
    )?;
    stream.set_read_timeout(None)?;
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
            let _ = events.send(Event::FromFrontEnd {
                front_end: id,
                message,
            });
        },
    )
}
