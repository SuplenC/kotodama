//! `kotodama config [--schema]`: the merged config with every default filled in and where each
//! value came from, or every key with its default and what it does. `kotodamad config` prints
//! them, since the Daemon reads the config: this command runs it in its own place, so the
//! command a keybind runs carries no config reading of its own.
//!
//! # Owns
//!
//! - Finding `kotodamad` and handing the command over to it.
//!
//! # Not here
//!
//! - Loading and printing the config: `kotodamad/src/schema.rs`.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitCode;

/// Runs `kotodama config` with `args`, what followed `config`: becomes `kotodamad config`, or
/// says why it couldn't.
pub fn main(args: &[String]) -> ExitCode {
    let program = daemon();
    let err = Command::new(&program).arg("config").args(args).exec();
    eprintln!("kotodama: couldn't run {}: {err}", program.display());
    ExitCode::from(2)
}

/// `kotodamad` beside this program, as a package installs them, else on `PATH`.
fn daemon() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("kotodamad"))
        .filter(|daemon| daemon.is_file())
        .unwrap_or_else(|| PathBuf::from("kotodamad"))
}
