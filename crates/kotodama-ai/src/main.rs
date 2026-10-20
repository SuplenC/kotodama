//! The AI Bundled extension: asks a model which Actions from the Action
//! catalog do what the user asked, and offers each as a Result for the user
//! to confirm. It never runs anything itself: confirming a Proposal answers
//! with `run_catalog_action`, and the Daemon runs the Action.
//!
//! It asks the model only for a query after its `?` prefix, once the query
//! has been still for `debounce_ms`, or when the user picks the "Ask AI" Result
//! a fallback query shows. Typing alone never reaches the model, and a query
//! that changes kills the request in flight. Off until configured: with
//! neither `api_key_command` nor `url`, it adds no Results and makes no network
//! calls.
//!
//! ```toml
//! [extensions.ai]
//! # Prints the API key; its output is never logged or written to disk.
//! api_key_command = ["pass", "show", "anthropic"]
//! # The Messages API endpoint; a local one (Ollama) may need no key.
//! url = "https://api.anthropic.com/v1/messages"
//! model = "claude-haiku-5-5"
//! # `output_config.effort`; "" leaves it out.
//! effort = "low"
//! debounce_ms = 600
//! ```
//!
//! # Owns
//!
//! - The protocol loop: the debounce of `?` queries, the one request in
//!   flight and its cancelling, the answer View, and confirming Proposals.
//!
//! # Not here
//!
//! - Settings, the key, the request and reading the answer: the library.

#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::error::Error as _;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Instant;

use kotodama_ai::ANSWER_VIEW;
use kotodama_ai::ASK;
use kotodama_ai::Failure;
use kotodama_ai::InFlight;
use kotodama_ai::Proposal;
use kotodama_ai::RUN;
use kotodama_ai::Secret;
use kotodama_ai::Settings;
use kotodama_ai::ask_result;
use kotodama_ai::live_results;
use kotodama_ai::parse_proposal_id;
use kotodama_ai::parse_response;
use kotodama_ai::post;
use kotodama_ai::proposal_id;
use kotodama_ai::read_key;
use kotodama_ai::request_body;
use kotodama_extension::Connection;
use kotodama_extension::Event;
use kotodama_extension::STACK_SIZE;
use kotodama_extension::Writer;
use kotodama_protocol::CatalogEntry;
use kotodama_protocol::FromExtension;
use kotodama_protocol::LiveResult;
use kotodama_protocol::Outcome;
use kotodama_protocol::ToExtension;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;

/// The curl the requests go through. A package may build in its path.
const CURL: &str = match option_env!("KOTODAMA_CURL") {
    Some(curl) => curl,
    None => "curl",
};

/// How many answers are kept for the user to confirm from.
const KEPT_ANSWERS: usize = 4;

type Error = kotodama_protocol::Error;

fn main() -> ExitCode {
    kotodama_extension::main("kotodama-ai", run)
}

