//! The web search Bundled extension, a Fallback (see `manifest.toml`): the
//! Daemon asks it what real matches leave rows empty for, and what looks
//! like an address. It answers with a search of each engine, which the
//! Daemon ranks below every real match, and, for an address, a Result
//! opening it, in the top row. Each opens with `xdg-open`, as a Quicklink
//! does.
//!
//! ```toml
//! [extensions.web-search]
//! # Searched in this order. `{query}` is the query, percent-encoded, as in a
//! # Quicklink. Unset: DuckDuckGo, Google and Wikipedia. Empty: addresses only.
//! engines = [
//!     { name = "DuckDuckGo", url = "https://duckduckgo.com/?q={query}" },
//!     { name = "YouTube", url = "https://www.youtube.com/results?search_query={query}" },
//! ]
//! ```

use std::process::ExitCode;

use kotodama_extension::{Connection, Event, launch};
use kotodama_protocol::{FromExtension, Outcome, ToExtension};
use kotodama_web_search::{Engine, command, default_engines, results};

fn main() -> ExitCode {
    kotodama_extension::main("kotodama-web-search", run)
}

fn run() -> Result<(), kotodama_protocol::Error> {
    let Connection {
        mut writer,
        received,
        ..
    } = kotodama_extension::connect::<()>()?;
    let mut engines: Vec<Engine> = default_engines();
    for event in received {
        match event {
            Event::Message(ToExtension::Config(table)) => {
                engines = kotodama_web_search::engines(&table);
            }
            Event::Message(ToExtension::LiveQuery { id, query, .. }) => {
                writer.send(&FromExtension::LiveResults {
                    live_query: id,
                    results: results(&query, &engines),
                    done: true,
                })?;
            }
            Event::Message(ToExtension::RunAction {
                invocation,
                action,
                item,
                ..
            }) => {
                let outcome = match command(&action, &item) {
                    Some(argv) => match launch::launch(&argv, None) {
                        Ok(()) => Outcome::Close,
                        Err(err) => Outcome::Failed {
                            message: err.to_string(),
                        },
                    },
                    None => Outcome::Failed {
                        message: format!("web-search has no Action `{action}`"),
                    },
                };
                writer.send(&FromExtension::ActionDone {
                    invocation,
                    outcome,
                })?;
            }
            Event::Message(_) | Event::Own(()) => {}
            Event::Hangup => break,
        }
    }
    Ok(())
}
