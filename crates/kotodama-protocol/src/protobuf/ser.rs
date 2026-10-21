//! Writing a serde value as a protobuf message.
//!
//! # Owns
//!
//! - [`to_vec`] and the serializers it writes with.
//!
//! # Not here
//!
//! - The mapping from serde onto protobuf: `protobuf.rs`'s docs. Reading a message back: `de.rs`.

use serde::ser;
use serde::ser::Impossible;
use serde::ser::Serialize;
use serde::ser::Serializer;

use super::FIXED32;
use super::FIXED64;
use super::LEN;
use super::ProtobufError;
use super::VARINT;
use super::varint_len;
use super::write_key;
use super::write_varint;
use super::zigzag;

type Result<T> = std::result::Result<T, ProtobufError>;

/// Appends `value`, a struct or an enum, to `buf` as one protobuf message.
pub(crate) fn to_vec<T: Serialize + ?Sized>(value: &T, buf: &mut Vec<u8>) -> Result<()> {
    value.serialize(Root { buf })
}

/// Starts a length-delimited body with one byte kept for its length, which is
/// all most bodies need; [`close`] fills it in.
fn open(buf: &mut Vec<u8>) -> usize {
    buf.push(0);
    buf.len()
}

/// Writes the length of the body that starts at `start`.
fn close(buf: &mut Vec<u8>, start: usize) {
    let len = buf.len() - start;
    if len < 0x80 {
        buf[start - 1] = len as u8;
        return;
    }
    // The body is too long for the one byte kept: move it up to make room for
    // the longer length.
    let extra = varint_len(len as u64) - 1;
    buf.resize(buf.len() + extra, 0);
    buf.copy_within(start..start + len, start + extra);
    let mut len = len as u64;
    for byte in &mut buf[start - 1..start + extra] {
        *byte = len as u8 | 0x80;
        len >>= 7;
    }
    buf[start + extra - 1] &= 0x7f;
}

fn not_a_message() -> ProtobufError {
    ProtobufError::new("a protobuf message must be a struct or an enum")
}

fn no_tuples() -> ProtobufError {
    ProtobufError::new("the protobuf Wire format has no tuples")
}

/// The top-level message: a struct's fields, or an enum's one variant, go
/// straight into the buffer, which the frame's length bounds.
struct Root<'a> {
    buf: &'a mut Vec<u8>,
}

macro_rules! refuse {
    ($($method:ident($($arg:ty),*);)*) => {
        $(fn $method(self, $(_: $arg),*) -> Result<()> {
            Err(not_a_message())
        })*
    };
}

impl<'a> Serializer for Root<'a> {
    type Ok = ();
    type Error = ProtobufError;
    type SerializeSeq = Impossible<(), ProtobufError>;
    type SerializeTuple = Impossible<(), ProtobufError>;
    type SerializeTupleStruct = Impossible<(), ProtobufError>;
    type SerializeTupleVariant = Impossible<(), ProtobufError>;
    type SerializeMap = Impossible<(), ProtobufError>;
    type SerializeStruct = Fields<'a>;
    type SerializeStructVariant = Fields<'a>;

