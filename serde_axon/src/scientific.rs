//! Scientific arrays: the AXON `Array{shape type data order}` node.
//!
//! This mirrors the reference `scientific` module's node shape so the two
//! implementations read and write the same `Array` documents. The reference is
//! a numpy bridge; this crate has no numpy, so it exposes the building blocks
//! (shape, dtype, raw `data`, memory `order`) plus typed extractors for the
//! primitive element types.
//!
//! ## Byte order
//!
//! The `Array` `data` payload is defined as **little-endian** for every
//! multi-byte element type, regardless of the host.  The reference bridge
//! serializes and reads little-endian too, so a non-native (e.g., big-endian)
//! source array is not silently corrupted on round-trip.  The typed extractors
//! and constructors here read/write little-endian to match.  The `order`
//! attribute is C/F memory layout and is unrelated to byte order.

use crate::error::{Category, Error};
use crate::value::{Node, NodeStyle, Value};
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

fn numpy_value_error(message: impl Into<String>) -> Error {
    Error::new(Category::UnexpectedToken, message, 0, 0)
}

fn numpy_type_error(message: impl Into<String>) -> Error {
    Error::new(Category::Serde, message, 0, 0)
}

/// Python type names surfaced by numpy's argument-conversion errors. AXON has
/// no bytearray/memoryview values, so every bytes-like value is `bytes` here.
fn python_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Decimal(_) => "Decimal",
        Value::Undefined => "_Undefined",
        Value::String(_) => "str",
        Value::Bytes(_) => "bytes",
        Value::Temporal(_) => "NanoTemporal",
        Value::Link(_) => "_RLink",
        Value::List(_) => "list",
        Value::Tuple(_) => "AxonTuple",
        Value::Set(_) => "AxonSet",
        Value::Map(_) => "dict",
        Value::OrderedMap(_) => "OrderedDict",
        Value::Node(_) => "Node",
        Value::Anchor(_) => "_Anchor",
        Value::Ref(_) => "_Ref",
    }
}

fn python_scalar_text(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Int(text) | Value::String(text) => text.clone(),
        Value::Float(number) if number.is_nan() => "nan".to_string(),
        Value::Float(number) if *number == f64::INFINITY => "inf".to_string(),
        Value::Float(number) if *number == f64::NEG_INFINITY => "-inf".to_string(),
        Value::Float(number) => alloc::format!("{number:?}"),
        _ => alloc::format!("<{}>", python_type_name(value)),
    }
}

/// An AXON scientific-array element type, mirroring the reference dtype tags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayDtype {
    /// Unsigned 8-bit integer (`u8`).
    U8,
    /// Unsigned 16-bit integer (`u16`).
    U16,
    /// Unsigned 32-bit integer (`u32`).
    U32,
    /// Unsigned 64-bit integer (`u64`).
    U64,
    /// Signed 8-bit integer (`i8`).
    I8,
    /// Signed 16-bit integer (`i16`).
    I16,
    /// Signed 32-bit integer (`i32`).
    I32,
    /// Signed 64-bit integer (`i64`).
    I64,
    /// IEEE-754 half-precision float (`f16`).
    F16,
    /// IEEE-754 single-precision float (`f32`).
    F32,
    /// IEEE-754 double-precision float (`f64`).
    F64,
    /// Complex number of two `f32` components (`c64`).
    C64,
    /// Complex number of two `f64` components (`c128`).
    C128,
}

impl ArrayDtype {
    /// Parse a dtype tag (`"f32"`, `"u8"`, ...). Returns `None` for an unknown tag.
    pub fn from_tag(tag: &str) -> Option<Self> {
        Some(match tag {
            "u8" => ArrayDtype::U8,
            "u16" => ArrayDtype::U16,
            "u32" => ArrayDtype::U32,
            "u64" => ArrayDtype::U64,
            "i8" => ArrayDtype::I8,
            "i16" => ArrayDtype::I16,
            "i32" => ArrayDtype::I32,
            "i64" => ArrayDtype::I64,
            "f16" => ArrayDtype::F16,
            "f32" => ArrayDtype::F32,
            "f64" => ArrayDtype::F64,
            "c64" => ArrayDtype::C64,
            "c128" => ArrayDtype::C128,
            _ => return None,
        })
    }

