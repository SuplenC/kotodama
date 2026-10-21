//! Length-prefixed frames over any byte stream: a Unix socket or an Extension's
//! stdio.
//!
//! A frame is the payload's length in ASCII decimal, a newline, then the payload:
//! `7\n{"a":1}`. The header is text so a shell script can read a frame with
//! `read -r len` and `read -r -N "$len"`, and write one with `printf '%d\n%s'`.

use std::io;
use std::io::BufRead;
use std::io::Write;

use crate::Error;

/// The longest payload a frame may carry. A peer announcing more is broken or
/// hostile, so the reader refuses before allocating.
pub const MAX_FRAME_LEN: usize = 64 << 20;

/// Enough digits for any length up to and past `MAX_FRAME_LEN`, and few enough
/// that the length can't overflow even a 32-bit `usize`.
const MAX_HEADER_DIGITS: usize = 9;

/// Writes one frame and flushes it.
pub fn write(writer: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    let mut header = itoa::Buffer::new();
    writer.write_all(header.format(payload.len()).as_bytes())?;
    writer.write_all(b"\n")?;
    writer.write_all(payload)?;
    writer.flush()
}

/// Reads the next frame's payload into `buf`, replacing its contents. Returns
/// `false` when the stream ends cleanly between frames.
pub fn read(reader: &mut impl BufRead, buf: &mut Vec<u8>) -> Result<bool, Error> {
    let Some(len) = read_header(reader)? else {
        return Ok(false);
    };
    if len > MAX_FRAME_LEN {
        return Err(Error::FrameTooLong(len));
    }
    buf.clear();
    buf.resize(len, 0);
    reader.read_exact(buf)?;
    Ok(true)
}

fn read_header(reader: &mut impl BufRead) -> Result<Option<usize>, Error> {
    let mut len: usize = 0;
    let mut digits = 0;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if digits == 0 {
                Ok(None)
            } else {
                Err(io::Error::from(io::ErrorKind::UnexpectedEof).into())
            };
        }
        let mut used = 0;
        for &byte in available {
            used += 1;
            match byte {
                b'\n' if digits > 0 => {
                    reader.consume(used);
                    return Ok(Some(len));
                }
                b'0'..=b'9' if digits < MAX_HEADER_DIGITS => {
                    len = len * 10 + usize::from(byte - b'0');
                    digits += 1;
                }
                _ => return Err(Error::BadFrameHeader),
            }
        }
        reader.consume(used);
    }
}
