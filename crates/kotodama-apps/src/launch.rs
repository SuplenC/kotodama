//! Starting an app, detached from the Extension.

use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

/// Small: the thread only waits for the app to exit.
const REAPER_STACK: usize = 64 * 1024;

/// Starts `command`, after `terminal` if the app runs in one, in `dir` if
/// given. The app gets no stdio of the Extension's, which carries the
/// protocol, and a process group of its own, so a signal to the Extension's
/// group doesn't reach it. A thread reaps it when it exits.
pub fn launch(
    command: &[String],
    terminal: Option<&[String]>,
    dir: Option<&Path>,
) -> io::Result<()> {
    let mut argv = terminal.unwrap_or_default().iter().chain(command);
    let program = argv
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "nothing to run"))?;
    let mut process = Command::new(program);
    process
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if let Some(dir) = dir {
        process.current_dir(dir);
    }
    let mut child = process
        .spawn()
        .map_err(|err| io::Error::new(err.kind(), format!("couldn't start {program}: {err}")))?;
    thread::Builder::new()
        .name("reap".into())
        .stack_size(REAPER_STACK)
        .spawn(move || {
            let _ = child.wait();
        })?;
    Ok(())
}
