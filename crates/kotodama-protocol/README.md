# kotodama-protocol

The protocol [Kotodama](https://github.com/SuplenC/kotodama) speaks: the messages between the Daemon, its Front ends and its Extensions, their framing and Wire formats, the handshake, the Manifest an Extension is described by, and how programs are launched. A Rust Extension usually builds on [`kotodama-extension`](https://crates.io/crates/kotodama-extension), which runs the connection over this crate. [docs/extensions.md](https://github.com/SuplenC/kotodama/blob/main/docs/extensions.md) describes the protocol for Extensions in any language.

## Versions

The crate's `major.minor` is the protocol version it speaks, `PROTOCOL_VERSION`: `kotodama-protocol` 1.0.x speaks protocol 1.0, and 1.1.x would speak 1.1.

- A minor version of the protocol only adds: an optional field, a message, an enum variant. It comes out as a minor version of the crate.
- Any other change to the protocol is a new major version of it, and of the crate.
- A patch release leaves the protocol as it is. It fixes bugs, and may add to the Rust API where that changes nothing that is sent.
- A change to the Rust API that would break code built on it waits for the protocol's next major version.

So depend on the protocol version your Extension speaks:

```toml
[dependencies]
kotodama-protocol = "1.0"
```

That takes every 1.0.x release. An Extension that speaks 1.0 keeps working with every Kotodama from 1.0 on: the Daemon talks to a peer on an older minor version by sending it nothing that version lacks, and keeps every major version it has spoken. `kotodama-extension` follows the same rule, so the two always go at the same `major.minor`.

## License

Apache-2.0.
