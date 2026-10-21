//! A bare Front end for trying the Daemon from a terminal: it prints the Results
//! for a query, and with `--run` runs the top Result's primary Action, or with
//! `--action <id>` its Action `id`, answering the Daemon's Close as a real menu
//! would. With `--app` first, the query's first word names an app, which it
//! enters first, as `kotodama open --app` does.
//!
//! ```sh
//! cargo run -p kotodamad --example front-end -- firefox
//! cargo run -p kotodamad --example front-end -- --run firefox
//! cargo run -p kotodamad --example front-end -- --action close firefox
//! cargo run -p kotodamad --example front-end -- --app --run emoji thumbs
//! ```

use std::collections::BTreeMap;
use std::env;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use kotodama_protocol::ActionRef;
use kotodama_protocol::Channel;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::Hello;
use kotodama_protocol::ResultRef;
use kotodama_protocol::ToFrontEnd;
use kotodama_protocol::WireFormat;

/// What to do once the Results are printed.
enum Then {
    Stop,
    /// Run the top Result's primary Action.
    RunPrimary,
    /// Run the top Result's Action of this id.
    Run(String),
}

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let app = args.first().is_some_and(|arg| arg == "--app");
    if app {
        args.remove(0);
    }
    let mut then = Then::Stop;
    if args.first().is_some_and(|arg| arg == "--run") {
        args.remove(0);
        then = Then::RunPrimary;
    } else if args.first().is_some_and(|arg| arg == "--action") && args.len() > 1 {
        args.remove(0);
        then = Then::Run(args.remove(0));
    }
    let query = args.join(" ");
    match front_end(&query, app, then) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("front-end: {err}");
            ExitCode::FAILURE
        }
    }
}

fn front_end(query: &str, app: bool, then: Then) -> Result<(), Box<dyn std::error::Error>> {
    let socket =
        PathBuf::from(env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR isn't set")?).join("kotodama.sock");
    let stream = UnixStream::connect(&socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut channel = Channel::handshake(stream.try_clone()?, stream, &Hello::new(&WireFormat::ALL))?;

    if app {
        let word = query.split_whitespace().next().unwrap_or_default();
        channel.send(&FromFrontEnd::Open {
            query: format!("{word} "),
            view: None,
        })?;
    }
    channel.send(&FromFrontEnd::Query { text: query.into() })?;
    let (shown, results) = loop {
        match channel.recv()?.ok_or("the Daemon hung up")? {
            ToFrontEnd::Results { version, results, .. } => break (version, results),
            ToFrontEnd::Status { down, .. } if !down.is_empty() => {
                eprintln!("down: {}", down.join(", "));
            }
            _ => {}
        }
    };
    for row in &results {
        println!("{}\t{}\t{}", row.extension, row.item.id, row.item.title);
    }
    let action = match then {
        Then::Stop => return Ok(()),
        Then::RunPrimary => None,
        Then::Run(id) => Some(id),
    };

    let top = results.first().ok_or("nothing to run")?;
    let action = match action {
        Some(id) => top
            .item
            .actions
            .iter()
            .find(|action| action.id == id)
            .ok_or_else(|| format!("the top Result has no Action `{id}`"))?,
        None => &top.item.actions[0],
    };
    channel.send(&FromFrontEnd::RunAction {
        result: ResultRef {
            extension: top.extension.clone(),
            id: top.item.id.clone(),
        },
        shown,
        action: ActionRef {
            extension: top.extension.clone(),
            id: action.id.clone(),
        },
        arguments: BTreeMap::default(),
    })?;
    loop {
        match channel.recv()?.ok_or("the Daemon hung up")? {
            ToFrontEnd::Close => {
                channel.send(&FromFrontEnd::Closed)?;
                return Ok(());
            }
            ToFrontEnd::ActionFailed { message } => return Err(message.into()),
            _ => {}
        }
    }
}
