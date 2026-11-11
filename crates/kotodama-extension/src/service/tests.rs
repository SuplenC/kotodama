use std::fs;
use std::io::Read;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::thread;

use super::*;
use crate::dbus::tests::answer;
use crate::dbus::tests::bus_passing_fds;
use crate::dbus::tests::error;
use crate::dbus::tests::fd_peer;
use crate::dbus::tests::job_removed;
use crate::dbus::tests::method_return;
use crate::dbus::tests::read_message;
use crate::dbus::tests::signal;

#[test]
fn a_service_is_named_for_its_app_escaped_and_told_apart_from_its_others() {
    assert_eq!(
        unit_name("firefox", 42),
        "app-kotodama-firefox@000000000000002a.service"
    );
    assert_eq!(
        unit_name("org.gnome.TextEditor", 3),
        "app-kotodama-org.gnome.TextEditor@0000000000000003.service"
    );
    assert_eq!(
        unit_name("gnome-text-editor", 7),
        "app-kotodama-gnome\\x2dtext\\x2deditor@0000000000000007.service"
    );
    assert_eq!(
        unit_name(".hidden ü", 1),
        "app-kotodama-\\x2ehidden\\x20\\xc3\\xbc@0000000000000001.service"
    );
    assert!(unit_name(&"x".repeat(1000), u64::MAX).len() < 256);
}

#[test]
fn an_app_is_named_by_its_desktop_id_else_by_its_programs_file_name() {
    assert_eq!(
        app_name("/usr/bin/firefox", Some("org.mozilla.firefox")),
        "org.mozilla.firefox"
    );
    assert_eq!(app_name("/usr/bin/firefox", None), "firefox");
    assert_eq!(app_name("foot", None), "foot");
}

/// As systemd names the object of `probe-x@two.service`.
#[test]
fn a_units_object_escapes_all_but_letters_and_digits() {
    assert_eq!(
        object_path("probe-x@two.service"),
        "/org/freedesktop/systemd1/unit/probe_2dx_40two_2eservice"
    );
    assert_eq!(
        object_path("app-kotodama-a\\x2db@1.service"),
        "/org/freedesktop/systemd1/unit/app_2dkotodama_2da_5cx2db_401_2eservice"
    );
}

#[test]
fn a_program_is_found_as_execvp_finds_it_on_the_path_given() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    fs::create_dir(&bin).unwrap();
    for (name, mode) in [("prog", 0o755), ("data", 0o644)] {
        fs::write(bin.join(name), "").unwrap();
        fs::set_permissions(bin.join(name), fs::Permissions::from_mode(mode)).unwrap();
    }
    let path = env::join_paths(["/nonexistent".as_ref(), bin.as_path()]).unwrap();
    let path = path.as_os_str();

    assert_eq!(find("prog", path, None), Some(bin.join("prog")));
    assert_eq!(find("data", path, None), None, "not executable");
    assert_eq!(find("missing", path, None), None);
    assert_eq!(find("prog", OsStr::new(""), None), None, "an empty PATH");
    let absolute = bin.join("prog");
    assert_eq!(find(absolute.to_str().unwrap(), path, None), Some(absolute));
    assert_eq!(
        find("./bin/prog", path, Some(dir.path())),
        Some(dir.path().join("./bin/prog"))
    );
    assert_eq!(find("./bin/prog", path, None), None, "nowhere to start from");
}

#[test]
fn how_the_main_process_ended_reads_as_its_wait_status() {
    assert_eq!(exit_status(CLD_EXITED, 3).and_then(|status| status.code()), Some(3));
    assert!(exit_status(CLD_EXITED, 0).unwrap().success());
    assert_eq!(exit_status(CLD_KILLED, 9).and_then(|status| status.signal()), Some(9));
    let dumped = exit_status(CLD_DUMPED, 11).unwrap();
    assert_eq!((dumped.signal(), dumped.core_dumped()), (Some(11), true));
    assert!(exit_status(0, 0).is_none(), "still running");
}

/// The variables `command` sets.
fn set(command: &Command) -> Vec<(String, String)> {
    command
        .get_envs()
        .filter_map(|(name, value)| Some((name.to_str()?.to_owned(), value?.to_str()?.to_owned())))
        .collect()
}

