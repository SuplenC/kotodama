//! Runs Extension processes. Each gets a thread that reads its stdout and one
//! that writes its stdin, so the core only ever touches channels and a stuck or
//! dead Extension can't stall it.

use std::io;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread;

use kotodama_protocol::{Channel, Error, FromExtension, Hello, ToExtension, WireFormat};

use crate::daemon::{Event, ExtensionId, Launcher};
use crate::discovery::Found;

/// Enough for decoding the deepest message serde_json allows, and small, since
/// every Extension has two of these threads.
pub(crate) const STACK_SIZE: usize = 256 * 1024;

pub(crate) fn spawn(name: String, run: impl FnOnce() + Send + 'static) {
    if let Err(err) = thread::Builder::new()
        .name(name)
        .stack_size(STACK_SIZE)
        .spawn(run)
    {
        log::error!("couldn't start a thread: {err}");
    }
}

/// Starts Extensions as child processes speaking the protocol on stdio.
pub struct Processes {
    events: Sender<Event>,
}

impl Processes {
    pub fn new(events: Sender<Event>) -> Self {
        Processes { events }
    }
}

impl Launcher for Processes {
    fn start(&mut self, extension: ExtensionId, generation: u64, found: &Found) {
        let events = self.events.clone();
        let found = found.clone();
        spawn(format!("ext-{}", found.manifest.name), move || {
            run(extension, generation, &found, &events);
            let _ = events.send(Event::ExtensionDown {
                extension,
                generation,
            });
        });
    }
}

/// Runs one Extension until its process ends or stops speaking the protocol.
fn run(extension: ExtensionId, generation: u64, found: &Found, events: &Sender<Event>) {
    let name = &found.manifest.name;
    let mut child = match Command::new(found.program())
        .args(&found.manifest.command[1..])
        .current_dir(&found.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            log::error!(
                "{name}: couldn't start {}: {err}",
                found.program().display()
            );
            return;
        }
    };
    if let Err(err) = speak(extension, generation, name, &mut child, events) {
        log::warn!("{name}: {err}");
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn speak(
    extension: ExtensionId,
    generation: u64,
    name: &str,
    child: &mut Child,
    events: &Sender<Event>,
) -> Result<(), Error> {
    let (Some(stdout), Some(stdin)) = (child.stdout.take(), child.stdin.take()) else {
        return Err(Error::Io(io::ErrorKind::BrokenPipe.into()));
    };
    let (mut reader, mut writer) =
        Channel::handshake(stdout, stdin, &Hello::new(&WireFormat::ALL))?.split();

    let (sender, outbox) = mpsc::channel::<ToExtension>();
    spawn(format!("ext-{name}-w"), move || {
        // Ends when the Daemon drops the sender or the Extension's stdin closes.
        for message in outbox {
            if writer.send(&message).is_err() {
                break;
            }
        }
    });
    let _ = events.send(Event::ExtensionUp {
        extension,
        generation,
        sender,
    });

    loop {
        match reader.recv::<FromExtension>() {
            Ok(Some(message)) => {
                let _ = events.send(Event::FromExtension {
                    extension,
                    generation,
                    message,
                });
            }
            Ok(None) => return Ok(()),
            // The frame was read whole, so skip just this message.
            Err(err @ Error::Decode { .. }) => log::warn!(
                "{name}: rejected a message: {err}: {}",
                std::error::Error::source(&err).map_or(String::new(), |s| s.to_string())
            ),
            Err(err) => return Err(err),
        }
    }
}
