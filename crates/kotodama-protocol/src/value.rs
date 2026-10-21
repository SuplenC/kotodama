//! Values whose shape the protocol doesn't fix: an Extension's config table and a
//! submitted form.
//!
//! JSON and MessagePack are self-describing, so a value there is plain data: a
//! table is a map, a list an array. Postcard and protobuf aren't, so there each
//! value carries a tag naming its type. The two cases are told apart by serde's
//! `is_human_readable`, which this crate turns on for MessagePack (see
//! [`WireFormat`](crate::WireFormat)).

use std::collections::BTreeMap;
use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::de;
use serde::de::MapAccess;
use serde::de::SeqAccess;
use serde::de::Visitor;

/// A config value: anything a TOML file can hold, except that dates arrive as
/// strings. There is no null, because TOML has none.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Array(Vec<Value>),
    Table(Table),
}

pub type Table = BTreeMap<String, Value>;

/// Postcard's and protobuf's form: the tag picks the variant.
#[derive(Serialize)]
#[serde(rename = "Value")]
enum TaggedRef<'a> {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(&'a str),
    Array(&'a [Value]),
    Table(&'a Table),
}

#[derive(Deserialize)]
#[serde(rename = "Value")]
enum Tagged {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Array(Vec<Value>),
    Table(Table),
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            match self {
                Value::Bool(value) => serializer.serialize_bool(*value),
                Value::Integer(value) => serializer.serialize_i64(*value),
                Value::Float(value) => serializer.serialize_f64(*value),
                Value::String(value) => serializer.serialize_str(value),
                Value::Array(values) => values.serialize(serializer),
                Value::Table(table) => table.serialize(serializer),
            }
        } else {
            match self {
                Value::Bool(value) => TaggedRef::Bool(*value),
                Value::Integer(value) => TaggedRef::Integer(*value),
                Value::Float(value) => TaggedRef::Float(*value),
                Value::String(value) => TaggedRef::String(value),
                Value::Array(values) => TaggedRef::Array(values),
                Value::Table(table) => TaggedRef::Table(table),
            }
            .serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            deserializer.deserialize_any(PlainVisitor)
        } else {
            Ok(match Tagged::deserialize(deserializer)? {
                Tagged::Bool(value) => Value::Bool(value),
                Tagged::Integer(value) => Value::Integer(value),
                Tagged::Float(value) => Value::Float(value),
                Tagged::String(value) => Value::String(value),
                Tagged::Array(values) => Value::Array(values),
                Tagged::Table(table) => Value::Table(table),
            })
        }
    }
}

struct PlainVisitor;

impl<'de> Visitor<'de> for PlainVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a boolean, number, string, array or table")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Integer(value))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        i64::try_from(value)
            .map(Value::Integer)
            .map_err(|_| E::custom(format!("{value} is too large; integers must fit in 64 signed bits")))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
        Ok(Value::Float(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut values = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
        while let Some(value) = seq.next_element()? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut table = Table::new();
        while let Some((key, value)) = map.next_entry()? {
            table.insert(key, value);
        }
        Ok(Value::Table(table))
    }
}

/// Binary data, such as an image to paste. JSON and MessagePack carry it as a
/// base64 string; postcard and protobuf as raw bytes. Decoding MessagePack also accepts its
/// native binary type, which other languages' libraries produce for byte strings.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Bytes(pub Vec<u8>);

impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bytes({} bytes)", self.0.len())
    }
}

impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&BASE64.encode(&self.0))
        } else {
            serializer.serialize_bytes(&self.0)
        }
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BytesVisitor;
        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = Bytes;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("base64 text or bytes")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Bytes, E> {
                BASE64.decode(value).map(Bytes).map_err(E::custom)
            }
            fn visit_bytes<E>(self, value: &[u8]) -> Result<Bytes, E> {
                Ok(Bytes(value.to_vec()))
            }
            fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Bytes, E> {
                Ok(Bytes(value))
            }
        }
        if deserializer.is_human_readable() {
            deserializer.deserialize_any(BytesVisitor)
        } else {
            deserializer.deserialize_byte_buf(BytesVisitor)
        }
    }
}

/// A number from 0 to 1 inclusive: a Live query result's score, or how far a job
/// has got.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f32", into = "f32")]
pub struct Fraction(f32);

impl Fraction {
    pub fn new(value: f32) -> Result<Self, String> {
        if (0.0..=1.0).contains(&value) {
            Ok(Fraction(value))
        } else {
            Err(format!("{value} isn't between 0 and 1"))
        }
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

impl TryFrom<f32> for Fraction {
    type Error = String;
    fn try_from(value: f32) -> Result<Self, String> {
        Fraction::new(value)
    }
}

impl From<Fraction> for f32 {
    fn from(fraction: Fraction) -> f32 {
        fraction.0
    }
}