    refuse! {
        serialize_bool(bool);
        serialize_i8(i8);
        serialize_i16(i16);
        serialize_i32(i32);
        serialize_i64(i64);
        serialize_u8(u8);
        serialize_u16(u16);
        serialize_u32(u32);
        serialize_u64(u64);
        serialize_f32(f32);
        serialize_f64(f64);
        serialize_char(char);
        serialize_str(&str);
        serialize_bytes(&[u8]);
        serialize_none();
        serialize_unit();
        serialize_unit_struct(&'static str);
    }

    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<()> {
        Err(not_a_message())
    }

    fn serialize_unit_variant(self, _: &'static str, index: u32, _: &'static str) -> Result<()> {
        write_key(self.buf, index + 1, LEN);
        self.buf.push(0);
        Ok(())
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, value: &T) -> Result<()> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        index: u32,
        _: &'static str,
        value: &T,
    ) -> Result<()> {
        value.serialize(Field {
            buf: self.buf,
            number: index + 1,
            wrap: true,
        })
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq> {
        Err(not_a_message())
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple> {
        Err(no_tuples())
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct> {
        Err(no_tuples())
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(no_tuples())
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap> {
        Err(not_a_message())
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct> {
        Ok(Fields::new(self.buf, &[]))
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        index: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant> {
        write_key(self.buf, index + 1, LEN);
        let start = open(self.buf);
        Ok(Fields::new(self.buf, &[start]))
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

/// A struct's fields, numbered from 1 as they are declared. A field serde
/// skips still takes its number, so the ones after it keep theirs.
pub(super) struct Fields<'a> {
    buf: &'a mut Vec<u8>,
    next: u32,
    /// The bodies to close at the end, innermost last: a struct variant opens
    /// one for its enum and one for itself.
    open: [usize; 2],
    opened: usize,
}

impl<'a> Fields<'a> {
    fn new(buf: &'a mut Vec<u8>, open: &[usize]) -> Self {
        let mut fields = Fields {
            buf,
            next: 1,
            open: [0; 2],
            opened: open.len(),
        };
        fields.open[..open.len()].copy_from_slice(open);
        fields
    }

    fn field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        let number = self.next;
        self.next += 1;
        value.serialize(Field {
            buf: self.buf,
            number,
            wrap: false,
        })
    }

    fn finish(self) {
        for &start in self.open[..self.opened].iter().rev() {
            close(self.buf, start);
        }
    }
}

impl ser::SerializeStruct for Fields<'_> {
    type Ok = ();
    type Error = ProtobufError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, value: &T) -> Result<()> {
        self.field(value)
    }

    fn skip_field(&mut self, _: &'static str) -> Result<()> {
        self.next += 1;
        Ok(())
    }

    fn end(self) -> Result<()> {
        self.finish();
        Ok(())
    }
}

impl ser::SerializeStructVariant for Fields<'_> {
    type Ok = ();
    type Error = ProtobufError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, value: &T) -> Result<()> {
        self.field(value)
    }

    fn skip_field(&mut self, _: &'static str) -> Result<()> {
        self.next += 1;
        Ok(())
    }

    fn end(self) -> Result<()> {
        self.finish();
        Ok(())
    }
}

/// The value of field `number`.
struct Field<'a> {
    buf: &'a mut Vec<u8>,
    number: u32,
    /// Where protobuf allows no repeated field, a list or a map goes in a
    /// message of its own, as that message's field 1.
    wrap: bool,
}

impl Field<'_> {
    fn varint(self, value: u64) {
        write_key(self.buf, self.number, VARINT);
        write_varint(self.buf, value);
    }

    fn bytes(self, value: &[u8]) {
        write_key(self.buf, self.number, LEN);
        write_varint(self.buf, value.len() as u64);
        self.buf.extend_from_slice(value);
    }
}

impl<'a> Serializer for Field<'a> {
    type Ok = ();
    type Error = ProtobufError;
    type SerializeSeq = Repeated<'a>;
    type SerializeTuple = Impossible<(), ProtobufError>;
    type SerializeTupleStruct = Impossible<(), ProtobufError>;
    type SerializeTupleVariant = Impossible<(), ProtobufError>;
    type SerializeMap = Entries<'a>;
    type SerializeStruct = Fields<'a>;
    type SerializeStructVariant = Fields<'a>;

    fn serialize_bool(self, value: bool) -> Result<()> {
        self.varint(u64::from(value));
        Ok(())
    }

    fn serialize_i8(self, value: i8) -> Result<()> {
        self.serialize_i64(i64::from(value))
    }

    fn serialize_i16(self, value: i16) -> Result<()> {
        self.serialize_i64(i64::from(value))
    }

    fn serialize_i32(self, value: i32) -> Result<()> {
        self.serialize_i64(i64::from(value))
    }

    fn serialize_i64(self, value: i64) -> Result<()> {
        self.varint(zigzag(value));
        Ok(())
    }

    fn serialize_u8(self, value: u8) -> Result<()> {
        self.serialize_u64(u64::from(value))
    }

    fn serialize_u16(self, value: u16) -> Result<()> {
        self.serialize_u64(u64::from(value))
    }

    fn serialize_u32(self, value: u32) -> Result<()> {
        self.serialize_u64(u64::from(value))
    }

