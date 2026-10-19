//! The Unix socket Front ends connect to.

use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::{self, Sender};

use kotodama_protocol::{Channel, Error, FromFrontEnd, Hello, ToFrontEnd, WireFormat};

use crate::daemon::{Event, FrontEndId};
use crate::host::spawn;

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
    let (mut reader, mut writer) =
        Channel::handshake(stream.try_clone()?, stream, &Hello::new(&WireFormat::ALL))?.split();

    let (sender, outbox) = mpsc::channel::<ToFrontEnd>();
    spawn(format!("front-end-{}-w", id.0), move || {
        for message in outbox {
            if writer.send(&message).is_err() {
                break;
            }
        }
    });
    let _ = events.send(Event::FrontEndUp {
        front_end: id,
        sender,
    });

    loop {
        match reader.recv::<FromFrontEnd>() {
            Ok(Some(message)) => {
                let _ = events.send(Event::FromFrontEnd {
                    front_end: id,
                    message,
                });
            }
            Ok(None) => return Ok(()),
            Err(err @ Error::Decode { .. }) => log::warn!("front end {}: {err}", id.0),
            Err(err) => return Err(err),
        }
    }
}
