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

fn main() -> ExitCode {
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