    fn serialize_u64(self, value: u64) -> Result<()> {
        self.varint(value);
        Ok(())
    }

    fn serialize_f32(self, value: f32) -> Result<()> {
        write_key(self.buf, self.number, FIXED32);
        self.buf.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn serialize_f64(self, value: f64) -> Result<()> {
        write_key(self.buf, self.number, FIXED64);
        self.buf.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn serialize_char(self, value: char) -> Result<()> {
        self.bytes(value.encode_utf8(&mut [0; 4]).as_bytes());
        Ok(())
    }

    fn serialize_str(self, value: &str) -> Result<()> {
        self.bytes(value.as_bytes());
        Ok(())
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<()> {
        self.bytes(value);
        Ok(())
    }

    fn serialize_none(self) -> Result<()> {
        Ok(())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<()> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<()> {
        Ok(())
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<()> {
        Ok(())
    }

    fn serialize_unit_variant(self, _: &'static str, index: u32, _: &'static str) -> Result<()> {
        write_key(self.buf, self.number, LEN);
        let start = open(self.buf);
        write_key(self.buf, index + 1, LEN);
        self.buf.push(0);
        close(self.buf, start);
        Ok(())
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, value: &T) -> Result<()> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        index: u32,
        _: &'static str,
        value: &T,
    ) -> Result<()> {
        write_key(self.buf, self.number, LEN);
        let start = open(self.buf);
        value.serialize(Field {
            buf: &mut *self.buf,
            number: index + 1,
            wrap: true,
        })?;
        close(self.buf, start);
        Ok(())
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Repeated<'a>> {
        if self.wrap {
            write_key(self.buf, self.number, LEN);
            let start = open(self.buf);
            return Ok(Repeated {
                buf: self.buf,
                number: 1,
                packed: None,
                wrapper: Some(start),
            });
        }
        Ok(Repeated {
            buf: self.buf,
            number: self.number,
            packed: None,
            wrapper: None,
        })
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple> {
        Err(no_tuples())
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct> {
        Err(no_tuples())
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(no_tuples())
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Entries<'a>> {
        if self.wrap {
            write_key(self.buf, self.number, LEN);
            let start = open(self.buf);
            return Ok(Entries {
                buf: self.buf,
                number: 1,
                entry: 0,
                wrapper: Some(start),
            });
        }
        Ok(Entries {
            buf: self.buf,
            number: self.number,
            entry: 0,
            wrapper: None,
        })
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Fields<'a>> {
        write_key(self.buf, self.number, LEN);
        let start = open(self.buf);
        Ok(Fields::new(self.buf, &[start]))
    }

    fn serialize_struct_variant(self, _: &'static str, index: u32, _: &'static str, _: usize) -> Result<Fields<'a>> {
        write_key(self.buf, self.number, LEN);
        let outer = open(self.buf);
        write_key(self.buf, index + 1, LEN);
        let inner = open(self.buf);
        Ok(Fields::new(self.buf, &[outer, inner]))
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

/// A repeated field: numbers packed into one length-delimited run, anything
/// else one entry per element.
pub(super) struct Repeated<'a> {
    buf: &'a mut Vec<u8>,
    number: u32,
    /// The body of the packed run, once the first number opened it.
    packed: Option<usize>,
    /// The body of the message wrapping the list, if it is wrapped.
    wrapper: Option<usize>,
}

impl Repeated<'_> {
    /// The buffer to append one packed number to.
    fn packed(&mut self) -> &mut Vec<u8> {
        if self.packed.is_none() {
            write_key(self.buf, self.number, LEN);
            self.packed = Some(open(self.buf));
        }
        self.buf
    }
}

impl ser::SerializeSeq for Repeated<'_> {
    type Ok = ();
    type Error = ProtobufError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        value.serialize(Element { list: self })
    }

    fn end(self) -> Result<()> {
        if let Some(start) = self.packed {
            close(self.buf, start);
        }
        if let Some(start) = self.wrapper {
            close(self.buf, start);
        }
        Ok(())
    }
}

/// One element of a list: a number joins the packed run, anything else is an
/// entry of the repeated field.
struct Element<'r, 'a> {
    list: &'r mut Repeated<'a>,
}

impl<'r> Element<'r, '_> {
    fn entry(self) -> Field<'r> {
        let list = self.list;
        Field {
            buf: &mut *list.buf,
            number: list.number,
            wrap: true,
        }
    }
}

impl<'r> Serializer for Element<'r, '_> {
    type Ok = ();
    type Error = ProtobufError;
    type SerializeSeq = Repeated<'r>;
    type SerializeTuple = Impossible<(), ProtobufError>;
    type SerializeTupleStruct = Impossible<(), ProtobufError>;
    type SerializeTupleVariant = Impossible<(), ProtobufError>;
    type SerializeMap = Entries<'r>;
    type SerializeStruct = Fields<'r>;
    type SerializeStructVariant = Fields<'r>;

