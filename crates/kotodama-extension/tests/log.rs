//! Where `kotodama_extension::log!` goes: to stderr in an Extension, to the
//! `log` logger in a program that installed one, such as the Daemon.

use std::sync::Mutex;

static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        RECORDS
            .lock()
            .unwrap()
            .push((record.level(), record.args().to_string()));
    }

    fn flush(&self) {}
}

// One test, as the logger is global: first as an Extension, then as the Daemon.
#[test]
fn warns_on_stderr_without_a_logger_and_through_log_with_one() {
    let mut stderr = Vec::new();
    kotodama_extension::warn(format_args!("won't reap {}: busy", "foot"), &mut stderr);
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        format!("{}: won't reap foot: busy\n", kotodama_extension::name())
    );
    assert!(RECORDS.lock().unwrap().is_empty());

    log::set_logger(&Capture).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    let mut stderr = Vec::new();
    kotodama_extension::warn(format_args!("won't reap {}: busy", "foot"), &mut stderr);
    assert!(stderr.is_empty());
    assert_eq!(
        *RECORDS.lock().unwrap(),
        [(log::Level::Warn, "won't reap foot: busy".to_owned())]
    );
}
