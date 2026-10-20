//! Proposals: the catalog Actions the model picked, as Results to confirm.
//!
//! # Owns
//!
//! - Reading the model's answer, and checking every Proposal against the
//!   catalog.
//! - The Results: the Proposals, and the "Ask AI" Result.
//!
//! # Not here
//!
//! - Running a confirmed Proposal: the Daemon, from the `run_catalog_action`
//!   outcome [`Proposal::outcome`] builds.

use std::collections::BTreeMap;

use kotodama_protocol::Action;
use kotodama_protocol::CatalogEntry;
use kotodama_protocol::Fraction;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::LiveResult;
use kotodama_protocol::NamespacedKind;
use kotodama_protocol::Outcome;

use crate::ASK;
use crate::MAX_PROPOSALS;
use crate::NAME;
use crate::RUN;
use crate::request::TOOL;

/// An Argument value the model fills in is cut to this many characters: a
/// name or a version, never an essay.
const MAX_ARGUMENT: usize = 200;

/// Why an answer couldn't be read.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("the answer isn't JSON")]
    NotJson(#[source] serde_json::Error),
    #[error("the answer has no content")]
    NoContent,
}

/// A catalog Action the model proposed, with the Arguments it filled in.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub entry: CatalogEntry,
    pub arguments: BTreeMap<String, String>,
}

fn kind(name: &str) -> ItemKind {
    ItemKind::Namespaced(NamespacedKind::new(name).expect("the Extension's kinds are valid"))
}

impl Proposal {
    /// The Result that stands for it. Its one Action confirms it.
    #[must_use]
    pub fn item(&self, id: String) -> Item {
        let mut title = self.entry.action.title.clone();
        if !self.arguments.is_empty() {
            let filled: Vec<_> = self
                .arguments
                .iter()
                .map(|(name, value)| format!("{name}: {value}"))
                .collect();
            title = format!("{title} ({})", filled.join(", "));
        }
        Item {
            id,
            kind: kind("ai.proposal"),
            identity: None,
            title,
            subtitle: Some(format!(
                "Proposed by AI, from {}; Enter runs it",
                self.entry.extension
            )),
            icon: None,
            keyword: None,
            keywords: Vec::new(),
            actions: vec![Action::new(RUN, format!("Run {}", self.entry.action.title))],
            hidden: false,
        }
    }

    /// What confirming it answers: the Daemon runs the catalog Action.
    #[must_use]
    pub fn outcome(&self) -> Outcome {
        Outcome::RunCatalogAction {
            extension: self.entry.extension.clone(),
            kind: self.entry.kind.clone(),
            action: self.entry.action.id.clone(),
            arguments: self.arguments.clone(),
        }
    }
}

/// The Proposals as Live results, best first.
#[must_use]
pub fn live_results(proposals: &[Proposal], request: u64) -> Vec<LiveResult> {
    let step = 1.0 / (MAX_PROPOSALS as f32 * 2.0);
    proposals
        .iter()
        .enumerate()
        .map(|(n, proposal)| LiveResult {
            item: proposal.item(proposal_id(request, n)),
            score: Fraction::new(1.0 - n as f32 * step).expect("between 0.5 and 1"),
            answer: false,
        })
        .collect()
}

/// A Proposal's Item id: the request it answered and its place there.
#[must_use]
pub fn proposal_id(request: u64, n: usize) -> String {
    format!("{request}-{n}")
}

/// The request and place a Proposal's Item id names.
#[must_use]
pub fn parse_proposal_id(id: &str) -> Option<(u64, usize)> {
    let (request, n) = id.split_once('-')?;
    Some((request.parse().ok()?, n.parse().ok()?))
}

/// The "Ask AI" Result for `query`, whose id is the query.
#[must_use]
pub fn ask_result(query: &str) -> LiveResult {
    LiveResult {
        item: Item {
            id: query.to_owned(),
            kind: kind("ai.ask"),
            identity: None,
            title: format!("Ask AI: “{query}”"),
            subtitle: Some("Proposes Actions for you to confirm".into()),
            icon: None,
            keyword: None,
            keywords: Vec::new(),
            actions: vec![Action::new(ASK, "Ask AI")],
            hidden: false,
        },
        score: Fraction::new(0.0).expect("0 is a fraction"),
        answer: false,
    }
}

/// The Arguments `proposed` fills in that `entry`'s Action takes.
fn arguments(entry: &CatalogEntry, proposed: &serde_json::Value) -> BTreeMap<String, String> {
    let mut arguments = BTreeMap::new();
    for argument in proposed["arguments"].as_array().into_iter().flatten() {
        let (Some(name), Some(value)) = (argument["name"].as_str(), argument["value"].as_str())
        else {
            continue;
        };
        let value = value.trim();
        let takes = entry.action.arguments.iter().any(|a| a.name == name);
        if takes && !value.is_empty() {
            arguments.insert(name.to_owned(), value.chars().take(MAX_ARGUMENT).collect());
        }
    }
    arguments
}

/// Reads the model's answer: the Proposals in its `propose_actions` call,
/// checked against the catalog. An Action not in the catalog is dropped, and
/// so is an Argument the Action doesn't take. A refusal proposes nothing.
///
/// # Errors
///
/// When the answer isn't a Messages API response.
pub fn parse_response(json: &[u8], catalog: &[CatalogEntry]) -> Result<Vec<Proposal>, ParseError> {
    let response: serde_json::Value = serde_json::from_slice(json).map_err(ParseError::NotJson)?;
    if response["stop_reason"] == "refusal" {
        return Ok(Vec::new());
    }
    let Some(content) = response["content"].as_array() else {
        return Err(ParseError::NoContent);
    };
    let proposed = content
        .iter()
        .filter(|block| block["type"] == "tool_use" && block["name"] == TOOL)
        .filter_map(|call| call["input"]["proposals"].as_array())
        .flatten();
    let mut proposals = Vec::new();
    for proposed in proposed {
        let entry = proposed["action"]
            .as_u64()
            .and_then(|n| catalog.get(usize::try_from(n).ok()?));
        let Some(entry) = entry.filter(|entry| entry.extension != NAME) else {
            continue;
        };
        let proposal = Proposal {
            entry: entry.clone(),
            arguments: arguments(entry, proposed),
        };
        if !proposals.contains(&proposal) && proposals.len() < MAX_PROPOSALS {
            proposals.push(proposal);
        }
    }
    Ok(proposals)
}

#[cfg(test)]
mod tests;