    /// The canonical dtype tag string.
    pub fn tag(self) -> &'static str {
        match self {
            ArrayDtype::U8 => "u8",
            ArrayDtype::U16 => "u16",
            ArrayDtype::U32 => "u32",
            ArrayDtype::U64 => "u64",
            ArrayDtype::I8 => "i8",
            ArrayDtype::I16 => "i16",
            ArrayDtype::I32 => "i32",
            ArrayDtype::I64 => "i64",
            ArrayDtype::F16 => "f16",
            ArrayDtype::F32 => "f32",
            ArrayDtype::F64 => "f64",
            ArrayDtype::C64 => "c64",
            ArrayDtype::C128 => "c128",
        }
    }

    /// The size, in bytes, of one element of this type.
    pub fn element_size(self) -> usize {
        match self {
            ArrayDtype::U8 | ArrayDtype::I8 => 1,
            ArrayDtype::U16 | ArrayDtype::I16 | ArrayDtype::F16 => 2,
            ArrayDtype::U32 | ArrayDtype::I32 | ArrayDtype::F32 => 4,
            ArrayDtype::U64 | ArrayDtype::I64 | ArrayDtype::F64 | ArrayDtype::C64 => 8,
            ArrayDtype::C128 => 16,
        }
    }
}

/// The resolved memory order of a scientific array.
///
/// The Python bridge also accepts `"A"`/`"a"` and `null`; because it first
/// creates a one-dimensional `numpy.frombuffer` view, those spellings resolve
/// to C order.  [`ScientificArray::numpy_layout`] performs that resolution and
/// therefore only needs these two final states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayOrder {
    /// C (row-major) order.
    C,
    /// Fortran (column-major) order.
    F,
}

impl ArrayOrder {
    /// Return the AXON attribute spelling for this order.
    pub fn tag(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::F => "F",
        }
    }
}

/// A numpy-compatible, fully resolved view of an AXON `Array` node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumpyLayout<'a> {
    shape: Vec<usize>,
    dtype: ArrayDtype,
    data: &'a [u8],
    order: ArrayOrder,
}

impl<'a> NumpyLayout<'a> {
    /// The resolved shape.  A single negative declared dimension has already
    /// been inferred from the payload length.
    pub fn shape(&self) -> &[usize] {
        &self.shape
    }

    /// The effective dtype, including the reference's missing-type `f32`
    /// default.
    pub fn dtype(&self) -> ArrayDtype {
        self.dtype
    }

    /// The effective byte payload, including the missing-data empty default.
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// The resolved C/F memory order.
    pub fn order(&self) -> ArrayOrder {
        self.order
    }
}

/// A strongly-typed wrapper around an AXON `Array` node for scientific tensors.
pub struct ScientificArray {
    node: Node,
}

impl ScientificArray {
    /// Attempts to parse an AXON Value into a ScientificArray.
    pub fn from_value(value: Value) -> Result<Self, Error> {
        match value {
            Value::Node(node) if node.name == "Array" => Ok(Self { node: *node }),
            _ => Err(numpy_value_error("Expected an AXON Array Node")),
        }
    }

    /// Borrow the underlying `Array` node without losing extension attributes.
    pub fn as_node(&self) -> &Node {
        &self.node
    }

    /// Recover the complete AXON node for serialization or further processing.
    pub fn into_value(self) -> Value {
        Value::Node(Box::new(self.node))
    }

