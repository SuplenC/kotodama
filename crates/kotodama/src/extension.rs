//! `kotodama extension`, for whoever writes an Extension. `check` runs one against a stand-in
//! for the Daemon and says in plain words what the Daemon would refuse; `new` writes a small
//! Extension that passes `check`, to start from.
//!
//! ```sh
//! kotodama extension check ./text-case --query "hello world"
//! kotodama extension new text-case --lang python
//! ```
//!
//! # Owns
//!
//! - The `extension` command line, its usage and exit codes.
//!
//! # Not here
//!
//! - The check: `extension/check.rs`. The scaffolds: `extension/scaffold.rs`.
//! - The commands for a compositor keybind: `main.rs`.

mod check;
mod scaffold;

use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: kotodama extension check <dir> [--query <text>] [--run <item id>]
       kotodama extension new <name> [--lang shell|python|rust] [--dir <parent dir>]";

/// What `kotodama extension` was asked to do.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Check(check::Options),
    New(scaffold::Options),
    Help,
}

/// Why the command line is not one `kotodama extension` understands. It is printed above the
/// usage.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
enum ParseError {
    #[error("no command given")]
    NoCommand,
    #[error("unknown command {0}")]
    UnknownCommand(String),
    #[error("{0} needs {1}")]
    Missing(&'static str, &'static str),
    #[error("unknown option {0}")]
    UnknownOption(String),
    /// The last argument, left without a value.
    #[error("{0} needs a value, or isn't an option")]
    NoValue(String),
    #[error("unknown language {0}; pick shell, python or rust")]
    UnknownLanguage(String),
}

/// Runs `kotodama extension` with the arguments after `extension`.
pub(crate) fn run(args: &[String]) -> ExitCode {
    let command = match parse(args) {
        Ok(command) => command,
        Err(err) => {
            eprintln!("kotodama extension: {err}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Help => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Command::Check(options) => {
            if check::check(&options, &mut io::stdout().lock()) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::New(options) => match scaffold::write(&options) {
            Ok(dir) => {
                println!("{}", scaffold::next_steps(&options, &dir));
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("kotodama extension new: {err}");
                ExitCode::FAILURE
            }
        },
    }
}

fn parse(args: &[String]) -> Result<Command, ParseError> {
    let (command, rest) = args.split_first().ok_or(ParseError::NoCommand)?;
    match command.as_str() {
        "check" => {
            let (dir, flags) = rest
                .split_first()
                .ok_or(ParseError::Missing("check", "the Extension's directory"))?;
            let mut options = check::Options::new(dir);
            for (flag, value) in pairs(flags)? {
                match flag {
                    "--query" => options.query = Some(value.to_owned()),
                    "--run" => options.run = Some(value.to_owned()),
                    _ => return Err(ParseError::UnknownOption(flag.to_owned())),
                }
            }
            Ok(Command::Check(options))
        }
        "new" => {
            let (name, flags) = rest
                .split_first()
                .ok_or(ParseError::Missing("new", "the Extension's name"))?;
            let mut options = scaffold::Options::new(name);
            for (flag, value) in pairs(flags)? {
                match flag {
                    "--lang" => {
                        options.language = value
                            .parse()
                            .map_err(|()| ParseError::UnknownLanguage(value.to_owned()))?;
                    }
                    "--dir" => options.parent = PathBuf::from(value),
                    _ => return Err(ParseError::UnknownOption(flag.to_owned())),
                }
            }
            Ok(Command::New(options))
        }
        "-h" | "--help" | "help" => Ok(Command::Help),
        _ => Err(ParseError::UnknownCommand(args.join(" "))),
    }
}

/// The flags after the positional argument, each with its value.
fn pairs(flags: &[String]) -> Result<Vec<(&str, &str)>, ParseError> {
    let (pairs, rest) = flags.as_chunks::<2>();
    match rest {
        [extra] => Err(ParseError::NoValue(extra.clone())),
        _ => Ok(pairs
            .iter()
            .map(|[flag, value]| (flag.as_str(), value.as_str()))
            .collect()),
    }
}

#[cfg(test)]
mod tests;
