//! The messages of `messages.rs` as other serialization libraries' derives
//! want them: one copy of the types per library, each converted from the
//! protocol's own. Only the formats benchmark and the protobuf
//! interoperability test use them; the protocol itself has one set of types.

use kotodama_protocol as real;

/// The protocol's types as `prost` derives them: the same field numbers the
/// protocol's protobuf Wire format gives them.
pub mod prost {
    use super::real;

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Argument {
        #[prost(string, tag = "1")]
        pub name: String,
        #[prost(string, optional, tag = "2")]
        pub placeholder: Option<String>,
        #[prost(bool, tag = "3")]
        pub optional: bool,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Action {
        #[prost(string, tag = "1")]
        pub id: String,
        #[prost(string, tag = "2")]
        pub title: String,
        #[prost(string, optional, tag = "3")]
        pub shortcut: Option<String>,
        #[prost(message, repeated, tag = "4")]
        pub arguments: Vec<Argument>,
        #[prost(bool, tag = "5")]
        pub confirm: bool,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Item {
        #[prost(string, tag = "1")]
        pub id: String,
        #[prost(string, tag = "2")]
        pub kind: String,
        #[prost(string, optional, tag = "3")]
        pub desktop_id: Option<String>,
        #[prost(string, optional, tag = "4")]
        pub path: Option<String>,
        #[prost(string, optional, tag = "5")]
        pub address: Option<String>,
        #[prost(string, optional, tag = "6")]
        pub url: Option<String>,
        #[prost(string, optional, tag = "7")]
        pub identity: Option<String>,
        #[prost(string, tag = "8")]
        pub title: String,
        #[prost(string, optional, tag = "9")]
        pub subtitle: Option<String>,
        #[prost(string, optional, tag = "10")]
        pub icon: Option<String>,
        #[prost(string, optional, tag = "11")]
        pub keyword: Option<String>,
        #[prost(string, repeated, tag = "12")]
        pub keywords: Vec<String>,
        #[prost(message, repeated, tag = "13")]
        pub actions: Vec<Action>,
        #[prost(bool, tag = "14")]
        pub hidden: bool,
        #[prost(bool, tag = "15")]
        pub app_only: bool,
        #[prost(string, repeated, tag = "16")]
        pub weak_keywords: Vec<String>,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct OfferedAction {
        #[prost(string, tag = "1")]
        pub extension: String,
        #[prost(message, optional, tag = "2")]
        pub action: Option<Action>,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct ShownResult {
        #[prost(string, tag = "1")]
        pub extension: String,
        #[prost(message, optional, tag = "2")]
        pub item: Option<Item>,
        #[prost(message, repeated, tag = "3")]
        pub contributed: Vec<OfferedAction>,
        #[prost(uint32, repeated, tag = "4")]
        pub highlights: Vec<u32>,
        #[prost(string, optional, tag = "5")]
        pub alias: Option<String>,
        #[prost(string, optional, tag = "6")]
        pub unavailable: Option<String>,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Scope {
        #[prost(string, tag = "1")]
        pub word: String,
        #[prost(string, tag = "2")]
        pub title: String,
        #[prost(string, optional, tag = "3")]
        pub icon: Option<String>,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    #[expect(
        clippy::struct_field_names,
        reason = "mirrors `ToFrontEnd::Results`, whose field is `results`"
    )]
    pub struct Results {
        #[prost(uint32, tag = "1")]
        pub version: u32,
        #[prost(message, repeated, tag = "2")]
        pub results: Vec<ShownResult>,
        #[prost(message, optional, tag = "3")]
        pub scope: Option<Scope>,
    }

    /// `ToFrontEnd`, with only the variant measured.
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct ToFrontEnd {
        #[prost(oneof = "ToFrontEndVariant", tags = "1")]
        pub variant: Option<ToFrontEndVariant>,
    }

    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum ToFrontEndVariant {
        #[prost(message, tag = "1")]
        Results(Results),
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Query {
        #[prost(string, tag = "1")]
        pub text: String,
    }

    /// `FromFrontEnd`, with only the variant measured.
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct FromFrontEnd {
        #[prost(oneof = "FromFrontEndVariant", tags = "1")]
        pub variant: Option<FromFrontEndVariant>,
    }

    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum FromFrontEndVariant {
        #[prost(message, tag = "1")]
        Query(Query),
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct AddItems {
        #[prost(message, repeated, tag = "1")]
        pub items: Vec<Item>,
    }

    /// `FromExtension`, with only the variant measured.
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct FromExtension {
        #[prost(oneof = "FromExtensionVariant", tags = "1")]
        pub variant: Option<FromExtensionVariant>,
    }

    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum FromExtensionVariant {
        #[prost(message, tag = "1")]
        AddItems(AddItems),
    }

    pub fn action(action: &real::Action) -> Action {
        Action {
            id: action.id.clone(),
            title: action.title.clone(),
            shortcut: action.shortcut.clone(),
            arguments: action
                .arguments
                .iter()
                .map(|argument| Argument {
                    name: argument.name.clone(),
                    placeholder: argument.placeholder.clone(),
                    optional: argument.optional,
                })
                .collect(),
            confirm: action.confirm,
        }
    }

    pub fn item(item: &real::Item) -> Item {
        let mut mirror = Item {
            id: item.id.clone(),
            kind: item.kind.name().to_owned(),
            identity: item.identity.clone(),
            title: item.title.clone(),
            subtitle: item.subtitle.clone(),
            icon: item.icon.clone(),
            keyword: item.keyword.clone(),
            keywords: item.keywords.clone(),
            actions: item.actions.iter().map(action).collect(),
            hidden: item.hidden,
            app_only: item.app_only,
            weak_keywords: item.weak_keywords.clone(),
            ..Item::default()
        };
        match &item.kind {
            real::ItemKind::App { desktop_id } => mirror.desktop_id = Some(desktop_id.clone()),
            real::ItemKind::File { path } | real::ItemKind::Folder { path } | real::ItemKind::Project { path } => {
                mirror.path = Some(path.display().to_string());
            }
            real::ItemKind::Window { address } => mirror.address = Some(address.clone()),
            real::ItemKind::Url { url } => mirror.url = Some(url.clone()),
            real::ItemKind::Namespaced(_) => {}
        }
        mirror
    }

    pub fn results(message: &real::ToFrontEnd) -> ToFrontEnd {
        let real::ToFrontEnd::Results {
            version,
            results,
            scope,
        } = message
        else {
            panic!("only Results is mirrored");
        };
        ToFrontEnd {
            variant: Some(ToFrontEndVariant::Results(Results {
                version: *version,
                results: results
                    .iter()
                    .map(|result| ShownResult {
                        extension: result.extension.clone(),
                        item: Some(item(&result.item)),
                        contributed: result
                            .contributed
                            .iter()
                            .map(|offered| OfferedAction {
                                extension: offered.extension.clone(),
                                action: Some(action(&offered.action)),
                            })
                            .collect(),
                        highlights: result.highlights.clone(),
                        alias: result.alias.clone(),
                        unavailable: result.unavailable.clone(),
                    })
                    .collect(),
                scope: scope.as_ref().map(|scope| Scope {
                    word: scope.word.clone(),
                    title: scope.title.clone(),
                    icon: scope.icon.clone(),
                }),
            })),
        }
    }

    pub fn query(message: &real::FromFrontEnd) -> FromFrontEnd {
        let real::FromFrontEnd::Query { text } = message else {
            panic!("only Query is mirrored");
        };
        FromFrontEnd {
            variant: Some(FromFrontEndVariant::Query(Query { text: text.clone() })),
        }
    }

    pub fn batch(message: &real::FromExtension) -> FromExtension {
        let real::FromExtension::AddItems { items } = message else {
            panic!("only AddItems is mirrored");
        };
        FromExtension {
            variant: Some(FromExtensionVariant::AddItems(AddItems {
                items: items.iter().map(item).collect(),
            })),
        }
    }
}

/// The protocol's types as müsli derives them, for its `wire` format.
pub mod musli {
    use ::musli::Decode;
    use ::musli::Encode;
    use serde::Deserialize;
    use serde::Serialize;

    use super::real;

    #[derive(Clone, Debug, PartialEq, Encode, Decode, Serialize, Deserialize)]
    pub struct Argument {
        pub name: String,
        pub placeholder: Option<String>,
        pub optional: bool,
    }

    #[derive(Clone, Debug, PartialEq, Encode, Decode, Serialize, Deserialize)]
    pub struct Action {
        pub id: String,
        pub title: String,
        pub shortcut: Option<String>,
        pub arguments: Vec<Argument>,
        pub confirm: bool,
    }

    #[derive(Clone, Debug, PartialEq, Encode, Decode, Serialize, Deserialize)]
    pub struct Item {
        pub id: String,
        pub kind: String,
        pub desktop_id: Option<String>,
        pub path: Option<String>,
        pub address: Option<String>,
        pub url: Option<String>,
        pub identity: Option<String>,
        pub title: String,
        pub subtitle: Option<String>,
        pub icon: Option<String>,
        pub keyword: Option<String>,
        pub keywords: Vec<String>,
        pub actions: Vec<Action>,
        pub hidden: bool,
        pub app_only: bool,
        pub weak_keywords: Vec<String>,
    }

    #[derive(Clone, Debug, PartialEq, Encode, Decode, Serialize, Deserialize)]
    pub struct OfferedAction {
        pub extension: String,
        pub action: Action,
    }

    #[derive(Clone, Debug, PartialEq, Encode, Decode, Serialize, Deserialize)]
    pub struct ShownResult {
        pub extension: String,
        pub item: Item,
        pub contributed: Vec<OfferedAction>,
        pub highlights: Vec<u32>,
        pub alias: Option<String>,
        pub unavailable: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq, Encode, Decode, Serialize, Deserialize)]
    pub struct Scope {
        pub word: String,
        pub title: String,
        pub icon: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq, Encode, Decode, Serialize, Deserialize)]
    pub enum ToFrontEnd {
        Results {
            version: u32,
            results: Vec<ShownResult>,
            scope: Option<Scope>,
        },
    }

    #[derive(Clone, Debug, PartialEq, Encode, Decode, Serialize, Deserialize)]
    pub enum FromFrontEnd {
        Query { text: String },
    }

    #[derive(Clone, Debug, PartialEq, Encode, Decode, Serialize, Deserialize)]
    pub enum FromExtension {
        AddItems { items: Vec<Item> },
    }

    fn action(action: &real::Action) -> Action {
        Action {
            id: action.id.clone(),
            title: action.title.clone(),
            shortcut: action.shortcut.clone(),
            arguments: action
                .arguments
                .iter()
                .map(|argument| Argument {
                    name: argument.name.clone(),
                    placeholder: argument.placeholder.clone(),
                    optional: argument.optional,
                })
                .collect(),
            confirm: action.confirm,
        }
    }

    fn item(item: &real::Item) -> Item {
        let mirror = super::prost::item(item);
        Item {
            id: mirror.id,
            kind: mirror.kind,
            desktop_id: mirror.desktop_id,
            path: mirror.path,
            address: mirror.address,
            url: mirror.url,
            identity: mirror.identity,
            title: mirror.title,
            subtitle: mirror.subtitle,
            icon: mirror.icon,
            keyword: mirror.keyword,
            keywords: mirror.keywords,
            actions: item.actions.iter().map(action).collect(),
            hidden: mirror.hidden,
            app_only: mirror.app_only,
            weak_keywords: mirror.weak_keywords,
        }
    }

    pub fn results(message: &real::ToFrontEnd) -> ToFrontEnd {
        let real::ToFrontEnd::Results {
            version,
            results,
            scope,
        } = message
        else {
            panic!("only Results is mirrored");
        };
        ToFrontEnd::Results {
            version: *version,
            results: results
                .iter()
                .map(|result| ShownResult {
                    extension: result.extension.clone(),
                    item: item(&result.item),
                    contributed: result
                        .contributed
                        .iter()
                        .map(|offered| OfferedAction {
                            extension: offered.extension.clone(),
                            action: action(&offered.action),
                        })
                        .collect(),
                    highlights: result.highlights.clone(),
                    alias: result.alias.clone(),
                    unavailable: result.unavailable.clone(),
                })
                .collect(),
            scope: scope.as_ref().map(|scope| Scope {
                word: scope.word.clone(),
                title: scope.title.clone(),
                icon: scope.icon.clone(),
            }),
        }
    }

    pub fn query(message: &real::FromFrontEnd) -> FromFrontEnd {
        let real::FromFrontEnd::Query { text } = message else {
            panic!("only Query is mirrored");
        };
        FromFrontEnd::Query { text: text.clone() }
    }

    pub fn batch(message: &real::FromExtension) -> FromExtension {
        let real::FromExtension::AddItems { items } = message else {
            panic!("only AddItems is mirrored");
        };
        FromExtension::AddItems {
            items: items.iter().map(item).collect(),
        }
    }
}

/// The protocol's types as bilrost derives them.
pub mod bilrost {
    use super::real;

    #[derive(Clone, Debug, PartialEq, ::bilrost::Message)]
    pub struct Argument {
        pub name: String,
        pub placeholder: Option<String>,
        pub optional: bool,
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Message)]
    pub struct Action {
        pub id: String,
        pub title: String,
        pub shortcut: Option<String>,
        pub arguments: Vec<Argument>,
        pub confirm: bool,
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Message)]
    pub struct Item {
        pub id: String,
        pub kind: String,
        pub desktop_id: Option<String>,
        pub path: Option<String>,
        pub address: Option<String>,
        pub url: Option<String>,
        pub identity: Option<String>,
        pub title: String,
        pub subtitle: Option<String>,
        pub icon: Option<String>,
        pub keyword: Option<String>,
        pub keywords: Vec<String>,
        pub actions: Vec<Action>,
        pub hidden: bool,
        pub app_only: bool,
        pub weak_keywords: Vec<String>,
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Message)]
    pub struct OfferedAction {
        pub extension: String,
        pub action: Action,
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Message)]
    pub struct ShownResult {
        pub extension: String,
        pub item: Item,
        pub contributed: Vec<OfferedAction>,
        #[bilrost(encoding(packed))]
        pub highlights: Vec<u32>,
        pub alias: Option<String>,
        pub unavailable: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Message)]
    pub struct Scope {
        pub word: String,
        pub title: String,
        pub icon: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Message)]
    #[expect(
        clippy::struct_field_names,
        reason = "mirrors `ToFrontEnd::Results`, whose field is `results`"
    )]
    pub struct Results {
        pub version: u32,
        pub results: Vec<ShownResult>,
        pub scope: Option<Scope>,
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Message)]
    pub struct ToFrontEnd {
        #[bilrost(oneof(1))]
        pub variant: Option<ToFrontEndVariant>,
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Oneof)]
    pub enum ToFrontEndVariant {
        #[bilrost(1)]
        Results(Results),
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Message)]
    pub struct Query {
        pub text: String,
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Message)]
    pub struct FromFrontEnd {
        #[bilrost(oneof(1))]
        pub variant: Option<FromFrontEndVariant>,
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Oneof)]
    pub enum FromFrontEndVariant {
        #[bilrost(1)]
        Query(Query),
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Message)]
    pub struct AddItems {
        pub items: Vec<Item>,
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Message)]
    pub struct FromExtension {
        #[bilrost(oneof(1))]
        pub variant: Option<FromExtensionVariant>,
    }

    #[derive(Clone, Debug, PartialEq, ::bilrost::Oneof)]
    pub enum FromExtensionVariant {
        #[bilrost(1)]
        AddItems(AddItems),
    }

    fn action(action: &real::Action) -> Action {
        Action {
            id: action.id.clone(),
            title: action.title.clone(),
            shortcut: action.shortcut.clone(),
            arguments: action
                .arguments
                .iter()
                .map(|argument| Argument {
                    name: argument.name.clone(),
                    placeholder: argument.placeholder.clone(),
                    optional: argument.optional,
                })
                .collect(),
            confirm: action.confirm,
        }
    }

    fn item(item: &real::Item) -> Item {
        let mirror = super::prost::item(item);
        Item {
            id: mirror.id,
            kind: mirror.kind,
            desktop_id: mirror.desktop_id,
            path: mirror.path,
            address: mirror.address,
            url: mirror.url,
            identity: mirror.identity,
            title: mirror.title,
            subtitle: mirror.subtitle,
            icon: mirror.icon,
            keyword: mirror.keyword,
            keywords: mirror.keywords,
            actions: item.actions.iter().map(action).collect(),
            hidden: mirror.hidden,
            app_only: mirror.app_only,
            weak_keywords: mirror.weak_keywords,
        }
    }

    pub fn results(message: &real::ToFrontEnd) -> ToFrontEnd {
        let real::ToFrontEnd::Results {
            version,
            results,
            scope,
        } = message
        else {
            panic!("only Results is mirrored");
        };
        ToFrontEnd {
            variant: Some(ToFrontEndVariant::Results(Results {
                version: *version,
                results: results
                    .iter()
                    .map(|result| ShownResult {
                        extension: result.extension.clone(),
                        item: item(&result.item),
                        contributed: result
                            .contributed
                            .iter()
                            .map(|offered| OfferedAction {
                                extension: offered.extension.clone(),
                                action: action(&offered.action),
                            })
                            .collect(),
                        highlights: result.highlights.clone(),
                        alias: result.alias.clone(),
                        unavailable: result.unavailable.clone(),
                    })
                    .collect(),
                scope: scope.as_ref().map(|scope| Scope {
                    word: scope.word.clone(),
                    title: scope.title.clone(),
                    icon: scope.icon.clone(),
                }),
            })),
        }
    }

    pub fn query(message: &real::FromFrontEnd) -> FromFrontEnd {
        let real::FromFrontEnd::Query { text } = message else {
            panic!("only Query is mirrored");
        };
        FromFrontEnd {
            variant: Some(FromFrontEndVariant::Query(Query { text: text.clone() })),
        }
    }

    pub fn batch(message: &real::FromExtension) -> FromExtension {
        let real::FromExtension::AddItems { items } = message else {
            panic!("only AddItems is mirrored");
        };
        FromExtension {
            variant: Some(FromExtensionVariant::AddItems(AddItems {
                items: items.iter().map(item).collect(),
            })),
        }
    }
}
