//! The `manifest.toml` that declares an Extension.
//!
//! # Owns
//!
//! - [`Manifest`], its [`LiveQueries`] and [`ContributedAction`]s (by kind or targeted), and the
//!   rules a Manifest must keep to parse.
//!
//! # Not here
//!
//! - What the Daemon refuses beyond them, such as a trigger that doesn't compile: `kotodamad`'s
//!   `discovery.rs`.

use serde::Deserialize;

use crate::Action;
use crate::ActionRole;
use crate::Argument;
use crate::Error;
use crate::Example;
use crate::KindName;
use crate::is_valid_name;

/// What an Extension is, how to start it, and what it contributes.
///
/// ```toml
/// name = "kubaos"
/// command = ["bin/kubaos-kotodama"]
/// action_catalog = true
/// # Its app, whose entry Item `suite` finds; inside it, `go` searches its
/// # Items alone.
/// word = "suite"
/// title = "KubaOS suites"
/// about = "Switches the Go, Rust and Node versions of a project."
/// examples = [{ query = "go 1.23", result = "Go 1.23 in this project" }]
/// # Its Items show inside its app alone, never in the search.
/// in_search = false
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
    /// What the Extension does, in a sentence.
    pub description: Option<String>,
    /// The program and its arguments. A relative program path is resolved
    /// against the Extension's directory. It speaks the protocol on stdio.
    pub command: Vec<String>,
    /// Present if the Extension answers Live queries.
    pub live_query: Option<LiveQueries>,
    /// Actions this Extension offers on other Extensions' Items, by Item kind
    /// or by Identity.
    pub contributed_actions: Vec<ContributedAction>,
    /// Whether the Extension may read the Action catalog.
    pub action_catalog: bool,
    /// The word that names its app, unless the user's config says otherwise:
    /// one word, such as `emoji`. Typing it finds the app's entry Item, and
    /// Tab on that enters the app. The `name` stands in without one. Read
    /// from `word`, or from `prefix`, its old name.
    pub word: Option<String>,
    /// The Extension's name as the user reads it, such as `Emoji`: the title
    /// of its app's entry Item and of the chip that shows the search is in
    /// its app. The `name` stands in without one.
    pub title: Option<String>,
    /// The app's icon: a freedesktop icon name or a path.
    pub icon: Option<String>,
    /// What the app does and how, shown on entering it, before anything is
    /// typed. The `description` stands in without one.
    pub about: Option<String>,
    /// A few queries to type in the app and what each gives, shown under
    /// `about`.
    pub examples: Vec<Example>,
    /// The ids of the Items that are the app's commands, such as one that
    /// opens a View, listed first on entering it.
    pub commands: Vec<String>,
    /// Whether its Items and Live queries are in the search, unless the
    /// user's config says otherwise. `false` keeps all of them inside its
    /// app; with `true`, an Item marked `app_only` still is.
    pub in_search: bool,
}

impl Manifest {
    /// Parses a `manifest.toml`.
    ///
    /// # Errors
    ///
    /// [`Error::Manifest`] if `toml` doesn't parse, has a key the Manifest
    /// doesn't know, or breaks a rule: an invalid name, an empty `command`, a
    /// `word` that isn't one word, or a `live_query` that routes nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use kotodama_protocol::Manifest;
    ///
    /// let manifest = Manifest::from_toml("name = \"notes\"\ncommand = [\"./notes\"]")?;
    /// assert_eq!(manifest.name, "notes");
    /// assert!(manifest.in_search);
    /// # Ok::<(), kotodama_protocol::Error>(())
    /// ```
    pub fn from_toml(toml: &str) -> Result<Self, Error> {
        toml::from_str(toml).map_err(|err| Error::Manifest(err.to_string()))
    }
}

/// Whether `word` can be an app's word: one word, with no whitespace in it or
/// around it.
pub fn is_valid_word(word: &str) -> bool {
    !word.is_empty() && !word.contains(char::is_whitespace)
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

/// An Action offered on other Extensions' Items: every Item of one kind, or
/// the Results whose Identity the Extension names.
///
/// ```toml
/// [[contributed_actions]]
/// targeted = true
/// id = "remove"
/// title = "Remove"
/// keywords = ["remove", "uninstall", "delete"]
/// role = "destructive"
/// confirm = true
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ContributedActionToml")]
pub struct ContributedAction {
    /// The Items it is offered on.
    pub to: ContributedTo,
    pub action: Action,
}

/// Which Items a [`ContributedAction`] is offered on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContributedTo {
    /// Every Item of this kind, from `kind` in the Manifest.
    Kind(KindName),
    /// Every Result whose Identity the Extension names, while it runs, in
    /// [`FromExtension::TargetAction`](crate::FromExtension::TargetAction),
    /// from `targeted = true` in the Manifest.
    Identities,
}

