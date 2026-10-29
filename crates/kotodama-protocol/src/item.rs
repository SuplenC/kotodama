//! Items, Item kinds and the Actions an Item offers.
//!
//! # Owns
//!
//! - [`Item`], [`ItemRef`], [`ItemKind`] and the kind names ([`KindName`], [`NamespacedKind`],
//!   [`SHARED_KINDS`]), [`Action`] and its [`Argument`]s.
//! - The flat shape an Item has on the wire, and the checks an Item passes as it is decoded.
//!
//! # Not here
//!
//! - Which minor version an Item kind needs: `version.rs`.

use std::fmt;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;

/// One searchable thing an Extension contributes.
///
/// On the wire an Item is flat: its kind's field sits next to `kind`, as in
/// `{"id": "k", "kind": "project", "path": "/src/kotodama", ...}`. Decoding rejects
/// an Item whose kind is missing its field, so an `Item` received from a peer is
/// always valid.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "ItemOwned")]
pub struct Item {
    /// Unique among the Items of one Extension. Sending an Item with an id that
    /// is already indexed replaces the old one.
    pub id: String,
    pub kind: ItemKind,
    /// The thing the Item stands for, named the same by every Extension that
    /// offers it, as `app:firefox.desktop` or `url:https://example.org`. Items
    /// with the same Identity share their History, whichever Extension owns
    /// them: picking one counts for all. `None` keeps the Item's History its
    /// own.
    pub identity: Option<String>,
    /// The Item's name, which the search matches first.
    pub title: String,
    /// A line under the title, such as an app's description.
    pub subtitle: Option<String>,
    /// A freedesktop icon name or an absolute path.
    pub icon: Option<String>,
    /// Typed before an Item's Arguments, as `go` in `go 1.23`.
    pub keyword: Option<String>,
    /// More words the Item is found by, besides its title.
    pub keywords: Vec<String>,
    /// The Item's own Actions. The first is the primary one, which Enter runs, so
    /// there is always at least one.
    pub actions: Vec<Action>,
    /// Kept out of the search: only `kotodama open --view` reaches it, as the
    /// entry point to a View of an Extension that would rather not list one,
    /// such as one with only Live queries. Ignored outside
    /// `add_items`.
    pub hidden: bool,
    /// Shown inside its Extension's app alone, never by the whole search,
    /// such as a keyboard shortcut among windows. Ignored outside
    /// `add_items`, and when the user puts the whole Extension in the search
    /// or behind its app.
    pub app_only: bool,
    /// Words the Item is still found by, but which never rank it above a
    /// title: one matches only as the query inside a word, and a Result
    /// found by them alone stays below every title that starts with the
    /// query, however often it is picked. For a name a user seldom types
    /// but may look for, such as an app's desktop id, whose first word may
    /// be another app's name.
    pub weak_keywords: Vec<String>,
}

/// An Item's id and kind, without anything shown on screen. This is how the
/// Daemon names an Item to the Extension whose Action runs on it, including a
/// Contributed action's Extension, which never sees the rest of the Item.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ItemRefOwned")]
pub struct ItemRef {
    pub id: String,
    pub kind: ItemKind,
}

impl Item {
    /// The Item's id and kind, as the Daemon names it to an Extension.
    #[must_use]
    pub fn to_ref(&self) -> ItemRef {
        ItemRef {
            id: self.id.clone(),
            kind: self.kind.clone(),
        }
    }
}

/// What sort of thing an Item is. Other Extensions offer Contributed actions by
/// kind, so each shared kind carries the one field those Actions need.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ItemKind {
    /// An installed app, by its desktop id, such as `firefox.desktop`.
    App { desktop_id: String },
    /// A file, by its absolute path.
    File { path: PathBuf },
    /// A folder, by its absolute path.
    Folder { path: PathBuf },
    /// A project's folder, by its absolute path.
    Project { path: PathBuf },
    /// An open window, by the compositor's address for it.
    Window { address: String },
    /// A web address.
    Url { url: String },
    /// Any other kind, owned by one Extension and named after it.
    Namespaced(NamespacedKind),
}

