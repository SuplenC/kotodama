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

/// Binds the socket. A socket file left by a Daemon that died is replaced; one
/// that a running Daemon answers on is an error.
pub fn bind(path: &Path) -> io::Result<UnixListener> {
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
    let channel = Channel::handshake(
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
