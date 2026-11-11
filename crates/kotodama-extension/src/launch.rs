//! Starting the programs Actions run, and programs in the background,
//! reporting how they ended.
//!
//! With a systemd user manager and no launch prefix, the manager starts each
//! as a service of its own ([`crate::service`]): it is never this process's
//! child, so it inherits nothing of the Extension's or the Daemon's, and gets
//! the session's environment. Otherwise it is a child, started through the
//! launch prefix ([`launch_command`]) if one is set: with no stdio of its
//! starter's, which in an Extension carries the protocol, in the home
//! directory unless the Action names one, and in a process group of its own,
//! so a signal to its starter's group doesn't reach it.
//!
//! # Owns
//!
//! Choosing how a program starts, starting it as a child, and for a program
//! in the background, its last line of output and how it ended.
//!
//! # Not here
//!
//! The service (`crate::service`); telling whether a launch closed soon after
//! (`crate::closing`); the launch prefix's format
//! (`kotodama_protocol::launch_command`).

use std::io;
use std::io::Read;
use std::os::fd::AsFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Child;
use std::process::ExitStatus;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use kotodama_protocol::LAUNCH_AS_SERVICE;
use kotodama_protocol::LAUNCH_PREFIX;
use kotodama_protocol::launch_command;
use kotodama_protocol::launch_prefix;

use crate::Chain;
use crate::dbus::Bus;
use crate::service;
use crate::service::Program;
use crate::service::Running;

/// Small: the threads only wait for the program, or read its output.
const SMALL_STACK: usize = 64 * 1024;
/// How much of a background program's last line of output is kept, in
/// characters.
const LAST_LINE: usize = 200;
/// How long a background program's output may stay open after it exits.
const OUTPUT_GRACE: Duration = Duration::from_millis(100);

/// A program started: a service the user manager runs, or a child of this
/// process.
enum Started {
    Service(Running),
    Child(Child),
}

/// Starts `program` as a service when there is a user manager and no launch
/// prefix, else as a child. Its output, if given, takes stdout and stderr;
/// without it, a service's go to the journal and a child's nowhere. The error
/// says what went wrong in words for the user.
fn start_program(program: &Program<'_>) -> io::Result<Started> {
    let name = program.argv.first().map_or("", String::as_str);
    let in_words = |err: io::Error| io::Error::new(err.kind(), format!("couldn't start {name}: {err}"));
    if let Some(dir) = program.dir
        && !dir.is_dir()
    {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{name} should start in {}, which doesn't exist", dir.display()),
        ));
    }
    if launch_prefix().is_none() {
        let refused = match Bus::user_manager() {
            Ok(manager) => match service::start(manager, program) {
                Ok(running) => return Ok(Started::Service(running)),
                Err(service::Error::Failed(err)) => return Err(in_words(err)),
                Err(err @ service::Error::Refused(_)) => Some(Chain(&err).to_string()),
            },
            Err(err) if err.is_absent() => None,
            Err(err) => Some(Chain(&err).to_string()),
        };
        if let Some(why) = refused {
            crate::log!("{name} starts as a child, not as a service: {why}");
        }
    }
    spawn_child(program).map(Started::Child).map_err(in_words)
}

/// Spawns `program` as a child, after the launch prefix if one is set,
/// detached as the module says. It doesn't inherit the launch prefix: that
/// is for Extensions, and a program the user picked would pass it on to all
/// it starts, such as every shell in a terminal, where a leftover
/// [`LAUNCH_AS_SERVICE`] would hide a prefix the user sets.
fn spawn_child(program: &Program<'_>) -> io::Result<Child> {
    let mut command = launch_command(program.argv)?;
    #[cfg(target_os = "linux")]
    crate::runtime::pass_wayland_display(&mut command);
    if let Some(dir) = program.dir.map(Path::to_owned).or_else(std::env::home_dir) {
        command.current_dir(dir);
    }
    let (stdout, stderr) = match program.output {
        Some(output) => (
            Stdio::from(output.try_clone_to_owned()?),
            Stdio::from(output.try_clone_to_owned()?),
        ),
        None => (Stdio::null(), Stdio::null()),
    };
    command
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .process_group(0)
        .env_remove(LAUNCH_PREFIX)
        .env_remove(LAUNCH_AS_SERVICE)
        .spawn()
}

/// The app a launch starts, as its desktop entry names it.
#[derive(Debug, Clone, Copy)]
pub struct App<'a> {
    /// Its desktop id without `.desktop`, as in `firefox` or
    /// `org.gnome.TextEditor`, which names its service.
    pub id: &'a str,
    /// Its own program, not a terminal it runs in, by which a copy of it
    /// running from before is found.
    pub program: &'a str,
}

/// Starts `argv`, in `dir` if given, and leaves it running. Its service is
/// named for its program. A child is reaped by a thread when it exits.
///
/// # Errors
///
/// When it can't be started; the error says why in words for the user.
pub fn launch(argv: &[String], dir: Option<&Path>) -> io::Result<()> {
    start(argv, dir, None, "", Duration::ZERO, || {})
}