    fn serialize_bool(self, value: bool) -> Result<()> {
        self.serialize_u64(u64::from(value))
    }

    fn serialize_i8(self, value: i8) -> Result<()> {
        self.serialize_i64(i64::from(value))
    }

    fn serialize_i16(self, value: i16) -> Result<()> {
        self.serialize_i64(i64::from(value))
    }

    fn serialize_i32(self, value: i32) -> Result<()> {
        self.serialize_i64(i64::from(value))
    }

    fn serialize_i64(self, value: i64) -> Result<()> {
        write_varint(self.list.packed(), zigzag(value));
        Ok(())
    }

    fn serialize_u8(self, value: u8) -> Result<()> {
        self.serialize_u64(u64::from(value))
    }

    fn serialize_u16(self, value: u16) -> Result<()> {
        self.serialize_u64(u64::from(value))
    }

    fn serialize_u32(self, value: u32) -> Result<()> {
        self.serialize_u64(u64::from(value))
    }

    fn serialize_u64(self, value: u64) -> Result<()> {
        write_varint(self.list.packed(), value);
        Ok(())
    }

    fn serialize_f32(self, value: f32) -> Result<()> {
        self.list.packed().extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn serialize_f64(self, value: f64) -> Result<()> {
        self.list.packed().extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn serialize_char(self, value: char) -> Result<()> {
        self.entry().serialize_char(value)
    }

    fn serialize_str(self, value: &str) -> Result<()> {
        self.entry().serialize_str(value)
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<()> {
        self.entry().serialize_bytes(value)
    }

    fn serialize_none(self) -> Result<()> {
        Err(ProtobufError::new("protobuf can't hold an empty element in a list"))
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<()> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<()> {
        self.entry().serialize_bytes(&[])
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<()> {
        self.entry().serialize_bytes(&[])
    }

    fn serialize_unit_variant(self, name: &'static str, index: u32, variant: &'static str) -> Result<()> {
        self.entry().serialize_unit_variant(name, index, variant)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, value: &T) -> Result<()> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<()> {
        self.entry().serialize_newtype_variant(name, index, variant, value)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Repeated<'r>> {
        self.entry().serialize_seq(len)
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple> {
        Err(no_tuples())
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct> {
        Err(no_tuples())
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(no_tuples())
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Entries<'r>> {
        self.entry().serialize_map(len)
    }

    fn serialize_struct(self, name: &'static str, len: usize) -> Result<Fields<'r>> {
        self.entry().serialize_struct(name, len)
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Fields<'r>> {
        self.entry().serialize_struct_variant(name, index, variant, len)
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

/// A map: one entry message per key, holding the key as field 1 and the value
/// as field 2.
pub(super) struct Entries<'a> {
    buf: &'a mut Vec<u8>,
    number: u32,
    /// The body of the entry being written.
    entry: usize,
    wrapper: Option<usize>,
}

impl ser::SerializeMap for Entries<'_> {
    type Ok = ();
    type Error = ProtobufError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<()> {
        write_key(self.buf, self.number, LEN);
        self.entry = open(self.buf);
        key.serialize(Field {
            buf: &mut *self.buf,
            number: 1,
            wrap: true,
        })
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        value.serialize(Field {
            buf: &mut *self.buf,
            number: 2,
            wrap: true,
        })?;
        close(self.buf, self.entry);
        Ok(())
    }

    fn end(self) -> Result<()> {
        if let Some(start) = self.wrapper {
            close(self.buf, start);
        }
        Ok(())
    }
}
