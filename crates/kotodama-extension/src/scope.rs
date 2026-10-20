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

#[cfg(target_os = "linux")]
pub use linux::spawn;

use std::path::Path;
use std::process::Command;

use kotodama_protocol::{LAUNCH_PREFIX, LAUNCH_SCOPED, SCOPE_PREFIX};

/// Gives the Extension `command` starts a launch prefix that makes a scope
/// for each program ([`SCOPE_PREFIX`], marked [`LAUNCH_SCOPED`]), when this
/// process has no prefix and there is a user manager to make scopes. An
/// Extension that starts programs through the prefix, in any language, then
/// gets the scope too; one built on this crate makes it itself.
pub fn pass_scope_prefix(command: &mut Command) {
    pass_scope_prefix_from(
        command,
        std::env::var_os(LAUNCH_PREFIX).is_some(),
        std::env::var_os("XDG_RUNTIME_DIR")
            .as_deref()
            .map(Path::new),
    );
}

/// [`pass_scope_prefix`] for a process whose own prefix is `prefixed`, with
/// `runtime_dir` its `XDG_RUNTIME_DIR`.
fn pass_scope_prefix_from(command: &mut Command, prefixed: bool, runtime_dir: Option<&Path>) {
    let Some(dir) = runtime_dir.filter(|_| !prefixed) else {
        return;
    };
    if has_user_manager(&dir.join("systemd/private")) {
        command
            .env(LAUNCH_PREFIX, SCOPE_PREFIX)
            .env(LAUNCH_SCOPED, "1");
    }
}

#[cfg(target_os = "linux")]
fn has_user_manager(socket: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    socket
        .metadata()
        .is_ok_and(|meta| meta.file_type().is_socket())
}

#[cfg(not(target_os = "linux"))]
fn has_user_manager(_socket: &Path) -> bool {
    false
}

/// Spawns `command`. Without systemd there is no scope to start it in.
///
/// # Errors
///
/// When it can't be started, as [`std::process::Command::spawn`].
#[cfg(not(target_os = "linux"))]
pub fn spawn(
    command: &mut std::process::Command,
    _unscoped: impl FnOnce(crate::dbus::Error) + Send + 'static,
) -> std::io::Result<std::process::Child> {
    command.spawn()
}

/// The unit name of the scope for the program `program` with process id
/// `pid`, as the freedesktop desktop entry and systemd conventions name an
/// app's scope: `app-<launcher>-<app id>-<unique>.scope`, with the app id
/// escaped as `systemd-escape` does.
#[cfg_attr(all(not(target_os = "linux"), not(test)), expect(dead_code))]
fn unit_name(program: &str, pid: u32) -> String {
    let name = program.rsplit('/').next().unwrap_or(program);
    let mut unit = String::from("app-kotodama-");
    for (n, byte) in name.bytes().take(MAX_NAME).enumerate() {
        let plain = byte.is_ascii_alphanumeric() || byte == b'_' || (byte == b'.' && n > 0);
        if plain {
            unit.push(char::from(byte));
        } else {
            unit.push_str(&format!("\\x{byte:02x}"));
        }
    }
    unit.push_str(&format!("-{pid}.scope"));
    unit
}

/// How much of a program's name goes in its scope's name, in bytes: a unit's
/// name may be 255 bytes at most, and each byte may take four escaped.
const MAX_NAME: usize = 48;

