//! The Protocol Buffers Wire format, written and read straight from the
//! protocol's serde types, so one set of types serves every Wire format.
//!
//! The bytes are standard proto3, and `kotodama.proto` beside this crate
//! describes them for other languages' generated code. serde's data model maps
//! onto protobuf by position:
//!
//! - A struct is a message, and its fields are numbered from 1 in the order they are declared. A
//!   field is never renumbered, so fields are only ever added at the end.
//! - An enum is a message holding a `oneof` of its variants, numbered from 1 in declaration order.
//!   A unit variant holds an empty message, a newtype variant its value, and a struct variant a
//!   message of its fields.
//! - `Option` is a field that is present or absent. A `Vec` is a repeated field, packed for
//!   numbers. A map is protobuf's `map`, which is a repeated message of a key (field 1) and a value
//!   (field 2).
//! - A list or a map where protobuf allows no repeated field (an element of a list, a map's value,
//!   a variant) is wrapped in a message holding it as field 1.
//! - Unsigned integers are `uint32`/`uint64`, signed ones `sint64` (zigzag), `f32` is `float` and
//!   `f64` `double`.
//!
//! Decoding follows proto3: fields may come in any order, unknown fields are
//! skipped, and a field that is absent takes protobuf's default (zero, false,
//! empty), so a peer's generated code may leave out whatever holds a default.
//!
//! # Owns
//! The encoding and decoding of one message.
//!
//! # Not here
//! Which messages a peer may be sent: `version.rs`. The frame around a message:
//! `frame.rs`.

mod de;
mod ser;

use std::fmt;

pub(crate) use de::from_slice;
pub(crate) use ser::to_vec;

const VARINT: u8 = 0;
const FIXED64: u8 = 1;
const LEN: u8 = 2;
const FIXED32: u8 = 5;

/// Why a message couldn't be encoded or decoded as protobuf.
#[derive(Debug)]
pub(crate) struct ProtobufError(String);

impl ProtobufError {
    fn new(message: impl Into<String>) -> Self {
        ProtobufError(message.into())
    }
}

impl fmt::Display for ProtobufError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProtobufError {}

impl serde::ser::Error for ProtobufError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        ProtobufError(message.to_string())
    }
}

impl serde::de::Error for ProtobufError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        ProtobufError(message.to_string())
    }
}

fn write_varint(buf: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        buf.push(value as u8 | 0x80);
        value >>= 7;
    }
    buf.push(value as u8);
}

fn varint_len(value: u64) -> usize {
    // Each byte carries 7 bits; zero still takes one byte.
    (64 - (value | 1).leading_zeros() as usize).div_ceil(7)
}

fn write_key(buf: &mut Vec<u8>, field: u32, wire_type: u8) {
    write_varint(buf, (u64::from(field) << 3) | u64::from(wire_type));
}

fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

fn unzigzag(value: u64) -> i64 {
    (value >> 1) as i64 ^ -((value & 1) as i64)
}

#[cfg(test)]
mod tests;
