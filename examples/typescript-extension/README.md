# A Kotodama Extension in TypeScript, over Protocol Buffers

`extension.ts` is a whole Extension in TypeScript that speaks the protobuf Wire format described by [`kotodama.proto`](../../crates/kotodama-protocol/kotodama.proto). Node 23.6 or later runs it as it is, stripping the types, so there is nothing to build and nothing to install. [docs/extensions.md](../../docs/extensions.md) is the guide to what it does.

Type `ts 1700000000` and the menu shows that moment as an ISO 8601 date, a Unix time and an HTTP date; `ts 2024-02-29` shows that date the same three ways, and `ts` alone shows now. Enter copies the form you pick; Ctrl+T types it into the window you came from.

It shows:

- the handshake, in JSON as always, offering `protobuf` and `json`: the Daemon then uses protobuf, which it prefers to JSON;
- reading `ToExtension` and writing `FromExtension` in protobuf: a message holds one field, the variant, numbered as in `kotodama.proto`;
- a Live query answered with `live_results`, and two Actions answered with `copy` and `type` outcomes;
- logging `rejected`, the Daemon's reason for refusing a message, to stderr.

`protobuf.ts` reads and writes the dozen fields the Extension uses, by hand, so that it needs no package. A bigger Extension generates its types from `kotodama.proto` instead, with `protoc-gen-es` (`@bufbuild/protobuf`) or `ts-proto`, and gets every message at once. Its `.proto` file is pinned to the protocol version, so the generated code stays right for every Daemon that speaks that version or a newer one.

## Trying it

```sh
kotodama extension check examples/typescript-extension --query 1700000000
ln -s "$PWD/examples/typescript-extension" ~/.config/kotodama/extensions/timestamp
```

`crates/kotodama-protocol/tests/typescript_extension.rs` runs it as the Daemon does.
