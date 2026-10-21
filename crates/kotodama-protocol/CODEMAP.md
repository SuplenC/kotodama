# kotodama-protocol code map

The shared protocol. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. Layout changes follow ADR 0026: see `tests/layout.rs`. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `lib.rs`: re-exports; `Error`.
- `message.rs`: Daemon and Extension messages, `ToExtension`, `FromExtension` (incl. `Status`, whether the Extension can do anything now, with a note for its app's intro card), `LiveResult`, `Outcome` (with `RunCatalogAction`, a confirmed Proposal the Daemon runs), `Clipboard`, `CatalogEntry`.
- `front_end.rs`: Daemon and Front end messages, `FromFrontEnd` (incl. `Query`, `Viewport`, `SelectionMoved`, `RunAction` with the `shown` Results version, `Open`, `Toggle`, `Undo`, `Shown`: the menu became visible), `ToFrontEnd` (incl. `Results` and its `version` and `Scope`, the app the query is in, with its `Intro` card while nothing is typed, and the Extension's `note` on it; `APP_ENTRY_KIND`, an app's entry Item; `SetQuery`; `Reached`, how many menus an `Open` or `Toggle` reached, sent to the Front end that asked; `ReplaceView`, `FilteredView`, `OfferUndo`/`WithdrawUndo` for the Toast), `ShownResult` (with its `alias` badge and why it is `unavailable`; an unavailable one decodes without Actions), `ListMatch`, `socket_path`.
- `shortcut.rs`: Reserved keys, `RESERVED_SHORTCUTS`, `is_reserved_shortcut` (however a binding is spelled), `FORGET_SHORTCUT`, `UNDO_SHORTCUT`.
- `item.rs`: `Item` (incl. `hidden`, ADR 0015, and `app_only`, ADR 0027), `ItemRef`, `Action` (`Action::new` with `shortcut`, `arguments` and `confirm` setters; `confirm` has the Front end ask before it runs), `ItemKind` (`ItemKind::shared_index`, its place in `SHARED_KINDS`), `SHARED_KINDS`, `NamespacedKind`, `KindName`.
- `view.rs`: `View`, `ViewContent` (a list's `filters_itself`), forms (`FormField`, `FormInput`), `Job`, `JobState`.
- `value.rs`: `Value`, `Table`, `Bytes`, `Fraction`.
- `manifest.rs`: `Manifest::from_toml`, `LiveQueries` (`fallback`), `ContributedAction` (ADR 0002); the app (ADR 0027): `word` (or `prefix`, its old name), `title`, `icon`, `about`, `examples`, `commands`, `in_search`; `is_valid_word`.
- `version.rs`: `Version`, `PROTOCOL_VERSION`, `FIRST_VERSION`, `Since` (the minor version a message needs), `unread` (a field left out for an older minor version) (ADR 0026).
- `handshake.rs`: `Hello` with `minor`, `accept_peer` (the Daemon's side: a newer peer is refused), `accept_daemon` (a peer's side), `negotiate` (postcard only on the same version).
- `frame.rs`: length-prefixed frames, `MAX_FRAME_LEN` (ADR 0004).
- `wire.rs`: `WireFormat` (postcard, protobuf, MessagePack, JSON) `encode`/`decode`; `encode_for` (for a peer on an older minor version).
- `protobuf.rs`: the protobuf Wire format straight from the serde types (`protobuf/ser.rs`, `protobuf/de.rs`); the mapping is in its module docs.
- `channel.rs`: `Channel::handshake` (a peer's side), `Channel::accept` (the Daemon's), `send` (for the peer's version), `reads` (whether the peer's version reads a message), `recv`, `split`; `ChannelReader::has_buffered` (what came with the last message).
- `launch.rs:launch_command`: detached program start, `KOTODAMA_LAUNCH_PREFIX`; `launch_prefix` (none when `KOTODAMA_LAUNCH_SCOPED` marks it as `SCOPE_PREFIX`, which only makes a scope).
- `../benches/batch.rs`: encoding and decoding 10k Indexed items in each Wire format.
- `../benches/formats.rs`: the Daemon's real messages in every Wire format and in prost, müsli `wire` and bilrost (`tests/support/mirror.rs`: their copies of the types; `tests/support/messages.rs`: the messages; both shared with `tests/protobuf.rs`).
- `../kotodama.proto`: the protobuf schema for other languages, written by `tests/layout.rs`.
- `../tests/layout.rs`: pins every message's encodings per version in `tests/layouts/<major>.<minor>.txt` and checks what an older minor version is sent; `tests/support/schema.rs` writes `kotodama.proto`. `../tests/protobuf.rs`: prost reads the protobuf Wire format and back.
- `../tests/protocol_doc.rs`: writes `docs/protocol.md`, the message-by-message reference, from this version's pinned samples (`describe` holds each sample's words); fails when it is stale, and `KOTODAMA_BLESS_LAYOUT=1` rewrites it.
- `../tests/{shell,python,typescript}_extension.rs`: drive the example Extensions in `examples/` as the Daemon does (`tests/support/example.rs`: starting one and speaking to it).
