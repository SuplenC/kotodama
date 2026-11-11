//! Starting a program the user picked as a transient service of the systemd
//! user manager. The manager starts it, so it is never the Extension's
//! child and inherits nothing of the Extension's or the Daemon's: not their
//! environment, working directory, limits or descriptors. It gets the
//! session's environment, as the manager has it, sits in a cgroup of its own
//! from the start, and outlives the Daemon, whose service would otherwise
//! take every process in its cgroup down with it.
//!
//! The service is `app-kotodama-<app>@<random>.service`, as systemd's
//! conventions for desktop environments name an app's service. A launch
//! prefix ([`kotodama_protocol::LAUNCH_PREFIX`]), when set, replaces it.
//! Extensions that start programs through the prefix alone get
//! [`SERVICE_PREFIX`] from the Daemon instead ([`pass_service_prefix`]).
//!
//! # Owns
//!
//! The service's name and properties, finding its program as the session
//! would, and following the manager's signals to tell when it ends.
//!
//! # Not here
//!
//! The D-Bus wire (`crate::dbus`); starting a program as a child where
//! there is no user manager, and what a launch waits for (`crate::launch`).

use std::env;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::hash::BuildHasher;
use std::hash::RandomState;
use std::io;
use std::os::fd::BorrowedFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;
use std::time::Duration;
use std::time::Instant;

use kotodama_protocol::LAUNCH_AS_SERVICE;
use kotodama_protocol::LAUNCH_PREFIX;
use kotodama_protocol::SERVICE_PREFIX;

use crate::dbus;
use crate::dbus::Bus;
use crate::dbus::Call;
use crate::dbus::Marshal;
use crate::dbus::Message;

/// How long a launch waits for the job starting its service, once the
/// manager has taken the service: until the program is executed.
const JOB_WAIT: Duration = Duration::from_secs(2);
/// Where a service's program is looked for when the manager's environment
/// has no `PATH`: systemd's own default for a service then.
const DEFAULT_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin";
/// How much of an app's name goes in its service's name, in bytes: a unit's
/// name may be 255 bytes at most, and each byte may take four escaped.
const MAX_NAME: usize = 48;
/// The interfaces whose `PropertiesChanged` say how a service runs.
const UNIT: &str = "org.freedesktop.systemd1.Unit";
const SERVICE: &str = "org.freedesktop.systemd1.Service";
/// `ExecMainCode`'s values, `waitid`'s `si_code`s.
const CLD_EXITED: i32 = 1;
const CLD_KILLED: i32 = 2;
const CLD_DUMPED: i32 = 3;

/// Gives the Extension `command` starts a launch prefix that starts each
/// program as a service of the user manager ([`SERVICE_PREFIX`], marked
/// [`LAUNCH_AS_SERVICE`]), when this process has no prefix and there is a
/// user manager. An Extension that starts programs through the prefix, in
/// any language, then starts services too; one built on this crate asks the
/// manager itself.
pub fn pass_service_prefix(command: &mut Command) {
    pass_service_prefix_from(
        command,
        env::var_os(LAUNCH_PREFIX).is_some(),
        env::var_os("XDG_RUNTIME_DIR").as_deref().map(Path::new),
    );
}

/// [`pass_service_prefix`] for a process whose own prefix is `prefixed`,
/// with `runtime_dir` its `XDG_RUNTIME_DIR`.
fn pass_service_prefix_from(command: &mut Command, prefixed: bool, runtime_dir: Option<&Path>) {
    let Some(dir) = runtime_dir.filter(|_| !prefixed) else {
        return;
    };
    if has_user_manager(&dir.join("systemd/private")) {
        command.env(LAUNCH_PREFIX, SERVICE_PREFIX).env(LAUNCH_AS_SERVICE, "1");
    }
}

fn has_user_manager(socket: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    socket.metadata().is_ok_and(|meta| meta.file_type().is_socket())
}

