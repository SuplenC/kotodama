//! The Kotodama Daemon. It owns the index and the ranking, runs the Extensions,
//! and serves Front ends on a Unix socket.

mod apps;
mod arguments;
pub mod clock;
pub mod config;
pub mod configured;
pub mod conflict;
pub mod daemon;
pub mod desktop;
pub mod discovery;
pub mod empty_query;
pub mod engine;
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
pub mod schema;
mod segment;
mod server;
pub mod usage;
mod views;

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::Sender;
use std::time::Duration;
use std::time::Instant;

use daemon::Daemon;
use daemon::Event;
use kotodama_extension::watch::RecursiveMode;
use kotodama_extension::watch::Watcher;
use paths::Overrides;
use paths::Paths;
use problem::Report;

/// How long the directories of programs that couldn't start must stay still
/// before they are tried again, so a build or an install that writes many
/// files is tried once, whole.
const PROGRAMS_SETTLE: Duration = Duration::from_millis(500);

/// Runs the Daemon with its config in `config_dirs` (see
/// [`paths::config_dirs`]) and the paths it sets, until it fails to start.
///
/// # Errors
///
/// When it can't start: a path it needs has no place, or it can't serve on
/// its socket.
pub fn start(config_dirs: Vec<PathBuf>) -> Result<(), String> {
    // The Daemon reports what is wrong with `[paths]` once it is there.
    let overrides = match config::load(&config_dirs) {
        Ok(config) => Overrides::from_config(&config, &mut Report::silent()),
        Err(_) => Overrides::default(),
    };
    run(&Paths::new(config_dirs, &overrides)?)
}

/// Runs the Daemon on `paths` until it fails to start. The Extension
/// directories follow `[paths]` as the config reloads; the other paths stay.
///
/// # Errors
///
/// When it can't serve on its socket.
pub fn run(paths: &Paths) -> Result<(), String> {
    let log_pinned = engine::log_from_env().is_some();
    let config = config::load(&paths.config_dirs);
    let started_with = match &config {
        Ok(config) => Overrides::from_config(config, &mut Report::silent()),
        Err(_) => Overrides::default(),
    };
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

    let listener = server::listen(&paths.socket).map_err(|err| format!("{}: {err}", paths.socket.display()))?;
    let (events_tx, events) = mpsc::channel();
    let _watcher = config::watch(paths.config_dirs.clone(), events_tx.clone())
        .map_err(|err| log::error!("not watching the config directory: {err}"))
        .ok();
    let mut _extension_watcher = watch_extensions(&paths.extension_dirs, &events_tx);
    // What `[paths]` said of the Extension directories when they were last
    // read.
    let mut extension_overrides = (started_with.extension_dirs.clone(), started_with.default_extension_dirs);
    let programs_watcher = watch_programs(events_tx.clone())
        .map_err(|err| log::error!("not watching for programs that couldn't start: {err}"))
        .ok();
    server::serve(listener, events_tx.clone());
    let indexed = events_tx.clone();
    let rediscover = events_tx.clone();

    let (config, unreadable) = match config {
        Ok(config) => (config, None),
        Err(err) => (kotodama_protocol::Table::default(), Some(err)),
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
    if !log_pinned {
        log::set_max_level(daemon.engine().log.unwrap_or(log::LevelFilter::Info));
    }
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
            Some(deadline) => match events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(event) => Some(event),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            },
            None => match events.recv() {
                Ok(event) => Some(event),
                Err(_) => return Ok(()),
            },
        };
        let now = Instant::now();
        if let Some(event) = event {
            let reloaded = matches!(event, Event::Config(_));
            daemon.handle(event, now);
            if reloaded {
                if !log_pinned {
                    log::set_max_level(daemon.engine().log.unwrap_or(log::LevelFilter::Info));
                }
                let now_says = daemon.paths();
                warn_of_restart_keys(&started_with, now_says);
                if (&now_says.extension_dirs, now_says.default_extension_dirs)
                    != (&extension_overrides.0, extension_overrides.1)
                {
                    extension_overrides = (now_says.extension_dirs.clone(), now_says.default_extension_dirs);
                    let dirs = paths::extension_dirs(&paths.config_dirs, now_says);
                    log::info!("reading Extensions from {}", joined(&dirs));
                    _extension_watcher = watch_extensions(&dirs, &rediscover);
                    daemon.handle(Event::Extensions(discovery::discover(&dirs)), now);
                }
            }
        }
        if daemon.next_wake().is_some_and(|deadline| deadline <= now) {
            daemon.tick(now);
        }
        if let (Some(watcher), Some(dirs)) = (&programs_watcher, daemon.failed_programs()) {
            watcher.watch(dirs);
        }
    }
}

/// Watches the Extension directories `dirs`, for as long as the watcher is
/// kept.
fn watch_extensions(dirs: &[PathBuf], events: &Sender<Event>) -> Option<Watcher> {
    discovery::watch(dirs.to_vec(), events.clone())
        .map_err(|err| log::error!("not watching the Extension directories: {err}"))
        .ok()
}

/// Warns that a `[paths]` key read only at start changed, so the user knows
/// it waits for a restart.
fn warn_of_restart_keys(started_with: &Overrides, now: &Overrides) {
    for (key, then, now) in [
        ("state_dir", &started_with.state_dir, &now.state_dir),
        ("cache_dir", &started_with.cache_dir, &now.cache_dir),
        ("socket", &started_with.socket, &now.socket),
    ] {
        if then != now {
            log::warn!("`[paths] {key}` changed; it takes effect when kotodamad restarts");
        }
    }
}

fn joined(dirs: &[PathBuf]) -> String {
    dirs.iter()
        .map(|dir| dir.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Prints the merged config in `config_dirs`, every default filled in and
/// each value's source, or with `schema` every key and what it does.
///
/// # Errors
///
/// When the config can't be loaded, its paths have no place, or stdout
/// can't be written.
pub fn print_config(config_dirs: Vec<PathBuf>, schema: bool) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    if schema {
        return schema::print_schema(&mut out).map_err(|err| err.to_string());
    }
    let (config, sources) = config::load_with_sources(&config_dirs).map_err(|err| err.to_string())?;
    let paths = Paths::new(config_dirs, &Overrides::from_config(&config, &mut Report::silent()))?;
    schema::print_config(&config, &sources, &paths, &mut out).map_err(|err| err.to_string())?;
    out.flush().map_err(|err| err.to_string())
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
