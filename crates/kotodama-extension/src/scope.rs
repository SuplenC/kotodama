//! Starting a program the user picked in a systemd scope of its own, so it
//! outlives the Daemon: under systemd every process the Daemon and its
//! Extensions start sits in the Daemon service's cgroup, and stopping the
//! service would kill the user's apps with it.
//!
//! The scope comes from the systemd user manager over D-Bus
//! (`StartTransientUnit`), as `systemd-run --user --scope` makes it, without
//! starting `systemd-run` for every program. A launch prefix
//! ([`kotodama_protocol::LAUNCH_PREFIX`]), when set, replaces this: the
//! program then starts after the prefix, in no scope of Kotodama's making.
//! Extensions that start programs through the prefix alone get
//! [`kotodama_protocol::SCOPE_PREFIX`] from the Daemon instead
//! ([`pass_scope_prefix`]).
//!
//! # Owns
//!
//! When a program gets a scope, the scope's name and properties, and moving
//! the program into it before it runs.
//!
//! # Not here
//!
//! The D-Bus calls themselves (`crate::dbus`); building the command and the
//! prefix (`crate::launch`, `kotodama_protocol::launch_command`).

use std::fmt::Write as _;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;

use kotodama_protocol::LAUNCH_PREFIX;
use kotodama_protocol::LAUNCH_SCOPED;
use kotodama_protocol::SCOPE_PREFIX;
#[cfg(target_os = "linux")]
pub use linux::spawn;

/// Gives the Extension `command` starts a launch prefix that makes a scope
/// for each program ([`SCOPE_PREFIX`], marked [`LAUNCH_SCOPED`]), when this
/// process has no prefix and there is a user manager to make scopes. An
/// Extension that starts programs through the prefix, in any language, then
/// gets the scope too; one built on this crate makes it itself.
pub fn pass_scope_prefix(command: &mut Command) {
    pass_scope_prefix_from(
        command,
        std::env::var_os(LAUNCH_PREFIX).is_some(),
        std::env::var_os("XDG_RUNTIME_DIR").as_deref().map(Path::new),
    );
}

/// [`pass_scope_prefix`] for a process whose own prefix is `prefixed`, with
/// `runtime_dir` its `XDG_RUNTIME_DIR`.
fn pass_scope_prefix_from(command: &mut Command, prefixed: bool, runtime_dir: Option<&Path>) {
    let Some(dir) = runtime_dir.filter(|_| !prefixed) else {
        return;
    };
    if has_user_manager(&dir.join("systemd/private")) {
        command.env(LAUNCH_PREFIX, SCOPE_PREFIX).env(LAUNCH_SCOPED, "1");
    }
}

#[cfg(target_os = "linux")]
fn has_user_manager(socket: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    socket.metadata().is_ok_and(|meta| meta.file_type().is_socket())
}

#[cfg(not(target_os = "linux"))]
fn has_user_manager(_socket: &Path) -> bool {
    false
}

/// A program started by [`spawn`].
#[derive(Debug)]
pub struct Launched {
    pub child: Child,
    /// The directory of its scope's cgroup, when it got a scope.
    pub cgroup: Option<PathBuf>,
}

impl Launched {
    fn unscoped(child: Child) -> Launched {
        Launched { child, cgroup: None }
    }
}

/// Keeps the launch prefix from the program `command` starts: it is for
/// Extensions, and a program the user picked would pass it on to all it
/// starts, such as every shell in a terminal, where a leftover
/// [`LAUNCH_SCOPED`] would hide a prefix the user sets.
fn for_the_user(command: &mut Command) -> &mut Command {
    command.env_remove(LAUNCH_PREFIX).env_remove(LAUNCH_SCOPED)
}

/// Spawns `command` for the user, in no scope.
fn spawn_unscoped(command: &mut Command) -> io::Result<Launched> {
    for_the_user(command).spawn().map(Launched::unscoped)
}

/// Whether no process is left in the cgroup at `cgroup`, as when a scope
/// emptied and went. Unreadable for another reason, it counts as not empty.
#[must_use]
pub fn is_empty(cgroup: &Path) -> bool {
    match std::fs::read(cgroup.join("cgroup.procs")) {
        Ok(procs) => procs.iter().all(u8::is_ascii_whitespace),
        Err(err) => err.kind() == io::ErrorKind::NotFound,
    }
}

/// Spawns `command` for the user (`for_the_user`). Without systemd there
/// is no scope to start it in.
///
/// # Errors
///
/// When it can't be started, as [`Command::spawn`].
#[cfg(not(target_os = "linux"))]
pub fn spawn(
    command: &mut Command,
    _app_id: Option<&str>,
    _unscoped: impl FnOnce(crate::dbus::Error) + Send + 'static,
) -> io::Result<Launched> {
    spawn_unscoped(command)
}

/// What names the scope of `command`: `app_id`, the desktop id of the app it
/// starts without `.desktop`, when known, else the file name of its program.
#[cfg_attr(
    all(not(target_os = "linux"), not(test)),
    expect(dead_code, reason = "only the Linux half and the tests call it")
)]
fn app_name(command: &Command, app_id: Option<&str>) -> String {
    if let Some(id) = app_id {
        return id.to_owned();
    }
    let program = command.get_program().to_string_lossy();
    program.rsplit('/').next().unwrap_or(&program).to_owned()
}

/// The unit name of the scope for the app `name` with process id `pid`, as
/// the freedesktop desktop entry and systemd conventions name an app's scope:
/// `app-<launcher>-<app id>-<unique>.scope`, with the app id escaped as
/// `systemd-escape` does.
#[cfg_attr(
    all(not(target_os = "linux"), not(test)),
    expect(dead_code, reason = "only the Linux half and the tests call it")
)]
fn unit_name(name: &str, pid: u32) -> String {
    let mut unit = String::from("app-kotodama-");
    for (n, byte) in name.bytes().take(MAX_NAME).enumerate() {
        let plain = byte.is_ascii_alphanumeric() || byte == b'_' || (byte == b'.' && n > 0);
        if plain {
            unit.push(char::from(byte));
        } else {
            let _ = write!(unit, "\\x{byte:02x}");
        }
    }
    let _ = write!(unit, "-{pid}.scope");
    unit
}

/// How much of an app's name goes in its scope's name, in bytes: a unit's
/// name may be 255 bytes at most, and each byte may take four escaped.
const MAX_NAME: usize = 48;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(test)]
mod tests;
