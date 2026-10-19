# 4. Frames have a text length header

Status: accepted (2026-10-19)

Every message travels as a frame: the payload's length in ASCII decimal, a newline, then the payload, as in `7\n{"a":1}`. A frame may carry at most 64 MiB. The same framing is used for every Wire format and for the handshake, so a reader never needs to know the format to find the end of a message.

A text header lets a bash Extension read a frame with `read -r len` and `read -r -N "$len"`, and write one with `printf '%d\n%s'`. Parsing a few digits costs nothing next to decoding the payload.

## Considered options

- **A 4-byte binary length.** Rejected: reading and writing it from a shell script needs `od`, `dd` or `printf` escapes, and the shell Extension is the reference for authors.
- **Newline-delimited JSON, with length prefixes only for binary formats.** Rejected: two framings, switched mid-connection after the handshake.
