//! Reading a protobuf message into a serde value.
//!
//! Protobuf isn't self-describing, but serde's derives always say what they
//! expect, which tells a field's type. Fields reach serde by their position,
//! as the derives accept, in the order they arrive; the ones that never arrive
//! follow with protobuf's defaults.
//!
//! # Owns
//!
//! - [`from_slice`] and the deserializers it reads with.
//!
//! # Not here
//!
//! - The mapping from serde onto protobuf: `protobuf.rs`'s docs. Writing a message: `ser.rs`.

use serde::de;
use serde::de::DeserializeOwned;
use serde::de::DeserializeSeed;
use serde::de::Deserializer;
use serde::de::Visitor;
use serde::de::value::U64Deserializer;

use super::FIXED32;
use super::FIXED64;
use super::LEN;
use super::ProtobufError;
use super::VARINT;
use super::unzigzag;

type Result<T> = std::result::Result<T, ProtobufError>;

/// Decodes `bytes`, one whole message.
pub(crate) fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    T::deserialize(Message { bytes })
}

#[derive(Clone, Copy)]
enum Value<'a> {
    Varint(u64),
    Fixed64([u8; 8]),
    Len(&'a [u8]),
    Fixed32([u8; 4]),
}

/// One field as it arrived: its number and its value.
#[derive(Clone, Copy)]
struct Entry<'a> {
    number: u32,
    value: Value<'a>,
}

fn truncated() -> ProtobufError {
    ProtobufError::new("the message ends in the middle of a field")
}

fn read_varint(bytes: &mut &[u8]) -> Result<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = bytes.split_first().ok_or_else(truncated)?;
        *bytes = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return Ok(value);
        }
    }
    Err(ProtobufError::new("a varint is longer than 64 bits"))
}

fn take<'a>(bytes: &mut &'a [u8], len: usize) -> Result<&'a [u8]> {
    if bytes.len() < len {
        return Err(truncated());
    }
    let (taken, rest) = bytes.split_at(len);
    *bytes = rest;
    Ok(taken)
}

fn read_entry<'a>(bytes: &mut &'a [u8]) -> Result<Entry<'a>> {
    let key = read_varint(bytes)?;
    let number = u32::try_from(key >> 3)
        .ok()
        .filter(|&number| number > 0)
        .ok_or_else(|| ProtobufError::new(format!("{} isn't a field number", key >> 3)))?;
    let value = match (key & 7) as u8 {
        VARINT => Value::Varint(read_varint(bytes)?),
        FIXED64 => Value::Fixed64(take(bytes, 8)?.try_into().expect("8 bytes taken")),
        LEN => {
            let len = usize::try_from(read_varint(bytes)?).map_err(|_| truncated())?;
            Value::Len(take(bytes, len)?)
        }
        FIXED32 => Value::Fixed32(take(bytes, 4)?.try_into().expect("4 bytes taken")),
        other => {
            return Err(ProtobufError::new(format!(
                "field {number} has wire type {other}, which this side doesn't read"
            )));
        }
    };
    Ok(Entry { number, value })
}

/// The next entry of field `number` in `bytes`, skipping any other field.
fn next_numbered<'a>(bytes: &mut &'a [u8], number: u32) -> Result<Option<Entry<'a>>> {
    while !bytes.is_empty() {
        let entry = read_entry(bytes)?;
        if entry.number == number {
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

fn position(index: usize) -> U64Deserializer<ProtobufError> {
    U64Deserializer::new(index as u64)
}

fn not_self_describing() -> ProtobufError {
    ProtobufError::new("protobuf isn't self-describing: the type must say what it expects")
}

/// A whole message: the top level, which the frame bounds.
struct Message<'a> {
    bytes: &'a [u8],
}

impl<'de> Deserializer<'de> for Message<'_> {
    type Error = ProtobufError;

    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value> {
        Err(ProtobufError::new("a protobuf message decodes as a struct or an enum"))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_map(Fields::new(self.bytes, fields.len())?)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_enum(Variant::find(self.bytes)?)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _: &'static str, visitor: V) -> Result<V::Value> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_unit()
    }

    fn is_human_readable(&self) -> bool {
        false
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit
        unit_struct seq tuple tuple_struct map identifier
    }
}

/// A struct's fields. Each field number reaches serde once, at its first
/// entry; a list reads its later entries itself, from [`Field::rest`].
struct Fields<'a> {
    rest: &'a [u8],
    count: usize,
    /// The positions given to serde so far, as bits.
    seen: u64,
    current: Current<'a>,
    /// Where the search for fields that never arrived goes on from.
    missing_from: usize,
}