/// Starts `argv` as [`launch`] does, as a service named for `app`, and calls
/// `closed` on a thread of its own if `app` closed within `within`:
/// everything the launch started ended by then, its whole service or process
/// group, so a launcher that starts the app and exits doesn't count. Nor
/// does a launch while a copy of `app`'s program already ran, which a
/// single-instance app hands off to and exits. A zero `within` never calls
/// it.
///
/// # Errors
///
/// When it can't be started; the error says why in words for the user.
pub fn launch_watched(
    argv: &[String],
    dir: Option<&Path>,
    app: App<'_>,
    within: Duration,
    closed: impl FnOnce() + Send + 'static,
) -> io::Result<()> {
    start(argv, dir, Some(app.id), app.program, within, closed)
}

/// [`launch_watched`] for the app `app_id`, if known, whose program is
/// `program`.
fn start(
    argv: &[String],
    dir: Option<&Path>,
    app_id: Option<&str>,
    program: &str,
    within: Duration,
    closed: impl FnOnce() + Send + 'static,
) -> io::Result<()> {
    let started = start_program(&Program {
        argv,
        dir,
        app_id,
        output: None,
    })?;
    // A service is the manager's to reap, and nothing waits for its end.
    if within.is_zero()
        && let Started::Service(_) = started
    {
        return Ok(());
    }
    let deadline = Instant::now() + within;
    let app = program.to_owned();
    // It runs either way; without the thread a child stays a zombie once it
    // exits, which is better than reporting a failure.
    if let Err(err) = thread::Builder::new()
        .name("reap".into())
        .stack_size(SMALL_STACK)
        .spawn(move || {
            // Looked for before the wait, while the process's start time,
            // what came before it, can still be read.
            let ran_before = |pid: Option<u32>| pid.is_some_and(|pid| crate::closing::ran_before(&app, pid));
            match started {
                Started::Service(running) => {
                    if !ran_before(running.main_pid()) && running.ended_by(deadline) {
                        closed();
                    }
                }
                Started::Child(mut child) => {
                    let pid = child.id();
                    let watched = !within.is_zero() && !ran_before(Some(pid));
                    if child.wait().is_ok() && watched && crate::closing::group_ended_by(pid, deadline) {
                        closed();
                    }
                }
            }
        })
    {
        crate::log!("won't reap {}: {err}", argv[0]);
    }
    Ok(())
}

/// How a background program ended.
#[derive(Debug)]
pub struct Ended {
    /// `None` if it couldn't be waited for.
    pub status: Option<ExitStatus>,
    /// The last line it wrote to stdout or stderr that wasn't blank.
    pub last_line: Option<String>,
}

/// Starts `argv`, in `dir` if given, with its output read, and calls `ended`
/// once it exits. Only its last line is kept, so a chatty program costs no
/// memory.
///
/// # Errors
///
/// When the pipe, the program or a thread to wait for it can't be made.
pub fn background(argv: &[String], dir: Option<&Path>, ended: impl FnOnce(Ended) + Send + 'static) -> io::Result<()> {
    let (reader, writer) = io::pipe()?;
    let started = start_program(&Program {
        argv,
        dir,
        app_id: None,
        output: Some(writer.as_fd()),
    })?;
    // The program has its own copy of the pipe's write end, a service's from
    // the manager; only the program's may keep it open.
    drop(writer);
    // The reading thread ends when the last holder of the output closes it,
    // which may be something the program left running, or the manager, which
    // keeps a service's copy until it lets the service go; it takes a small
    // stack and a pipe until then. So the line is kept as it is read, not
    // only once the output closes.
    let latest = Arc::new(Mutex::new(None));
    let (closed, output_closed) = mpsc::channel();
    let reading = Arc::clone(&latest);
    thread::Builder::new()
        .name("job-output".into())
        .stack_size(SMALL_STACK)
        .spawn(move || {
            each_line(reader, |line| *lock(&reading) = Some(line));
            let _ = closed.send(());
        })?;
    thread::Builder::new()
        .name("job".into())
        .stack_size(SMALL_STACK)
        .spawn(move || {
            let status = match started {
                Started::Service(running) => running.main_exit(),
                Started::Child(mut child) => child.wait().ok(),
            };
            // What the program wrote itself has been read already, or soon
            // will be.
            let _ = output_closed.recv_timeout(OUTPUT_GRACE);
            let last_line = lock(&latest).take();
            ended(Ended { status, last_line });
        })?;
    Ok(())
}

/// The value behind `mutex`, even if a thread panicked holding it: a line is
/// whole either way.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Passes each line of `output` that isn't blank to `line`, trimmed and cut
/// to [`LAST_LINE`] characters, until the output closes.
fn each_line(mut output: impl Read, mut line: impl FnMut(String)) {
    let mut bytes = Vec::new();
    let mut finish = |bytes: &mut Vec<u8>| {
        let text = String::from_utf8_lossy(bytes);
        let text = text.trim();
        if !text.is_empty() {
            line(text.chars().take(LAST_LINE).collect::<String>());
        }
        bytes.clear();
    };
    let mut buf = [0; 4096];
    loop {
        let n = match output.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        for &byte in &buf[..n] {
            if byte == b'\n' {
                finish(&mut bytes);
            } else if bytes.len() < LAST_LINE * 4 {
                bytes.push(byte);
            }
        }
    }
    finish(&mut bytes);
}

#[cfg(test)]
mod tests;
