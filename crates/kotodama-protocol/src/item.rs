//! Items, Item kinds and the Actions an Item offers.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};

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
    pub title: String,
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
    /// such as one with only Live queries (ADR 0015). Ignored outside
    /// `add_items`.
    pub hidden: bool,
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
    pub fn to_ref(&self) -> ItemRef {
        ItemRef {
            id: self.id.clone(),
            kind: self.kind.clone(),
        }
    }
}

/// What sort of thing an Item is. Other Extensions offer Contributed actions by
/// kind (ADR 0002), so each shared kind carries the one field those Actions need.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ItemKind {
    App {
        desktop_id: String,
    },
    File {
        path: PathBuf,
    },
    Folder {
        path: PathBuf,
    },
    Project {
        path: PathBuf,
    },
    Window {
        address: String,
    },
    Url {
        url: String,
    },
    /// Any other kind, owned by one Extension and named after it.
    Namespaced(NamespacedKind),
}

impl ItemKind {
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

    /// The Extension a namespaced kind belongs to; `None` for a shared kind.
    pub fn extension(&self) -> Option<&str> {
        match self {
            ItemKind::Namespaced(kind) => Some(kind.extension()),
            _ => None,
        }
    }

    fn from_wire(name: String, fields: KindFields) -> Result<Self, String> {
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
        if let Some((extra, _)) = present
            .iter()
            .find(|&&(field, set)| set && Some(field) != needs)
        {
            return Err(format!("a `{name}` Item can't have `{extra}`"));
        }
        let missing = || format!("a `{name}` Item needs `{}`", needs.unwrap_or_default());
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
            _ => ItemKind::Namespaced(NamespacedKind::new(name)?),
        })
    }

    fn wire_fields(&self) -> KindFieldsRef<'_> {
        let mut fields = KindFieldsRef::default();
        match self {
            ItemKind::App { desktop_id } => fields.desktop_id = Some(desktop_id),
            ItemKind::File { path } | ItemKind::Folder { path } | ItemKind::Project { path } => {
                fields.path = Some(path)
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
    pub fn new(kind: impl Into<String>) -> Result<Self, String> {
        let kind = kind.into();
        match kind.split_once('.') {
            Some((extension, local)) if is_valid_name(extension) && is_valid_name(local) => {
                Ok(NamespacedKind(kind))
            }
            None if is_valid_name(&kind) => Err(format!(
                "unknown Item kind `{kind}`; an Extension's own kinds are named `<extension>.{kind}`"
            )),
            _ => Err(format!(
                "`{kind}` isn't a valid Item kind; it should look like `<extension>.<kind>`"
            )),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn extension(&self) -> &str {
        self.0
            .split_once('.')
            .map_or("", |(extension, _)| extension)
    }
}

/// The name of an Item kind, shared or namespaced, without an Item: what a
/// Manifest or the Action catalog names when it means "Items of this kind".
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct KindName(String);

impl KindName {
    pub fn new(name: impl Into<String>) -> Result<Self, String> {
        let name = name.into();
        if SHARED_KINDS.contains(&name.as_str()) {
            Ok(KindName(name))
        } else {
            NamespacedKind::new(name).map(|kind| KindName(kind.0))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for KindName {
    type Error = String;
    fn try_from(name: String) -> Result<Self, String> {
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
    pub title: String,
    /// A key binding such as `ctrl+o`, which runs the Action without opening the
    /// Action panel.
    #[serde(default)]
    pub shortcut: Option<String>,
    #[serde(default)]
    pub arguments: Vec<Argument>,
}

/// A value the user types inline after the Item's keyword. One that is required
/// but not typed opens a form.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argument {
    pub name: String,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub optional: bool,
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

#[derive(Serialize)]
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
}

#[derive(Deserialize)]
struct ItemOwned {
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
        }
        .serialize(serializer)
    }
}

impl TryFrom<ItemOwned> for Item {
    type Error = String;
    fn try_from(wire: ItemOwned) -> Result<Self, String> {
        let fields = KindFields {
            desktop_id: wire.desktop_id,
            path: wire.path,
            address: wire.address,
            url: wire.url,
        };
        let kind = ItemKind::from_wire(wire.kind, fields)?;
        if wire.actions.is_empty() {
            return Err(format!(
                "Item `{}` has no Actions; it needs at least its primary Action",
                wire.id
            ));
        }
        if wire.identity.as_deref() == Some("") {
            return Err(format!(
                "Item `{}` has an empty identity; leave it out instead",
                wire.id
            ));
        }
        Ok(Item {
            id: wire.id,
            kind,
            identity: wire.identity,
            title: wire.title,
            subtitle: wire.subtitle,
            icon: wire.icon,
            keyword: wire.keyword,
            keywords: wire.keywords,
            actions: wire.actions,
            hidden: wire.hidden,
        })
    }
}

#[derive(Serialize)]
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
    type Error = String;
    fn try_from(wire: ItemRefOwned) -> Result<Self, String> {
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
