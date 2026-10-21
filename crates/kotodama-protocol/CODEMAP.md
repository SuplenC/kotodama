# kotodama-protocol code map

The shared protocol. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. Layout changes follow ADR 0012: see `tests/layout.rs`. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `lib.rs`: re-exports; `Error`.
- `message.rs`: Daemon and Extension messages, `ToExtension`, `FromExtension`, `LiveResult`, `Outcome` (with `RunCatalogAction`, a confirmed Proposal the Daemon runs), `Clipboard`, `CatalogEntry`.
- `front_end.rs`: Daemon and Front end messages, `FromFrontEnd` (incl. `Query`, `Viewport`, `SelectionMoved`, `RunAction` with the `shown` Results version, `Open`, `Toggle`, `Undo`, `Shown`: the menu became visible), `ToFrontEnd` (incl. `Results` and its `version` and `Scope`, the Prefix the query is behind; `SetQuery`; `ReplaceView`, `FilteredView`, `OfferUndo`/`WithdrawUndo` for the Toast), `ShownResult` (with its `alias` badge and why it is `unavailable`; an unavailable one decodes without Actions), `ListMatch`, `socket_path`.
- `shortcut.rs`: Reserved keys, `RESERVED_SHORTCUTS`, `is_reserved_shortcut` (however a binding is spelled), `FORGET_SHORTCUT`, `UNDO_SHORTCUT`.
- `item.rs`: `Item` (incl. `hidden`, ADR 0015), `ItemRef`, `Action` (`Action::new` with `shortcut` and `arguments` setters), `ItemKind` (`ItemKind::shared_index`, its place in `SHARED_KINDS`), `SHARED_KINDS`, `NamespacedKind`, `KindName`.
- `view.rs`: `View`, `ViewContent` (a list's `filters_itself`), forms (`FormField`, `FormInput`), `Job`, `JobState`.
- `value.rs`: `Value`, `Table`, `Bytes`, `Fraction`.
- `manifest.rs`: `Manifest::from_toml`, `LiveQueries` (`fallback`), `ContributedAction` (ADR 0002); `prefix`, `title` and `icon` (ADR 0024), `is_valid_prefix`.
- `handshake.rs`: `PROTOCOL_VERSION`, `Hello::negotiate` (ADR 0012).
- `frame.rs`: length-prefixed frames, `MAX_FRAME_LEN` (ADR 0004).
- `wire.rs`: `WireFormat` (postcard, JSON, MessagePack) `encode`/`decode`.
- `channel.rs`: `Channel::handshake`, `send`, `recv`, `split`; `ChannelReader::has_buffered` (what came with the last message).
- `launch.rs:launch_command`: detached program start, `KOTODAMA_LAUNCH_PREFIX`; `launch_prefix` (none when `KOTODAMA_LAUNCH_SCOPED` marks it as `SCOPE_PREFIX`, which only makes a scope).
- `../benches/batch.rs`: encoding and decoding 10k Indexed items in each Wire format.