/// A program to start as a service.
#[derive(Debug, Clone, Copy)]
pub struct Program<'a> {
    /// The command line: the program, then its arguments.
    pub argv: &'a [String],
    /// The directory it starts in. Without one, the manager's default: the
    /// user's home directory.
    pub dir: Option<&'a Path>,
    /// The desktop id of the app it starts, without `.desktop`, which names
    /// its service. Without one, the program's file name does.
    pub app_id: Option<&'a str>,
    /// Where its output, stdout and stderr, goes. Without it, the journal,
    /// under the service's name.
    pub output: Option<BorrowedFd<'a>>,
}

/// Why a program didn't start as a service.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The manager wouldn't make the service, as one too old for its
    /// properties does, or couldn't be heard before it was asked. The program
    /// didn't start, so it may still start another way.
    #[error("the user manager refused its service")]
    Refused(#[source] dbus::Error),
    /// The program can't start, in words for the user: it isn't installed,
    /// or the manager couldn't execute it.
    #[error(transparent)]
    Failed(io::Error),
}

/// A program started as a service, followed through the manager's signals.
#[derive(Debug)]
pub struct Running {
    /// `None` once a read failed: one cut short leaves the connection in the
    /// middle of a message, so nothing after it can be trusted.
    manager: Option<Bus>,
    unit: Unit,
}

impl Running {
    /// The process the service started, while it runs.
    #[must_use]
    pub fn main_pid(&self) -> Option<u32> {
        self.unit.main_pid
    }

    /// Waits, without a time limit, for the program's own process to exit,
    /// and says how it ended. What it left running doesn't count. `None`
    /// when the manager can't be heard any more.
    #[must_use]
    pub fn main_exit(mut self) -> Option<ExitStatus> {
        self.hear_until(None, |unit| unit.main_exit.is_some() || unit.ended);
        self.unit.main_exit
    }

    /// Whether everything the service ran ended by `deadline`.
    #[must_use]
    pub fn ended_by(mut self, deadline: Instant) -> bool {
        self.hear_until(Some(deadline), |unit| unit.ended)
    }

    /// Takes in the manager's messages until `heard` holds of the unit,
    /// which it returns, or `deadline` passes, or the manager can't be heard.
    fn hear_until(&mut self, deadline: Option<Instant>, heard: impl Fn(&Unit) -> bool) -> bool {
        loop {
            if heard(&self.unit) {
                return true;
            }
            let Some(manager) = self.manager.as_mut() else {
                return false;
            };
            let timeout = match deadline {
                Some(deadline) => match deadline.saturating_duration_since(Instant::now()) {
                    Duration::ZERO => return false,
                    left => Some(left),
                },
                None => None,
            };
            let Ok(message) = manager.wait_at_most(timeout).and_then(|()| manager.read()) else {
                self.manager = None;
                return false;
            };
            self.unit.observe(&message);
        }
    }

    /// Asks the manager for the main process when no signal named it yet,
    /// as when the start job's end came before the unit's changes.
    fn ask_main_pid(&mut self) {
        let Running { manager, unit } = self;
        if unit.main_pid.is_some() || unit.main_exit.is_some() || unit.ended {
            return;
        }
        let Some(bus) = manager.as_mut() else {
            return;
        };
        let mut body = Marshal::default();
        body.string(SERVICE);
        body.string("MainPID");
        // The call borrows the path while the unit takes in signals.
        let path = unit.path.clone();
        let call = Call::new(
            &path,
            "org.freedesktop.DBus.Properties",
            "Get",
            "org.freedesktop.systemd1",
        )
        .with_body("ss", body);
        match bus.call(&call, |message| unit.observe(message)) {
            Ok(answer) => {
                let mut body = answer.body();
                if body.variant() == Some("u") {
                    unit.main_pid = body.u32().filter(|&pid| pid != 0);
                }
            }
            // The unit is gone already.
            Err(dbus::Error::Failed(_)) => {}
            Err(_) => *manager = None,
        }
    }
}