enum Current<'a> {
    Nothing,
    Present(Field<'a>),
    Missing,
}

impl<'a> Fields<'a> {
    fn new(bytes: &'a [u8], count: usize) -> Result<Self> {
        if count > 64 {
            return Err(ProtobufError::new("a struct with more than 64 fields"));
        }
        Ok(Fields {
            rest: bytes,
            count,
            seen: 0,
            current: Current::Nothing,
            missing_from: 0,
        })
    }
}

impl<'de> de::MapAccess<'de> for Fields<'_> {
    type Error = ProtobufError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>> {
        while !self.rest.is_empty() {
            let entry = read_entry(&mut self.rest)?;
            let index = entry.number as usize - 1;
            // A field this side doesn't know is a newer peer's, and an entry
            // seen before is a later element of a list, read with the first.
            if index >= self.count || self.seen & (1 << index) != 0 {
                continue;
            }
            self.seen |= 1 << index;
            self.current = Current::Present(Field {
                entry,
                rest: self.rest,
                wrap: false,
            });
            return seed.deserialize(position(index)).map(Some);
        }
        while self.missing_from < self.count {
            let index = self.missing_from;
            self.missing_from += 1;
            if self.seen & (1 << index) == 0 {
                self.current = Current::Missing;
                return seed.deserialize(position(index)).map(Some);
            }
        }
        Ok(None)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value> {
        match std::mem::replace(&mut self.current, Current::Nothing) {
            Current::Present(field) => seed.deserialize(field),
            Current::Missing => seed.deserialize(Absent),
            Current::Nothing => Err(ProtobufError::new("a value asked for before its key")),
        }
    }
}

/// An enum's variant: the one field of its message, whose number names it.
struct Variant<'a> {
    entry: Entry<'a>,
}

impl<'a> Variant<'a> {
    fn find(mut bytes: &'a [u8]) -> Result<Self> {
        let mut last = None;
        // The last entry wins, as in a protobuf `oneof`.
        while !bytes.is_empty() {
            last = Some(read_entry(&mut bytes)?);
        }
        last.map(|entry| Variant { entry })
            .ok_or_else(|| ProtobufError::new("an enum's message holds no variant"))
    }
}

impl<'de> de::EnumAccess<'de> for Variant<'_> {
    type Error = ProtobufError;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self)> {
        let variant = seed.deserialize(position(self.entry.number as usize - 1))?;
        Ok((variant, self))
    }
}

impl<'de> de::VariantAccess<'de> for Variant<'_> {
    type Error = ProtobufError;

    fn unit_variant(self) -> Result<()> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value> {
        seed.deserialize(Field {
            entry: self.entry,
            rest: &[],
            wrap: true,
        })
    }

    fn tuple_variant<V: Visitor<'de>>(self, _: usize, _: V) -> Result<V::Value> {
        Err(ProtobufError::new("the protobuf Wire format has no tuples"))
    }

    fn struct_variant<V: Visitor<'de>>(self, fields: &'static [&'static str], visitor: V) -> Result<V::Value> {
        let Value::Len(body) = self.entry.value else {
            return Err(wrong_type(self.entry, "a message"));
        };
        visitor.visit_map(Fields::new(body, fields.len())?)
    }
}

fn wrong_type(entry: Entry<'_>, expected: &str) -> ProtobufError {
    ProtobufError::new(format!("field {} isn't {expected}", entry.number))
}

/// The value of one field.
struct Field<'a> {
    entry: Entry<'a>,
    /// The message after this entry, where a list's later entries are.
    rest: &'a [u8],
    /// A list or a map comes wrapped in a message of its own, as its field 1.
    wrap: bool,
}

impl<'a> Field<'a> {
    fn varint(&self) -> Result<u64> {
        match self.entry.value {
            Value::Varint(value) => Ok(value),
            _ => Err(wrong_type(self.entry, "a varint")),
        }
    }

    fn body(&self) -> Result<&'a [u8]> {
        match self.entry.value {
            Value::Len(body) => Ok(body),
            _ => Err(wrong_type(self.entry, "length-delimited")),
        }
    }

    fn str(&self) -> Result<&'a str> {
        std::str::from_utf8(self.body()?).map_err(|_| wrong_type(self.entry, "UTF-8 text"))
    }

    fn list(self) -> Result<Repeated<'a>> {
        if self.wrap {
            return Ok(Repeated {
                next: None,
                rest: self.body()?,
                number: 1,
                packed: &[],
            });
        }
        Ok(Repeated {
            next: Some(self.entry),
            rest: self.rest,
            number: self.entry.number,
            packed: &[],
        })
    }
}

