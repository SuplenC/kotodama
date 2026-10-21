//! The request to the model, and the `curl` that sends it.
//!
//! # Owns
//!
//! - The request body: fixed instructions and tool, plus the user's query and the Action catalog,
//!   and nothing else.
//! - Running `curl` with the key and body on its stdin, and cancelling it.
//!
//! # Not here
//!
//! - Reading the answer: `proposal.rs`.
//! - When a request starts or is cancelled: `main.rs`.

use std::io;
use std::io::Read;
use std::io::Write;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::sync::Mutex;

use kotodama_protocol::CatalogEntry;
use serde_json::json;

use crate::KeyError;
use crate::NAME;
use crate::ParseError;
use crate::Secret;
use crate::Settings;

/// The tool the model answers through.
pub(crate) const TOOL: &str = "propose_actions";
/// The longest the model may take, in seconds: past it the user has moved on.
const TIMEOUT_SECS: &str = "30";
/// Room for adaptive thinking at low effort, and a handful of proposals.
const MAX_TOKENS: u32 = 2048;
/// An answer is a few proposals; anything bigger is not one.
const MAX_ANSWER_BYTES: &str = "1000000";

const SYSTEM: &str = "You work inside Kotodama, a command menu. The user typed a request. \
The catalog lists every Action that exists, each with its number `n`, title, the Extension \
that offers it, the kind of Item it runs on, and its Arguments. Call `propose_actions` once \
with the catalog Actions that carry out the request, in the order to run them, at most 5. \
Fill in an Action's Arguments only from what the request says; leave out the rest, and the \
user is asked for them. Never make up an Action that isn't in the catalog. If none fits, \
propose none. The user confirms each proposal before anything runs.";

/// The catalog as the model sees it: entries numbered by position, never this
/// Extension's own.
fn catalog_json(catalog: &[CatalogEntry]) -> Vec<serde_json::Value> {
    catalog
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.extension != NAME)
        .map(|(n, entry)| {
            let arguments: Vec<_> = entry
                .action
                .arguments
                .iter()
                .map(|argument| {
                    json!({
                        "name": argument.name,
                        "placeholder": argument.placeholder,
                        "optional": argument.optional,
                    })
                })
                .collect();
            json!({
                "n": n,
                "title": entry.action.title,
                "extension": entry.extension,
                "kind": entry.kind.as_str(),
                "arguments": arguments,
            })
        })
        .collect()
}

/// The user's message: the query and the catalog, and nothing else.
#[must_use]
pub fn user_message(query: &str, catalog: &[CatalogEntry]) -> String {
    json!({ "query": query, "catalog": catalog_json(catalog) }).to_string()
}

/// The whole request for `query`. Besides the fixed instructions and tool, it
/// holds only [`user_message`].
#[must_use]
pub fn request_body(settings: &Settings, query: &str, catalog: &[CatalogEntry]) -> String {
    let argument = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "value": { "type": "string" },
        },
        "required": ["name", "value"],
        "additionalProperties": false,
    });
    let proposal = json!({
        "type": "object",
        "properties": {
            "action": { "type": "integer", "description": "The Action's `n` in the catalog" },
            "arguments": { "type": "array", "items": argument },
        },
        "required": ["action", "arguments"],
        "additionalProperties": false,
    });
    let mut body = json!({
        "model": settings.model,
        "max_tokens": MAX_TOKENS,
        "system": SYSTEM,
        "tools": [{
            "name": TOOL,
            "description": "Proposes catalog Actions for the user to confirm, best first. \
                            An empty list proposes nothing.",
            "strict": true,
            "input_schema": {
                "type": "object",
                "properties": { "proposals": { "type": "array", "items": proposal } },
                "required": ["proposals"],
                "additionalProperties": false,
            },
        }],
        "tool_choice": { "type": "auto" },
        "messages": [{ "role": "user", "content": user_message(query, catalog) }],
    });
    if let Some(effort) = &settings.effort {
        body["output_config"] = json!({ "effort": effort });
    }
    body.to_string()
}

