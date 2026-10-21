#[cfg(target_os = "linux")]
use super::*;

#[cfg(target_os = "linux")]
#[test]
fn a_wrapped_executable_is_the_program_it_wraps() {
    assert!(is_named("firefox", "firefox"));
    assert!(is_named(".firefox-wrapped", "firefox"));
    assert!(!is_named("firefox-bin", "firefox"));
    assert!(!is_named(".firefox", "firefox"));
}

#[cfg(target_os = "linux")]
#[test]
fn the_start_time_is_read_past_a_name_with_spaces_and_parentheses() {
    let stat = "42 (a (b) c) S 1 42 42 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 98765 1000 10";
    assert_eq!(stat_start_time(stat), Some(98765));
    assert_eq!(stat_start_time("42 (short) S 1"), None);
}

#[test]
fn an_empty_or_vanished_cgroup_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert!(crate::scope::is_empty(&dir.path().join("gone")));
    std::fs::write(dir.path().join("cgroup.procs"), "").unwrap();
    assert!(crate::scope::is_empty(dir.path()));
    std::fs::write(dir.path().join("cgroup.procs"), "1234\n").unwrap();
    assert!(!crate::scope::is_empty(dir.path()));
}

#[cfg(target_os = "linux")]
#[test]
fn a_launch_ends_once_its_process_group_does() {
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    // A launcher that leaves the app running in its group, and exits.
    let mut launcher = Command::new("sh")
        .args(["-c", "sleep 1 & exit 0"])
        .process_group(0)
        .spawn()
        .unwrap();
    let group = launcher.id();
    launcher.wait().unwrap();
    let started = Instant::now();
    assert!(!ended_by(None, group, started + Duration::from_millis(300)));
    assert!(ended_by(None, group, started + Duration::from_secs(5)));
}

#[cfg(target_os = "linux")]
#[test]
fn a_copy_started_before_the_launch_ran_before_it() {
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    let program = "kotodama-ran-before-test";
    let mut copy = Command::new("sh")
        .arg0(program)
        .args(["-c", "sleep 3; :"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // Start times count in clock ticks, of 10 ms.
    thread::sleep(Duration::from_millis(50));
    let mut launched = Command::new("sh")
        .arg0(format!("/some/dir/{program}"))
        .args(["-c", "sleep 3; :"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    assert!(ran_before(program, launched.id()));
    assert!(!ran_before(program, copy.id()), "the copy ran first");
    assert!(!ran_before("kotodama-never-ran", launched.id()));
    for child in [&mut copy, &mut launched] {
        child.kill().unwrap();
        child.wait().unwrap();
    }
}
