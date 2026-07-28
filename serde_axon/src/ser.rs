//! Serde `Serializer`: turns any `Serialize` type into a [`Value`], then into
//! AXON text. Going through `Value` keeps the mapping explicit and lets the
//! writer own all formatting/canonicalisation.
//!
//! Rust -> AXON mapping (the enum-representation choices per the roadmap):
//! * unit / unit-struct -> `null`
//! * struct / map -> AXON map `{ .. }`
//! * seq / tuple / tuple-struct -> AXON list `[ .. ]`
//! * newtype/tuple/struct enum variants -> externally tagged as a node
//!   `Variant( .. )` / `Variant{ .. }`, matching AXON's tagged-node model.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde::ser::{self, Serialize};

use crate::error::{Category, Error, Result};
use crate::value::{Node, NodeStyle, Value};
use crate::writer;

/// Serialise `value` to compact AXON text.
pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    writer::to_string(&to_value(value)?)
}

/// Serialise `value` to Canonical AXON text.
pub fn to_string_canonical<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    writer::to_string_canonical(&to_value(value)?)
}

/// Serialise `value` into the [`Value`] model.
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value> {
    value.serialize(Serializer)
}

pub(crate) struct Serializer;

impl ser::Serializer for Serializer {
    type Ok = Value;
    type Error = Error;
    type SerializeSeq = SeqSer;
    type SerializeTuple = SeqSer;
    type SerializeTupleStruct = SeqSer;
    type SerializeTupleVariant = VariantSeqSer;
    type SerializeMap = MapSer;
    type SerializeStruct = MapSer;
    type SerializeStructVariant = VariantMapSer;

