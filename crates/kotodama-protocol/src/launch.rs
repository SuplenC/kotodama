//! Starting a program the user picked, such as an app or a Quicklink's
//! command, so that it outlives the Daemon.

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
/// Under systemd every process the Daemon starts sits in its service's
/// cgroup, so stopping the service would kill the user's apps with it.
/// When the prefix isn't set, the Daemon and the Rust Extensions therefore
/// ask the user manager for a scope for each program
/// (`kotodama_extension::scope`). Setting it, even empty, replaces that
/// scope: for another way to start apps, such as `uwsm app --`, or for none.
pub const LAUNCH_PREFIX: &str = "KOTODAMA_LAUNCH_PREFIX";

/// Set, beside [`LAUNCH_PREFIX`], when the prefix is only there to give
/// each program a scope: the Daemon then hands Extensions [`SCOPE_PREFIX`],
/// so one that starts programs through the prefix gets the scope too. A
/// launcher that asks for the scope itself ignores such a prefix
/// ([`launch_prefix`]).
pub const LAUNCH_SCOPED: &str = "KOTODAMA_LAUNCH_SCOPED";

/// The launch prefix that gives a program a scope of its own, for
/// Extensions that don't ask the user manager for one themselves.
pub const SCOPE_PREFIX: &str = "systemd-run --user --scope --slice=app.slice --collect --quiet";

/// The launch prefix in effect: [`LAUNCH_PREFIX`], unless it is unset or
/// only stands in for a scope ([`LAUNCH_SCOPED`]).
#[must_use]
pub fn launch_prefix() -> Option<String> {
    if env::var_os(LAUNCH_SCOPED).is_some() {
        return None;
    }
    env::var(LAUNCH_PREFIX).ok()
}

/// A command that runs `argv` after the launch prefix ([`launch_prefix`]), if
/// one is in effect. Fails if
/// `argv` is empty, or if its program isn't found, as starting it without a
/// prefix would: with one, the prefix itself would start, and the error
/// would be lost.
pub fn launch_command(argv: &[String]) -> io::Result<Command> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "nothing to run"))?;
    let prefix = launch_prefix().unwrap_or_default();
    let mut words = prefix.split_whitespace();
    let Some(first) = words.next() else {
        let mut command = Command::new(program);
        command.args(args);
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
    env::var_os("PATH")
        .is_some_and(|dirs| env::split_paths(&dirs).any(|dir| is_executable(&dir.join(program))))
}
