//! Arbitrary AXON, carried through a typed structure without being interpreted.
//!
//! Serde's data model is narrower than AXON's. Crossing into it turns a decimal
//! and a temporal into strings, a tuple and a set into sequences, and an ordered
//! map and a tagged node into plain maps — exactly the distinctions this crate
//! exists to keep. That is why [`Value`](crate::Value) deliberately implements
//! neither `Serialize` nor `Deserialize`: doing so would offer a lossy round
//! trip that looks lossless.
//!
//! The cost of that decision falls on anyone who needs a field of *unknown*
//! shape inside an otherwise typed structure — a schema's open extension point,
//! a passthrough field, a document format that requires unrecognized data to be
//! preserved rather than dropped. Without somewhere to put it, such a field
//! forces the whole document to be handled untyped.
//!
//! [`RawValue`] is that somewhere. It holds the AXON source text of one value
//! and hands it back untouched:
//!
//! ```
//! use serde::{Deserialize, Serialize};
//! use serde_axon::RawValue;
//!
//! #[derive(Serialize, Deserialize)]
//! struct Record {
//!     name: String,
//!     // Any AXON at all, kept exactly as written.
//!     extra: RawValue,
//! }
//!
//! let text = r#"{name:"Ada" extra:{amount:1.50D pair:(1 2) tagged:point{x:1}}}"#;
//! let record: Record = serde_axon::from_str(text)?;
//! let again = serde_axon::to_string(&record)?;
//!
//! // The decimal is still a decimal, the tuple is still a tuple, and the node
//! // still has its name — none of which survives Serde's data model.
//! assert!(again.contains("1.50D"));
//! assert!(again.contains("(1 2)"));
//! assert!(again.contains("point{"));
//! # Ok::<(), serde_axon::Error>(())
//! ```
//!
//! Because the text is produced by this crate's own writer and read back by its
//! own parser, nothing crosses Serde's data model on the way through, and the
//! canonical bytes are unchanged. A `RawValue` therefore cannot alter how a
//! document compares against another implementation.

use alloc::string::{String, ToString};
use core::fmt;
use core::ops::Deref;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Category, Error, Result};

/// The name both halves of the crate agree to recognize.
///
/// Serde passes the name of a newtype struct through to the format, which is
/// the only channel available for saying "this one is special". The name is
/// deliberately not a valid Rust path, so nothing else can collide with it.
pub(crate) const RAW_VALUE_TOKEN: &str = "$serde_axon::private::RawValue";

/// One AXON value, held as source text rather than as a typed structure.
///
/// Owned rather than borrowed. The `serde_json` equivalent is an unsized type
/// so that it can borrow from the input buffer, which requires a
/// transparent-newtype cast through `unsafe`. This crate is
/// `#![forbid(unsafe_code)]`, and borrowing would buy nothing here anyway: a
/// value is always rendered from the parsed tree rather than sliced out of the
/// source text. An owned `String` is therefore both safer and simpler, and it
/// is used directly as a field type with no `Box` around it.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RawValue(String);

impl RawValue {
    /// Builds a value from AXON source text, checking that it parses.
    ///
    /// Checking here rather than on the way out means a mistake is reported at
    /// the point it was made, with the text that caused it still to hand.
    pub fn from_string(text: String) -> Result<RawValue> {
        crate::de::from_str_value(&text)?;
        Ok(RawValue(text))
    }

    /// The AXON source text of this value.
    pub fn get(&self) -> &str {
        &self.0
    }

    /// Consumes the value, returning its AXON source text.
    pub fn into_string(self) -> String {
        self.0
    }

    /// Parses this value into the tree model.
    pub fn to_value(&self) -> Result<crate::Value> {
        crate::de::from_str_value(&self.0)
    }

    /// Wraps text that is already known to parse.
    fn from_checked(text: String) -> RawValue {
        RawValue(text)
    }
}

impl Deref for RawValue {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RawValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RawValue(")?;
        fmt::Debug::fmt(&self.0, formatter)?;
        formatter.write_str(")")
    }
}

impl fmt::Display for RawValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for RawValue {
    /// Announces itself by name so that this crate's serializer can splice the
    /// text in, and so that any *other* format sees a plain string rather than
    /// something it cannot represent.
    fn serialize<S: Serializer>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct(RAW_VALUE_TOKEN, &self.0)
    }
}

impl<'de> Deserialize<'de> for RawValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> core::result::Result<Self, D::Error> {
        struct RawVisitor;

        impl<'de> Visitor<'de> for RawVisitor {
            type Value = RawValue;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("any AXON value")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> core::result::Result<Self::Value, E> {
                Ok(RawValue::from_checked(value.to_string()))
            }

            fn visit_string<E: de::Error>(
                self,
                value: String,
            ) -> core::result::Result<Self::Value, E> {
                Ok(RawValue::from_checked(value))
            }
        }

        deserializer.deserialize_newtype_struct(RAW_VALUE_TOKEN, RawVisitor)
    }
}

/// Pulls the `&str` back out of the payload a [`RawValue`] serializes.
///
/// [`RawValue::serialize`] always passes a `&str`, so every other method is
/// unreachable in practice and reports a clear error rather than panicking, in
/// case a future change to this file forgets.
pub(crate) struct RawTextCapture;

impl Serializer for RawTextCapture {
    type Ok = String;
    type Error = Error;
    type SerializeSeq = serde::ser::Impossible<String, Error>;
    type SerializeTuple = serde::ser::Impossible<String, Error>;
    type SerializeTupleStruct = serde::ser::Impossible<String, Error>;
    type SerializeTupleVariant = serde::ser::Impossible<String, Error>;
    type SerializeMap = serde::ser::Impossible<String, Error>;
    type SerializeStruct = serde::ser::Impossible<String, Error>;
    type SerializeStructVariant = serde::ser::Impossible<String, Error>;

    fn serialize_str(self, value: &str) -> Result<String> {
        Ok(value.to_string())
    }

    fn serialize_bool(self, _value: bool) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_i8(self, _value: i8) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_i16(self, _value: i16) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_i32(self, _value: i32) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_i64(self, _value: i64) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_u8(self, _value: u8) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_u16(self, _value: u16) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_u32(self, _value: u32) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_u64(self, _value: u64) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_f32(self, _value: f32) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_f64(self, _value: f64) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_char(self, _value: char) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_bytes(self, _value: &[u8]) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_none(self) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, _value: &T) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_unit(self) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
    ) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<String> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<String> {
        Err(raw_payload_error())
    }
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq> {
        Err(raw_payload_error())
    }
    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple> {
        Err(raw_payload_error())
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        Err(raw_payload_error())
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(raw_payload_error())
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap> {
        Err(raw_payload_error())
    }
    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        Err(raw_payload_error())
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Err(raw_payload_error())
    }
}

fn raw_payload_error() -> Error {
    Error::new(
        Category::Serde,
        "a RawValue must carry AXON source text",
        0,
        0,
    )
}
