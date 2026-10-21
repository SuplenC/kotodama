//! Writes `kotodama.proto` from the layout samples: a serde serializer that,
//! instead of encoding a sample, records the protobuf message each type maps
//! to, by the same rules as the protobuf Wire format (`src/protobuf.rs`). A
//! field takes its type from the samples that show it, and every field holds
//! a value in some sample, so every field gets one.

use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write as _;

use serde::Serialize;
use serde::ser;
use serde::ser::Impossible;
use serde::ser::SerializeMap;
use serde::ser::SerializeSeq;
use serde::ser::SerializeStruct;
use serde::ser::SerializeStructVariant;

/// What a value maps to in protobuf, before deciding how a field declares it.
#[derive(Clone, Debug, PartialEq)]
enum Shape {
    /// Not shown by any sample yet: a `None`, an empty list, a skipped field.
    Unknown,
    Scalar(&'static str),
    Message(String),
    Optional(Box<Shape>),
    Repeated(Box<Shape>),
    Map(Box<Shape>, Box<Shape>),
}

fn merge(a: Shape, b: Shape) -> Result<Shape, String> {
    Ok(match (a, b) {
        (Shape::Unknown, other) | (other, Shape::Unknown) => other,
        (Shape::Optional(a), Shape::Optional(b)) => Shape::Optional(Box::new(merge(*a, *b)?)),
        // A field skipped when it holds nothing reads as a plain value
        // elsewhere: it is optional.
        (Shape::Optional(a), b) | (b, Shape::Optional(a)) => Shape::Optional(Box::new(merge(*a, b)?)),
        (Shape::Repeated(a), Shape::Repeated(b)) => Shape::Repeated(Box::new(merge(*a, *b)?)),
        (Shape::Map(ak, av), Shape::Map(bk, bv)) => Shape::Map(Box::new(merge(*ak, *bk)?), Box::new(merge(*av, *bv)?)),
        (a, b) if a == b => a,
        (a, b) => return Err(format!("one field is both {a:?} and {b:?}")),
    })
}

#[derive(Debug, Default)]
struct Message {
    /// An enum: its fields are the variants of one `oneof`.
    oneof: bool,
    fields: BTreeMap<u32, (String, Shape)>,
}

/// Every message the samples reach.
#[derive(Debug, Default)]
pub struct Schema {
    messages: BTreeMap<String, Message>,
}

impl Schema {
    pub fn add<T: Serialize + ?Sized>(&mut self, sample: &T) {
        sample.serialize(Walk(self)).unwrap_or_else(|err| panic!("{err}"));
    }

    fn message(&mut self, name: &str, oneof: bool) -> &mut Message {
        let message = self.messages.entry(name.to_owned()).or_default();
        message.oneof |= oneof;
        message
    }

    fn field(&mut self, message: &str, oneof: bool, number: u32, name: &str, shape: Shape) -> Result<(), Error> {
        let fields = &mut self.message(message, oneof).fields;
        let (known, before) = fields.remove(&number).unwrap_or((name.to_owned(), Shape::Unknown));
        if known != name {
            return Err(Error(format!("{message} field {number} is both {known} and {name}")));
        }
        let shape = merge(before, shape).map_err(|err| Error(format!("{message}.{name}: {err}")))?;
        fields.insert(number, (known, shape));
        Ok(())
    }

