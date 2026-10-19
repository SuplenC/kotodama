//! The `manifest.toml` that declares an Extension.

use serde::Deserialize;

use crate::{Action, Argument, Error, KindName, is_valid_name};

/// What an Extension is, how to start it, and what it contributes.
///
/// ```toml
/// name = "kubaos"
/// command = ["bin/kubaos-kotodama"]
/// action_catalog = true
///
/// [live_query]
/// prefixes = ["suite "]
/// debounce_ms = 120
///
/// [[contributed_actions]]
/// kind = "project"
/// id = "pin-version"
/// title = "Pin a version here"
/// ```
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "ManifestToml")]
pub struct Manifest {
    /// Names the Extension everywhere: its config table is `[extensions.<name>]`
    /// and its own Item kinds are `<name>.<kind>`.
    pub name: String,
    pub description: Option<String>,
    /// The program and its arguments. A relative program path is resolved
    /// against the Extension's directory. It speaks the protocol on stdio.
    pub command: Vec<String>,
    /// Present if the Extension answers Live queries.
    pub live_query: Option<LiveQueries>,
    /// Actions this Extension offers on other Extensions' Items, by Item kind.
    pub contributed_actions: Vec<ContributedAction>,
    /// Whether the Extension may read the Action catalog.
    pub action_catalog: bool,
}

impl Manifest {
    pub fn from_toml(toml: &str) -> Result<Self, Error> {
        toml::from_str(toml).map_err(|err| Error::Manifest(err.to_string()))
    }
}

/// Which queries are sent to the Extension as Live queries.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "LiveQueriesToml")]
pub struct LiveQueries {
    /// A query that starts with one of these is sent, without the prefix.
    pub prefixes: Vec<String>,
    /// A query that matches one of these regular expressions (Rust `regex`
    /// syntax) is sent whole. The Daemon refuses a Manifest whose trigger doesn't
    /// compile.
    pub triggers: Vec<String>,
    /// How long the query must stay unchanged before it is sent. 0 sends on
    /// every keystroke, and lets the Extension mark a result as the answer.
    pub debounce_ms: u32,
}

/// An Action offered on every Item of one kind, whichever Extension owns it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(from = "ContributedActionToml")]
pub struct ContributedAction {
    pub kind: KindName,
    pub action: Action,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestToml {
    name: String,
    #[serde(default)]
    description: Option<String>,
    command: Vec<String>,
    #[serde(default)]
    live_query: Option<LiveQueries>,
    #[serde(default)]
    contributed_actions: Vec<ContributedAction>,
    #[serde(default)]
    action_catalog: bool,
}

impl TryFrom<ManifestToml> for Manifest {
    type Error = String;
    fn try_from(toml: ManifestToml) -> Result<Self, String> {
        if !is_valid_name(&toml.name) {
            return Err(format!(
                "`{}` isn't a valid Extension name; use lowercase letters, digits and hyphens",
                toml.name
            ));
        }
        if toml.command.is_empty() {
            return Err("`command` is empty; it needs at least the program to run".into());
        }
        Ok(Manifest {
            name: toml.name,
            description: toml.description,
            command: toml.command,
            live_query: toml.live_query,
            contributed_actions: toml.contributed_actions,
            action_catalog: toml.action_catalog,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveQueriesToml {
    #[serde(default)]
    prefixes: Vec<String>,
    #[serde(default)]
    triggers: Vec<String>,
    debounce_ms: u32,
}

impl TryFrom<LiveQueriesToml> for LiveQueries {
    type Error = String;
    fn try_from(toml: LiveQueriesToml) -> Result<Self, String> {
        if toml.prefixes.is_empty() && toml.triggers.is_empty() {
            return Err("`live_query` needs at least one prefix or trigger".into());
        }
        Ok(LiveQueries {
            prefixes: toml.prefixes,
            triggers: toml.triggers,
            debounce_ms: toml.debounce_ms,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContributedActionToml {
    kind: KindName,
    id: String,
    title: String,
    #[serde(default)]
    shortcut: Option<String>,
    #[serde(default)]
    arguments: Vec<Argument>,
}

impl From<ContributedActionToml> for ContributedAction {
    fn from(toml: ContributedActionToml) -> Self {
        ContributedAction {
            kind: toml.kind,
            action: Action {
                id: toml.id,
                title: toml.title,
                shortcut: toml.shortcut,
                arguments: toml.arguments,
            },
        }
    }
}
