//! The Linux half of `scope.rs`: asking the systemd user manager for a scope
//! and moving the program into it between the fork and the exec.
//!
//! # Owns
//!
//! - [`spawn`] on Linux, and the thread that makes the `StartTransientUnit` call while the child
//!   waits.
//!
//! # Not here
//!
//! - The scope's name, and whether a cgroup is empty: `scope.rs`. The D-Bus wire: `dbus.rs`.

use std::io;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::fd::RawFd;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use kotodama_protocol::launch_prefix;

use super::Launched;
use super::app_name;
use super::for_the_user;
use super::spawn_unscoped;
use super::unit_name;
use crate::dbus::Bus;
use crate::dbus::Call;
use crate::dbus::Error;
use crate::dbus::Marshal;
use crate::dbus::Message;

/// Small: the thread only makes one D-Bus call.
const SCOPE_STACK: usize = 64 * 1024;
/// How long the program waits for the job starting its scope, once the
/// call is answered.
const JOB_WAIT: Duration = Duration::from_secs(2);

/// Spawns `command`, without the launch prefix, which is for Extensions
/// alone, in a scope of its own, unless a launch prefix is set
/// or there is no systemd user manager to make one. The scope is named
/// for `app_id`, the desktop id of the app the command starts without
/// `.desktop`, if known, else for the command's program. A program that
/// can't get its scope still starts, and `unscoped` hears why, possibly
/// on another thread.
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
    app_id: Option<&str>,
    unscoped: impl FnOnce(Error) + Send + 'static,
) -> io::Result<Launched> {
    if launch_prefix().is_some() {
        return spawn_unscoped(command);
    }
    match Bus::user_manager() {
        Ok(manager) => spawn_scoped(command, app_id, manager, unscoped),
        Err(err) if err.is_absent() => spawn_unscoped(command),
        Err(err) => {
            unscoped(err);
            spawn_unscoped(command)
        }
    }
}

/// Spawns `command` in a scope `manager` makes, named as [`spawn`] says,
/// for the user as [`spawn_unscoped`] does.
pub(super) fn spawn_scoped(
    command: &mut Command,
    app_id: Option<&str>,
    manager: Bus,
    unscoped: impl FnOnce(Error) + Send + 'static,
) -> io::Result<Launched> {
    for_the_user(command);
    let app = app_name(command, app_id);
    let description = description(command);
    let (mut pid_reader, pid_writer) = io::pipe()?;
    let (go_reader, mut go_writer) = io::pipe()?;
    let (found, cgroup) = mpsc::channel();
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
            let scoped = start_scope(manager, &unit_name(&app, pid), &description, pid);
            if scoped.is_ok()
                && let Some(dir) = cgroup_of(pid)
            {
                // Sent before the child runs, so it is there once
                // `spawn` returns.
                let _ = found.send(dir);
            }
            // The child runs once it reads this, whether or not it got
            // its scope.
            let _ = go_writer.write_all(&[1]);
            if let Err(err) = scoped {
                unscoped(err);
            }
        });
    if mover.is_err() {
        return command.spawn().map(Launched::unscoped);
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
    Ok(Launched {
        child: child?,
        cgroup: cgroup.try_recv().ok(),
    })
}

/// The directory of the cgroup process `pid` is in, in the unified
/// hierarchy.
fn cgroup_of(pid: u32) -> Option<PathBuf> {
    let cgroups = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    let path = cgroups.lines().find_map(|line| line.strip_prefix("0::"))?;
    Some(PathBuf::from(format!("/sys/fs/cgroup{path}")))
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