/// Escapes `text` as a double-quoted value of a curl config file.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The curl config the request goes in on stdin: the headers, with the key,
/// and the body. Neither is ever in curl's arguments, which any process on the
/// machine can read.
#[must_use]
pub fn curl_config(key: Option<&Secret>, body: &str) -> String {
    let mut config = String::with_capacity(body.len() + 256);
    if let Some(key) = key {
        let header = format!("x-api-key: {}", key.expose());
        config.push_str(&format!("header = {}\n", quoted(&header)));
    }
    config.push_str("header = \"anthropic-version: 2023-06-01\"\n");
    config.push_str("header = \"content-type: application/json\"\n");
    config.push_str(&format!("data-binary = {}\n", quoted(body)));
    config
}

/// Why a request got no proposals.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    #[error("the request was cancelled")]
    Cancelled,
    /// The endpoint refused the key, which is read again next time.
    #[error("the endpoint refused the API key")]
    Unauthorized,
    #[error("reading the API key")]
    Key(#[source] KeyError),
    #[error("can't run {program}")]
    Start {
        program: String,
        #[source]
        source: io::Error,
    },
    /// What curl said on stderr, which never holds the key or the body.
    #[error("the model's endpoint didn't answer: {0}")]
    Endpoint(String),
    #[error("reading the model's answer")]
    Answer(#[source] ParseError),
}

/// A request's `curl`, which a cancel kills.
#[derive(Debug, Default)]
pub struct InFlight {
    cancelled: bool,
    child: Option<Child>,
}

impl InFlight {
    /// Stops the request: kills its `curl`, or keeps it from starting.
    pub fn cancel(&mut self) {
        self.cancelled = true;
        if let Some(child) = &mut self.child {
            let _ = child.kill();
        }
    }
}

/// Posts `body` to `url` with `curl`, so the Extension carries no TLS stack
/// of its own, and returns the answer. Blocks, so it runs off the main
/// thread; `in_flight` lets another thread cancel it.
///
/// # Errors
///
/// When cancelled, when `curl` can't start, or when the endpoint fails or
/// refuses the key.
pub fn post(
    curl: &str,
    url: &str,
    key: Option<&Secret>,
    body: &str,
    in_flight: &Mutex<InFlight>,
) -> Result<Vec<u8>, Failure> {
    let (mut stdin, mut stdout, mut stderr) = {
        let mut state = in_flight.lock().expect("request state mutex poisoned");
        if state.cancelled {
            return Err(Failure::Cancelled);
        }
        let mut child = Command::new(curl)
            .args(["--silent", "--show-error", "--fail", "--max-time", TIMEOUT_SECS])
            .args(["--max-filesize", MAX_ANSWER_BYTES, "--config", "-", "--", url])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| Failure::Start {
                program: curl.to_owned(),
                source,
            })?;
        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            return Err(Failure::Start {
                program: curl.to_owned(),
                source: io::Error::other("its stdio wasn't piped"),
            });
        };
        state.child = Some(child);
        (stdin, stdout, stderr)
    };
    // curl reads its whole config before it connects, so this can't block on
    // curl waiting for its stdout to be read.
    let _ = stdin.write_all(curl_config(key, body).as_bytes());
    drop(stdin);
    let mut answer = Vec::new();
    let _ = stdout.read_to_end(&mut answer);
    let mut error = String::new();
    let _ = stderr.read_to_string(&mut error);

    let mut state = in_flight.lock().expect("request state mutex poisoned");
    let status = state.child.take().map(|mut child| child.wait());
    if state.cancelled {
        return Err(Failure::Cancelled);
    }
    match status {
        Some(Ok(status)) if status.success() => Ok(answer),
        _ if error.contains(" 401") || error.contains(" 403") => Err(Failure::Unauthorized),
        _ => Err(Failure::Endpoint(error.trim().to_owned())),
    }
}

#[cfg(test)]
mod tests;
