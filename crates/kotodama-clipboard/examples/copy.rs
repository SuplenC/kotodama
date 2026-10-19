//! Copies a file to the Wayland clipboard, offered as each MIME type given,
//! and serves it until killed. Unlike `wl-copy`, it offers several types at
//! once, so it can copy text the way a password manager does:
//!
//! ```sh
//! printf hunter2 >secret
//! cargo run -p kotodama-clipboard --example copy -- secret \
//!     'text/plain;charset=utf-8' x-kde-passwordManagerHint
//! ```

use std::process::ExitCode;
use std::thread;

use kotodama_clipboard::wayland::Clipboard;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: copy FILE MIME...");
        return ExitCode::FAILURE;
    };
    let mimes: Vec<String> = args.collect();
    let data = match std::fs::read(&path) {
        Ok(data) => data,
        Err(err) => {
            eprintln!("copy: {path}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let clipboard = match Clipboard::watch(|_| {}) {
        Ok(clipboard) => clipboard,
        Err(err) => {
            eprintln!("copy: {err}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = clipboard.set(mimes, data) {
        eprintln!("copy: {err}");
        return ExitCode::FAILURE;
    }
    eprintln!("copy: copied");
    loop {
        thread::park();
    }
}