fn run() -> Result<(), Error> {
    let Connection {
        writer,
        events,
        received,
    } = kotodama_extension::connect()?;
    let mut extension = Extension {
        writer,
        events,
        settings: Settings::default(),
        key: KeyCache::default(),
        catalog: Arc::new(Vec::new()),
        asked_catalog: false,
        pending: None,
        request: None,
        next_request: 1,
        answers: VecDeque::new(),
        view_open: false,
    };
    loop {
        let event = match extension.pending.as_ref().map(|pending| pending.due) {
            Some(due) => match received.recv_timeout(due.saturating_duration_since(Instant::now()))
            {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => {
                    extension.ask_pending();
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match received.recv() {
                Ok(event) => event,
                Err(_) => break,
            },
        };
        match event {
            Event::Message(message) => extension.message(message)?,
            Event::Own(answered) => extension.answered(answered)?,
            Event::Hangup => break,
        }
    }
    Ok(())
}

/// A request finished, on its own thread.
struct Answered {
    request: u64,
    result: Result<Vec<Proposal>, Failure>,
}

/// The key, read by the first request that needs it and kept in memory only.
/// Requests share it under the lock, so two at once run `api_key_command`
/// once, and a new config starts a fresh one that no older request can fill.
type KeyCache = Arc<Mutex<Option<Secret>>>;

/// Who waits for a request's answer.
#[derive(Clone, Debug, PartialEq)]
enum Purpose {
    /// The `?` Live query with this id.
    Live(u64),
    /// The "Ask AI" Result's View.
    View,
}

struct Request {
    id: u64,
    purpose: Purpose,
    in_flight: Arc<Mutex<InFlight>>,
}

/// A `?` query waiting out the debounce.
struct Pending {
    live: u64,
    query: String,
    due: Instant,
}

struct Extension {
    writer: Writer,
    events: Sender<Event<Answered>>,
    settings: Settings,
    key: KeyCache,
    catalog: Arc<Vec<CatalogEntry>>,
    asked_catalog: bool,
    pending: Option<Pending>,
    /// The one request in flight; a newer one cancels it.
    request: Option<Request>,
    next_request: u64,
    /// The last answers, by request, which the user confirms proposals from.
    answers: VecDeque<(u64, Vec<Proposal>)>,
    view_open: bool,
}

impl Extension {
    fn message(&mut self, message: ToExtension) -> Result<(), Error> {
        match message {
            ToExtension::Config(table) => {
                self.settings = Settings::from_config(&table);
                // The command, the endpoint or the model may have changed, so
                // nothing asked under the old settings answers, and no key
                // read under them is kept.
                self.key = KeyCache::default();
                self.cancel();
                if !self.settings.enabled() {
                    self.pending = None;
                } else if !self.asked_catalog {
                    self.asked_catalog = true;
                    self.writer.send(&FromExtension::RequestActionCatalog)?;
                }
                Ok(())
            }
            ToExtension::ActionCatalog { entries } => {
                self.catalog = Arc::new(entries);
                Ok(())
            }
            ToExtension::LiveQuery { id, query, prefix } => {
                let query = query.trim();
                if !self.settings.enabled() || query.is_empty() {
                    return self.send_live(id, Vec::new());
                }
                match prefix {
                    // Asked for: once the query is still, it goes to the model.
                    Some(_) => {
                        self.pending = Some(Pending {
                            live: id,
                            query: query.to_owned(),
                            due: Instant::now() + self.settings.debounce,
                        });
                        Ok(())
                    }
                    // A fallback query: only the Result that asks, if picked.
                    None => self.send_live(id, vec![ask_result(query)]),
                }
            }
            ToExtension::CancelLiveQuery { id } => {
                if self.pending.as_ref().is_some_and(|p| p.live == id) {
                    self.pending = None;
                }
                if self
                    .request
                    .as_ref()
                    .is_some_and(|r| r.purpose == Purpose::Live(id))
                {
                    self.cancel();
                }
                Ok(())
            }
            ToExtension::ViewPopped { view } if view == ANSWER_VIEW => {
                self.view_open = false;
                if self
                    .request
                    .as_ref()
                    .is_some_and(|r| r.purpose == Purpose::View)
                {
                    self.cancel();
                }
                Ok(())
            }
            ToExtension::RunAction {
                invocation,
                action,
                item,
                ..
            } => {
                let outcome = match action.as_str() {
                    ASK => self.ask_in_view(&item.id)?,
                    RUN => self.confirm(&item.id)?,
                    _ => Outcome::Failed {
                        message: format!("ai has no Action `{action}`"),
                    },
                };
                self.writer.send(&FromExtension::ActionDone {
                    invocation,
                    outcome,
                })
            }
            _ => Ok(()),
        }
    }

    fn send_live(&mut self, id: u64, results: Vec<LiveResult>) -> Result<(), Error> {
        self.writer.send(&FromExtension::LiveResults {
            live_query: id,
            results,
            done: true,
        })
    }

    /// Sends the `?` query that waited out the debounce to the model.
    fn ask_pending(&mut self) {
        if let Some(Pending { live, query, .. }) = self.pending.take() {
            self.start(Purpose::Live(live), &query);
        }
    }

    /// Opens the View that waits for the Proposals for `query`.
    fn ask_in_view(&mut self, query: &str) -> Result<Outcome, Error> {
        if !self.settings.enabled() {
            return Ok(Outcome::Failed {
                message: "the AI isn't configured".into(),
            });
        }
        self.writer.send(&FromExtension::PushView(View {
            id: ANSWER_VIEW.into(),
            title: format!("AI: {query}"),
            content: detail(format!("Asking which Actions do “{query}”…")),
        }))?;
        self.view_open = true;
        self.start(Purpose::View, query);
        Ok(Outcome::KeepOpen)
    }

    /// Answers the user's confirmation of a Proposal with its catalog Action,
    /// which the Daemon runs.
    fn confirm(&mut self, id: &str) -> Result<Outcome, Error> {
        let found = parse_proposal_id(id).and_then(|(request, n)| {
            self.answers
                .iter()
                .find(|(answered, _)| *answered == request)
                .and_then(|(_, proposals)| proposals.get(n))
        });
        let Some(proposal) = found else {
            return Ok(Outcome::Failed {
                message: "that proposal is gone; ask again".into(),
            });
        };
        let outcome = proposal.outcome();
        // An open View shows again with the menu, over whatever ran.
        if self.view_open {
            self.view_open = false;
            self.writer.send(&FromExtension::PopView {
                view: ANSWER_VIEW.into(),
            })?;
        }
        Ok(outcome)
    }

    fn cancel(&mut self) {
        if let Some(request) = self.request.take() {
            request
                .in_flight
                .lock()
                .expect("request state mutex poisoned")
                .cancel();
        }
    }

    /// Sends `query` and the catalog to the model, on a thread of its own,
    /// which reads the key first if it isn't known yet.
    fn start(&mut self, purpose: Purpose, query: &str) {
        self.cancel();
        let id = self.next_request;
        self.next_request += 1;
        let in_flight = Arc::new(Mutex::new(InFlight::default()));
        self.request = Some(Request {
            id,
            purpose,
            in_flight: Arc::clone(&in_flight),
        });
        let events = self.events.clone();
        if self.catalog.is_empty() {
            let _ = events.send(Event::Own(Answered {
                request: id,
                result: Ok(Vec::new()),
            }));
            return;
        }
        let body = request_body(&self.settings, query, &self.catalog);
        let catalog = Arc::clone(&self.catalog);
        let url = self.settings.url.clone();
        let cache = Arc::clone(&self.key);
        let command = self.settings.api_key_command.clone();
        let spawned = thread::Builder::new()
            .name("ask".into())
            .stack_size(STACK_SIZE)
            .spawn(move || {
                let result = ask(&cache, command.as_deref(), &url, &body, &in_flight)
                    .and_then(|answer| parse_response(&answer, &catalog).map_err(Failure::Answer));
                let _ = events.send(Event::Own(Answered {
                    request: id,
                    result,
                }));
            });
        if spawned.is_err() {
            kotodama_extension::log!("couldn't start a request");
            self.request = None;
        }
    }

    fn answered(&mut self, answered: Answered) -> Result<(), Error> {
        let Answered { request, result } = answered;
        let Some(current) = self.request.take_if(|current| current.id == request) else {
            return Ok(());
        };
        let (proposals, failure) = match result {
            Ok(proposals) => (proposals, None),
            Err(Failure::Cancelled) => return Ok(()),
            Err(failure) => {
                let message = report(&failure);
                kotodama_extension::log!("{message}");
                (Vec::new(), Some(message))
            }
        };
        let sent = match current.purpose {
            Purpose::Live(live) => self.send_live(live, live_results(&proposals, request)),
            Purpose::View if self.view_open => {
                let content = match failure {
                    Some(message) => detail(message),
                    None if proposals.is_empty() => {
                        detail("No Action in the catalog does that.".into())
                    }
                    None => ViewContent::List {
                        items: proposals
                            .iter()
                            .enumerate()
                            .map(|(n, proposal)| proposal.item(proposal_id(request, n)))
                            .collect(),
                        filters_itself: false,
                    },
                };
                self.writer.send(&FromExtension::UpdateView(View {
                    id: ANSWER_VIEW.into(),
                    title: "AI proposes".into(),
                    content,
                }))
            }
            Purpose::View => Ok(()),
        };
        // An answer with nothing to confirm would only push out one the user
        // may still confirm from.
        if !proposals.is_empty() {
            if self.answers.len() == KEPT_ANSWERS {
                self.answers.pop_front();
            }
            self.answers.push_back((request, proposals));
        }
        sent
    }
}

/// Posts `body` with the key, which it reads first if no request has yet.
/// Runs on a request's own thread.
fn ask(
    cache: &Mutex<Option<Secret>>,
    command: Option<&[String]>,
    url: &str,
    body: &str,
    in_flight: &Mutex<InFlight>,
) -> Result<Vec<u8>, Failure> {
    let key = {
        let mut cached = cache.lock().expect("key cache mutex poisoned");
        if let (None, Some(command)) = (cached.as_ref(), command) {
            *cached = Some(read_key(command).map_err(Failure::Key)?);
        }
        // `None` here is a local endpoint that takes no key.
        cached.clone()
    };
    let posted = post(CURL, url, key.as_ref(), body, in_flight);
    if matches!(posted, Err(Failure::Unauthorized)) {
        // The key may have been rotated: the next request reads it again.
        *cache.lock().expect("key cache mutex poisoned") = None;
    }
    posted
}

fn detail(markdown: String) -> ViewContent {
    ViewContent::Detail {
        markdown,
        metadata: Vec::new(),
    }
}

/// `failure` and its causes, on one line.
fn report(failure: &Failure) -> String {
    let mut message = failure.to_string();
    let mut cause = failure.source();
    while let Some(err) = cause {
        message.push_str(&format!(": {err}"));
        cause = err.source();
    }
    message
}
