//! Telling whether a program the user launched closed soon after, as when
//! it was picked by mistake, and not just handed off to something else.
//!
//! # Owns
//!
//! Finding a copy of the program that ran before the launch, and waiting
//! for everything the launch started to end.
//!
//! # Not here
//!
//! Starting the program (`crate::launch`), its scope (`crate::scope`).

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

/// How often a launch is looked at again while what it started runs.
const POLL: Duration = Duration::from_millis(100);

/// Whether everything the launch started ended by `deadline`: its scope's
/// cgroup when it has one, else the process group the launched process
/// `group` leads. Call it once that process has been waited for.
pub(crate) fn ended_by(cgroup: Option<&Path>, group: u32, deadline: Instant) -> bool {
    loop {
        // Before looking, so what ended only after the deadline doesn't
        // count.
        let now = Instant::now();
        if now > deadline {
            return false;
        }
        let ended = match cgroup {
            Some(cgroup) => crate::scope::is_empty(cgroup),
            None => group_ended(group),
        };
        if ended {
            return true;
        }
        thread::sleep(POLL.min(deadline - now));
    }
}

/// Whether the user ran a copy of `program` that started before the
/// process `launched`: one a launch of a single-instance app hands off to,
/// or an app already open that a second launch only raised. Reads `/proc`,
/// so call it off the main loop.
#[cfg(target_os = "linux")]
pub(crate) fn ran_before(program: &str, launched: u32) -> bool {
    use std::os::unix::fs::MetadataExt;

    let name = file_name(program);
    let Some(since) = start_time(launched) else {
        return false;
    };
    // SAFETY: getuid can't fail and has no preconditions.
    let uid = unsafe { libc::getuid() };
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    entries.flatten().any(|entry| {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|pid| pid.parse::<u32>().ok())
        else {
            return false;
        };
        let dir = entry.path();
        pid != launched
            && entry.metadata().is_ok_and(|meta| meta.uid() == uid)
            && runs(&dir, name)
            && start_time(pid).is_some_and(|started| started < since)
    })
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn ran_before(_program: &str, _launched: u32) -> bool {
    false
}

/// Whether the process whose `/proc` directory is `dir` runs the program
/// named `name`: by the name it was started under, which a wrapper keeps
/// with `exec -a`, or by its executable's file name, which a wrapper may
/// have renamed `.<name>-wrapped`.
#[cfg(target_os = "linux")]
fn runs(dir: &Path, name: &str) -> bool {
    let started_as = std::fs::read(dir.join("cmdline")).ok().and_then(|cmdline| {
        let argv0 = cmdline.split(|&byte| byte == 0).next()?;
        Some(String::from_utf8_lossy(argv0).into_owned())
    });
    if started_as.is_some_and(|argv0| file_name(&argv0) == name) {
        return true;
    }
    std::fs::read_link(dir.join("exe")).is_ok_and(|exe| {
        exe.file_name()
            .and_then(|file| file.to_str())
            .is_some_and(|file| is_named(file, name))
    })
}

/// Whether an executable's file name `file` is the program `name`'s.
#[cfg(target_os = "linux")]
fn is_named(file: &str, name: &str) -> bool {
    file == name
        || file
            .strip_prefix('.')
            .and_then(|file| file.strip_suffix("-wrapped"))
            == Some(name)
}

#[cfg(target_os = "linux")]
fn file_name(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

/// When process `pid` started, in clock ticks since boot.
#[cfg(target_os = "linux")]
fn start_time(pid: u32) -> Option<u64> {
    stat_start_time(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// The start time in a `/proc/<pid>/stat` line: its 22nd field. The name,
/// the 2nd, is in parentheses and may hold spaces and parentheses itself.
#[cfg(target_os = "linux")]
fn stat_start_time(stat: &str) -> Option<u64> {
    let after_name = &stat[stat.rfind(')')? + 1..];
    after_name.split_whitespace().nth(19)?.parse().ok()
}

/// Whether no process is left in the process group `group`.
#[cfg(target_os = "linux")]
fn group_ended(group: u32) -> bool {
    let Ok(group) = libc::pid_t::try_from(group) else {
        return true;
    };
    // SAFETY: signal 0 only checks that the group exists.
    let sent = unsafe { libc::kill(-group, 0) };
    sent != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// Without a way to look, the process group counts as ended with the
/// process that led it.
#[cfg(not(target_os = "linux"))]
fn group_ended(_group: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
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
}
