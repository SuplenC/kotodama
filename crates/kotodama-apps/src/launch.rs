//! Starting an app, detached from the Extension.

use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Stdio;
use std::thread;

use kotodama_protocol::launch_command;

/// Small: the thread only waits for the app to exit.
const REAPER_STACK: usize = 64 * 1024;

/// Starts `command`, after `terminal` if the app runs in one, in `dir` if
/// given, through the launch prefix ([`launch_command`]). The app gets no
/// stdio of the Extension's, which carries the protocol, and a process group
/// of its own, so a signal to the Extension's group doesn't reach it. A thread
/// reaps it when it exits.
pub fn launch(
    command: &[String],
    terminal: Option<&[String]>,
    dir: Option<&Path>,
) -> io::Result<()> {
    let argv: Vec<String> = terminal
        .unwrap_or_default()
        .iter()
        .chain(command)
        .cloned()
        .collect();
    let mut process = launch_command(&argv)?;
    let program = &argv[0];
    process
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if let Some(dir) = dir {
        if !dir.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{program} should start in {}, which doesn't exist",
                    dir.display()
                ),
            ));
        }
        process.current_dir(dir);
    }
    let mut child = process
        .spawn()
        .map_err(|err| io::Error::new(err.kind(), format!("couldn't start {program}: {err}")))?;
    // The app runs either way; without the thread it stays a zombie once it
    // exits, which is better than reporting a failure.
    if let Err(err) = thread::Builder::new()
        .name("reap".into())
        .stack_size(REAPER_STACK)
        .spawn(move || {
            let _ = child.wait();
        })
    {
        eprintln!("kotodama-apps: won't reap {program}: {err}");
    }
    Ok(())
}
