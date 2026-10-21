//! The `kotodama` command, for a compositor keybind: it asks the Daemon to open
//! or toggle the menu, and the Daemon passes that on to the Front ends.
//!
//! ```sh
//! kotodama toggle
//! kotodama open --query "fir"
//! kotodama open --view clipboard/history
//! kotodama open --prefix emoji
//! ```
//!
//! `--view <extension>/<item>` opens the menu on the View that Item's primary
//! Action pushes. `--prefix <prefix>` opens it behind an Extension's Prefix,
//! searching that Extension alone, as `--query "<prefix> "` does.

#![forbid(unsafe_code)]

use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::time::Duration;

use kotodama_protocol::Channel;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::Hello;
use kotodama_protocol::WireFormat;
use kotodama_protocol::socket_path;

const USAGE: &str = "usage: kotodama toggle
       kotodama open [--view <extension>/<item>] [--prefix <prefix>] [--query <text>]";

/// Under socket activation the first keypress also starts the Daemon, so the
/// handshake may wait for it to come up.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let message = match parse(&args) {
        Ok(message) => message,
        Err(err) => {
            eprintln!("kotodama: {err}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match send(&message) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("kotodama: {err}");
            ExitCode::FAILURE
        }
    }
}

fn parse(args: &[String]) -> Result<FromFrontEnd, String> {
    let (command, mut rest) = args.split_first().ok_or("no command given")?;
    match command.as_str() {
        "toggle" if rest.is_empty() => Ok(FromFrontEnd::Toggle),
        "open" => {
            let (mut query, mut view, mut prefix) = (String::new(), None, None);
            while let [flag, value, tail @ ..] = rest {
                match flag.as_str() {
                    "--query" => query.clone_from(value),
                    "--view" => view = Some(value.clone()),
                    "--prefix" => prefix = Some(value.as_str()),
                    _ => return Err(format!("unknown option {flag}")),
                }
                rest = tail;
            }
            if let [extra] = rest {
                return Err(format!("{extra} needs a value, or isn't an option"));
            }
            if let Some(prefix) = prefix {
                query = format!("{prefix} {query}");
            }
            Ok(FromFrontEnd::Open { query, view })
        }
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            std::process::exit(0);
        }
        _ => Err(format!("unknown command {}", args.join(" "))),
    }
}

fn send(message: &FromFrontEnd) -> Result<(), Box<dyn std::error::Error>> {
    let socket = socket_path().ok_or("XDG_RUNTIME_DIR isn't set")?;
    let stream =
        UnixStream::connect(&socket).map_err(|err| format!("{}: {err}; is kotodamad running?", socket.display()))?;
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let mut channel = Channel::handshake(stream.try_clone()?, stream, &Hello::new(&WireFormat::ALL))?;
    channel.send(message)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split(' ').map(String::from).collect()
    }

    #[test]
    fn open_takes_a_query_and_a_view_in_any_order() {
        assert_eq!(
            parse(&args("open --view clipboard --query fir")),
            Ok(FromFrontEnd::Open {
                query: "fir".into(),
                view: Some("clipboard".into()),
            })
        );
        assert_eq!(
            parse(&args("open")),
            Ok(FromFrontEnd::Open {
                query: String::new(),
                view: None,
            })
        );
    }

    #[test]
    fn open_behind_a_prefix_types_it_before_the_query() {
        assert_eq!(
            parse(&args("open --prefix emoji")),
            Ok(FromFrontEnd::Open {
                query: "emoji ".into(),
                view: None,
            })
        );
        assert_eq!(
            parse(&args("open --query fire --prefix emoji")),
            Ok(FromFrontEnd::Open {
                query: "emoji fire".into(),
                view: None,
            })
        );
    }

    #[test]
    fn a_flag_without_a_value_or_an_unknown_one_is_an_error() {
        assert!(parse(&args("open --query")).is_err());
        assert!(parse(&args("open --size 3")).is_err());
        assert!(parse(&args("toggle now")).is_err());
        assert!(parse(&[]).is_err());
    }
}