#[test]
fn extensions_get_the_service_prefix_only_when_there_is_a_user_manager_and_no_prefix() {
    let runtime = tempfile::tempdir().unwrap();
    let mut command = Command::new("true");
    pass_service_prefix_from(&mut command, false, Some(runtime.path()));
    assert_eq!(set(&command), [], "no user manager");

    fs::create_dir(runtime.path().join("systemd")).unwrap();
    let _manager = std::os::unix::net::UnixListener::bind(runtime.path().join("systemd/private")).unwrap();
    let mut command = Command::new("true");
    pass_service_prefix_from(&mut command, true, Some(runtime.path()));
    assert_eq!(set(&command), [], "a prefix of the Daemon's own");

    let mut command = Command::new("true");
    pass_service_prefix_from(&mut command, false, Some(runtime.path()));
    assert_eq!(
        set(&command),
        [
            (LAUNCH_AS_SERVICE.to_owned(), "1".to_owned()),
            (LAUNCH_PREFIX.to_owned(), SERVICE_PREFIX.to_owned())
        ]
    );
}

/// What the manager says changed about `unit`'s `interface`: `changed`
/// marshals each property, as [`Marshal::property`] does.
fn properties_changed(unit: &str, interface: &str, changed: impl FnOnce(&mut Marshal)) -> Vec<u8> {
    let mut body = Marshal::default();
    body.string(interface);
    body.array(8, changed);
    body.array(4, |_| {});
    signal(
        &object_path(unit),
        "org.freedesktop.DBus.Properties",
        "PropertiesChanged",
        "sa{sv}as",
        &body,
    )
}

fn active_state(unit: &str, state: &str) -> Vec<u8> {
    properties_changed(unit, UNIT, |m| m.property("ActiveState", "s", |m| m.string(state)))
}

/// The main process's state, as the manager signals it: `pid` while it runs,
/// 0 once it exited with `code` and `status`.
fn main_process(unit: &str, pid: u32, code: i32, status: i32) -> Vec<u8> {
    properties_changed(unit, SERVICE, |m| {
        m.property("MainPID", "u", |m| m.u32(pid));
        m.property("ExecMainCode", "i", |m| m.i32(code));
        m.property("ExecMainStatus", "i", |m| m.i32(status));
    })
}

#[test]
fn a_unit_ends_only_once_it_started_and_nothing_in_it_runs() {
    let mut unit = Unit::new("app-kotodama-x@1.service".into());
    unit.observe(&read_message(&active_state(&unit.name, "inactive")));
    assert!(!unit.ended, "a new unit is inactive before it starts");
    unit.observe(&read_message(&active_state(&unit.name, "activating")));
    unit.observe(&read_message(&main_process(&unit.name, 42, 0, 0)));
    assert_eq!((unit.main_pid, unit.main_exit), (Some(42), None));

    unit.observe(&read_message(&main_process(&unit.name, 0, CLD_EXITED, 3)));
    assert_eq!(unit.main_exit.and_then(|status| status.code()), Some(3));
    assert_eq!(unit.main_pid, None);
    assert!(!unit.ended, "what the program started may still run");

    unit.observe(&read_message(&active_state("app-kotodama-other@1.service", "failed")));
    assert!(!unit.ended, "another unit's end");
    unit.observe(&read_message(&active_state(&unit.name, "failed")));
    assert!(unit.ended);
}

#[test]
fn a_start_job_done_counts_as_started_whatever_order_the_changes_come_in() {
    let mut unit = Unit::new("app-kotodama-x@1.service".into());
    unit.observe(&read_message(&job_removed(&unit.name, "done")));
    unit.observe(&read_message(&active_state(&unit.name, "inactive")));
    assert!(unit.ended);
}

/// The unit a `StartTransientUnit` call in `call` asks for.
fn unit_asked(call: &[u8]) -> String {
    let at = call
        .windows(13)
        .position(|window| window == b"app-kotodama-")
        .expect("a unit name");
    let len = call[at..].iter().position(|&byte| byte == 0).unwrap();
    String::from_utf8(call[at..at + len].to_vec()).unwrap()
}

fn has(bytes: &[u8], text: &[u8]) -> bool {
    bytes.windows(text.len()).any(|window| window == text)
}

/// The `StartTransientUnit` call among `calls`.
fn start_call(calls: &[crate::dbus::tests::Received]) -> &crate::dbus::tests::Received {
    calls
        .iter()
        .find(|call| has(&call.bytes, b"StartTransientUnit"))
        .expect("a StartTransientUnit")
}

/// The manager's answer to a `Get` of its `environment`.
fn environment_answer(serial: u32, environment: &[String]) -> Vec<u8> {
    let mut body = Marshal::default();
    body.signature("as");
    body.array(4, |m| {
        for variable in environment {
            m.string(variable);
        }
    });
    answer(serial, "v", &body)
}

