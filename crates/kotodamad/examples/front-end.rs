//! A bare Front end for trying the Daemon from a terminal: it prints the Results
//! for a query, and with `--run` runs the top Result's primary Action, answering
//! the Daemon's Close as a real menu would.
//!
//! ```sh
//! cargo run -p kotodamad --example front-end -- firefox
//! cargo run -p kotodamad --example front-end -- --run firefox
//! ```

use std::env;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use kotodama_protocol::{
    ActionRef, Channel, FromFrontEnd, Hello, ResultRef, ToFrontEnd, WireFormat,
};

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let run = args.first().is_some_and(|arg| arg == "--run");
    if run {
        args.remove(0);
    }
    let query = args.join(" ");
    match front_end(&query, run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("front-end: {err}");
            ExitCode::FAILURE
        }
    }
}

fn front_end(query: &str, run: bool) -> Result<(), Box<dyn std::error::Error>> {
    let socket = PathBuf::from(env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR isn't set")?)
        .join("kotodama.sock");
    let stream = UnixStream::connect(&socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut channel =
        Channel::handshake(stream.try_clone()?, stream, &Hello::new(&WireFormat::ALL))?;

    channel.send(&FromFrontEnd::Query { text: query.into() })?;
    let results = loop {
        match channel.recv()?.ok_or("the Daemon hung up")? {
            ToFrontEnd::Results { results } => break results,
            ToFrontEnd::Status { down } if !down.is_empty() => {
                eprintln!("down: {}", down.join(", "))
            }
            _ => {}
        }
    };
    for row in &results {
        println!("{}\t{}\t{}", row.extension, row.item.id, row.item.title);
    }
    if !run {
        return Ok(());
    }

    let top = results.first().ok_or("nothing to run")?;
    channel.send(&FromFrontEnd::RunAction {
        result: ResultRef {
            extension: top.extension.clone(),
            id: top.item.id.clone(),
        },
        action: ActionRef {
            extension: top.extension.clone(),
            id: top.item.actions[0].id.clone(),
        },
        arguments: Default::default(),
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