impl ItemKind {
    /// The kind's name on the wire: a shared kind's, or `<extension>.<kind>`.
    pub fn name(&self) -> &str {
        match self {
            ItemKind::App { .. } => "app",
            ItemKind::File { .. } => "file",
            ItemKind::Folder { .. } => "folder",
            ItemKind::Project { .. } => "project",
            ItemKind::Window { .. } => "window",
            ItemKind::Url { .. } => "url",
            ItemKind::Namespaced(kind) => kind.as_str(),
        }
    }

    /// A shared kind's index in [`SHARED_KINDS`], without comparing names;
    /// `None` for a namespaced kind.
    pub fn shared_index(&self) -> Option<usize> {
        match self {
            ItemKind::App { .. } => Some(0),
            ItemKind::File { .. } => Some(1),
            ItemKind::Folder { .. } => Some(2),
            ItemKind::Project { .. } => Some(3),
            ItemKind::Window { .. } => Some(4),
            ItemKind::Url { .. } => Some(5),
            ItemKind::Namespaced(_) => None,
        }
    }

    /// The Extension a namespaced kind belongs to; `None` for a shared kind.
    pub fn extension(&self) -> Option<&str> {
        match self {
            ItemKind::Namespaced(kind) => Some(kind.extension()),
            _ => None,
        }
    }

    fn from_wire(name: String, fields: KindFields) -> Result<Self, ItemError> {
        let KindFields {
            desktop_id,
            path,
            address,
            url,
        } = fields;
        let present = [
            ("desktop_id", desktop_id.is_some()),
            ("path", path.is_some()),
            ("address", address.is_some()),
            ("url", url.is_some()),
        ];
        let needs = match name.as_str() {
            "app" => Some("desktop_id"),
            "file" | "folder" | "project" => Some("path"),
            "window" => Some("address"),
            "url" => Some("url"),
            _ => None,
        };
        if let Some(&(extra, _)) = present.iter().find(|&&(field, set)| set && Some(field) != needs) {
            return Err(ItemError::extra_field(name, extra));
        }
        let missing = || ItemError::missing_field(&name, needs.unwrap_or_default());
        Ok(match name.as_str() {
            "app" => ItemKind::App {
                desktop_id: desktop_id.ok_or_else(missing)?,
            },
            "file" => ItemKind::File {
                path: path.ok_or_else(missing)?,
            },
            "folder" => ItemKind::Folder {
                path: path.ok_or_else(missing)?,
            },
            "project" => ItemKind::Project {
                path: path.ok_or_else(missing)?,
            },
            "window" => ItemKind::Window {
                address: address.ok_or_else(missing)?,
            },
            "url" => ItemKind::Url {
                url: url.ok_or_else(missing)?,
            },
            _ => ItemKind::Namespaced(NamespacedKind::new(name).map_err(ItemError::Kind)?),
        })
    }

    fn wire_fields(&self) -> KindFieldsRef<'_> {
        let mut fields = KindFieldsRef::default();
        match self {
            ItemKind::App { desktop_id } => fields.desktop_id = Some(desktop_id),
            ItemKind::File { path } | ItemKind::Folder { path } | ItemKind::Project { path } => {
                fields.path = Some(path);
            }
            ItemKind::Window { address } => fields.address = Some(address),
            ItemKind::Url { url } => fields.url = Some(url),
            ItemKind::Namespaced(_) => {}
        }
        fields
    }
}

/// The Item kinds every Extension shares. Any other kind is namespaced.
pub const SHARED_KINDS: [&str; 6] = ["app", "file", "folder", "project", "window", "url"];

/// An Item kind owned by one Extension: `<extension>.<kind>`, as in
/// `kubaos.suite`. Both halves are names (see [`is_valid_name`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NamespacedKind(String);

impl NamespacedKind {
    /// The kind `<extension>.<kind>`.
    ///
    /// # Errors
    ///
    /// [`KindError::Unknown`] for a single name, which no shared kind has, and
    /// [`KindError::Invalid`] for anything else that isn't two names.
    pub fn new(kind: impl Into<String>) -> Result<Self, KindError> {
        let kind = kind.into();
        match kind.split_once('.') {
            Some((extension, local)) if is_valid_name(extension) && is_valid_name(local) => Ok(NamespacedKind(kind)),
            None if is_valid_name(&kind) => Err(KindError::Unknown(kind)),
            _ => Err(KindError::Invalid(kind)),
        }
    }

