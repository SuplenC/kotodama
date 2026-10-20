# kotodama-protocol code map

The shared protocol. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. Layout changes follow ADR 0012: see `tests/layout.rs`. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `lib.rs`: re-exports; `Error`.
- `message.rs`: Daemon and Extension messages, `ToExtension`, `FromExtension`, `LiveResult`, `Outcome`, `Clipboard`, `CatalogEntry`.
- `front_end.rs`: Daemon and Front end messages, `FromFrontEnd` (incl. `Query`, `Viewport`, `Open`, `Toggle`), `ToFrontEnd` (incl. `ReplaceView`, `FilteredView`), `ShownResult`, `ListMatch`, `socket_path`.
- `item.rs`: `Item` (incl. `hidden`, ADR 0015), `ItemRef`, `Action` (`Action::new` with `shortcut` and `arguments` setters), `ItemKind`, `SHARED_KINDS`, `NamespacedKind`, `KindName`.
- `view.rs`: `View`, `ViewContent` (a list's `filters_itself`), forms (`FormField`, `FormInput`), `Job`, `JobState`.
- `value.rs`: `Value`, `Table`, `Bytes`, `Fraction`.
- `manifest.rs`: `Manifest::from_toml`, `LiveQueries`, `ContributedAction` (ADR 0002).
- `handshake.rs`: `PROTOCOL_VERSION`, `Hello::negotiate` (ADR 0012).
- `frame.rs`: length-prefixed frames, `MAX_FRAME_LEN` (ADR 0004).
- `wire.rs`: `WireFormat` (postcard, JSON, MessagePack) `encode`/`decode`.
- `channel.rs`: `Channel::handshake`, `send`, `recv`, `split`; `ChannelReader::has_buffered` (what came with the last message).
- `launch.rs:launch_command`: detached program start, `KOTODAMA_LAUNCH_PREFIX`.
