# 26. The protocol stays compatible from 1.0: minor versions add, and every major version is kept

Status: accepted (2026-10-21). Supersedes ADR 0012 and amends ADR 0018.

Until now both sides of a connection had to name the same `PROTOCOL_VERSION` (ADR 0012), and every change to a message raised it. That was fine while every peer shipped with the Daemon. From Kotodama v1, Extensions written by other people have to keep working when the user updates Kotodama, and one written in Python or Go can't follow every release.

## Versions

- **A version is `major.minor`.** Today's protocol is 1.0. The old single number had reached 10, but no release spoke it, so numbering starts over. A peer built before this change names a version from 2 to 10 and is turned away as too new. That is the one wart, and nothing released is affected.
- **The hello keeps `version` and adds `minor`:** `{"version":1,"minor":0,"formats":["json"]}`. `version` is the major. `minor` defaults to 0, so a hello without it means `<major>.0`.
- **A minor version only adds:** an optional field (`#[serde(default)]`) at the end of a struct, a new message, or a new enum variant, including a new shared Item kind. Anything else is a new major version: removing, renaming, retyping or reordering a field or a variant, making a field required, or a rule that refuses what used to be valid.
- **Protocol 1.0 freezes at the Kotodama v1 release.** Until then, 1.0 can still change by deleting `tests/layouts/1.0.txt` and pinning it again, which shows in review. After the release that file is never rewritten.

## The handshake

Both sides still send their hello at once, in JSON.

- **The Daemon (`Channel::accept`, `Hello::accept_peer`) turns away a peer newer than itself.** The error says what to do: "the peer needs protocol 1.3, newer than this Daemon's 1.2: update Kotodama". It also refuses a version older than 1.0 (`PeerTooOld`). It accepts every version from `FIRST_VERSION` (1.0) up to its own.
- **A peer (`Channel::handshake`, `Hello::accept_daemon`) turns away a Daemon older than itself** (`DaemonTooOld`). The Rust peers (`kotodama-extension`, the QML Front end, the CLI) do this already. The shell example shows how to do it in jq. A peer newer than the Daemon therefore gets a clear error on both sides: the Daemon logs the reason, and the peer's own check says the same.
- **Postcard is only used when both sides name the same version.** It encodes fields by position with nothing to mark where one ends, so a field one side lacks garbles everything after it. Between different versions, the fastest format both sides offer from protobuf, MessagePack and JSON is used. All three skip unknown fields and give a missing field its default. JSON stays required.

## Not sending a peer what it can't read

The Daemon adapts down. A peer only ever speaks its own version.

- **Fields.** A field added in minor version N is `#[serde(default, skip_serializing_if = "crate::version::unread::<N, _>")]`. `WireFormat::encode_for(peer, …)` (what `Channel::send` calls) puts an older peer's minor version in a thread-local for the length of the encode, and `unread` reads it. For a peer on the Daemon's own version nothing is set, and the check is one comparison.
- **Messages and variants.** `Since` gives the minor version a message needs. It looks into what the message carries: an Item of a newer kind among the Results, or a newer form input in a pushed View. The writer thread (`kotodamad/src/link.rs`) skips a message the peer can't read, through `ChannelWriter::reads`, which returns at once for a peer on the Daemon's version. The `Since` impls match every variant, so a new variant doesn't compile until it names its minor version.
- **What the Daemon receives needs nothing.** An older peer never sends what its version lacks, and missing fields take their defaults.

Dropping a whole message is coarse: `Results` with one Item of a newer kind is dropped for an older Front end, rather than sent without that Item. No minor version adds anything yet, so this costs nothing today. Finer filtering is for the first minor version that adds an Item kind.

## A future major version

A new major version keeps every older one. When 2.0 comes, the 1.x types are frozen as they are in a module of their own, and the Daemon translates at the Channel: it decodes 1.x and converts up, and converts down and encodes 1.x. The hello already carries the major. `accept_peer` then accepts the kept majors, and `FIRST_VERSION` stays 1.0. None of this is built yet, because there is no 2.0. The pinned 1.x layouts are what the frozen types will have to match.

## Enforcement

- **`tests/layout.rs`** pins a sample of every message, every variant and every field in all four formats, one file per version: `tests/layouts/<major>.<minor>.txt`.
  - The current version's file must match exactly, and a pinned file is never rewritten.
  - For every older minor version of the current major:
    - each recorded protobuf, MessagePack and JSON encoding still decodes;
    - each sample encoded for that version is byte for byte what that version recorded, so a new field without `unread` fails;
    - no recorded message is gone;
    - a message or variant added since then has a `since()` above that minor version.
  - Its own tests feed it a doctored older layout, a removed message and a new variant without `Since`.
