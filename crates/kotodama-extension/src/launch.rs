//! Starting the programs Actions run, detached from the Extension, and
//! programs in the background, reporting how they ended.
//!
//! Each runs through the launch prefix ([`launch_command`]), with no stdio of
//! the Extension's, which carries the protocol, and in a process group of its
//! own, so a signal to the Extension's group doesn't reach it.

use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use kotodama_protocol::launch_command;

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
            format!(
                "{program} should start in {}, which doesn't exist",
                dir.display()
            ),
        ));
    }
    let mut command = launch_command(argv)?;
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

fn spawn_error(argv: &[String], err: io::Error) -> io::Error {
    let program = argv.first().map_or("", String::as_str);
    io::Error::new(err.kind(), format!("couldn't start {program}: {err}"))
}

/// Starts `argv`, in `dir` if given, and leaves it running. A thread reaps it
/// when it exits. The error says what went wrong in words for the user.
pub fn launch(argv: &[String], dir: Option<&Path>) -> io::Result<()> {
    let mut child = command(argv, dir)?
        .spawn()
        .map_err(|err| spawn_error(argv, err))?;
    // It runs either way; without the thread it stays a zombie once it exits,
    // which is better than reporting a failure.
    if let Err(err) = thread::Builder::new()
        .name("reap".into())
        .stack_size(SMALL_STACK)
        .spawn(move || {
            let _ = child.wait();
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
pub fn background(
    argv: &[String],
    dir: Option<&Path>,
    ended: impl FnOnce(Ended) + Send + 'static,
) -> io::Result<()> {
    let (reader, writer) = io::pipe()?;
    let mut command = command(argv, dir)?;
    command.stdout(writer.try_clone()?).stderr(writer);
    let mut child = command.spawn().map_err(|err| spawn_error(argv, err))?;
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
        assert_eq!(
            last_line_of(long.as_bytes()).map(|line| line.len()),
            Some(LAST_LINE)
        );
    }
}