/// Asks `manager` to start `program` as a service, and waits until it has
/// been executed.
///
/// # Errors
///
/// [`Error::Refused`] when the manager won't make the service, and nothing
/// started; [`Error::Failed`] when the program can't start, or the manager
/// didn't answer in time.
pub fn start(mut manager: Bus, program: &Program<'_>) -> Result<Running, Error> {
    let name = program
        .argv
        .first()
        .ok_or_else(|| Error::Failed(io::Error::new(io::ErrorKind::InvalidInput, "nothing to run")))?;
    let environment = manager_environment(&mut manager).map_err(Error::Refused)?;
    let dir = program
        .dir
        .map(std::path::absolute)
        .transpose()
        .map_err(Error::Failed)?;
    let executable = executable(name, &environment, dir.as_deref())?;
    let unit = unit_name(
        &app_name(name, program.app_id),
        RandomState::new().hash_one(program.argv),
    );
    let added = added_environment(&environment);
    let properties = Properties {
        unit: &unit,
        executable: &executable,
        argv: program.argv,
        dir: dir.as_deref(),
        environment: &added,
        output: program.output.is_some(),
    };
    let fds: Vec<BorrowedFd<'_>> = program.output.into_iter().flat_map(|fd| [fd, fd]).collect();
    let call = start_transient_unit(&properties).with_fds(&fds);
    let mut watched = Unit::new(unit);
    // Its signals may come before the call's answer, and its job may finish
    // then too.
    match manager.call(&call, |message| watched.observe(message)) {
        Ok(_) => {}
        // The manager answered that it made nothing, or never got the call.
        Err(err @ (dbus::Error::Failed(_) | dbus::Error::NoUnixFds)) => return Err(Error::Refused(err)),
        // The call may have reached it, and the service may start yet:
        // starting the program another way could start it twice.
        Err(_) => {
            return Err(Error::Failed(io::Error::new(
                io::ErrorKind::TimedOut,
                "the user manager didn't answer in time",
            )));
        }
    }
    let mut running = Running {
        manager: Some(manager),
        unit: watched,
    };
    // The manager's signals about other units come too, so a steady stream
    // of them must not keep the launch waiting.
    running.hear_until(Some(Instant::now() + JOB_WAIT), |unit| unit.job_result.is_some());
    match running.unit.job_result.as_deref() {
        // Without the job's end in time, the service is still the manager's:
        // starting the program another way would start it twice.
        None | Some("done") => {
            running.ask_main_pid();
            Ok(running)
        }
        Some("failed") => Err(Error::Failed(io::Error::other("the user manager couldn't execute it"))),
        Some(result) => Err(Error::Failed(io::Error::other(format!(
            "the user manager gave up starting it ({result})"
        )))),
    }
}

/// The executable `name` runs, found as the session finds it: on the `PATH`
/// of the manager's `environment`, or a relative path from `dir`, else the
/// home directory, where the service starts.
fn executable(name: &str, environment: &[String], dir: Option<&Path>) -> Result<PathBuf, Error> {
    let path = variable(environment, "PATH").unwrap_or(DEFAULT_PATH);
    let relative_to = dir.map(Path::to_owned).or_else(env::home_dir);
    find(name, OsStr::new(path), relative_to.as_deref()).ok_or_else(|| {
        Error::Failed(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{name} isn't installed"),
        ))
    })
}

/// The variables, `NAME=value`, a service needs beyond the manager's
/// `environment`: `WAYLAND_DISPLAY` when the session has none yet, as when
/// the compositor started after the manager took its environment.
fn added_environment(environment: &[String]) -> Vec<String> {
    let mut added = Vec::new();
    #[cfg(target_os = "linux")]
    if let Some(display) = crate::runtime::missing_wayland_display(
        variable(environment, "WAYLAND_DISPLAY").map(OsStr::new),
        crate::runtime::dir().as_deref(),
    ) {
        added.push(format!("WAYLAND_DISPLAY={}", display.to_string_lossy()));
    }
    added
}

/// What a service is made with.
struct Properties<'a> {
    unit: &'a str,
    executable: &'a Path,
    argv: &'a [String],
    dir: Option<&'a Path>,
    /// Variables to add to the manager's, as `NAME=value`.
    environment: &'a [String],
    /// Whether its stdout and stderr are the descriptors 0 and 1 passed with
    /// the call.
    output: bool,
}