    /// Returns every shape entry that is a non-negative, platform-sized integer.
    ///
    /// This preserves the original permissive accessor. Use [`Self::try_shape`]
    /// when malformed or missing dimensions must be reported rather than
    /// omitted.
    pub fn shape(&self) -> Vec<usize> {
        if let Some(Value::List(items)) = self
            .node
            .attributes
            .iter()
            .find(|(k, _)| k == "shape")
            .map(|(_, v)| v)
        {
            items
                .iter()
                .filter_map(|v| match v {
                    Value::Int(s) => s.parse::<usize>().ok(),
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Return the shape, rejecting missing, malformed, negative, or
    /// platform-sized dimensions instead of silently omitting them.
    pub fn try_shape(&self) -> Result<Vec<usize>, Error> {
        let shape = self
            .attribute("shape")
            .ok_or_else(|| Error::new(Category::UnexpectedEnd, "Missing shape", 0, 0))?;
        let Value::List(items) = shape else {
            return Err(Error::new(
                Category::UnexpectedToken,
                "Array shape must be a list of non-negative integers",
                0,
                0,
            ));
        };
        let mut dimensions = Vec::with_capacity(items.len());
        for item in items {
            let Value::Int(text) = item else {
                return Err(Error::new(
                    Category::UnexpectedToken,
                    "Array shape dimensions must be non-negative integers that fit usize",
                    0,
                    0,
                ));
            };
            dimensions.push(text.parse::<usize>().map_err(|_| {
                Error::new(
                    Category::UnexpectedToken,
                    "Array shape dimensions must be non-negative integers that fit usize",
                    0,
                    0,
                )
            })?);
        }
        Ok(dimensions)
    }

    /// Returns the raw datatype tag string (e.g., "f32", "u8").
    pub fn datatype(&self) -> Option<&str> {
        if let Some(Value::String(s)) = self
            .node
            .attributes
            .iter()
            .find(|(k, _)| k == "type")
            .map(|(_, v)| v)
        {
            Some(s.as_str())
        } else {
            None
        }
    }

    /// Returns the parsed element type, if it is a recognized dtype.
    pub fn dtype(&self) -> Option<ArrayDtype> {
        self.datatype().and_then(ArrayDtype::from_tag)
    }

    /// Returns the raw binary payload.
    pub fn data(&self) -> Option<&[u8]> {
        if let Some(Value::Bytes(b)) = self
            .node
            .attributes
            .iter()
            .find(|(k, _)| k == "data")
            .map(|(_, v)| v)
        {
            Some(b.as_slice())
        } else {
            None
        }
    }

    /// Returns the raw string order, defaulting to `"C"` when absent or not a
    /// string. Use [`Self::numpy_layout`] for the reference's resolved order.
    pub fn order(&self) -> &str {
        if let Some(Value::String(s)) = self
            .node
            .attributes
            .iter()
            .find(|(k, _)| k == "order")
            .map(|(_, v)| v)
        {
            s.as_str()
        } else {
            "C"
        }
    }

    /// Resolve this node using the exact defaults and reshape rules of the
    /// reference numpy bridge.
    ///
    /// Missing `shape`, `type`, `data`, and `order` attributes default to
    /// scalar (`[]`), `f32`, empty bytes, and C order respectively.  Explicit
    /// `null` shape means flatten to one dimension.  Integer, list, and tuple
    /// shapes are accepted, with one arbitrary negative dimension inferred in
    /// the same way as numpy.  Node style and children are deliberately ignored
    /// because the reference checks only the node's name.
    pub fn numpy_layout(&self) -> Result<NumpyLayout<'_>, Error> {
        let dtype = match self.attribute("type") {
            None => ArrayDtype::F32,
            Some(Value::String(tag)) => ArrayDtype::from_tag(tag).ok_or_else(|| {
                numpy_value_error(alloc::format!("Unsupported AXON array type: {tag}"))
            })?,
            Some(value @ (Value::List(_) | Value::Map(_) | Value::Set(_))) => {
                return Err(numpy_type_error(alloc::format!(
                    "unhashable type: '{}'",
                    python_type_name(value)
                )));
            }
            Some(value) => {
                return Err(numpy_value_error(alloc::format!(
                    "Unsupported AXON array type: {}",
                    python_scalar_text(value)
                )));
            }
        };

        let data = match self.attribute("data") {
            None => &[][..],
            Some(Value::Bytes(bytes)) => bytes.as_slice(),
            Some(value) => {
                return Err(numpy_type_error(alloc::format!(
                    "a bytes-like object is required, not '{}'",
                    python_type_name(value)
                )));
            }
        };
        if data.len() % dtype.element_size() != 0 {
            return Err(numpy_value_error(
                "buffer size must be a multiple of element size",
            ));
        }
        let elements = data.len() / dtype.element_size();
        let shape = resolve_numpy_shape(self.attribute("shape"), elements)?;

        let order = match self.attribute("order") {
            None | Some(Value::Null) => ArrayOrder::C,
            Some(Value::String(value)) => match value.as_str() {
                "C" | "c" | "A" | "a" => ArrayOrder::C,
                "F" | "f" => ArrayOrder::F,
                "K" | "k" => {
                    return Err(numpy_value_error(
                        "order 'K' is not permitted for reshaping",
                    ));
                }
                _ => {
                    return Err(numpy_value_error(alloc::format!(
                        "order must be one of 'C', 'F', 'A', or 'K' (got '{value}')"
                    )));
                }
            },
            Some(value) => {
                return Err(numpy_type_error(alloc::format!(
                    "order must be str, not {}",
                    python_type_name(value)
                )));
            }
        };

        Ok(NumpyLayout {
            shape,
            dtype,
            data,
            order,
        })
    }

    /// Return the effective, numpy-compatible shape.
    pub fn resolved_shape(&self) -> Result<Vec<usize>, Error> {
        Ok(self.numpy_layout()?.shape)
    }

    /// Validate this array with the reference numpy bridge's effective defaults
    /// and reshape behavior.
    pub fn validate_layout(&self) -> Result<(), Error> {
        self.numpy_layout().map(|_| ())
    }

    /// Validate a stricter, self-contained AXON array layout.
    ///
    /// This checks the brace-node shape, required attributes, recognized dtype,
    /// `C`/`F` order, checked shape multiplication, and exact agreement between
    /// the declared element count and the byte payload.  It is an extension;
    /// [`Self::validate_layout`] is the reference-parity validator.
    pub fn validate_strict_layout(&self) -> Result<(), Error> {
        if self.node.style != NodeStyle::Brace || !self.node.children.is_empty() {
            return Err(Error::new(
                Category::UnexpectedToken,
                "Array must be a brace node without children",
                0,
                0,
            ));
        }
        let dimensions = self.try_shape()?;
        let dtype = match self.attribute("type") {
            Some(Value::String(tag)) => ArrayDtype::from_tag(tag).ok_or_else(|| {
                Error::new(
                    Category::UnexpectedToken,
                    "Unsupported AXON array type",
                    0,
                    0,
                )
            })?,
            None => {
                return Err(Error::new(Category::UnexpectedEnd, "Missing type", 0, 0));
            }
            Some(_) => {
                return Err(Error::new(
                    Category::UnexpectedToken,
                    "Array type must be a string",
                    0,
                    0,
                ));
            }
        };
        let data = match self.attribute("data") {
            Some(Value::Bytes(bytes)) => bytes.as_slice(),
            None => {
                return Err(Error::new(Category::UnexpectedEnd, "Missing data", 0, 0));
            }
            Some(_) => {
                return Err(Error::new(
                    Category::UnexpectedToken,
                    "Array data must be binary",
                    0,
                    0,
                ));
            }
        };
        match self.attribute("order") {
            None => {}
            Some(Value::String(order)) if matches!(order.as_str(), "C" | "F") => {}
            Some(Value::String(_)) => {
                return Err(Error::new(
                    Category::UnexpectedToken,
                    "Array order must be C or F",
                    0,
                    0,
                ));
            }
            Some(_) => {
                return Err(Error::new(
                    Category::UnexpectedToken,
                    "Array order must be a string",
                    0,
                    0,
                ));
            }
        }

        let elements = dimensions
            .into_iter()
            .try_fold(1usize, |product, dimension| {
                product.checked_mul(dimension).ok_or_else(|| {
                    Error::new(
                        Category::ResourceLimitExceeded,
                        "Array shape product exceeds this platform's limit",
                        0,
                        0,
                    )
                })
            })?;
        let expected = elements.checked_mul(dtype.element_size()).ok_or_else(|| {
            Error::new(
                Category::ResourceLimitExceeded,
                "Array byte length exceeds this platform's limit",
                0,
                0,
            )
        })?;
        if expected != data.len() {
            return Err(Error::new(
                Category::UnexpectedToken,
                "Array data length does not match shape and element type",
                0,
                0,
            ));
        }
        Ok(())
    }

    /// Return the raw little-endian payload after checking the requested dtype
    /// and the numpy-compatible effective layout.
    fn typed_bytes(&self, want: ArrayDtype) -> Result<&[u8], Error> {
        let layout = self.numpy_layout()?;
        if layout.dtype != want {
            return Err(Error::new(
                Category::UnexpectedToken,
                "Array type does not match the requested element type",
                0,
                0,
            ));
        }
        Ok(layout.data)
    }

    fn attribute(&self, name: &str) -> Option<&Value> {
        self.node
            .attributes
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value))
    }
}

fn invalid_shape(message: impl Into<String>) -> Error {
    numpy_value_error(message)
}

fn parse_numpy_dimension(value: &Value) -> Result<isize, Error> {
    if matches!(value, Value::Bool(_)) {
        return Err(numpy_type_error("an integer is required"));
    }
    let Value::Int(text) = value else {
        return Err(numpy_type_error(alloc::format!(
            "'{}' object cannot be interpreted as an integer",
            python_type_name(value)
        )));
    };
    text.parse::<isize>()
        .map_err(|_| invalid_shape("Maximum allowed dimension exceeded"))
}

fn numpy_shape_text(dimensions: &[isize]) -> String {
    if dimensions.is_empty() {
        return "()".to_string();
    }
    let mut shape = "(".to_string();
    for (index, dimension) in dimensions.iter().enumerate() {
        if index != 0 {
            shape.push(',');
        }
        if *dimension < 0 {
            shape.push_str("newaxis");
        } else {
            shape.push_str(&dimension.to_string());
        }
    }
    if dimensions.len() == 1 {
        shape.push(',');
    }
    shape.push(')');
    shape
}

fn cannot_reshape(elements: usize, dimensions: &[isize]) -> Error {
    invalid_shape(alloc::format!(
        "cannot reshape array of size {elements} into shape {}",
        numpy_shape_text(dimensions)
    ))
}

fn resolve_numpy_shape(shape: Option<&Value>, elements: usize) -> Result<Vec<usize>, Error> {
    // `reshape(None)` is a numpy flatten operation.  Missing shape is different:
    // the reference's `.get(..., [])` supplies an empty scalar shape.
    if matches!(shape, Some(Value::Null)) {
        return Ok(alloc::vec![elements]);
    }

    let declared: Vec<isize> = match shape {
        None => Vec::new(),
        Some(value @ Value::Int(_)) => alloc::vec![parse_numpy_dimension(value)?],
        Some(Value::List(items)) | Some(Value::Tuple(items)) => items
            .iter()
            .map(parse_numpy_dimension)
            .collect::<Result<_, _>>()?,
        Some(value) => {
            return Err(numpy_type_error(alloc::format!(
                "expected a sequence of integers or a single integer, got '{}'",
                python_scalar_text(value)
            )));
        }
    };

    let mut negative = None;
    let mut known_product = 1usize;
    for (index, dimension) in declared.iter().copied().enumerate() {
        if dimension < 0 {
            if negative.replace(index).is_some() {
                return Err(invalid_shape("can only specify one unknown dimension"));
            }
        } else {
            known_product = known_product
                .checked_mul(dimension as usize)
                .ok_or_else(|| invalid_shape("Maximum allowed dimension exceeded"))?;
        }
    }

    let inferred = if negative.is_some() {
        if known_product == 0 || !elements.is_multiple_of(known_product) {
            return Err(cannot_reshape(elements, &declared));
        }
        Some(elements / known_product)
    } else {
        if known_product != elements {
            return Err(cannot_reshape(elements, &declared));
        }
        None
    };

    Ok(declared
        .into_iter()
        .enumerate()
        .map(|(index, dimension)| {
            if Some(index) == negative {
                inferred.expect("negative dimension has inferred extent")
            } else {
                dimension as usize
            }
        })
        .collect())
}

/// Generate a `to_<t>_vec` extractor for a fixed-width primitive dtype: it
/// checks the dtype tag and decodes the little-endian `data` payload.
macro_rules! native_extractor {
    ($method:ident, $ty:ty, $dtype:expr, $width:literal, $doc:literal) => {
        impl ScientificArray {
            #[doc = $doc]
            pub fn $method(&self) -> Result<Vec<$ty>, Error> {
                let bytes = self.typed_bytes($dtype)?;
                let mut out = Vec::with_capacity(bytes.len() / $width);
                for chunk in bytes.chunks_exact($width) {
                    // `chunks_exact($width)` yields exactly `$width` bytes, so
                    // the conversion cannot fail.
                    let arr: [u8; $width] = chunk.try_into().expect("chunks_exact width");
                    out.push(<$ty>::from_le_bytes(arr));
                }
                Ok(out)
            }
        }
    };
}

native_extractor!(
    to_u8_vec,
    u8,
    ArrayDtype::U8,
    1,
    "Decode the payload as `u8` elements."
);
native_extractor!(
    to_u16_vec,
    u16,
    ArrayDtype::U16,
    2,
    "Decode the little-endian payload as `u16` elements."
);
native_extractor!(
    to_u32_vec,
    u32,
    ArrayDtype::U32,
    4,
    "Decode the little-endian payload as `u32` elements."
);
native_extractor!(
    to_u64_vec,
    u64,
    ArrayDtype::U64,
    8,
    "Decode the little-endian payload as `u64` elements."
);
native_extractor!(
    to_i8_vec,
    i8,
    ArrayDtype::I8,
    1,
    "Decode the payload as `i8` elements."
);
native_extractor!(
    to_i16_vec,
    i16,
    ArrayDtype::I16,
    2,
    "Decode the little-endian payload as `i16` elements."
);
native_extractor!(
    to_i32_vec,
    i32,
    ArrayDtype::I32,
    4,
    "Decode the little-endian payload as `i32` elements."
);
native_extractor!(
    to_i64_vec,
    i64,
    ArrayDtype::I64,
    8,
    "Decode the little-endian payload as `i64` elements."
);
native_extractor!(
    to_f32_vec,
    f32,
    ArrayDtype::F32,
    4,
    "Decode the little-endian payload as `f32` elements."
);
native_extractor!(
    to_f64_vec,
    f64,
    ArrayDtype::F64,
    8,
    "Decode the little-endian payload as `f64` elements."
);
native_extractor!(
    to_f16_bits_vec,
    u16,
    ArrayDtype::F16,
    2,
    "Decode each little-endian `f16` element as its raw IEEE-754 binary16 bits."
);

macro_rules! native_constructor {
    ($method:ident, $ty:ty, $dtype:expr, $doc:literal) => {
        #[doc = $doc]
        pub fn $method(
            shape: Vec<usize>,
            values: Vec<$ty>,
            order: ArrayOrder,
        ) -> Result<Self, Error> {
            let mut data = Vec::with_capacity(values.len() * core::mem::size_of::<$ty>());
            for value in values {
                data.extend_from_slice(&value.to_le_bytes());
            }
            Self::from_native_bytes(shape, $dtype, data, order)
        }
    };
}

impl ScientificArray {
    /// Decode each binary16 value to an exactly representable `f32`.
    pub fn to_f16_vec(&self) -> Result<Vec<f32>, Error> {
        Ok(self
            .to_f16_bits_vec()?
            .into_iter()
            .map(f16_bits_to_f32)
            .collect())
    }

