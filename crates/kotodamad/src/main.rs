use std::path::PathBuf;
use std::process::ExitCode;

use kotodamad::engine;
use kotodamad::paths;

const USAGE: &str = "usage: kotodamad [--config <dir>]
       kotodamad [--config <dir>] config [--schema]

  --config <dir>  read the config from <dir> alone (or KOTODAMA_CONFIG_DIR)
  config          print the merged config, every default filled in, and where each value came from
  --schema        print every key of the config with its default and what it does";

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

/// What the command line asks for.
enum Command {
    Run,
    Config { schema: bool },
}

/// The command line, without the program's name: the config directory it
/// names, and what to do.
fn parse(args: &[String]) -> Option<(Option<PathBuf>, Command)> {
    let mut config_dir = None;
    let mut rest = args;
    if let [flag, dir, after @ ..] = rest
        && flag == "--config"
    {
        config_dir = Some(PathBuf::from(dir));
        rest = after;
    }
    let command = match rest {
        [] => Command::Run,
        [config] if config == "config" => Command::Config { schema: false },
        [config, schema] if config == "config" && schema == "--schema" => {
            Command::Config { schema: true }
        }
        _ => return None,
    };
    Some((config_dir, command))
}

fn main() -> ExitCode {
    // The Daemon's threads mostly wait on I/O, so one malloc arena costs no
    // speed.
    kotodama_extension::lean_memory();
    let pinned = engine::log_from_env();
    let _ = log::set_logger(&Stderr);
    log::set_max_level(pinned.unwrap_or(log::LevelFilter::Info));

    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.as_slice(), [help] if help == "--help" || help == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let Some((config_dir, command)) = parse(&args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let result = paths::config_dirs(config_dir).and_then(|config_dirs| match command {
        Command::Run => kotodamad::start(config_dirs),
        Command::Config { schema } => kotodamad::print_config(config_dirs, schema),
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            log::error!("{err}");
            ExitCode::FAILURE
        }
    }
}
