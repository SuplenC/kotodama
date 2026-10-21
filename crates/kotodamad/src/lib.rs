//! The Kotodama Daemon. It owns the index and the ranking, runs the Extensions,
//! and serves Front ends on a Unix socket.

mod apps;
mod arguments;
pub mod clock;
pub mod config;
pub mod conflict;
pub mod daemon;
pub mod desktop;
pub mod discovery;
pub mod empty_query;
pub mod history;
mod host;
mod index;
mod indexer;
pub mod keywords;
pub mod kind_order;
mod link;
mod most_used;
mod narrowing;
mod notify;
mod open_view;
pub mod paths;
mod payload;
mod pick;
pub mod problem;
pub mod rank;
mod reserved;
mod segment;
mod server;
pub mod usage;
mod views;

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use daemon::{Daemon, Event};
use kotodama_extension::watch::{RecursiveMode, Watcher};
use paths::Paths;

/// How long the directories of programs that couldn't start must stay still
/// before they are tried again, so a build or an install that writes many
/// files is tried once, whole.
const PROGRAMS_SETTLE: Duration = Duration::from_millis(500);

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
    let config = config::load(&paths.config_dirs);
    let discovered = discovery::discover(&paths.extension_dirs);
    let found = discovered.found.clone();
    log::info!(
        "found {} extension(s): {}",
        found.len(),
        found
            .iter()
            .map(|found| found.manifest.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let listener = server::listen(&paths.socket)
        .map_err(|err| format!("{}: {err}", paths.socket.display()))?;
    let (events_tx, events) = mpsc::channel();
    let _watcher = config::watch(paths.config_dirs.clone(), events_tx.clone())
        .map_err(|err| log::error!("not watching the config directory: {err}"))
        .ok();
    let _extension_watcher = discovery::watch(paths.extension_dirs.clone(), events_tx.clone())
        .map_err(|err| log::error!("not watching the Extension directories: {err}"))
        .ok();
    let programs_watcher = watch_programs(events_tx.clone())
        .map_err(|err| log::error!("not watching for programs that couldn't start: {err}"))
        .ok();
    server::serve(listener, events_tx.clone());
    let indexed = events_tx.clone();

    let (config, unreadable) = match config {
        Ok(config) => (config, None),
        Err(err) => (Default::default(), Some(err)),
    };
    let mut daemon = Daemon::new(
        found,
        config,
        history::load(&paths.history),
        host::Processes::new(events_tx),
        desktop::Wayland::new(),
        history::Disk::new(paths.history.clone(), paths.usage_log.clone()),
        Instant::now(),
    );
    daemon.keep_items_in(&paths.cache);
    daemon.index_in_background(indexed);
    // What discovery and the config found wrong, now that there is a
    // Daemon to tell.
    daemon.handle(Event::Extensions(discovered), Instant::now());
    if let Some(err) = unreadable {
        daemon.handle(Event::ConfigFailed(err), Instant::now());
    }
    loop {
        let event = match daemon.next_wake() {
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
        if daemon.next_wake().is_some_and(|deadline| deadline <= now) {
            daemon.tick(now);
        }
        if let (Some(watcher), Some(dirs)) = (&programs_watcher, daemon.failed_programs()) {
            watcher.watch(dirs);
        }
    }
}

/// A watcher, watching nothing until told, that sends
/// [`Event::ProgramsChanged`] when a file changes in a directory it watches:
/// those of the programs of Extensions that couldn't start, so one starts
/// once it is installed or rebuilt.
fn watch_programs(events: Sender<Event>) -> kotodama_extension::watch::notify::Result<Watcher> {
    Watcher::start(
        "programs",
        RecursiveMode::NonRecursive,
        PROGRAMS_SETTLE,
        |event, _| !event.kind.is_access(),
        move |_| {
            let _ = events.send(Event::ProgramsChanged);
        },
    )
}