    /// The whole kind, `<extension>.<kind>`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The Extension the kind belongs to: the part before the dot.
    pub fn extension(&self) -> &str {
        self.0.split_once('.').map_or("", |(extension, _)| extension)
    }
}

/// Why a name isn't an Item kind.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KindError {
    /// One name, with no Extension before it, that isn't a shared kind.
    #[error("unknown Item kind `{0}`; an Extension's own kinds are named `<extension>.{0}`")]
    Unknown(String),
    /// Not a shared kind, nor two names joined by a dot.
    #[error("`{0}` isn't a valid Item kind; it should look like `<extension>.<kind>`")]
    Invalid(String),
}

/// The name of an Item kind, shared or namespaced, without an Item: what a
/// Manifest or the Action catalog names when it means "Items of this kind".
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct KindName(String);

impl KindName {
    /// A shared kind's name, or a namespaced kind's.
    ///
    /// # Errors
    ///
    /// As [`NamespacedKind::new`], for a name that isn't a shared kind's.
    pub fn new(name: impl Into<String>) -> Result<Self, KindError> {
        let name = name.into();
        if SHARED_KINDS.contains(&name.as_str()) {
            Ok(KindName(name))
        } else {
            NamespacedKind::new(name).map(|kind| KindName(kind.0))
        }
    }

    /// The kind's name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for KindName {
    type Error = KindError;
    fn try_from(name: String) -> Result<Self, KindError> {
        KindName::new(name)
    }
}

impl From<KindName> for String {
    fn from(name: KindName) -> String {
        name.0
    }
}

impl fmt::Display for KindName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether `name` is a valid Extension name or local kind name: lowercase ASCII
/// letters, digits and hyphens, starting with a letter or digit.
pub fn is_valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Something the user can do to an Item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    /// Unique among one Extension's Actions; the Daemon sends it back when the
    /// Action runs.
    pub id: String,
    /// What the Action panel lists it as, such as `Open in a new window`.
    pub title: String,
    /// A key binding such as `ctrl+o`, which runs the Action without opening the
    /// Action panel.
    #[serde(default)]
    pub shortcut: Option<String>,
    /// What the user types or fills in for the Action.
    #[serde(default)]
    pub arguments: Vec<Argument>,
    /// Whether the Front end asks the user to confirm before it runs, as for
    /// an Action that can't be undone, such as powering off.
    #[serde(default)]
    pub confirm: bool,
}

impl Action {
    /// An Action with no shortcut, no Arguments and no confirm step; the
    /// setters below add them.
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Action {
            id: id.into(),
            title: title.into(),
            shortcut: None,
            arguments: Vec::new(),
            confirm: false,
        }
    }

    /// Has the Front end ask the user to confirm before the Action runs, so
    /// a stray Enter can't run it.
    ///
    /// ```
    /// use kotodama_protocol::Action;
    ///
    /// let clear = Action::new("clear", "Clear History").confirm();
    /// assert!(clear.confirm);
    /// ```
    #[must_use]
    pub fn confirm(mut self) -> Self {
        self.confirm = true;
        self
    }

    /// Binds the Action to `shortcut`, such as `ctrl+o`.
    #[must_use]
    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Asks for `arguments` when the Action runs.
    #[must_use]
    pub fn arguments(mut self, arguments: Vec<Argument>) -> Self {
        self.arguments = arguments;
        self
    }
}

/// A value the user types inline after the Item's keyword. One that is required
/// but not typed opens a form.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argument {
    /// Names the value in the Action's `arguments`.
    pub name: String,
    /// Shown greyed out until something is typed.
    #[serde(default)]
    pub placeholder: Option<String>,
    /// The Action runs without it.
    #[serde(default)]
    pub optional: bool,
    /// Whether it is text, or the path to a file or a folder. A path reaches
    /// the Action absolute: the Daemon expands a leading `~` and puts the
    /// home directory before a relative one.
    #[serde(default)]
    pub kind: ArgumentKind,
    /// For a file, the ones it takes, by extension (`conf`, without the dot)
    /// or by MIME type (`application/pdf`); empty takes any file.
    #[serde(default)]
    pub accept: Vec<String>,
}