    fn serialize_bool(self, v: bool) -> Result<Value> {
        Ok(Value::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<Value> {
        Ok(Value::Int(v.to_string()))
    }
    fn serialize_i16(self, v: i16) -> Result<Value> {
        Ok(Value::Int(v.to_string()))
    }
    fn serialize_i32(self, v: i32) -> Result<Value> {
        Ok(Value::Int(v.to_string()))
    }
    fn serialize_i64(self, v: i64) -> Result<Value> {
        Ok(Value::Int(v.to_string()))
    }
    fn serialize_i128(self, v: i128) -> Result<Value> {
        Ok(Value::Int(v.to_string()))
    }
    fn serialize_u8(self, v: u8) -> Result<Value> {
        Ok(Value::Int(v.to_string()))
    }
    fn serialize_u16(self, v: u16) -> Result<Value> {
        Ok(Value::Int(v.to_string()))
    }
    fn serialize_u32(self, v: u32) -> Result<Value> {
        Ok(Value::Int(v.to_string()))
    }
    fn serialize_u64(self, v: u64) -> Result<Value> {
        Ok(Value::Int(v.to_string()))
    }
    fn serialize_u128(self, v: u128) -> Result<Value> {
        Ok(Value::Int(v.to_string()))
    }
    fn serialize_f32(self, v: f32) -> Result<Value> {
        Ok(Value::Float(v as f64))
    }
    fn serialize_f64(self, v: f64) -> Result<Value> {
        Ok(Value::Float(v))
    }
    fn serialize_char(self, v: char) -> Result<Value> {
        let mut s = String::new();
        s.push(v);
        Ok(Value::String(s))
    }
    fn serialize_str(self, v: &str) -> Result<Value> {
        Ok(Value::String(v.to_string()))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<Value> {
        Ok(Value::Bytes(v.to_vec()))
    }
    fn serialize_none(self) -> Result<Value> {
        Ok(Value::Null)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Value> {
        Ok(Value::Null)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value> {
        Ok(Value::Null)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
    ) -> Result<Value> {
        // externally tagged unit variant -> a unit node named for the variant
        Ok(Value::Node(Box::new(Node {
            name: variant.to_string(),
            style: NodeStyle::Unit,
            attributes: Vec::new(),
            children: Vec::new(),
        })))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value> {
        // externally tagged: Variant(inner)
        Ok(Value::Node(Box::new(Node {
            name: variant.to_string(),
            style: NodeStyle::Tuple,
            attributes: Vec::new(),
            children: alloc::vec![value.serialize(Serializer)?],
        })))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SeqSer> {
        Ok(SeqSer {
            items: Vec::with_capacity(len.unwrap_or(0)),
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<SeqSer> {
        Ok(SeqSer {
            items: Vec::with_capacity(len),
        })
    }
    fn serialize_tuple_struct(self, _name: &'static str, len: usize) -> Result<SeqSer> {
        Ok(SeqSer {
            items: Vec::with_capacity(len),
        })
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<VariantSeqSer> {
        Ok(VariantSeqSer {
            name: variant,
            items: Vec::with_capacity(len),
        })
    }
    fn serialize_map(self, len: Option<usize>) -> Result<MapSer> {
        Ok(MapSer {
            pairs: Vec::with_capacity(len.unwrap_or(0)),
            next_key: None,
        })
    }
    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<MapSer> {
        Ok(MapSer {
            pairs: Vec::with_capacity(len),
            next_key: None,
        })
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<VariantMapSer> {
        Ok(VariantMapSer {
            name: variant,
            attrs: Vec::with_capacity(len),
        })
    }
}

pub(crate) struct SeqSer {
    items: Vec<Value>,
}
impl ser::SerializeSeq for SeqSer {
    type Ok = Value;
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        self.items.push(value.serialize(Serializer)?);
        Ok(())
    }
    fn end(self) -> Result<Value> {
        Ok(Value::List(self.items))
    }
}
impl ser::SerializeTuple for SeqSer {
    type Ok = Value;
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        self.items.push(value.serialize(Serializer)?);
        Ok(())
    }
    fn end(self) -> Result<Value> {
        // a Rust tuple maps to an AXON tuple
        Ok(Value::Tuple(self.items))
    }
}
impl ser::SerializeTupleStruct for SeqSer {
    type Ok = Value;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        self.items.push(value.serialize(Serializer)?);
        Ok(())
    }
    fn end(self) -> Result<Value> {
        Ok(Value::Tuple(self.items))
    }
}

pub(crate) struct VariantSeqSer {
    name: &'static str,
    items: Vec<Value>,
}
impl ser::SerializeTupleVariant for VariantSeqSer {
    type Ok = Value;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        self.items.push(value.serialize(Serializer)?);
        Ok(())
    }
    fn end(self) -> Result<Value> {
        Ok(Value::Node(Box::new(Node {
            name: self.name.to_string(),
            style: NodeStyle::Tuple,
            attributes: Vec::new(),
            children: self.items,
        })))
    }
}

pub(crate) struct MapSer {
    pairs: Vec<(Value, Value)>,
    next_key: Option<Value>,
}
impl ser::SerializeMap for MapSer {
    type Ok = Value;
    type Error = Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<()> {
        if self.next_key.is_some() {
            return Err(Error::new(
                Category::Serde,
                "key serialized twice without a value",
                0,
                0,
            ));
        }
        self.next_key = Some(key.serialize(Serializer)?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        let k = self
            .next_key
            .take()
            .ok_or_else(|| Error::new(Category::Serde, "value serialized before key", 0, 0))?;
        self.pairs.push((k, value.serialize(Serializer)?));
        Ok(())
    }
    fn end(self) -> Result<Value> {
        if self.next_key.is_some() {
            return Err(Error::new(
                Category::Serde,
                "map ended with a key but no value",
                0,
                0,
            ));
        }
        Ok(Value::Map(self.pairs))
    }
}
impl ser::SerializeStruct for MapSer {
    type Ok = Value;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        self.pairs
            .push((Value::String(key.to_string()), value.serialize(Serializer)?));
        Ok(())
    }
    fn end(self) -> Result<Value> {
        Ok(Value::Map(self.pairs))
    }
}

pub(crate) struct VariantMapSer {
    name: &'static str,
    attrs: Vec<(String, Value)>,
}
impl ser::SerializeStructVariant for VariantMapSer {
    type Ok = Value;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        self.attrs
            .push((key.to_string(), value.serialize(Serializer)?));
        Ok(())
    }
    fn end(self) -> Result<Value> {
        Ok(Value::Node(Box::new(Node {
            name: self.name.to_string(),
            style: NodeStyle::Brace,
            attributes: self.attrs,
            children: Vec::new(),
        })))
    }
}