impl<'de> Deserializer<'de> for Field<'_> {
    type Error = ProtobufError;

    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value> {
        Err(not_self_describing())
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_bool(self.varint()? != 0)
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_i64(unzigzag(self.varint()?))
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_u64(self.varint()?)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        match self.entry.value {
            Value::Fixed32(bytes) => visitor.visit_f32(f32::from_le_bytes(bytes)),
            _ => Err(wrong_type(self.entry, "a float")),
        }
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        match self.entry.value {
            Value::Fixed64(bytes) => visitor.visit_f64(f64::from_le_bytes(bytes)),
            _ => Err(wrong_type(self.entry, "a double")),
        }
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_str(self.str()?)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_str(self.str()?)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_str(self.str()?)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_bytes(self.body()?)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_bytes(self.body()?)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_some(self)
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(self, _: &'static str, visitor: V) -> Result<V::Value> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _: &'static str, visitor: V) -> Result<V::Value> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_seq(self.list()?)
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, _: usize, _: V) -> Result<V::Value> {
        Err(ProtobufError::new("the protobuf Wire format has no tuples"))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(self, _: &'static str, _: usize, _: V) -> Result<V::Value> {
        Err(ProtobufError::new("the protobuf Wire format has no tuples"))
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_map(Entries {
            list: self.list()?,
            value: None,
        })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_map(Fields::new(self.body()?, fields.len())?)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_enum(Variant::find(self.body()?)?)
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, _: V) -> Result<V::Value> {
        Err(not_self_describing())
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_unit()
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

/// The entries of a repeated field, the first one included, and any packed
/// run of numbers among them.
struct Repeated<'a> {
    /// The first entry, not yet read.
    next: Option<Entry<'a>>,
    /// Where the later entries are, among other fields'.
    rest: &'a [u8],
    number: u32,
    /// What is left of the packed run being read.
    packed: &'a [u8],
}

impl<'a> Repeated<'a> {
    fn next_entry(&mut self) -> Result<Option<Entry<'a>>> {
        match self.next.take() {
            Some(entry) => Ok(Some(entry)),
            None => next_numbered(&mut self.rest, self.number),
        }
    }
}

impl<'de> de::SeqAccess<'de> for Repeated<'_> {
    type Error = ProtobufError;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        if !self.packed.is_empty() {
            return seed
                .deserialize(Packed {
                    bytes: &mut self.packed,
                })
                .map(Some);
        }
        let Some(entry) = self.next_entry()? else {
            return Ok(None);
        };
        seed.deserialize(Element {
            entry,
            packed: &mut self.packed,
        })
        .map(Some)
    }
}

/// One entry of a repeated field. A number arriving as a length-delimited
/// entry starts a packed run.
struct Element<'r, 'a> {
    entry: Entry<'a>,
    packed: &'r mut &'a [u8],
}

impl<'a> Element<'_, 'a> {
    fn field(self) -> Field<'a> {
        Field {
            entry: self.entry,
            rest: &[],
            wrap: true,
        }
    }

    /// The packed run this entry starts, if it is one.
    fn run(&mut self) -> Result<bool> {
        let Value::Len(body) = self.entry.value else {
            return Ok(false);
        };
        if body.is_empty() {
            return Err(ProtobufError::new(format!(
                "field {} is an empty packed run",
                self.entry.number
            )));
        }
        *self.packed = body;
        Ok(true)
    }
}

macro_rules! number {
    ($($method:ident)*) => {
        $(fn $method<V: Visitor<'de>>(mut self, visitor: V) -> Result<V::Value> {
            if self.run()? {
                return Packed { bytes: self.packed }.$method(visitor);
            }
            self.field().$method(visitor)
        })*
    };
}

macro_rules! forward_to_field {
    ($($method:ident)*) => {
        $(fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
            self.field().$method(visitor)
        })*
    };
}

impl<'de> Deserializer<'de> for Element<'_, '_> {
    type Error = ProtobufError;

    number! {
        deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64 deserialize_u8
        deserialize_u16 deserialize_u32 deserialize_u64 deserialize_f32 deserialize_f64
    }