- **`kotodama.proto`**, the schema for other languages, is written by the same test from the samples, by the same mapping the codec uses, and pinned. `protoc` parses it and decodes every pinned protobuf sample with no unknown fields. `tests/protobuf.rs` checks that prost reads what this crate encodes and that this crate reads what prost encodes.
- **`kotodamad/tests/kotodamad.rs`** runs the binary with recorded 1.0 peers:
  - a Front end that sends 1.0's pinned protobuf bytes for a query and reads the Results;
  - the recording Extension, whose hello names 1.0 and which speaks JSON.
  - A Front end and an Extension that name the next major version are turned away, and the reason is in the Daemon's log.

## Wire formats measured

The issue asked whether one binary format that tolerates added and missing fields could replace postcard. The owner also asked for Protocol Buffers as a Wire format in any case, so that non-Rust Extensions get a fast, tolerant binary format, and for müsli's `wire` format (and bilrost) to be measured only as a replacement for postcard between Rust peers. FlatBuffers, Cap'n Proto and CBOR were not measured.

`benches/formats` encodes and decodes the Daemon's real messages: a query, 50 Results (the keystroke path), and a batch of 1,000 Items. Medians in µs, criterion, on an Apple M-series host:

| Message | postcard | protobuf (ours) | MessagePack | JSON | prost | müsli `wire` | bilrost |
|---|---|---|---|---|---|---|---|
| 50 Results, encode | 6.8 | 8.0 | 8.5 | 24.8 | 8.8 | 5.1 | 9.9 |
| 50 Results, decode | 31.4 | 59.6 | 63.6 | 69.6 | 38.7 | 39.8 | 38.6 |
| 50 Results, both | **38.2** | 67.9 | 72.1 | 94.4 | 47.5 | 44.9 | 48.5 |
| 1k Items, encode | 81.1 | 84.9 | 86.1 | 265 | 83.5 | 70.8 | 81.7 |
| 1k Items, decode | 350 | 653 | 697 | 730 | 482 | 477 | 432 |
| Query, decode | 0.017 | 0.028 | 0.062 | 0.050 | 0.023 | 0.026 | 0.024 |

prost, müsli and bilrost need types of their own (`benches/formats/mirror.rs`). So postcard and our protobuf are measured both on the protocol's own types, which check every Item as they decode it, and on those same copies. The table shows the copies for those two, so every column decodes the same types. On the protocol's own types the figures are within a few percent of these.

- **Postcard stays between peers on the same version.** It is the fastest to decode, and a keystroke is the Daemon encoding Results and the Front end decoding them. müsli `wire` encodes faster but decodes 27% slower: 17% more for the two together. bilrost and prost are 24–27% slower together. None of them comes within a few percent, which the issue set as the bar.
- **Protobuf is added as a Wire format,** preferred after postcard and before MessagePack. It is written and read straight from the serde types (`src/protobuf.rs`), not through prost's derive. prost would need a second copy of every type and a conversion on every message, and the protocol would have two sets of types to keep in step. Ours decodes 1.5 to 1.9 times slower than prost, but faster than MessagePack and JSON, the other formats a peer on another version can use. The mapping:
  - struct fields are numbered by position;
  - an enum is a `oneof` numbered by variant;
  - `Option` means presence;
  - a `Vec` is repeated, packed for numbers;
  - a map is a protobuf `map`;
  - signed integers are zigzag-encoded;
  - a list or map where protobuf allows none is wrapped in a message.

  This mapping follows the same rule as a minor version: fields are only added at the end.

Instruction counts per keystroke (`scripts/bench-instructions`, base against this change) are unchanged: every benchmark runs one instruction fewer.

## Considered options

- **Keep one exact version (ADR 0012, 0018).** Rejected: every release that touches a message would break every third-party Extension.
- **Drop postcard for one tolerant format everywhere.** Rejected: the Bundled extensions and the Front end always ship with the Daemon. A tolerant format would cost them 17% or more on the keystroke path for nothing.
- **prost with derive for the protobuf format.** Rejected for the reasons above. It is used only in the benchmark and the interoperability test.
- **Negotiate down to the lower version on both sides.** Rejected: the Daemon adapts, and peers stay simple. Only the Daemon carries the cost of compatibility.
- **Give postcard its own layout version (ADR 0018's option).** No longer needed: postcard is only used between peers on the same version.