    /// Decode `c64` storage into `(real, imaginary)` `f32` component pairs.
    pub fn to_c64_components_vec(&self) -> Result<Vec<(f32, f32)>, Error> {
        let bytes = self.typed_bytes(ArrayDtype::C64)?;
        let mut output = Vec::with_capacity(bytes.len() / 8);
        for chunk in bytes.chunks_exact(8) {
            let real = f32::from_le_bytes(chunk[..4].try_into().expect("c64 real width"));
            let imaginary = f32::from_le_bytes(chunk[4..].try_into().expect("c64 imaginary width"));
            output.push((real, imaginary));
        }
        Ok(output)
    }

    /// Decode `c128` storage into `(real, imaginary)` `f64` component pairs.
    pub fn to_c128_components_vec(&self) -> Result<Vec<(f64, f64)>, Error> {
        let bytes = self.typed_bytes(ArrayDtype::C128)?;
        let mut output = Vec::with_capacity(bytes.len() / 16);
        for chunk in bytes.chunks_exact(16) {
            let real = f64::from_le_bytes(chunk[..8].try_into().expect("c128 real width"));
            let imaginary =
                f64::from_le_bytes(chunk[8..].try_into().expect("c128 imaginary width"));
            output.push((real, imaginary));
        }
        Ok(output)
    }

