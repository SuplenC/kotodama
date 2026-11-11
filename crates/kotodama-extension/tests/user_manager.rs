//! Programs started as services of the real systemd user manager. Where
//! there is none, as on CI, each test passes without looking; the NixOS VM
//! test runs the Daemon under one.

use std::io::PipeReader;
use std::io::Read;
use std::os::fd::AsFd;
use std::time::Duration;
use std::time::Instant;

use kotodama_extension::dbus::Bus;
use kotodama_extension::service;
use kotodama_extension::service::Program;

/// The user manager, or `None` where there is none to test against.
fn user_manager() -> Option<Bus> {
    match Bus::user_manager() {
        Ok(manager) => Some(manager),
        Err(err) if err.is_absent() => None,
        Err(err) => panic!("{err}"),
    }
}

fn sh(script: &str) -> Vec<String> {
    vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()]
}

fn read_all(mut reader: PipeReader) -> String {
    let mut out = String::new();
    reader.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn a_program_runs_as_a_service_of_its_own_in_the_sessions_environment_not_ours() {
    let Some(manager) = user_manager() else {
        return;
    };
    // Cargo sets it for the tests it runs, and no session has it.
    let ours = std::env::var("CARGO_MANIFEST_DIR").is_ok();
    let argv = sh("pwd; echo \"${CARGO_MANIFEST_DIR-unset}\"; grep '^0::' /proc/self/cgroup; exit 3");
    let (reader, writer) = std::io::pipe().unwrap();
    let program = Program {
        argv: &argv,
        dir: None,
        app_id: Some("kotodama-test"),
        output: Some(writer.as_fd()),
    };

    let running = service::start(manager, &program).unwrap();
    drop(writer);
    let status = running.main_exit();
    let out = read_all(reader);

    assert_eq!(status.and_then(|status| status.code()), Some(3), "{out}");
    let lines: Vec<&str> = out.lines().collect();
    let home = std::env::home_dir().unwrap();
    assert_eq!(lines[0], home.to_str().unwrap(), "starts in the home directory");
    if ours {
        assert_eq!(lines[1], "unset", "inherited the test's environment");
    }
    assert!(
        lines[2].contains("/app.slice/app-kotodama-kotodama\\x2dtest@") && lines[2].ends_with(".service"),
        "{}",
        lines[2]
    );
}

#[test]
fn a_service_ends_once_everything_it_started_is_gone() {
    let Some(manager) = user_manager() else {
        return;
    };
    // A launcher that leaves its app running a moment longer, and exits.
    let argv = sh("sleep 0.3 & exit 0");
    let program = Program {
        argv: &argv,
        dir: None,
        app_id: None,
        output: None,
    };

    let running = service::start(manager, &program).unwrap();
    assert!(!running.ended_by(Instant::now() + Duration::from_millis(100)));

    let running = service::start(user_manager().unwrap(), &program).unwrap();
    assert!(running.ended_by(Instant::now() + Duration::from_secs(5)));
}