    /// The `.proto` file, after `header`.
    pub fn render(&self, header: &str) -> Result<String, String> {
        let mut wrappers = BTreeMap::new();
        let mut bodies: BTreeMap<String, String> = BTreeMap::new();
        for (name, message) in &self.messages {
            let mut body = String::new();
            if message.oneof {
                body.push_str("  oneof variant {\n");
            }
            for (number, (field, shape)) in &message.fields {
                let indent = if message.oneof { "    " } else { "  " };
                let (label, ty) = if message.oneof {
                    single(shape, &mut wrappers).map(|ty| ("", ty))
                } else {
                    declare(shape, &mut wrappers)
                }
                .map_err(|err| format!("{name}.{field}: {err}"))?;
                writeln!(body, "{indent}{label}{ty} {} = {number};", snake(field)).unwrap();
            }
            if message.oneof {
                body.push_str("  }\n");
            }
            bodies.insert(name.clone(), body);
        }
        for (name, body) in wrappers {
            bodies.insert(name, body);
        }

        let mut out = header.to_owned();
        for (name, body) in &bodies {
            if name.contains('.') {
                continue;
            }
            write!(out, "\nmessage {name} {{\n{body}").unwrap();
            let nested = format!("{name}.");
            for (inner, inner_body) in bodies.range(nested.clone()..) {
                let Some(short) = inner.strip_prefix(&nested) else {
                    break;
                };
                write!(out, "\n  message {short} {{\n").unwrap();
                for line in inner_body.lines() {
                    writeln!(out, "  {line}").unwrap();
                }
                out.push_str("  }\n");
            }
            out.push_str("}\n");
        }
        Ok(out)
    }
}

/// How a message field declares `shape`: a label and a type.
fn declare(shape: &Shape, wrappers: &mut BTreeMap<String, String>) -> Result<(&'static str, String), String> {
    Ok(match shape {
        Shape::Unknown => return Err("no sample shows its type".into()),
        Shape::Scalar(scalar) => ("", (*scalar).to_owned()),
        Shape::Message(name) => ("", name.clone()),
        Shape::Optional(inner) => match &**inner {
            Shape::Scalar(scalar) => ("optional ", (*scalar).to_owned()),
            other => declare(other, wrappers)?,
        },
        Shape::Repeated(element) => ("repeated ", single(element, wrappers)?),
        Shape::Map(key, value) => (
            "",
            format!("map<{}, {}>", single(key, wrappers)?, single(value, wrappers)?),
        ),
    })
}

/// The type of a value where protobuf takes no label (a list's element, a
/// map's value, a variant), wrapping a list or a map in a message.
fn single(shape: &Shape, wrappers: &mut BTreeMap<String, String>) -> Result<String, String> {
    Ok(match shape {
        Shape::Repeated(_) | Shape::Map(..) => {
            let name = base(shape)?;
            let (label, ty) = declare(shape, wrappers)?;
            wrappers.insert(name.clone(), format!("  {label}{ty} value = 1;\n"));
            name
        }
        Shape::Optional(inner) => single(inner, wrappers)?,
        other => declare(other, wrappers)?.1,
    })
}

/// The name a wrapper takes after what it holds: `ValueList`, `StringMap`.
fn base(shape: &Shape) -> Result<String, String> {
    Ok(match shape {
        Shape::Unknown => return Err("no sample shows its type".into()),
        Shape::Scalar(scalar) => pascal(scalar),
        Shape::Message(name) => name.replace('.', ""),
        Shape::Optional(inner) => base(inner)?,
        Shape::Repeated(element) => format!("{}List", base(element)?),
        Shape::Map(_, value) => format!("{}Map", base(value)?),
    })
}

fn pascal(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect()
}

fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Debug)]
struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Error(message.to_string())
    }
}

/// Finds the shape of one value, recording the messages it reaches.
struct Walk<'s>(&'s mut Schema);

const EMPTY: &str = "Empty";

impl<'s> ser::Serializer for Walk<'s> {
    type Ok = Shape;
    type Error = Error;
    type SerializeSeq = Seq<'s>;
    type SerializeTuple = Impossible<Shape, Error>;
    type SerializeTupleStruct = Impossible<Shape, Error>;
    type SerializeTupleVariant = Impossible<Shape, Error>;
    type SerializeMap = Map<'s>;
    type SerializeStruct = Fields<'s>;
    type SerializeStructVariant = Fields<'s>;