    /// Construct an `Array` node from a little-endian byte payload.
    ///
    /// The supplied flat payload is already arranged in `order`.  C order is
    /// represented by omitting the `order` attribute, exactly like
    /// `from_numpy`; F order is emitted explicitly.
    pub fn from_native_bytes(
        shape: Vec<usize>,
        dtype: ArrayDtype,
        data: Vec<u8>,
        order: ArrayOrder,
    ) -> Result<Self, Error> {
        let mut attributes = alloc::vec![
            (
                "shape".to_string(),
                Value::List(
                    shape
                        .into_iter()
                        .map(|dimension| Value::Int(dimension.to_string()))
                        .collect(),
                ),
            ),
            ("type".to_string(), Value::String(dtype.tag().to_string())),
            ("data".to_string(), Value::Bytes(data)),
        ];
        if order == ArrayOrder::F {
            attributes.push(("order".to_string(), Value::String("F".to_string())));
        }
        let array = Self {
            node: Node {
                name: "Array".to_string(),
                style: NodeStyle::Brace,
                attributes,
                children: Vec::new(),
            },
        };
        array.validate_layout()?;
        Ok(array)
    }

    native_constructor!(
        from_u8_vec,
        u8,
        ArrayDtype::U8,
        "Construct a little-endian `u8` Array from flat storage-order values."
    );
    native_constructor!(
        from_u16_vec,
        u16,
        ArrayDtype::U16,
        "Construct a little-endian `u16` Array from flat storage-order values."
    );
    native_constructor!(
        from_u32_vec,
        u32,
        ArrayDtype::U32,
        "Construct a little-endian `u32` Array from flat storage-order values."
    );
    native_constructor!(
        from_u64_vec,
        u64,
        ArrayDtype::U64,
        "Construct a little-endian `u64` Array from flat storage-order values."
    );
    native_constructor!(
        from_i8_vec,
        i8,
        ArrayDtype::I8,
        "Construct a little-endian `i8` Array from flat storage-order values."
    );
    native_constructor!(
        from_i16_vec,
        i16,
        ArrayDtype::I16,
        "Construct a little-endian `i16` Array from flat storage-order values."
    );
    native_constructor!(
        from_i32_vec,
        i32,
        ArrayDtype::I32,
        "Construct a little-endian `i32` Array from flat storage-order values."
    );
    native_constructor!(
        from_i64_vec,
        i64,
        ArrayDtype::I64,
        "Construct a little-endian `i64` Array from flat storage-order values."
    );
    native_constructor!(
        from_f16_bits_vec,
        u16,
        ArrayDtype::F16,
        "Construct a little-endian `f16` Array from exact binary16 bit patterns."
    );
    native_constructor!(
        from_f32_vec,
        f32,
        ArrayDtype::F32,
        "Construct a little-endian `f32` Array from flat storage-order values."
    );
    native_constructor!(
        from_f64_vec,
        f64,
        ArrayDtype::F64,
        "Construct a little-endian `f64` Array from flat storage-order values."
    );

