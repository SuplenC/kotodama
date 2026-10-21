//! The AI Bundled extension's parts that need no Daemon. What goes to the
//! model is the user's query and the Action catalog, never anything else:
//! [`request_body`] builds the whole request from those two.
//!
//! # Owns
//!
//! - The `[extensions.ai]` settings ([`Settings`]).
//! - The API key, read from `api_key_command` and kept in memory ([`Secret`]).
//! - The request to the model and its `curl` ([`request_body`], [`post`]).
//! - Reading the model's answer into proposals, and their Results ([`parse_response`],
//!   [`Proposal`]).
//!
//! # Not here
//!
//! - The protocol loop, debouncing and cancelling: `main.rs`.
//! - Running a confirmed Proposal's Action: the Daemon, through `Outcome::RunCatalogAction`.

#![forbid(unsafe_code)]

mod key;
mod proposal;
mod request;
mod settings;

pub use key::KeyError;
pub use key::Secret;
pub use key::read_key;
pub use proposal::ParseError;
pub use proposal::Proposal;
pub use proposal::ask_result;
pub use proposal::live_results;
pub use proposal::parse_proposal_id;
pub use proposal::parse_response;
pub use proposal::proposal_id;
pub use request::Failure;
pub use request::InFlight;
pub use request::curl_config;
pub use request::post;
pub use request::request_body;
pub use request::user_message;
pub use settings::DEFAULT_DEBOUNCE;
pub use settings::DEFAULT_EFFORT;
pub use settings::DEFAULT_MODEL;
pub use settings::DEFAULT_URL;
pub use settings::Settings;

/// The Extension's name, as its Manifest gives it.
pub const NAME: &str = "ai";
/// The "Ask AI" Result's Action.
pub const ASK: &str = "ask";
/// A Proposal's Action: the user confirms it, and the Daemon runs the
/// catalog Action it stands for.
pub const RUN: &str = "run";
/// The View the "Ask AI" Result opens, which shows the Proposals once they come.
pub const ANSWER_VIEW: &str = "answer";
/// At most this many proposals for one query: more would push the real
/// matches off screen, and the model ranks the best first.
pub const MAX_PROPOSALS: usize = 5;