    forward_to_field! {
        deserialize_any deserialize_char deserialize_str deserialize_string deserialize_bytes
        deserialize_byte_buf deserialize_option deserialize_unit deserialize_seq deserialize_map
        deserialize_identifier deserialize_ignored_any
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(self, name: &'static str, visitor: V) -> Result<V::Value> {
        self.field().deserialize_unit_struct(name, visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _: &'static str, visitor: V) -> Result<V::Value> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value> {
        self.field().deserialize_tuple(len, visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(self, name: &'static str, len: usize, visitor: V) -> Result<V::Value> {
        self.field().deserialize_tuple_struct(name, len, visitor)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        self.field().deserialize_struct(name, fields, visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        self.field().deserialize_enum(name, variants, visitor)
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

/// The next number of a packed run.
struct Packed<'r, 'a> {
    bytes: &'r mut &'a [u8],
}

impl<'de> Deserializer<'de> for Packed<'_, '_> {
    type Error = ProtobufError;

    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value> {
        Err(ProtobufError::new("a packed run holds numbers only"))
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_bool(read_varint(self.bytes)? != 0)
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_i64(unzigzag(read_varint(self.bytes)?))
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_u64(read_varint(self.bytes)?)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        let bytes = take(self.bytes, 4)?;
        visitor.visit_f32(f32::from_le_bytes(bytes.try_into().expect("4 bytes taken")))
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        let bytes = take(self.bytes, 8)?;
        visitor.visit_f64(f64::from_le_bytes(bytes.try_into().expect("8 bytes taken")))
    }

    fn is_human_readable(&self) -> bool {
        false
    }

    serde::forward_to_deserialize_any! {
        i128 u128 char str string bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

/// A map's entries: each a message of the key (field 1) and the value
/// (field 2).
struct Entries<'a> {
    list: Repeated<'a>,
    /// The value of the entry whose key was read last.
    value: Option<Entry<'a>>,
}

impl<'de> de::MapAccess<'de> for Entries<'_> {
    type Error = ProtobufError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>> {
        let Some(entry) = self.list.next_entry()? else {
            return Ok(None);
        };
        let Value::Len(mut body) = entry.value else {
            return Err(wrong_type(entry, "a map entry"));
        };
        let mut key = None;
        self.value = None;
        while !body.is_empty() {
            let field = read_entry(&mut body)?;
            match field.number {
                1 => key = Some(field),
                2 => self.value = Some(field),
                _ => {}
            }
        }
        match key {
            Some(entry) => seed.deserialize(Field {
                entry,
                rest: &[],
                wrap: true,
            }),
            None => seed.deserialize(Absent),
        }
        .map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value> {
        match self.value.take() {
            Some(entry) => seed.deserialize(Field {
                entry,
                rest: &[],
                wrap: true,
            }),
            None => seed.deserialize(Absent),
        }
    }
}

/// A field that never arrived: protobuf's default for its type.
struct Absent;

struct Nothing;

impl<'de> de::SeqAccess<'de> for Nothing {
    type Error = ProtobufError;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, _: T) -> Result<Option<T::Value>> {
        Ok(None)
    }
}

impl<'de> de::MapAccess<'de> for Nothing {
    type Error = ProtobufError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, _: K) -> Result<Option<K::Value>> {
        Ok(None)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, _: V) -> Result<V::Value> {
        Err(ProtobufError::new("a value asked for before its key"))
    }
}

impl<'de> Deserializer<'de> for Absent {
    type Error = ProtobufError;

    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value> {
        Err(not_self_describing())
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_bool(false)
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_i64(0)
    }

    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_i64(0)
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_i64(0)
    }

    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_i64(0)
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_u64(0)
    }

    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_u64(0)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_u64(0)
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_u64(0)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_f32(0.0)
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_f64(0.0)
    }

    fn deserialize_char<V: Visitor<'de>>(self, _: V) -> Result<V::Value> {
        Err(ProtobufError::new("a character field is missing"))
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_str("")
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_str("")
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_bytes(&[])
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_bytes(&[])
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_none()
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(self, _: &'static str, visitor: V) -> Result<V::Value> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _: &'static str, visitor: V) -> Result<V::Value> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_seq(Nothing)
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, _: usize, _: V) -> Result<V::Value> {
        Err(ProtobufError::new("the protobuf Wire format has no tuples"))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(self, _: &'static str, _: usize, _: V) -> Result<V::Value> {
        Err(ProtobufError::new("the protobuf Wire format has no tuples"))
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_map(Nothing)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_map(Fields::new(&[], fields.len())?)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        _: &'static [&'static str],
        _: V,
    ) -> Result<V::Value> {
        Err(ProtobufError::new(format!("a required {name} is missing")))
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, _: V) -> Result<V::Value> {
        Err(not_self_describing())
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_unit()
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}
