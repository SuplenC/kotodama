//! Starting a program the user picked, such as an app or a Quicklink's
//! command, so that it outlives the Daemon.
//!
//! # Owns
//!
//! - The launch prefix ([`LAUNCH_PREFIX`], [`launch_prefix`]) and the [`launch_command`] built with
//!   it.
//!
//! # Not here
//!
//! - Running the command, as a service of the user manager or detached: `kotodama-extension`'s
//!   `launch.rs` and `service.rs`.

use std::env;
use std::ffi::OsStr;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

/// The environment variable holding the launch prefix: words, split on
/// whitespace, that every program the user picks runs after. Extensions get
/// it from the Daemon's environment.
///
/// A program the user picks belongs to the session, not to the Daemon: it
/// should inherit nothing of the Daemon's service, and outlive it. When the
/// prefix isn't set, the Daemon and the Rust Extensions therefore ask the
/// user manager to start each program as a service of its own
/// (`kotodama_extension::service`). Setting it, even empty, replaces that
/// service: for another way to start apps, such as `uwsm app --`, or for
/// none.
pub const LAUNCH_PREFIX: &str = "KOTODAMA_LAUNCH_PREFIX";

/// Set, beside [`LAUNCH_PREFIX`], when the prefix is only there to start
/// each program as a service: the Daemon then hands Extensions
/// [`SERVICE_PREFIX`], so one that starts programs through the prefix starts
/// services too. A launcher that asks the user manager itself ignores such a
/// prefix ([`launch_prefix`]).
pub const LAUNCH_AS_SERVICE: &str = "KOTODAMA_LAUNCH_AS_SERVICE";

/// The launch prefix that has the user manager start a program as a service
/// of its own, in the manager's environment and not its caller's, for
/// Extensions that don't ask the manager themselves. `--same-dir` keeps the
/// directory an Action names; `ExitType=cgroup` keeps the service while
/// anything in it runs, so a launcher that exits leaves its app running;
/// `--expand-environment=no` passes a `$` in the arguments on as it is.
pub const SERVICE_PREFIX: &str = "systemd-run --user --slice=app.slice --collect --quiet --same-dir --property=Type=exec --property=ExitType=cgroup --expand-environment=no";

/// The launch prefix in effect: [`LAUNCH_PREFIX`], unless it is unset or
/// only stands in for a service ([`LAUNCH_AS_SERVICE`]).
#[must_use]
pub fn launch_prefix() -> Option<String> {
    if env::var_os(LAUNCH_AS_SERVICE).is_some() {
        return None;
    }
    env::var(LAUNCH_PREFIX).ok()
}

/// A command that runs `argv` after the launch prefix ([`launch_prefix`]), if
/// one is in effect.
///
/// # Errors
///
/// [`io::ErrorKind::InvalidInput`] if `argv` is empty. With a prefix,
/// [`io::ErrorKind::NotFound`] if its program isn't found, as starting it
/// without a prefix would: otherwise the prefix itself would start, and the
/// error would be lost.
pub fn launch_command(argv: &[String]) -> io::Result<Command> {
    let (program, rest) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "nothing to run"))?;
    let prefix = launch_prefix().unwrap_or_default();
    let mut words = prefix.split_whitespace();
    let Some(first) = words.next() else {
        let mut command = Command::new(program);
        command.args(rest);
        return Ok(command);
    };
    if !is_found(program.as_ref()) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{program} isn't installed"),
        ));
    }
    let mut command = Command::new(first);
    command.args(words).args(argv);
    Ok(command)
}

/// Whether `program` names an executable file, directly or through `PATH`,
/// as `execvp` would find it. A relative path such as `./run` resolves against
/// the directory the caller gives the command, not this process's, so it
/// counts as found.
fn is_found(program: &OsStr) -> bool {
    let is_executable = |path: &Path| {
        path.metadata()
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    };
    let path = Path::new(program);
    if path.is_absolute() {
        return is_executable(path);
    }
    if path.components().count() > 1 {
        return true;
    }
    env::var_os("PATH").is_some_and(|dirs| env::split_paths(&dirs).any(|dir| is_executable(&dir.join(program))))
}