impl Argument {
    /// A required text Argument named `name`, with no placeholder; the
    /// setters below change that.
    ///
    /// ```
    /// use kotodama_protocol::Argument;
    /// use kotodama_protocol::ArgumentKind;
    ///
    /// let config = Argument::new("path").file(["conf"]).placeholder("~/Downloads/work.conf");
    /// assert_eq!(config.kind, ArgumentKind::File);
    /// assert_eq!(config.accept, ["conf"]);
    /// ```
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Argument {
            name: name.into(),
            placeholder: None,
            optional: false,
            kind: ArgumentKind::Text,
            accept: Vec::new(),
        }
    }

    /// Shows `placeholder` greyed out until something is typed.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Lets the Action run without it.
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Asks for the path to a file of one of the kinds in `accept`, by
    /// extension or MIME type; none takes any file.
    #[must_use]
    pub fn file<S: Into<String>>(mut self, accept: impl IntoIterator<Item = S>) -> Self {
        self.kind = ArgumentKind::File;
        self.accept = accept.into_iter().map(Into::into).collect();
        self
    }

    /// Asks for the path to a folder.
    #[must_use]
    pub fn folder(mut self) -> Self {
        self.kind = ArgumentKind::Folder;
        self.accept = Vec::new();
        self
    }
}

/// What an [`Argument`] asks for, and so how a form asks for it.
///
/// On the wire it is a string, `text`, `file` or `folder`, so that in
/// protobuf an Argument without one, or with an empty one, is text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "&'static str", try_from = "String")]
pub enum ArgumentKind {
    /// Any text: a text field.
    #[default]
    Text,
    /// The path to a file: a file field.
    File,
    /// The path to a folder: a file field that asks for a folder.
    Folder,
}

impl From<ArgumentKind> for &'static str {
    fn from(kind: ArgumentKind) -> Self {
        match kind {
            ArgumentKind::Text => "text",
            ArgumentKind::File => "file",
            ArgumentKind::Folder => "folder",
        }
    }
}

impl TryFrom<String> for ArgumentKind {
    type Error = ArgumentKindError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        match name.as_str() {
            "" | "text" => Ok(ArgumentKind::Text),
            "file" => Ok(ArgumentKind::File),
            "folder" => Ok(ArgumentKind::Folder),
            _ => Err(ArgumentKindError(name)),
        }
    }
}

/// An Argument's `kind` that is none of `text`, `file` and `folder`.
#[derive(Debug, thiserror::Error)]
#[error("`{0}` isn't an Argument kind: it is `text`, `file` or `folder`")]
pub struct ArgumentKindError(String);

/// Why an Item or an [`ItemRef`] that arrived isn't valid. Serde reads it into
/// the Wire format's decode error.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ItemError {
    #[error("a `{kind}` Item can't have `{field}`")]
    ExtraField { kind: String, field: &'static str },
    #[error("a `{kind}` Item needs `{field}`")]
    MissingField { kind: String, field: &'static str },
    #[error(transparent)]
    Kind(KindError),
    #[error("Item `{0}` has no Actions; it needs at least its primary Action")]
    NoActions(String),
    #[error("Item `{0}` has an empty identity; leave it out instead")]
    EmptyIdentity(String),
}

impl ItemError {
    fn extra_field(kind: String, field: &'static str) -> Self {
        ItemError::ExtraField { kind, field }
    }

    fn missing_field(kind: &str, field: &'static str) -> Self {
        ItemError::MissingField {
            kind: kind.to_owned(),
            field,
        }
    }
}

// The flat wire shapes. Serializing borrows the Item; deserializing owns it and
// validates on the way in.

#[derive(Default)]
struct KindFields {
    desktop_id: Option<String>,
    path: Option<PathBuf>,
    address: Option<String>,
    url: Option<String>,
}

#[derive(Default)]
struct KindFieldsRef<'a> {
    desktop_id: Option<&'a str>,
    path: Option<&'a Path>,
    address: Option<&'a str>,
    url: Option<&'a str>,
}