    fn serialize_bool(self, _: bool) -> Result<Shape, Error> {
        Ok(Shape::Scalar("bool"))
    }
    fn serialize_i8(self, _: i8) -> Result<Shape, Error> {
        Ok(Shape::Scalar("sint32"))
    }
    fn serialize_i16(self, _: i16) -> Result<Shape, Error> {
        Ok(Shape::Scalar("sint32"))
    }
    fn serialize_i32(self, _: i32) -> Result<Shape, Error> {
        Ok(Shape::Scalar("sint32"))
    }
    fn serialize_i64(self, _: i64) -> Result<Shape, Error> {
        Ok(Shape::Scalar("sint64"))
    }
    fn serialize_u8(self, _: u8) -> Result<Shape, Error> {
        Ok(Shape::Scalar("uint32"))
    }
    fn serialize_u16(self, _: u16) -> Result<Shape, Error> {
        Ok(Shape::Scalar("uint32"))
    }
    fn serialize_u32(self, _: u32) -> Result<Shape, Error> {
        Ok(Shape::Scalar("uint32"))
    }
    fn serialize_u64(self, _: u64) -> Result<Shape, Error> {
        Ok(Shape::Scalar("uint64"))
    }
    fn serialize_f32(self, _: f32) -> Result<Shape, Error> {
        Ok(Shape::Scalar("float"))
    }
    fn serialize_f64(self, _: f64) -> Result<Shape, Error> {
        Ok(Shape::Scalar("double"))
    }
    fn serialize_char(self, _: char) -> Result<Shape, Error> {
        Ok(Shape::Scalar("string"))
    }
    fn serialize_str(self, _: &str) -> Result<Shape, Error> {
        Ok(Shape::Scalar("string"))
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<Shape, Error> {
        Ok(Shape::Scalar("bytes"))
    }
    fn serialize_none(self) -> Result<Shape, Error> {
        Ok(Shape::Optional(Box::new(Shape::Unknown)))
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Shape, Error> {
        Ok(Shape::Optional(Box::new(value.serialize(self)?)))
    }
    fn serialize_unit(self) -> Result<Shape, Error> {
        self.0.message(EMPTY, false);
        Ok(Shape::Message(EMPTY.into()))
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Shape, Error> {
        self.serialize_unit()
    }
    fn serialize_unit_variant(self, name: &'static str, index: u32, variant: &'static str) -> Result<Shape, Error> {
        self.0.message(EMPTY, false);
        self.0
            .field(name, true, index + 1, variant, Shape::Message(EMPTY.into()))?;
        Ok(Shape::Message(name.into()))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, value: &T) -> Result<Shape, Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Shape, Error> {
        let shape = value.serialize(Walk(&mut *self.0))?;
        self.0.field(name, true, index + 1, variant, shape)?;
        Ok(Shape::Message(name.into()))
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Seq<'s>, Error> {
        Ok(Seq {
            schema: self.0,
            element: Shape::Unknown,
        })
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Error> {
        Err(Error("protobuf has no tuples".into()))
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct, Error> {
        Err(Error("protobuf has no tuples".into()))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Err(Error("protobuf has no tuples".into()))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Map<'s>, Error> {
        Ok(Map {
            schema: self.0,
            key: Shape::Unknown,
            value: Shape::Unknown,
        })
    }
    fn serialize_struct(self, name: &'static str, _: usize) -> Result<Fields<'s>, Error> {
        self.0.message(name, false);
        Ok(Fields {
            schema: self.0,
            message: name.into(),
            number: 0,
            shape: name.into(),
        })
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Fields<'s>, Error> {
        let message = format!("{name}.{}", pascal(variant));
        self.0
            .field(name, true, index + 1, variant, Shape::Message(message.clone()))?;
        self.0.message(&message, false);
        Ok(Fields {
            schema: self.0,
            message,
            number: 0,
            shape: name.into(),
        })
    }
    fn is_human_readable(&self) -> bool {
        false
    }
}

struct Seq<'s> {
    schema: &'s mut Schema,
    element: Shape,
}

impl SerializeSeq for Seq<'_> {
    type Ok = Shape;
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let shape = value.serialize(Walk(&mut *self.schema))?;
        self.element = merge(std::mem::replace(&mut self.element, Shape::Unknown), shape).map_err(Error)?;
        Ok(())
    }
    fn end(self) -> Result<Shape, Error> {
        Ok(Shape::Repeated(Box::new(self.element)))
    }
}

struct Map<'s> {
    schema: &'s mut Schema,
    key: Shape,
    value: Shape,
}

impl SerializeMap for Map<'_> {
    type Ok = Shape;
    type Error = Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        let shape = key.serialize(Walk(&mut *self.schema))?;
        self.key = merge(std::mem::replace(&mut self.key, Shape::Unknown), shape).map_err(Error)?;
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let shape = value.serialize(Walk(&mut *self.schema))?;
        self.value = merge(std::mem::replace(&mut self.value, Shape::Unknown), shape).map_err(Error)?;
        Ok(())
    }
    fn end(self) -> Result<Shape, Error> {
        Ok(Shape::Map(Box::new(self.key), Box::new(self.value)))
    }
}

struct Fields<'s> {
    schema: &'s mut Schema,
    message: String,
    number: u32,
    /// The message the value is: the struct, or the enum of a struct variant.
    shape: String,
}

impl Fields<'_> {
    fn add(&mut self, name: &str, shape: Shape) -> Result<(), Error> {
        self.number += 1;
        self.schema.field(&self.message, false, self.number, name, shape)
    }
}

impl SerializeStruct for Fields<'_> {
    type Ok = Shape;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, name: &'static str, value: &T) -> Result<(), Error> {
        let shape = value.serialize(Walk(&mut *self.schema))?;
        self.add(name, shape)
    }
    fn skip_field(&mut self, name: &'static str) -> Result<(), Error> {
        self.add(name, Shape::Unknown)
    }
    fn end(self) -> Result<Shape, Error> {
        Ok(Shape::Message(self.shape))
    }
}

impl SerializeStructVariant for Fields<'_> {
    type Ok = Shape;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, name: &'static str, value: &T) -> Result<(), Error> {
        SerializeStruct::serialize_field(self, name, value)
    }
    fn skip_field(&mut self, name: &'static str) -> Result<(), Error> {
        SerializeStruct::skip_field(self, name)
    }
    fn end(self) -> Result<Shape, Error> {
        SerializeStruct::end(self)
    }
}
