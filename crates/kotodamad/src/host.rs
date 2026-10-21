//! Runs Extension processes. Each gets a thread that reads its stdout and one
//! that writes its stdin, so the Daemon's state machine only ever touches channels and a stuck or
//! dead Extension can't stall it.

use std::collections::HashMap;
use std::process::Child;
use std::process::ChildStdin;
use std::process::ChildStdout;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::thread;

use kotodama_protocol::Channel;
use kotodama_protocol::Error;
use kotodama_protocol::FromExtension;
use kotodama_protocol::Hello;
use kotodama_protocol::ToExtension;
use kotodama_protocol::WireFormat;

use crate::daemon::Ended;
use crate::daemon::Event;
use crate::daemon::ExtensionId;
use crate::daemon::Launcher;
use crate::discovery::Found;
use crate::link::pump;

/// Enough for decoding the deepest message serde_json allows, and small, since
/// every Extension has two of these threads.
pub(crate) const STACK_SIZE: usize = 256 * 1024;

pub(crate) fn spawn(name: String, run: impl FnOnce() + Send + 'static) {
    if let Err(err) = thread::Builder::new().name(name).stack_size(STACK_SIZE).spawn(run) {
        log::error!("couldn't start a thread: {err}");
    }
}

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
        let down = move |ended| Event::ExtensionDown {
            extension,
            generation,
            ended,
        };
        let mut command = Command::new(found.program());
        command
            .args(&found.manifest.command[1..])
            .envs(&found.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if !found.dir.as_os_str().is_empty() {
            command.current_dir(&found.dir);
        }
        // An Extension started once the compositor is there finds it, even
        // when the Daemon started before.
        #[cfg(target_os = "linux")]
        kotodama_extension::runtime::pass_wayland_display(&mut command);
        kotodama_extension::scope::pass_scope_prefix(&mut command);
        die_with_daemon(&mut command);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                let program = found.program();
                let reason = match err.kind() {
                    std::io::ErrorKind::NotFound if program.is_absolute() => {
                        format!("its program {} doesn't exist", program.display())
                    }
                    std::io::ErrorKind::NotFound => {
                        format!("its program `{}` isn't on PATH", program.display())
                    }
                    _ => format!("couldn't start {}: {err}", program.display()),
                };
                let _ = self.events.send(down(Ended::CantStart(reason)));
                return;
            }
        };
        let stdio = (child.stdout.take(), child.stdin.take());
        let child = Arc::new(Mutex::new(child));
        self.running.insert(extension, (generation, Arc::clone(&child)));

        let events = self.events.clone();
        spawn(format!("ext-{name}"), move || {
            let spoke = match stdio {
                (Some(stdout), Some(stdin)) => speak(extension, generation, &name, stdout, stdin, &events),
                _ => Ok(()),
            };
            // Kill before waiting, so a process that closed its stdout but kept
            // running doesn't hold this thread. std won't signal a process it
            // has already reaped, so `stop` can't hit a reused pid.
            let mut child = child.lock().unwrap_or_else(|poison| poison.into_inner());
            let _ = child.kill();
            let status = child.wait();
            drop(child);
            let ended = match spoke {
                // Restarting won't make it speak a protocol it doesn't.
                Err(err @ (Error::PeerTooNew { .. } | Error::PeerTooOld { .. } | Error::JsonNotOffered)) => {
                    Ended::Refused(err.to_string())
                }
                Err(err) => Ended::Stopped(err.to_string()),
                Ok(()) => Ended::Stopped(match status {
                    Ok(status) => status.to_string(),
                    Err(err) => err.to_string(),
                }),
            };
            let _ = events.send(down(ended));
        });
    }

    fn stop(&mut self, extension: ExtensionId, generation: u64) {
        if let Some((running, child)) = self.running.get(&extension)
            && *running == generation
        {
            let _ = child.lock().unwrap_or_else(|poison| poison.into_inner()).kill();
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
    let channel = Channel::accept(stdout, stdin, &Hello::new(&WireFormat::ALL))?;
    pump(
        &format!("ext-{name}"),
        channel,
        |sender| {
            let _ = events.send(Event::ExtensionUp {
                extension,
                generation,
                sender,
            });
        },
        |message: FromExtension| {
            let _ = events.send(Event::FromExtension {
                extension,
                generation,
                message,
            });
        },
        Some(|reason| ToExtension::Rejected { reason }),
    )
}
