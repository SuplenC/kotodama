# 12. The protocol version goes up with every change to a message's layout

Status: accepted (2026-10-20), amended by ADR 0018 (one version for every Wire format)

Both sides of a connection send `PROTOCOL_VERSION` in their hello, and the handshake fails unless the two are equal. That only protects a peer if the version changes whenever a message does. Postcard encodes a struct's fields by position and an enum's variants by index, so a peer built before a field was added misreads everything after it: it passes the handshake and then fails at some later decode, or decodes garbage.

`PROTOCOL_VERSION` goes up by one with any change that a peer built before it could fail to read in some Wire format:

- adding, removing, reordering or retyping a field of a message, or of any type a message carries, even an optional one;
- adding, removing or reordering an enum variant;
- renaming a field or a variant, which JSON and MessagePack see;
- a rule that refuses what used to be valid, such as a new shared Item kind or a newly required field.

A change that only makes decoding accept more, such as adding `#[serde(default)]` to a field, needs no new version. `Hello` itself never changes, because it is how the versions are compared.

`tests/layout.rs` in `kotodama-protocol` enforces the rule. It encodes a sample of every message, every enum variant and every field in postcard and JSON, and compares the result with `tests/layout.txt`, whose first line names the version it belongs to. A changed encoding under the same version fails, and the test only rewrites `layout.txt` (with `KOTODAMA_BLESS_LAYOUT=1`) once the version has gone up. Samples are written without `..`, so a new field doesn't compile until it has one, and a new variant without one fails the test.

Version 2 applies the rule to what changed after version 1: the Front end messages, the Item `identity` and the `copy` Outcome.

## Considered options

- **A range of versions each side accepts.** Rejected for now: the Daemon, the Front end and the Bundled extensions ship together, and keeping old layouts decodable would mean keeping old types. One number keeps the handshake a single comparison.
- **Bump only for changes postcard sees, since JSON and MessagePack tolerate added fields.** Rejected: a JSON Extension would then pass the handshake with a Daemon that has renamed a field it sends. Every Extension updating its hello's version on each change is the cost.
- **Version only the postcard layout, and fall back to JSON or MessagePack when it differs.** Possible later: it would spare JSON Extensions a change for every field added, but needs a second number in the hello.
- **Pin a hash of the encodings instead of the encodings.** Rejected: a failing hash says nothing about which message changed.