#[cfg(target_os = "linux")]
mod linux {
    use std::io::{self, Read, Write};
    use std::os::fd::{AsRawFd, RawFd};
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};
    use std::thread;
    use std::time::{Duration, Instant};

    use kotodama_protocol::launch_prefix;

    use super::unit_name;
    use crate::dbus::{Bus, Call, Error, Marshal, Message};

    /// Small: the thread only makes one D-Bus call.
    const SCOPE_STACK: usize = 64 * 1024;
    /// How long the program waits for the job starting its scope, once the
    /// call is answered.
    const JOB_WAIT: Duration = Duration::from_secs(2);

    /// Spawns `command` in a scope of its own, unless a launch prefix is set
    /// or there is no systemd user manager to make one. A program that can't
    /// get its scope still starts, and `unscoped` hears why, possibly on
    /// another thread.
    ///
    /// The program is moved into its scope after the fork and before it
    /// runs, so whatever it starts at once lands in the scope too: the child
    /// sends its process id to a thread, which asks the user manager for the
    /// scope and lets the child run once the scope is there.
    ///
    /// # Errors
    ///
    /// When it can't be started, as [`Command::spawn`].
    pub fn spawn(
        command: &mut Command,
        unscoped: impl FnOnce(Error) + Send + 'static,
    ) -> io::Result<Child> {
        if launch_prefix().is_some() {
            return command.spawn();
        }
        match Bus::user_manager() {
            Ok(manager) => spawn_scoped(command, manager, unscoped),
            Err(err) if err.is_absent() => command.spawn(),
            Err(err) => {
                unscoped(err);
                command.spawn()
            }
        }
    }

    /// Spawns `command` in a scope `manager` makes.
    pub(super) fn spawn_scoped(
        command: &mut Command,
        manager: Bus,
        unscoped: impl FnOnce(Error) + Send + 'static,
    ) -> io::Result<Child> {
        let program = command.get_program().to_string_lossy().into_owned();
        let description = description(command);
        let (mut pid_reader, pid_writer) = io::pipe()?;
        let (go_reader, mut go_writer) = io::pipe()?;
        let mover = thread::Builder::new()
            .name("scope".into())
            .stack_size(SCOPE_STACK)
            .spawn(move || {
                let mut pid = [0; 4];
                // Nothing to read when the fork failed.
                if pid_reader.read_exact(&mut pid).is_err() {
                    return;
                }
                let pid = u32::from_ne_bytes(pid);
                let scoped = start_scope(manager, &unit_name(&program, pid), &description, pid);
                // The child runs once it reads this, whether or not it got
                // its scope.
                let _ = go_writer.write_all(&[1]);
                if let Err(err) = scoped {
                    unscoped(err);
                }
            });
        if mover.is_err() {
            return command.spawn();
        }
        let (pid_fd, go_fd) = (pid_writer.as_raw_fd(), go_reader.as_raw_fd());
        // SAFETY: the hook runs in the child between fork and exec, and only
        // calls `getpid`, `write` and `read`, which are async-signal-safe,
        // on descriptors the parent keeps open until `spawn` returns.
        unsafe {
            command.pre_exec(move || {
                wait_for_scope(pid_fd, go_fd);
                Ok(())
            });
        }
        // HAZARD: `spawn` returns only once the child has run the program,
        // and the child runs it only once the thread has written to
        // `go_writer`. Keep the D-Bus call on that thread: made here, after
        // `spawn`, it would wait forever.
        let child = command.spawn();
        drop((pid_writer, go_reader));
        child
    }

    /// In the child: sends its process id down `pid_fd`, then waits for a
    /// byte on `go_fd`, or for its end.
    fn wait_for_scope(pid_fd: RawFd, go_fd: RawFd) {
        // SAFETY: getpid can't fail and has no preconditions.
        let pid = unsafe { libc::getpid() }.cast_unsigned().to_ne_bytes();
        retry(|| {
            // SAFETY: `pid` is 4 readable bytes, and `pid_fd` is open.
            unsafe { libc::write(pid_fd, pid.as_ptr().cast(), pid.len()) }
        });
        let mut go = 0u8;
        retry(|| {
            // SAFETY: `go` is 1 writable byte, and `go_fd` is open.
            unsafe { libc::read(go_fd, (&raw mut go).cast(), 1) }
        });
    }

    /// Calls `call` again while a signal interrupts it.
    fn retry(mut call: impl FnMut() -> isize) {
        while call() < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {}
    }

    /// What the scope's description says: the command line, as
    /// `systemd-run` has it.
    fn description(command: &Command) -> String {
        let mut description = command.get_program().to_string_lossy().into_owned();
        for arg in command.get_args() {
            description.push(' ');
            description.push_str(&arg.to_string_lossy());
        }
        description
    }

    /// Asks the user manager for the scope `unit` holding `pid`, in
    /// `app.slice`, collected once it is empty or failed, and waits for the
    /// job that starts it to finish.
    fn start_scope(mut manager: Bus, unit: &str, description: &str, pid: u32) -> Result<(), Error> {
        let mut body = Marshal::default();
        body.string(unit);
        body.string("fail");
        body.array(8, |m| {
            m.property("Description", "s", |m| m.string(description));
            m.property("Slice", "s", |m| m.string("app.slice"));
            m.property("CollectMode", "s", |m| m.string("inactive-or-failed"));
            m.property("PIDs", "au", |m| m.array(4, |m| m.u32(pid)));
        });
        // No auxiliary units.
        body.array(8, |_| {});
        let call = Call::new(
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "StartTransientUnit",
            "org.freedesktop.systemd1",
        )
        .with_body("ssa(sv)a(sa(sv))", body);
        // The job may finish before the call's answer is read.
        let mut finished = None;
        manager.call(&call, |message| {
            finished = finished.take().or_else(|| job_removed(message, unit));
        })?;
        // The manager's signals about other units come too, so a steady
        // stream of them must not keep the program waiting.
        let deadline = Instant::now() + JOB_WAIT;
        let result = match finished {
            Some(result) => result,
            None => loop {
                if let Some(result) = job_removed(&manager.read()?, unit) {
                    break result;
                }
                if Instant::now() > deadline {
                    return Err(Error::Failed(format!("{unit} didn't start in time")));
                }
            },
        };
        if result == "done" {
            return Ok(());
        }
        Err(Error::Failed(format!("starting {unit}: {result}")))
    }

    /// The result of the job for `unit`, if `message` says it finished:
    /// `JobRemoved(id, job, unit, result)`.
    fn job_removed(message: &Message, unit: &str) -> Option<String> {
        if !message.is_signal("JobRemoved", "uoss") {
            return None;
        }
        let mut body = message.body();
        body.u32()?;
        body.string()?;
        if body.string()? != unit {
            return None;
        }
        body.string().map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_is_named_for_its_program_escaped_and_its_process() {
        assert_eq!(
            unit_name("/usr/bin/firefox", 42),
            "app-kotodama-firefox-42.scope"
        );
        assert_eq!(
            unit_name("gnome-text-editor", 7),
            "app-kotodama-gnome\\x2dtext\\x2deditor-7.scope"
        );
        assert_eq!(
            unit_name(".hidden ü", 1),
            "app-kotodama-\\x2ehidden\\x20\\xc3\\xbc-1.scope"
        );
        assert!(unit_name(&"x".repeat(1000), u32::MAX).len() < 256);
    }

    /// The variables `command` sets.
    #[cfg(target_os = "linux")]
    fn set(command: &Command) -> Vec<(String, String)> {
        command
            .get_envs()
            .filter_map(|(name, value)| {
                Some((name.to_str()?.to_owned(), value?.to_str()?.to_owned()))
            })
            .collect()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn extensions_get_the_scope_prefix_only_when_there_is_a_user_manager_and_no_prefix() {
        let runtime = tempfile::tempdir().unwrap();
        let mut command = Command::new("true");
        pass_scope_prefix_from(&mut command, false, Some(runtime.path()));
        assert_eq!(set(&command), [], "no user manager");

        std::fs::create_dir(runtime.path().join("systemd")).unwrap();
        let _manager =
            std::os::unix::net::UnixListener::bind(runtime.path().join("systemd/private")).unwrap();
        let mut command = Command::new("true");
        pass_scope_prefix_from(&mut command, true, Some(runtime.path()));
        assert_eq!(set(&command), [], "a prefix of the Daemon's own");

        let mut command = Command::new("true");
        pass_scope_prefix_from(&mut command, false, Some(runtime.path()));
        assert_eq!(
            set(&command),
            [
                (LAUNCH_PREFIX.to_owned(), SCOPE_PREFIX.to_owned()),
                (LAUNCH_SCOPED.to_owned(), "1".to_owned())
            ]
        );
    }

    #[cfg(target_os = "linux")]
    mod linux {
        use std::os::unix::net::UnixStream;
        use std::process::{Command, Stdio};
        use std::sync::mpsc;
        use std::thread;
        use std::time::{Duration, Instant};

        use super::super::linux::spawn_scoped;
        use super::unit_name;
        use crate::dbus::tests::{bus, job_removed, method_return, peer};

        /// The unit a `StartTransientUnit` call in `call` asks for.
        fn unit_asked(call: &[u8]) -> String {
            let at = call
                .windows(13)
                .position(|window| window == b"app-kotodama-")
                .expect("a unit name");
            let len = call[at..].iter().position(|&byte| byte == 0).unwrap();
            String::from_utf8(call[at..at + len].to_vec()).unwrap()
        }

        /// A manager that answers after `delay`, and finishes the job with
        /// `result`.
        fn manager(
            delay: Duration,
            result: &'static str,
        ) -> (UnixStream, thread::JoinHandle<Vec<u8>>) {
            let (ours, theirs) = UnixStream::pair().unwrap();
            let peer = peer(theirs, move |serial, call| {
                thread::sleep(delay);
                let mut answer = method_return(serial, "/org/freedesktop/systemd1/job/4");
                answer.extend(job_removed(&unit_asked(call), result));
                answer
            });
            (ours, peer)
        }

        fn echo_pid() -> Command {
            let mut command = Command::new("sh");
            command.args(["-c", "echo $$"]).stdout(Stdio::piped());
            command
        }

        #[test]
        fn a_program_runs_only_once_the_scope_named_for_it_holds_it() {
            let (ours, peer) = manager(Duration::from_millis(300), "done");
            let (errors, unscoped) = mpsc::channel();
            let started = Instant::now();
            let child = spawn_scoped(&mut echo_pid(), bus(ours), move |err| {
                let _ = errors.send(err.to_string());
            })
            .unwrap();
            assert!(started.elapsed() >= Duration::from_millis(300));
            let pid = child.id();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                pid.to_string()
            );

            let call = peer.join().unwrap();
            assert_eq!(unit_asked(&call), unit_name("sh", pid));
            let has = |text: &[u8]| call.windows(text.len()).any(|window| window == text);
            assert!(has(b"StartTransientUnit"));
            assert!(has(b"app.slice"));
            assert!(has(&pid.to_le_bytes()));
            assert!(unscoped.recv_timeout(Duration::from_millis(200)).is_err());
        }

        #[test]
        fn a_program_whose_scope_failed_still_runs_and_says_why() {
            let (ours, peer) = manager(Duration::ZERO, "failed");
            let (errors, unscoped) = mpsc::channel();
            let child = spawn_scoped(&mut echo_pid(), bus(ours), move |err| {
                let _ = errors.send(err.to_string());
            })
            .unwrap();
            assert!(child.wait_with_output().unwrap().status.success());
            peer.join().unwrap();
            let err = unscoped.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(err.ends_with(": failed"), "{err}");
        }

        #[test]
        fn a_program_that_cant_start_fails_without_waiting_forever() {
            let (ours, _peer) = manager(Duration::ZERO, "done");
            let started =
                spawn_scoped(&mut Command::new("/nonexistent/program"), bus(ours), |_| {});
            assert!(started.is_err());
        }
    }
}