// The renames give the published protobuf schema the type's public name; no
// format writes struct names into the bytes.
#[derive(Serialize)]
#[serde(rename = "Item")]
struct ItemWire<'a> {
    id: &'a str,
    kind: &'a str,
    desktop_id: Option<&'a str>,
    path: Option<&'a Path>,
    address: Option<&'a str>,
    url: Option<&'a str>,
    identity: Option<&'a str>,
    title: &'a str,
    subtitle: Option<&'a str>,
    icon: Option<&'a str>,
    keyword: Option<&'a str>,
    keywords: &'a [String],
    actions: &'a [Action],
    hidden: bool,
    app_only: bool,
    weak_keywords: &'a [String],
}

/// An Item as it arrives, before its kind's field and its Actions are checked.
#[derive(Deserialize)]
pub(crate) struct ItemOwned {
    id: String,
    kind: String,
    #[serde(default)]
    desktop_id: Option<String>,
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    identity: Option<String>,
    title: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    keyword: Option<String>,
    #[serde(default)]
    keywords: Vec<String>,
    actions: Vec<Action>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    app_only: bool,
    #[serde(default)]
    weak_keywords: Vec<String>,
}

impl Serialize for Item {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = self.kind.wire_fields();
        ItemWire {
            id: &self.id,
            kind: self.kind.name(),
            desktop_id: fields.desktop_id,
            path: fields.path,
            address: fields.address,
            url: fields.url,
            identity: self.identity.as_deref(),
            title: &self.title,
            subtitle: self.subtitle.as_deref(),
            icon: self.icon.as_deref(),
            keyword: self.keyword.as_deref(),
            keywords: &self.keywords,
            actions: &self.actions,
            hidden: self.hidden,
            app_only: self.app_only,
            weak_keywords: &self.weak_keywords,
        }
        .serialize(serializer)
    }
}

impl TryFrom<ItemOwned> for Item {
    type Error = ItemError;
    fn try_from(wire: ItemOwned) -> Result<Self, ItemError> {
        if wire.actions.is_empty() {
            return Err(ItemError::NoActions(wire.id));
        }
        wire.into_item_without_actions()
    }
}

impl ItemOwned {
    /// The Item, checked for everything but having an Action: a Result the
    /// Daemon shows as unavailable carries none.
    pub(crate) fn into_item_without_actions(self) -> Result<Item, ItemError> {
        let fields = KindFields {
            desktop_id: self.desktop_id,
            path: self.path,
            address: self.address,
            url: self.url,
        };
        let kind = ItemKind::from_wire(self.kind, fields)?;
        if self.identity.as_deref() == Some("") {
            return Err(ItemError::EmptyIdentity(self.id));
        }
        Ok(Item {
            id: self.id,
            kind,
            identity: self.identity,
            title: self.title,
            subtitle: self.subtitle,
            icon: self.icon,
            keyword: self.keyword,
            keywords: self.keywords,
            actions: self.actions,
            hidden: self.hidden,
            app_only: self.app_only,
            weak_keywords: self.weak_keywords,
        })
    }
}

#[derive(Serialize)]
#[serde(rename = "ItemRef")]
struct ItemRefWire<'a> {
    id: &'a str,
    kind: &'a str,
    desktop_id: Option<&'a str>,
    path: Option<&'a Path>,
    address: Option<&'a str>,
    url: Option<&'a str>,
}

#[derive(Deserialize)]
struct ItemRefOwned {
    id: String,
    kind: String,
    #[serde(default)]
    desktop_id: Option<String>,
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

impl Serialize for ItemRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = self.kind.wire_fields();
        ItemRefWire {
            id: &self.id,
            kind: self.kind.name(),
            desktop_id: fields.desktop_id,
            path: fields.path,
            address: fields.address,
            url: fields.url,
        }
        .serialize(serializer)
    }
}

impl TryFrom<ItemRefOwned> for ItemRef {
    type Error = ItemError;
    fn try_from(wire: ItemRefOwned) -> Result<Self, ItemError> {
        let fields = KindFields {
            desktop_id: wire.desktop_id,
            path: wire.path,
            address: wire.address,
            url: wire.url,
        };
        Ok(ItemRef {
            id: wire.id,
            kind: ItemKind::from_wire(wire.kind, fields)?,
        })
    }
}

#[cfg(test)]
mod tests;
