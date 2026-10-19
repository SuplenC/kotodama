//! Runs Extension processes. Each gets a thread that reads its stdout and one
//! that writes its stdin, so the Daemon's state machine only ever touches channels and a stuck or
//! dead Extension can't stall it.

use std::collections::HashMap;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
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

/// How many messages may wait for an Extension to read them. One that leaves
/// this many unread has stopped reading, and the Daemon stops it.
const OUTBOX: usize = 1024;

/// Starts Extensions as child processes speaking the protocol on stdio.
pub struct Processes {
    events: Sender<Event>,
    /// The latest run of each Extension, so it can be stopped.
    running: HashMap<ExtensionId, (u64, Arc<Mutex<Child>>)>,
}

impl Processes {
    pub fn new(events: Sender<Event>) -> Self {
        Processes {
            events,
            running: HashMap::new(),
        }
    }
}

impl Launcher for Processes {
    fn start(&mut self, extension: ExtensionId, generation: u64, found: &Found) {
        let name = found.manifest.name.clone();
        let down = Event::ExtensionDown {
            extension,
            generation,
        };
        let mut command = Command::new(found.program());
        command
            .args(&found.manifest.command[1..])
            .current_dir(&found.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        die_with_daemon(&mut command);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                log::error!(
                    "{name}: couldn't start {}: {err}",
                    found.program().display()
                );
                let _ = self.events.send(down);
                return;
            }
        };
        let stdio = (child.stdout.take(), child.stdin.take());
        let child = Arc::new(Mutex::new(child));
        self.running
            .insert(extension, (generation, Arc::clone(&child)));

        let events = self.events.clone();
        spawn(format!("ext-{name}"), move || {
            if let (Some(stdout), Some(stdin)) = stdio
                && let Err(err) = speak(extension, generation, &name, stdout, stdin, &events)
            {
                log::warn!("{name}: {err}");
            }
            // Kill before waiting, so a process that closed its stdout but kept
            // running doesn't hold this thread. std won't signal a process it
            // has already reaped, so `stop` can't hit a reused pid.
            let mut child = child.lock().unwrap_or_else(|poison| poison.into_inner());
            let _ = child.kill();
            let _ = child.wait();
            drop(child);
            let _ = events.send(down);
        });
    }

    fn stop(&mut self, extension: ExtensionId, generation: u64) {
        if let Some((running, child)) = self.running.get(&extension)
            && *running == generation
        {
            let _ = child
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .kill();
        }
    }
}

/// Has the kernel kill the Extension when the thread that started it dies. That
/// is the main loop's thread, which lives as long as the Daemon, so an
/// Extension that never reads its stdin still can't outlive the Daemon.
#[cfg(target_os = "linux")]
fn die_with_daemon(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: prctl is async-signal-safe, as code between fork and exec must be.
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn die_with_daemon(_command: &mut Command) {}

/// Speaks the protocol with one Extension until it hangs up.
fn speak(
    extension: ExtensionId,
    generation: u64,
    name: &str,
    stdout: ChildStdout,
    stdin: ChildStdin,
    events: &Sender<Event>,
) -> Result<(), Error> {
    let (mut reader, mut writer) =
        Channel::handshake(stdout, stdin, &Hello::new(&WireFormat::ALL))?.split();

    let (sender, outbox) = mpsc::sync_channel::<ToExtension>(OUTBOX);
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
