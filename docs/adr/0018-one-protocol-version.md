# 18. One protocol version covers every Wire format

Status: accepted (2026-10-20), amended by ADR 0026 (2026-10-21): one version still covers every Wire format, but it is now `major.minor`, and postcard alone needs the same version on both sides

This amends ADR 0012. Every layout change bumps `PROTOCOL_VERSION`, including one that only postcard sees, such as a reordered field. A third-party JSON Extension whose messages read the same is still turned away until it updates its hello. #79 asked whether postcard should get its own layout version, with a fallback to MessagePack or JSON when that version differs.

We keep one `PROTOCOL_VERSION` for all Wire formats. The rule in ADR 0012 stays as it is.

- **Before v1, a third-party Extension re-checks the version anyway.** Field names and messages still change often enough that a JSON Extension has to follow most releases. A separate postcard number would rarely save it an update.
- **Two numbers are easy to get wrong.** Every change would need someone to decide which formats it affects. A rename that JSON sees but postcard doesn't, or the reverse, would bump the wrong number and pass review.
- **The hello stays smaller.** It carries one number and the handshake is still a single comparison.
- **The layout guard stays simple.** `tests/layout.rs` checks postcard and JSON against one version. Two versions would mean splitting `layout.txt` by format and teaching the test which number each format's change belongs to.

## When to revisit

Once third-party JSON Extensions exist in numbers and postcard-only changes come often enough that the forced updates hurt them. The option from ADR 0012 is still the way to do it: version the postcard layout separately and fall back to MessagePack or JSON when it differs.

## Considered options

- **A separate postcard layout version, with a fallback to MessagePack or JSON.** Rejected for now, for the reasons above.
