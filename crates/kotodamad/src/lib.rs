//! The Kotodama Daemon. It owns the index and the ranking, runs the Extensions,
//! and serves Front ends on a Unix socket.

pub mod config;
pub mod daemon;
pub mod desktop;
pub mod discovery;
mod host;
mod index;
mod link;
pub mod paths;
mod server;

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Instant;

use daemon::Daemon;
use paths::Paths;

/// Runs the Daemon until it fails to start.
pub fn run(paths: &Paths) -> Result<(), String> {
    // The user's directories are created so they can be watched from the
    // start: a directory that appears later would go unwatched until a restart.
    for dir in [paths.config_dirs.last(), paths.extension_dirs.last()]
        .into_iter()
        .flatten()
    {
        if let Err(err) = std::fs::create_dir_all(dir) {
            log::warn!("{}: {err}", dir.display());
        }
    }
    let config = config::load(&paths.config_dirs).unwrap_or_else(|err| {
        log::error!("starting with an empty config: {err}");
        Default::default()
    });
    let found = discovery::discover(&paths.extension_dirs);
    log::info!(
        "found {} extension(s): {}",
        found.len(),
        found
            .iter()
            .map(|found| found.manifest.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let listener =
        server::bind(&paths.socket).map_err(|err| format!("{}: {err}", paths.socket.display()))?;
    let (events_tx, events) = mpsc::channel();
    let _watcher = config::watch(paths.config_dirs.clone(), events_tx.clone())
        .map_err(|err| log::error!("not watching the config directory: {err}"))
        .ok();
    let _extension_watcher = discovery::watch(paths.extension_dirs.clone(), events_tx.clone())
        .map_err(|err| log::error!("not watching the Extension directories: {err}"))
        .ok();
    server::serve(listener, events_tx.clone());

    let mut daemon = Daemon::new(
        found,
        config,
        host::Processes::new(events_tx),
        desktop::Wayland::new(),
        Instant::now(),
    );
    loop {
        let event = match daemon.next_deadline() {
            Some(deadline) => {
                match events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(event) => Some(event),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return Ok(()),
                }
            }
            None => match events.recv() {
                Ok(event) => Some(event),
                Err(_) => return Ok(()),
            },
        };
        let now = Instant::now();
        if let Some(event) = event {
            daemon.handle(event, now);
        }
        if daemon
            .next_deadline()
            .is_some_and(|deadline| deadline <= now)
        {
            daemon.tick(now);
        }
    }
}