/// A user manager whose environment is `environment`, which answers a
/// `StartTransientUnit` with what `started` makes of the unit asked for, after
/// its return, and whose services' main process is 4242.
fn manager(
    environment: Vec<String>,
    started: impl Fn(&str) -> Vec<u8> + Send + 'static,
) -> (Bus, thread::JoinHandle<Vec<crate::dbus::tests::Received>>) {
    let (ours, theirs) = UnixStream::pair().unwrap();
    let peer = fd_peer(theirs, move |serial, call| {
        if has(call, b"StartTransientUnit") {
            let mut bytes = method_return(serial, "/org/freedesktop/systemd1/job/4");
            bytes.extend(started(&unit_asked(call)));
            bytes
        } else if has(call, b"MainPID") {
            let mut pid = Marshal::default();
            pid.signature("u");
            pid.u32(4242);
            answer(serial, "v", &pid)
        } else {
            environment_answer(serial, &environment)
        }
    });
    (bus_passing_fds(ours), peer)
}

/// A directory holding the executable `prog`, on no `PATH` but the fake
/// manager's.
fn bin() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("prog"), "#!/bin/sh\n").unwrap();
    fs::set_permissions(dir.path().join("prog"), fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

fn argv(args: &[&str]) -> Vec<String> {
    args.iter().map(|&arg| arg.to_owned()).collect()
}

/// A session's environment, whose `PATH` leads to `dir`.
fn session(dir: &Path) -> Vec<String> {
    vec![
        format!("PATH=/nonexistent:{}", dir.display()),
        "WAYLAND_DISPLAY=wayland-1".to_owned(),
        "LANG=pl_PL.UTF-8".to_owned(),
    ]
}

#[test]
fn a_program_starts_as_a_service_found_on_the_managers_path() {
    let bin = bin();
    let (manager, peer) = manager(session(bin.path()), |unit| job_removed(unit, "done"));
    let argv = argv(&["prog", "--flag"]);
    let program = Program {
        argv: &argv,
        dir: None,
        app_id: None,
        output: None,
    };

    let running = start(manager, &program).unwrap();
    assert_eq!(running.main_pid(), Some(4242), "asked for, as no signal named it");
    drop(running);
    let calls = peer.join().unwrap();

    assert!(has(&calls[0].bytes, b"Environment"), "asks for the environment first");
    let call = start_call(&calls);
    assert!(unit_asked(&call.bytes).starts_with("app-kotodama-prog@"));
    assert!(unit_asked(&call.bytes).ends_with(".service"));
    assert!(has(&call.bytes, bin.path().join("prog").to_str().unwrap().as_bytes()));
    for property in [
        "Type",
        "exec",
        "ExitType",
        "cgroup",
        "app.slice",
        "--flag",
        "ExecStartEx",
        "no-env-expand",
    ] {
        assert!(has(&call.bytes, property.as_bytes()), "{property}");
    }
    // The manager's own environment and directory are the program's.
    for property in ["Environment", "WorkingDirectory", "FileDescriptor"] {
        assert!(!has(&call.bytes, property.as_bytes()), "{property}");
    }
    assert!(call.fds.is_empty());
}

#[test]
fn a_program_starts_in_the_directory_its_action_names() {
    let bin = bin();
    let (manager, peer) = manager(session(bin.path()), |unit| job_removed(unit, "done"));
    let argv = argv(&["./prog"]);
    let program = Program {
        argv: &argv,
        dir: Some(bin.path()),
        app_id: Some("org.example.App"),
        output: None,
    };

    drop(start(manager, &program).unwrap());
    let calls = peer.join().unwrap();
    let call = start_call(&calls);

    assert!(unit_asked(&call.bytes).starts_with("app-kotodama-org.example.App@"));
    assert!(has(&call.bytes, b"WorkingDirectory"));
    assert!(has(&call.bytes, bin.path().join("./prog").to_str().unwrap().as_bytes()));
}

#[test]
fn a_program_not_on_the_managers_path_fails_before_anything_starts() {
    let (manager, peer) = manager(vec!["PATH=/nonexistent".to_owned()], |unit| job_removed(unit, "done"));
    let argv = argv(&["prog"]);
    let program = Program {
        argv: &argv,
        dir: None,
        app_id: None,
        output: None,
    };

    let Err(Error::Failed(err)) = start(manager, &program) else {
        panic!("started");
    };
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert_eq!(err.to_string(), "prog isn't installed");
    let calls = peer.join().unwrap();
    assert!(calls.iter().all(|call| !has(&call.bytes, b"StartTransientUnit")));
}

#[test]
fn a_program_the_manager_couldnt_run_fails_its_launch() {
    let bin = bin();
    let (manager, peer) = manager(session(bin.path()), |unit| job_removed(unit, "failed"));
    let argv = argv(&["prog"]);
    let program = Program {
        argv: &argv,
        dir: None,
        app_id: None,
        output: None,
    };

    let Err(Error::Failed(err)) = start(manager, &program) else {
        panic!("started");
    };
    assert_eq!(err.to_string(), "the user manager couldn't execute it");
    peer.join().unwrap();
}

#[test]
fn a_service_the_manager_refuses_starts_nothing() {
    let bin = bin();
    let environment = session(bin.path());
    let (ours, theirs) = UnixStream::pair().unwrap();
    let peer = fd_peer(theirs, move |serial, call| {
        if has(call, b"StartTransientUnit") {
            error(
                serial,
                "org.freedesktop.DBus.Error.InvalidArgs",
                "Unknown assignment ExitType",
            )
        } else {
            environment_answer(serial, &environment)
        }
    });
    let argv = argv(&["prog"]);
    let program = Program {
        argv: &argv,
        dir: None,
        app_id: None,
        output: None,
    };

    let refused = start(bus_passing_fds(ours), &program);
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
    peer.join().unwrap();
}

/// The call may have reached the manager, which may still start the service,
/// so the launch must not start the program another way.
#[test]
fn a_manager_that_answers_too_late_fails_the_launch_rather_than_refusing_it() {
    let bin = bin();
    let environment = session(bin.path());
    let (ours, theirs) = UnixStream::pair().unwrap();
    let peer = fd_peer(theirs, move |serial, call| {
        if has(call, b"StartTransientUnit") {
            thread::sleep(Duration::from_millis(2500));
            method_return(serial, "/org/freedesktop/systemd1/job/4")
        } else {
            environment_answer(serial, &environment)
        }
    });
    let argv = argv(&["prog"]);
    let program = Program {
        argv: &argv,
        dir: None,
        app_id: None,
        output: None,
    };

    let Err(Error::Failed(err)) = start(bus_passing_fds(ours), &program) else {
        panic!("not a failure");
    };
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    peer.join().unwrap();
}

#[test]
fn a_background_programs_output_goes_to_the_manager_and_its_exit_is_heard() {
    use std::os::fd::AsFd;

    let bin = bin();
    let (manager, peer) = manager(session(bin.path()), |unit| {
        let mut bytes = active_state(unit, "activating");
        bytes.extend(main_process(unit, 42, 0, 0));
        bytes.extend(job_removed(unit, "done"));
        bytes.extend(active_state(unit, "active"));
        bytes.extend(main_process(unit, 0, CLD_EXITED, 3));
        bytes
    });
    let (mut reader, writer) = io::pipe().unwrap();
    let argv = argv(&["prog"]);
    let program = Program {
        argv: &argv,
        dir: None,
        app_id: None,
        output: Some(writer.as_fd()),
    };

    let running = start(manager, &program).unwrap();
    assert_eq!(running.main_pid(), Some(42));
    assert_eq!(running.main_exit().and_then(|status| status.code()), Some(3));
    drop(writer);
    let calls = peer.join().unwrap();
    let call = calls
        .into_iter()
        .find(|call| has(&call.bytes, b"StartTransientUnit"))
        .unwrap();

    assert!(has(&call.bytes, b"StandardOutputFileDescriptor"));
    assert!(has(&call.bytes, b"StandardErrorFileDescriptor"));
    assert_eq!(call.fds.len(), 2);
    for fd in call.fds {
        fs::File::from(fd).write_all(b"line\n").unwrap();
    }
    let mut out = String::new();
    reader.read_to_string(&mut out).unwrap();
    assert_eq!(out, "line\nline\n");
}

#[test]
fn a_service_ends_once_nothing_in_it_runs_and_not_when_its_main_process_exits() {
    let bin = bin();
    let started = |unit: &str| {
        let mut bytes = active_state(unit, "active");
        bytes.extend(job_removed(unit, "done"));
        bytes.extend(main_process(unit, 0, CLD_EXITED, 0));
        bytes
    };
    let argv = argv(&["prog"]);
    let program = Program {
        argv: &argv,
        dir: None,
        app_id: None,
        output: None,
    };

    let (bus, peer) = manager(session(bin.path()), started);
    let running = start(bus, &program).unwrap();
    assert!(
        !running.ended_by(Instant::now() + Duration::from_millis(100)),
        "the app it left runs"
    );
    peer.join().unwrap();

    let (bus, peer) = manager(session(bin.path()), move |unit| {
        let mut bytes = started(unit);
        bytes.extend(active_state(unit, "inactive"));
        bytes
    });
    let running = start(bus, &program).unwrap();
    assert!(running.ended_by(Instant::now() + Duration::from_secs(2)));
    peer.join().unwrap();
}
