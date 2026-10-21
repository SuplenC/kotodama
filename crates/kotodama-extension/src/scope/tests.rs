use super::*;

#[test]
fn a_scope_is_named_for_its_app_escaped_and_its_process() {
    assert_eq!(unit_name("firefox", 42), "app-kotodama-firefox-42.scope");
    assert_eq!(
        unit_name("org.gnome.TextEditor", 3),
        "app-kotodama-org.gnome.TextEditor-3.scope"
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

#[test]
fn an_app_is_named_by_its_desktop_id_else_by_its_programs_file_name() {
    let firefox = Command::new("/usr/bin/firefox");

    assert_eq!(app_name(&firefox, Some("org.mozilla.firefox")), "org.mozilla.firefox");
    assert_eq!(app_name(&firefox, None), "firefox");
    assert_eq!(app_name(&Command::new("foot"), None), "foot");
}

/// The variables `command` sets.
#[cfg(target_os = "linux")]
fn set(command: &Command) -> Vec<(String, String)> {
    command
        .get_envs()
        .filter_map(|(name, value)| Some((name.to_str()?.to_owned(), value?.to_str()?.to_owned())))
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
    let _manager = std::os::unix::net::UnixListener::bind(runtime.path().join("systemd/private")).unwrap();
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
    use std::process::Command;
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use std::time::Instant;

    use super::super::linux::spawn_scoped;
    use super::unit_name;
    use crate::dbus::tests::bus;
    use crate::dbus::tests::job_removed;
    use crate::dbus::tests::method_return;
    use crate::dbus::tests::peer;

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
    fn manager(delay: Duration, result: &'static str) -> (UnixStream, thread::JoinHandle<Vec<u8>>) {
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
        let launched = spawn_scoped(&mut echo_pid(), None, bus(ours), move |err| {
            let _ = errors.send(err.to_string());
        })
        .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(300));
        assert!(launched.cgroup.is_some(), "the scope's cgroup is known");
        let child = launched.child;
        let pid = child.id();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), pid.to_string());

        let call = peer.join().unwrap();
        assert_eq!(unit_asked(&call), unit_name("sh", pid));
        let has = |text: &[u8]| call.windows(text.len()).any(|window| window == text);
        assert!(has(b"StartTransientUnit"));
        assert!(has(b"app.slice"));
        assert!(has(&pid.to_le_bytes()));
        assert!(unscoped.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn an_app_launched_by_desktop_id_gets_a_scope_named_for_it() {
        let (ours, peer) = manager(Duration::ZERO, "done");

        let launched = spawn_scoped(&mut echo_pid(), Some("org.example.Sleeper"), bus(ours), |_| {}).unwrap();
        let pid = launched.child.id();
        assert!(launched.child.wait_with_output().unwrap().status.success());

        let call = peer.join().unwrap();
        assert_eq!(
            unit_asked(&call),
            format!("app-kotodama-org.example.Sleeper-{pid}.scope")
        );
    }

    #[test]
    fn a_program_whose_scope_failed_still_runs_and_says_why() {
        let (ours, peer) = manager(Duration::ZERO, "failed");
        let (errors, unscoped) = mpsc::channel();
        let child = spawn_scoped(&mut echo_pid(), None, bus(ours), move |err| {
            let _ = errors.send(err.to_string());
        })
        .unwrap()
        .child;
        assert!(child.wait_with_output().unwrap().status.success());
        peer.join().unwrap();
        let err = unscoped.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(err.ends_with(": failed"), "{err}");
    }

    #[test]
    fn a_program_that_cant_start_fails_without_waiting_forever() {
        let (ours, _peer) = manager(Duration::ZERO, "done");
        let started = spawn_scoped(&mut Command::new("/nonexistent/program"), None, bus(ours), |_| {});
        assert!(started.is_err());
    }
}