    /// Construct a little-endian `c64` Array from real/imaginary component
    /// pairs in flat storage order.
    pub fn from_c64_components_vec(
        shape: Vec<usize>,
        values: Vec<(f32, f32)>,
        order: ArrayOrder,
    ) -> Result<Self, Error> {
        let mut data = Vec::with_capacity(values.len() * 8);
        for (real, imaginary) in values {
            data.extend_from_slice(&real.to_le_bytes());
            data.extend_from_slice(&imaginary.to_le_bytes());
        }
        Self::from_native_bytes(shape, ArrayDtype::C64, data, order)
    }

    /// Construct a little-endian `c128` Array from real/imaginary component
    /// pairs in flat storage order.
    pub fn from_c128_components_vec(
        shape: Vec<usize>,
        values: Vec<(f64, f64)>,
        order: ArrayOrder,
    ) -> Result<Self, Error> {
        let mut data = Vec::with_capacity(values.len() * 16);
        for (real, imaginary) in values {
            data.extend_from_slice(&real.to_le_bytes());
            data.extend_from_slice(&imaginary.to_le_bytes());
        }
        Self::from_native_bytes(shape, ArrayDtype::C128, data, order)
    }
}

fn f16_bits_to_f32(bits: u16) -> f32 {
    let sign = (u32::from(bits & 0x8000)) << 16;
    let exponent = (bits >> 10) & 0x1f;
    let fraction = u32::from(bits & 0x03ff);
    let output = match exponent {
        0 if fraction == 0 => sign,
        0 => {
            let mut normalized = fraction;
            let mut unbiased = -14i32;
            while normalized & 0x0400 == 0 {
                normalized <<= 1;
                unbiased -= 1;
            }
            normalized &= 0x03ff;
            sign | (((unbiased + 127) as u32) << 23) | (normalized << 13)
        }
        0x1f => sign | 0x7f80_0000 | (fraction << 13),
        _ => sign | ((u32::from(exponent) + 112) << 23) | (fraction << 13),
    };
    f32::from_bits(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid_layout(source: &str) -> Error {
        let value = crate::from_str_stream(source)
            .expect("scientific test source parses")
            .remove(0);
        match ScientificArray::from_value(value) {
            Ok(array) => array
                .validate_layout()
                .expect_err("scientific test source must be rejected"),
            Err(error) => error,
        }
    }

    #[test]
    fn numpy_invalid_messages_and_error_classes_match_reference() {
        for (source, category, message) in [
            (
                r#"NotArray{shape:[1] type:"u8" data:|AA==|}"#,
                Category::UnexpectedToken,
                "Expected an AXON Array Node",
            ),
            (
                r#"Array{shape:[1] type:"x8" data:|AA==|}"#,
                Category::UnexpectedToken,
                "Unsupported AXON array type: x8",
            ),
            (
                r#"Array{shape:["x"] type:"u8" data:|AA==|}"#,
                Category::Serde,
                "'str' object cannot be interpreted as an integer",
            ),
            (
                r#"Array{shape:[2 2] type:"f32" data:|AACAPwAAAEA=|}"#,
                Category::UnexpectedToken,
                "cannot reshape array of size 2 into shape (2,2)",
            ),
            (
                r#"Array{shape:[1] type:"u8" data:"x"}"#,
                Category::Serde,
                "a bytes-like object is required, not 'str'",
            ),
            (
                r#"Array{shape:[1] type:"u8" data:|AA==| order:"X"}"#,
                Category::UnexpectedToken,
                "order must be one of 'C', 'F', 'A', or 'K' (got 'X')",
            ),
        ] {
            let error = invalid_layout(source);
            assert_eq!(error.category, category, "source: {source}");
            assert_eq!(error.message, message, "source: {source}");
        }
    }
}
