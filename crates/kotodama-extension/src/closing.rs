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
use std::time::Duration;
use std::time::Instant;

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
        let Some(pid) = entry.file_name().to_str().and_then(|pid| pid.parse::<u32>().ok()) else {
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
    file == name || file.strip_prefix('.').and_then(|file| file.strip_suffix("-wrapped")) == Some(name)
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
mod tests;
