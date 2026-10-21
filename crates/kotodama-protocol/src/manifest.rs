//! The `manifest.toml` that declares an Extension.

use serde::Deserialize;

use crate::{Action, Argument, Error, KindName, is_valid_name};

/// What an Extension is, how to start it, and what it contributes.
///
/// ```toml
/// name = "kubaos"
/// command = ["bin/kubaos-kotodama"]
/// action_catalog = true
/// # Its Items are searched only behind `suite`, as in `suite go`.
/// prefix = "suite"
/// title = "KubaOS suites"
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
    /// The Prefix its Items are searched behind, unless the user's config
    /// says otherwise: one word, such as `emoji`. The search leaves them out
    /// and shows an entry Item instead; `emoji fire` searches them alone.
    pub prefix: Option<String>,
    /// The Extension's name as the user reads it, such as `Emoji`: the title
    /// of its entry Item and of the chip that shows a search is behind its
    /// Prefix. The `name` stands in without one.
    pub title: Option<String>,
    /// The entry Item's icon: a freedesktop icon name or a path.
    pub icon: Option<String>,
}

impl Manifest {
    pub fn from_toml(toml: &str) -> Result<Self, Error> {
        toml::from_str(toml).map_err(|err| Error::Manifest(err.to_string()))
    }
}

/// Whether `prefix` can be a Prefix: one word, with no whitespace in it or
/// around it.
pub fn is_valid_prefix(prefix: &str) -> bool {
    !prefix.is_empty() && !prefix.contains(char::is_whitespace)
}

/// Which queries are sent to the Extension as Live queries.
///
/// ```toml
/// # A fallback: sent what nothing matches well, and what looks like an address.
/// [live_query]
/// fallback = true
/// triggers = ['^\S+\.\S+$']
/// debounce_ms = 0
/// ```
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
    /// The Extension is a fallback, such as web search: it is also sent any
    /// query whose real matches leave rows on screen empty, and its results,
    /// but for an answer, rank below every real match.
    pub fallback: bool,
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
    #[serde(default)]
    prefix: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    icon: Option<String>,
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
        if let Some(prefix) = &toml.prefix
            && !is_valid_prefix(prefix)
        {
            return Err(format!(
                "`prefix = {prefix:?}` isn't one word; leave `prefix` out for none"
            ));
        }
        Ok(Manifest {
            name: toml.name,
            description: toml.description,
            command: toml.command,
            live_query: toml.live_query,
            contributed_actions: toml.contributed_actions,
            action_catalog: toml.action_catalog,
            prefix: toml.prefix,
            title: toml.title,
            icon: toml.icon,
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
    #[serde(default)]
    fallback: bool,
}

impl TryFrom<LiveQueriesToml> for LiveQueries {
    type Error = String;
    fn try_from(toml: LiveQueriesToml) -> Result<Self, String> {
        if toml.prefixes.is_empty() && toml.triggers.is_empty() && !toml.fallback {
            return Err("`live_query` needs at least one prefix or trigger, or `fallback`".into());
        }
        Ok(LiveQueries {
            prefixes: toml.prefixes,
            triggers: toml.triggers,
            debounce_ms: toml.debounce_ms,
            fallback: toml.fallback,
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