/// The `StartTransientUnit` call for the service `properties` describe.
/// `Type=exec` finishes its start job once the program is executed, so one
/// that can't be fails the job. `ExitType=cgroup` keeps the service while
/// anything in it runs, so a launcher that starts an app and exits doesn't
/// take the app down with it. `ExecStartEx` with `no-env-expand` passes the
/// arguments as they are: `ExecStart` would replace each `$NAME` in them
/// with the service's variable.
fn start_transient_unit(properties: &Properties<'_>) -> Call<'static> {
    let mut body = Marshal::default();
    body.string(properties.unit);
    body.string("fail");
    body.array(8, |m| {
        m.property("Description", "s", |m| m.string(&properties.argv.join(" ")));
        m.property("Slice", "s", |m| m.string("app.slice"));
        m.property("CollectMode", "s", |m| m.string("inactive-or-failed"));
        m.property("Type", "s", |m| m.string("exec"));
        m.property("ExitType", "s", |m| m.string("cgroup"));
        m.property("ExecStartEx", "a(sasas)", |m| {
            m.array(8, |m| {
                m.pad(8);
                m.string(&properties.executable.to_string_lossy());
                m.array(4, |m| {
                    for arg in properties.argv {
                        m.string(arg);
                    }
                });
                m.array(4, |m| m.string("no-env-expand"));
            });
        });
        if let Some(dir) = properties.dir {
            m.property("WorkingDirectory", "s", |m| m.string(&dir.to_string_lossy()));
        }
        if !properties.environment.is_empty() {
            m.property("Environment", "as", |m| {
                m.array(4, |m| {
                    for variable in properties.environment {
                        m.string(variable);
                    }
                });
            });
        }
        if properties.output {
            m.property("StandardOutputFileDescriptor", "h", |m| m.fd(0));
            m.property("StandardErrorFileDescriptor", "h", |m| m.fd(1));
        }
    });
    // No auxiliary units.
    body.array(8, |_| {});
    Call::new(
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
        "StartTransientUnit",
        "org.freedesktop.systemd1",
    )
    .with_body("ssa(sv)a(sa(sv))", body)
}

/// The manager's environment, which every service it starts gets: the
/// session's, as `NAME=value`.
fn manager_environment(manager: &mut Bus) -> Result<Vec<String>, dbus::Error> {
    let mut body = Marshal::default();
    body.string("org.freedesktop.systemd1.Manager");
    body.string("Environment");
    let call = Call::new(
        "/org/freedesktop/systemd1",
        "org.freedesktop.DBus.Properties",
        "Get",
        "org.freedesktop.systemd1",
    )
    .with_body("ss", body);
    let answer = manager.call(&call, |_| {})?;
    let malformed = || dbus::Error::Failed("the manager's environment isn't a list of strings".into());
    let mut body = answer.body();
    if body.variant() != Some("as") {
        return Err(malformed());
    }
    let mut values = body.array("s").ok_or_else(malformed)?;
    let mut environment = Vec::new();
    while !values.is_empty() {
        environment.push(values.string().ok_or_else(malformed)?.to_owned());
    }
    Ok(environment)
}

/// The value of `name` in `environment`, a list of `NAME=value`.
fn variable<'a>(environment: &'a [String], name: &str) -> Option<&'a str> {
    environment.iter().find_map(|variable| {
        let (key, value) = variable.split_once('=')?;
        (key == name).then_some(value)
    })
}

/// The executable `program` names, as `execvp` would find it with `path` as
/// its `PATH`: a name is looked for in each directory of `path`, a relative
/// path such as `./run` is taken from `dir`, the directory it starts in.
fn find(program: &str, path: &OsStr, dir: Option<&Path>) -> Option<PathBuf> {
    let is_executable = |path: &Path| {
        path.metadata()
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    };
    let named = Path::new(program);
    if named.is_absolute() {
        return is_executable(named).then(|| named.to_owned());
    }
    if program.contains('/') {
        let full = dir?.join(named);
        return is_executable(&full).then_some(full);
    }
    env::split_paths(path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_absolute() && is_executable(candidate))
}

/// What names the service of `program`: `app_id`, the desktop id of the app
/// it starts without `.desktop`, when known, else the program's file name.
fn app_name(program: &str, app_id: Option<&str>) -> String {
    if let Some(id) = app_id {
        return id.to_owned();
    }
    program.rsplit('/').next().unwrap_or(program).to_owned()
}