impl ContributedTo {
    /// The kind it is offered on; `None` for a targeted Action.
    #[must_use]
    pub fn kind(&self) -> Option<&KindName> {
        match self {
            ContributedTo::Kind(kind) => Some(kind),
            ContributedTo::Identities => None,
        }
    }
}

/// Why a Manifest's TOML breaks its rules. Serde reads it into the parser's
/// error, which names the line.
#[derive(Debug, thiserror::Error)]
enum ManifestError {
    #[error("`{0}` isn't a valid Extension name; use lowercase letters, digits and hyphens")]
    InvalidName(String),
    #[error("`command` is empty; it needs at least the program to run")]
    NoCommand,
    #[error("`word = {0:?}` isn't one word; leave `word` out for the Extension's name")]
    NotOneWord(String),
    #[error("`live_query` needs at least one prefix or trigger, or `fallback`")]
    RoutesNothing,
    #[error("Contributed action `{0}` needs `kind`, or `targeted = true` for the Identities the Extension names")]
    OfferedOnNothing(String),
    #[error("Contributed action `{0}` has both `kind` and `targeted = true`; it is offered by one or the other")]
    OfferedTwice(String),
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
    #[serde(default, alias = "prefix")]
    word: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    about: Option<String>,
    #[serde(default)]
    examples: Vec<Example>,
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default = "in_search_by_default")]
    in_search: bool,
}

/// An Extension's Items are in the search unless its Manifest says
/// otherwise.
fn in_search_by_default() -> bool {
    true
}

impl TryFrom<ManifestToml> for Manifest {
    type Error = ManifestError;
    fn try_from(toml: ManifestToml) -> Result<Self, ManifestError> {
        if !is_valid_name(&toml.name) {
            return Err(ManifestError::InvalidName(toml.name));
        }
        if toml.command.is_empty() {
            return Err(ManifestError::NoCommand);
        }
        if let Some(word) = toml.word.as_ref()
            && !is_valid_word(word)
        {
            return Err(ManifestError::NotOneWord(word.clone()));
        }
        Ok(Manifest {
            name: toml.name,
            description: toml.description,
            command: toml.command,
            live_query: toml.live_query,
            contributed_actions: toml.contributed_actions,
            action_catalog: toml.action_catalog,
            word: toml.word,
            title: toml.title,
            icon: toml.icon,
            about: toml.about,
            examples: toml.examples,
            commands: toml.commands,
            in_search: toml.in_search,
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
    type Error = ManifestError;
    fn try_from(toml: LiveQueriesToml) -> Result<Self, ManifestError> {
        if toml.prefixes.is_empty() && toml.triggers.is_empty() && !toml.fallback {
            return Err(ManifestError::RoutesNothing);
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
    #[serde(default)]
    kind: Option<KindName>,
    #[serde(default)]
    targeted: bool,
    id: String,
    title: String,
    #[serde(default)]
    shortcut: Option<String>,
    #[serde(default)]
    arguments: Vec<Argument>,
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    role: ActionRole,
}

impl TryFrom<ContributedActionToml> for ContributedAction {
    type Error = ManifestError;
    fn try_from(toml: ContributedActionToml) -> Result<Self, ManifestError> {
        let to = match (toml.kind, toml.targeted) {
            (Some(kind), false) => ContributedTo::Kind(kind),
            (None, true) => ContributedTo::Identities,
            (None, false) => return Err(ManifestError::OfferedOnNothing(toml.id)),
            (Some(_), true) => return Err(ManifestError::OfferedTwice(toml.id)),
        };
        Ok(ContributedAction {
            to,
            action: Action {
                id: toml.id,
                title: toml.title,
                shortcut: toml.shortcut,
                arguments: toml.arguments,
                confirm: toml.confirm,
                keywords: toml.keywords,
                role: toml.role,
            },
        })
    }
}
