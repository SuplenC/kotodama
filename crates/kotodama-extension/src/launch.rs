//! Starting the programs Actions run, detached from the Extension or the
//! Daemon that starts them, and programs in the background, reporting how
//! they ended.
//!
//! Each runs through the launch prefix ([`launch_command`]) if one is set,
//! else in a systemd scope of its own when there is a user manager to make
//! one ([`crate::scope`]); with no stdio of its starter's, which in an
//! Extension carries the protocol; and in a process group of its own, so a
//! signal to its starter's group doesn't reach it.

use std::io;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use kotodama_protocol::launch_command;

use crate::Chain;
use crate::scope::Launched;

/// Small: the threads only wait for the program, or read its output.
const SMALL_STACK: usize = 64 * 1024;
/// How much of a background program's last line of output is kept, in
/// characters.
const LAST_LINE: usize = 200;
/// How long a background program's output may stay open after it exits.
const OUTPUT_GRACE: Duration = Duration::from_millis(100);

/// A command for `argv`, in `dir` if given, detached as the module says.
fn command(argv: &[String], dir: Option<&Path>) -> io::Result<Command> {
    let program = argv.first().map_or("", String::as_str);
    if let Some(dir) = dir
        && !dir.is_dir()
    {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{program} should start in {}, which doesn't exist", dir.display()),
        ));
    }
    let mut command = launch_command(argv)?;
    #[cfg(target_os = "linux")]
    crate::runtime::pass_wayland_display(&mut command);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    Ok(command)
}

/// Spawns `command`, which runs `argv`, in a scope of its own if it gets one,
/// named for `app_id` if given ([`crate::scope::spawn`]). The error says what
/// went wrong in words for the user.
fn spawn(command: &mut Command, argv: &[String], app_id: Option<&str>) -> io::Result<Launched> {
    let program = argv.first().map_or("", String::as_str);
    let named = program.to_owned();
    crate::scope::spawn(command, app_id, move |err| {
        crate::log!("{named} runs without a scope of its own: {}", Chain(&err));
    })
    .map_err(|err| io::Error::new(err.kind(), format!("couldn't start {program}: {err}")))
}

/// The app a launch starts, as its desktop entry names it.
#[derive(Debug, Clone, Copy)]
pub struct App<'a> {
    /// Its desktop id without `.desktop`, as in `firefox` or
    /// `org.gnome.TextEditor`, which names its scope.
    pub id: &'a str,
    /// Its own program, not a terminal it runs in, by which a copy of it
    /// running from before is found.
    pub program: &'a str,
}

/// Starts `argv`, in `dir` if given, and leaves it running. A thread reaps it
/// when it exits. Its scope is named for its program.
///
/// # Errors
///
/// When it can't be started; the error says why in words for the user.
pub fn launch(argv: &[String], dir: Option<&Path>) -> io::Result<()> {
    start(argv, dir, None, "", Duration::ZERO, || {})
}

/// Starts `argv` as [`launch`] does, in a scope named for `app`, and calls
/// `closed` on the thread that reaps it if `app` closed within `within`:
/// everything the launch started ended by then, its whole scope or process
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
    let Launched { mut child, cgroup } = spawn(&mut command(argv, dir)?, argv, app_id)?;
    let deadline = Instant::now() + within;
    let app = program.to_owned();
    // It runs either way; without the thread it stays a zombie once it exits,
    // which is better than reporting a failure.
    if let Err(err) = thread::Builder::new()
        .name("reap".into())
        .stack_size(SMALL_STACK)
        .spawn(move || {
            let pid = child.id();
            // Looked for before the wait, while the process's start time,
            // what came before it, can still be read.
            let watched = !within.is_zero() && !crate::closing::ran_before(&app, pid);
            if child.wait().is_ok() && watched && crate::closing::ended_by(cgroup.as_deref(), pid, deadline) {
                closed();
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
pub fn background(argv: &[String], dir: Option<&Path>, ended: impl FnOnce(Ended) + Send + 'static) -> io::Result<()> {
    let (reader, writer) = io::pipe()?;
    let mut command = command(argv, dir)?;
    command.stdout(writer.try_clone()?).stderr(writer);
    let mut child = spawn(&mut command, argv, None)?.child;
    // The command holds the pipe's write ends too; only the child's may
    // keep it open.
    drop(command);
    // The reading thread ends when the last holder of the output closes it,
    // which may be something the program left running; it takes a small stack
    // and a pipe until then.
    let (read, last_line) = mpsc::channel();
    thread::Builder::new()
        .name("job-output".into())
        .stack_size(SMALL_STACK)
        .spawn(move || {
            let _ = read.send(last_line_of(reader));
        })?;
    thread::Builder::new()
        .name("job".into())
        .stack_size(SMALL_STACK)
        .spawn(move || {
            let status = child.wait().ok();
            // Something the program started may keep its output open; what it
            // wrote itself is read by now, or soon.
            let last_line = last_line.recv_timeout(OUTPUT_GRACE).ok().flatten();
            ended(Ended { status, last_line });
        })?;
    Ok(())
}

/// The last line of `output` that isn't blank, cut to [`LAST_LINE`]
/// characters.
fn last_line_of(mut output: impl Read) -> Option<String> {
    let mut last = None;
    let mut line = Vec::new();
    let mut finish = |line: &mut Vec<u8>| {
        let text = String::from_utf8_lossy(line);
        let text = text.trim();
        if !text.is_empty() {
            last = Some(text.chars().take(LAST_LINE).collect::<String>());
        }
        line.clear();
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
                finish(&mut line);
            } else if line.len() < LAST_LINE * 4 {
                line.push(byte);
            }
        }
    }
    finish(&mut line);
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_line_that_isnt_blank_is_kept_and_cut() {
        assert_eq!(last_line_of(&b"one\ntwo\n\n  \n"[..]), Some("two".into()));
        assert_eq!(last_line_of(&b"no end"[..]), Some("no end".into()));
        assert_eq!(last_line_of(&b""[..]), None);
        let long = "x".repeat(10_000);
        assert_eq!(last_line_of(long.as_bytes()).map(|line| line.len()), Some(LAST_LINE));
    }
}
