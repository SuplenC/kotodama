# kotodama-extension

What every Rust Extension of [Kotodama](https://github.com/SuplenC/kotodama) needs, so each one only holds what it does: the handshake and the Daemon's messages over stdio, lean memory, the config readers, sending only the Items that changed, the Extension's status, starting programs in a systemd scope of their own, and, with the `watch` feature, watching directories. Kotodama's Bundled extensions are built on it.

`kotodama extension new <name> --lang rust` writes an Extension to start from, and the crate docs show a whole one. [docs/extensions.md](https://github.com/SuplenC/kotodama/blob/main/docs/extensions.md) describes what an Extension can do.

## Versions

The crate's `major.minor` is the protocol version it speaks, as [`kotodama-protocol`](https://crates.io/crates/kotodama-protocol)'s is: `kotodama-extension` 1.0.x speaks protocol 1.0, and 1.1.x would speak 1.1.

- A minor version of the protocol only adds, and comes out as a minor version of both crates.
- Any other change to the protocol is a new major version of it, and of both crates.
- A patch release leaves the protocol as it is. It fixes bugs, and may add to the Rust API where that changes nothing that is sent.
- A change to the Rust API that would break code built on it waits for the protocol's next major version.

So depend on both at the protocol version your Extension speaks:

```toml
[dependencies]
kotodama-extension = "1.0"
kotodama-protocol = "1.0"
```

That takes every 1.0.x release. An Extension that speaks 1.0 keeps working with every Kotodama from 1.0 on: the Daemon talks to a peer on an older minor version by sending it nothing that version lacks, and keeps every major version it has spoken. A newer Extension than the Daemon is turned away, with an error that says to update Kotodama.

## License

Apache-2.0.