/// The unit name of a service for the app `name`, told apart from the
/// app's other services by `unique`, as systemd's conventions for desktop
/// environments name it: `app-<launcher>-<app id>@<unique>.service`, with
/// the app id escaped as `systemd-escape` does.
fn unit_name(name: &str, unique: u64) -> String {
    let mut unit = String::from("app-kotodama-");
    for (n, byte) in name.bytes().take(MAX_NAME).enumerate() {
        let plain = byte.is_ascii_alphanumeric() || byte == b'_' || (byte == b'.' && n > 0);
        if plain {
            unit.push(char::from(byte));
        } else {
            let _ = write!(unit, "\\x{byte:02x}");
        }
    }
    let _ = write!(unit, "@{unique:016x}.service");
    unit
}

/// The manager's object for the unit `unit`, which its `PropertiesChanged`
/// come from: every byte but a letter or a digit escaped as `_xx`.
fn object_path(unit: &str) -> String {
    let mut path = String::from("/org/freedesktop/systemd1/unit/");
    for byte in unit.bytes() {
        if byte.is_ascii_alphanumeric() {
            path.push(char::from(byte));
        } else {
            let _ = write!(path, "_{byte:02x}");
        }
    }
    path
}

/// What the manager's signals said about one service.
#[derive(Debug)]
struct Unit {
    name: String,
    path: String,
    /// The result of the job that started it, once that finished.
    job_result: Option<String>,
    main_pid: Option<u32>,
    main_exit: Option<ExitStatus>,
    /// Whether it got as far as starting. A new unit is `inactive` before it
    /// starts, which isn't an end.
    activated: bool,
    /// Whether nothing in it runs any more.
    ended: bool,
}

impl Unit {
    fn new(name: String) -> Unit {
        Unit {
            path: object_path(&name),
            name,
            job_result: None,
            main_pid: None,
            main_exit: None,
            activated: false,
            ended: false,
        }
    }

    /// Takes in what `message` says about the service, if anything.
    fn observe(&mut self, message: &Message) {
        if let Some(result) = job_removed(message, &self.name) {
            // A start job done proves the unit started, whatever order its
            // changes come in.
            self.activated |= result == "done";
            self.job_result = Some(result);
        } else if message.path() == Some(self.path.as_str()) && message.is_signal("PropertiesChanged", "sa{sv}as") {
            // A malformed signal says nothing.
            let _ = self.properties_changed(message);
        }
    }

    /// `PropertiesChanged(interface, changed, invalidated)`.
    fn properties_changed(&mut self, message: &Message) -> Option<()> {
        let mut body = message.body();
        let interface = body.string()?;
        let mut changed = body.array("{sv}")?;
        let (mut code, mut status) = (None, None);
        while !changed.is_empty() {
            changed.align_struct();
            let name = changed.string()?;
            let signature = changed.variant()?;
            match (interface, name, signature) {
                (SERVICE, "MainPID", "u") => self.main_pid = changed.u32().filter(|&pid| pid != 0),
                (SERVICE, "ExecMainCode", "i") => code = changed.i32(),
                (SERVICE, "ExecMainStatus", "i") => status = changed.i32(),
                (UNIT, "ActiveState", "s") => match changed.string()? {
                    "activating" | "active" | "reloading" | "deactivating" => self.activated = true,
                    "inactive" | "failed" => self.ended = self.activated,
                    _ => {}
                },
                _ => changed.skip(signature)?,
            }
        }
        if let (Some(code), Some(status)) = (code, status) {
            self.main_exit = self.main_exit.or(exit_status(code, status));
        }
        Some(())
    }
}

/// How the main process ended, from `ExecMainCode` and `ExecMainStatus`:
/// `None` while it runs.
fn exit_status(code: i32, status: i32) -> Option<ExitStatus> {
    // The raw values are `waitpid`'s status words.
    let raw = match code {
        CLD_EXITED => (status & 0xff) << 8,
        CLD_KILLED => status & 0x7f,
        CLD_DUMPED => (status & 0x7f) | 0x80,
        _ => return None,
    };
    Some(ExitStatus::from_raw(raw))
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

#[cfg(test)]
mod tests;
