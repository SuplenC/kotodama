use std::process::ExitCode;

use kotodamad::paths::Paths;

/// Logs to stderr, which systemd's journal picks up. `KOTODAMA_LOG=debug`
/// shows more.
struct Stderr;

impl log::Log for Stderr {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("kotodamad: {}: {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

/// Keeps resident memory down. Each thread that allocates would get its own
/// glibc malloc arena, and with transparent huge pages set to `always` each
/// arena costs a 2 MiB page however little it holds. The Daemon's threads mostly
/// wait on I/O, so one arena costs no speed.
fn lean_memory() {
    #[cfg(target_os = "linux")]
    // SAFETY: both calls only set process-wide flags, before any thread starts.
    unsafe {
        libc::prctl(libc::PR_SET_THP_DISABLE, 1, 0, 0, 0);
        #[cfg(target_env = "gnu")]
        libc::mallopt(libc::M_ARENA_MAX, 1);
    }
}

fn main() -> ExitCode {
    lean_memory();
    let level = match std::env::var("KOTODAMA_LOG").as_deref() {
        Ok("debug") => log::LevelFilter::Debug,
        Ok("warn") => log::LevelFilter::Warn,
        _ => log::LevelFilter::Info,
    };
    let _ = log::set_logger(&Stderr);
    log::set_max_level(level);

    match Paths::from_env().and_then(|paths| kotodamad::run(&paths)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            log::error!("{err}");
            ExitCode::FAILURE
        }
    }
}
