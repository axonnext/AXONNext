//! Serde `Deserializer`: parses AXON text into a [`Value`], then drives any
//! `Deserialize` type from it. `Value` is self-describing, so
//! `deserialize_any` is fully supported (serde's `Deserialize` for untyped
//! data, enums, structs, and maps all work).

use alloc::string::String;
use alloc::vec::IntoIter;
use alloc::vec::Vec;

use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};

use alloc::borrow::Cow;
use alloc::boxed::Box;

use crate::error::{Category, Error, Result};
use crate::parser::{Options, Parser, ValueTrace, rewrite_indented_bodies};
use crate::value::{Decimal, Link, Temporal, Value};

fn utf8_text(input: &[u8]) -> Result<&str> {
    core::str::from_utf8(input).map_err(|error| {
        let offset = error.valid_up_to();
        let end = error
            .error_len()
            .map_or(input.len(), |length| offset.saturating_add(length));
        Error::new_with_span(
            Category::InvalidUnicode,
            "input is not well-formed utf-8",
            offset,
            end,
            1,
            offset.saturating_add(1) as u32,
        )
    })
}

/// Apply the `indented_bodies` source rewrite when that compat flag is on,
/// leaving the input untouched otherwise. The rewrite is whole-document, so it
/// belongs at the document entry points, not the incremental stream iterator --
/// mirroring the reference, whose incremental reader buffers the full input
/// before applying it.
fn preprocess(input: &str, opts: Options) -> Cow<'_, str> {
    if opts.indented_bodies_enabled() {
        Cow::Owned(rewrite_indented_bodies(input))
    } else {
        Cow::Borrowed(input)
    }
}

/// Parse `input` into a single [`Value`]. Errors if the input is empty or holds
/// more than one top-level value.
pub fn from_str_value(input: &str) -> Result<Value> {
    from_str_value_with(input, Options::default())
}

/// Parse one value from UTF-8 bytes, classifying malformed input as
/// [`Category::InvalidUnicode`] with the exact byte span.
pub fn from_slice_value(input: &[u8]) -> Result<Value> {
    from_str_value(utf8_text(input)?)
}

/// Parse one value from UTF-8 bytes with explicit parser options.
pub fn from_slice_value_with(input: &[u8], opts: Options) -> Result<Value> {
    from_str_value_with(utf8_text(input)?, opts)
}

/// Like [`from_str_value`] but with explicit parser [`Options`].
pub fn from_str_value_with(input: &str, opts: Options) -> Result<Value> {
    from_str_value_with_constants(input, opts, &[])
}

/// Like [`from_str_value_with`] with an application constant registry. A
/// non-empty registry replaces the four normative defaults.
pub fn from_str_value_with_constants(
    input: &str,
    opts: Options,
    constants: &[(String, Value)],
) -> Result<Value> {
    let mut values = parse_document_with_constants(input, opts, constants)?;
    match values.len() {
        1 => Ok(values.pop().unwrap()),
        0 => Err(Error::new(
            Category::UnexpectedEnd,
            "no value in input",
            0,
            0,
        )),
        _ => Err(Error::new(
            Category::UnexpectedToken,
            "expected a single top-level value",
            0,
            0,
        )),
    }
}

/// Parse every top-level value in a document stream (Core profile).
pub fn from_str_stream(input: &str) -> Result<Vec<Value>> {
    from_str_stream_with(input, Options::default())
}

/// Parse a complete AXON document stream from UTF-8 bytes.
pub fn from_slice_stream(input: &[u8]) -> Result<Vec<Value>> {
    from_str_stream(utf8_text(input)?)
}

/// Parse a complete UTF-8 byte stream with explicit parser options.
pub fn from_slice_stream_with(input: &[u8], opts: Options) -> Result<Vec<Value>> {
    from_str_stream_with(utf8_text(input)?, opts)
}

/// Parse every top-level value in a document stream with explicit [`Options`].
pub fn from_str_stream_with(input: &str, opts: Options) -> Result<Vec<Value>> {
    parse_document_with(input, opts)
}

/// Parse every top-level value with explicit options and constants.
pub fn from_str_stream_with_constants(
    input: &str,
    opts: Options,
    constants: &[(String, Value)],
) -> Result<Vec<Value>> {
    parse_document_with_constants(input, opts, constants)
}

/// Shared document parse: apply the indented-bodies rewrite when enabled, parse
/// the whole stream, then enforce the `canonical_verify` profile if requested.
fn parse_document_with(input: &str, opts: Options) -> Result<Vec<Value>> {
    parse_document_with_constants(input, opts, &[])
}

fn parse_document_with_constants(
    input: &str,
    opts: Options,
    constants: &[(String, Value)],
) -> Result<Vec<Value>> {
    parse_document_internal(input, opts, constants)
}

/// Trace-enabled document parse outcome used by the CST layer.
pub(crate) struct TracedDocument {
    /// Values parsed before canonical verification.
    pub(crate) values: Vec<Value>,
    /// Successful semantic value events. Canonical-verification failures retain
    /// these events, matching the reference implementation.
    pub(crate) traces: Vec<ValueTrace>,
    /// Parse or canonical-verification failure, if any.
    pub(crate) error: Option<Error>,
}

/// Parse a document while retaining exact semantic value byte ranges for the
/// CST layer. This deliberately remains crate-private: the public trace model
/// is exposed by `cst`, not by the Serde deserializer.
pub(crate) fn parse_document_with_trace(input: &str, opts: Options) -> TracedDocument {
    if let Err(error) = check_original_input(input, opts) {
        return TracedDocument {
            values: Vec::new(),
            traces: Vec::new(),
            error: Some(error),
        };
    }
    let src = preprocess(input, opts);
    let mut parser_opts = opts;
    // `max_input` applies to the caller's source, before the optional
    // indentation rewrite, exactly as in the reference implementation.
    parser_opts.limits.max_input = usize::MAX;
    let mut parser = Parser::new_tracing(&src, parser_opts);
    let values = match parser.parse_document() {
        Ok(values) => values,
        Err(error) => {
            return TracedDocument {
                values: Vec::new(),
                traces: Vec::new(),
                error: Some(error),
            };
        }
    };
    let error = canonical_error(&parser, &src, &values);
    let traces = parser.into_value_traces();
    TracedDocument {
        values,
        traces,
        error,
    }
}

fn check_original_input(input: &str, opts: Options) -> Result<()> {
    if input.len() > opts.limits.max_input {
        return Err(Error::new(
            Category::ResourceLimitExceeded,
            "input is too large",
            1,
            1,
        ));
    }
    Ok(())
}

fn canonical_error(parser: &Parser<'_>, src: &str, values: &[Value]) -> Option<Error> {
    if !parser.canonical_verify() {
        return None;
    }
    let (line, column) = parser.line_column();
    if parser.scale_significant() {
        return Some(Error::new_with_span(
            Category::UnsupportedProfileFeature,
            "scale-significant is incompatible with canonical verification",
            src.len(),
            src.len(),
            line,
            column,
        ));
    }
    let payload_start = parser.payload_start();
    let payload = &src[payload_start..];
    let payload = if payload_start > 0 {
        payload.trim_start_matches([' ', '\t', '\r', '\n'])
    } else {
        payload
    };
    let rendered = match crate::writer::to_string_stream_canonical(values) {
        Ok(rendered) => rendered,
        Err(error) => return Some(error),
    };
    (payload != rendered).then(|| {
        Error::new_with_span(
            Category::UnexpectedToken,
            "document payload is not canonical AXON",
            src.len(),
            src.len(),
            line,
            column,
        )
    })
}

fn parse_document_internal(
    input: &str,
    opts: Options,
    constants: &[(String, Value)],
) -> Result<Vec<Value>> {
    check_original_input(input, opts)?;
    let src = preprocess(input, opts);
    let mut parser_opts = opts;
    parser_opts.limits.max_input = usize::MAX;
    let mut parser = if constants.is_empty() {
        Parser::new(&src, parser_opts)
    } else {
        Parser::new_with_constants(&src, parser_opts, constants)
    };
    let values = parser.parse_document()?;
    if let Some(error) = canonical_error(&parser, &src, &values) {
        return Err(error);
    }
    Ok(values)
}

/// An iterator that incrementally parses and yields top-level AXON values.
pub struct StreamDeserializer<'a> {
    mode: StreamMode<'a>,
    failed: bool,
}

enum StreamMode<'a> {
    Direct(Box<Parser<'a>>),
    Buffered(IntoIter<Result<Value>>),
}

impl<'a> StreamDeserializer<'a> {
    /// Create a new stream deserializer.
    pub fn new(input: &'a str, opts: Options) -> Self {
        Self::new_with_constants(input, opts, &[])
    }

    /// Create an incremental reader with application constants.
    pub fn new_with_constants(
        input: &'a str,
        opts: Options,
        constants: &[(String, Value)],
    ) -> Self {
        fn buffered<'a>(result: Result<Vec<Value>>) -> StreamMode<'a> {
            let items: Vec<Result<Value>> = match result {
                Ok(values) => values.into_iter().map(Ok).collect(),
                Err(error) => alloc::vec![Err(error)],
            };
            StreamMode::Buffered(items.into_iter())
        }

        let mode = if opts.canonical_verify || opts.indented_bodies_enabled() {
            buffered(parse_document_with_constants(input, opts, constants))
        } else {
            let mut parser = if constants.is_empty() {
                Parser::new(input, opts)
            } else {
                Parser::new_with_constants(input, opts, constants)
            };
            match parser.starts_attribute_document() {
                Ok(true) => buffered(parser.parse_document()),
                Ok(false) => StreamMode::Direct(Box::new(parser)),
                Err(error) => StreamMode::Buffered(alloc::vec![Err(error)].into_iter()),
            }
        };
        Self {
            mode,
            failed: false,
        }
    }
}

impl<'a> Iterator for StreamDeserializer<'a> {
    type Item = Result<Value>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        let next = match &mut self.mode {
            StreamMode::Direct(parser) => parser.parse_next_value().transpose(),
            StreamMode::Buffered(items) => items.next(),
        };
        if matches!(next, Some(Err(_))) {
            self.failed = true;
        }
        next
    }
}

impl core::iter::FusedIterator for StreamDeserializer<'_> {}

/// Create an iterator that incrementally parses a stream of top-level AXON values.
pub fn iter_stream(input: &str) -> StreamDeserializer<'_> {
    iter_stream_with(input, Options::default())
}

/// Create a stream iterator with explicit [`Options`].
pub fn iter_stream_with(input: &str, opts: Options) -> StreamDeserializer<'_> {
    StreamDeserializer::new(input, opts)
}

/// Create an incremental stream iterator with explicit options and constants.
pub fn iter_stream_with_constants<'a>(
    input: &'a str,
    opts: Options,
    constants: &[(String, Value)],
) -> StreamDeserializer<'a> {
    StreamDeserializer::new_with_constants(input, opts, constants)
}

/// Deserialize a `T` from AXON text.
pub fn from_str<T: DeserializeOwned>(input: &str) -> Result<T> {
    let value = from_str_value(input)?;
    T::deserialize(value)
}

/// Deserialize a `T` from AXON text with explicit [`Options`].
pub fn from_str_with<T: DeserializeOwned>(input: &str, opts: Options) -> Result<T> {
    let value = from_str_value_with(input, opts)?;
    T::deserialize(value)
}

/// Deserialize a `T` with explicit options and an application constant registry.
pub fn from_str_with_constants<T: DeserializeOwned>(
    input: &str,
    opts: Options,
    constants: &[(String, Value)],
) -> Result<T> {
    let value = from_str_value_with_constants(input, opts, constants)?;
    T::deserialize(value)
}

/// Deserialize a `T` from UTF-8 bytes.
pub fn from_slice<T: DeserializeOwned>(input: &[u8]) -> Result<T> {
    from_str(utf8_text(input)?)
}

/// Deserialize a `T` from UTF-8 bytes with explicit parser options.
pub fn from_slice_with<T: DeserializeOwned>(input: &[u8], opts: Options) -> Result<T> {
    from_str_with(utf8_text(input)?, opts)
}

/// Deserialize a `T` from UTF-8 bytes with options and custom constants.
pub fn from_slice_with_constants<T: DeserializeOwned>(
    input: &[u8],
    opts: Options,
    constants: &[(String, Value)],
) -> Result<T> {
    from_str_with_constants(utf8_text(input)?, opts, constants)
}

impl<'de> de::Deserializer<'de> for Value {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        match self {
            Value::Null => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(b),
            Value::Int(s) => {
                if let Ok(i) = s.parse::<i64>() {
                    visitor.visit_i64(i)
                } else if let Ok(u) = s.parse::<u64>() {
                    visitor.visit_u64(u)
                } else if let Ok(i) = s.parse::<i128>() {
                    visitor.visit_i128(i)
                } else if let Ok(u) = s.parse::<u128>() {
                    visitor.visit_u128(u)
                } else {
                    visitor.visit_string(s)
                }
            }
            Value::Float(f) => visitor.visit_f64(f),
            Value::Decimal(d) => visitor.visit_string(decimal_to_string(&d)),
            Value::String(s) => visitor.visit_string(s),
            Value::Bytes(b) => visitor.visit_byte_buf(b),
            Value::Temporal(t) => visitor.visit_string(temporal_to_string(&t)),
            Value::List(items) => visitor.visit_seq(SeqDeserializer::new(items)),
            Value::Tuple(items) => visitor.visit_seq(SeqDeserializer::new(items)),
            Value::Set(items) => visitor.visit_seq(SeqDeserializer::new(items)),
            Value::Map(pairs) => visitor.visit_map(MapDeserializer::new(pairs)),
            Value::OrderedMap(pairs) => visitor.visit_map(MapDeserializer::new(pairs)),
            Value::Node(node) => visitor.visit_map(NodeDeserializer::new(*node)),
            Value::Link(link) => match link {
                Link::Cid(s) | Link::Uri(s) => visitor.visit_string(s),
            },
            // An anchor is transparent to Serde: `&a 1` deserializes as `1`.
            Value::Anchor(a) => a.value.deserialize_any(visitor),
            // A reference cannot be: resolving it needs the whole document, so
            // the tree model has nothing to hand the visitor here. Call
            // `Value::resolve_refs` before deserializing a graph document.
            Value::Ref(label) => Err(Error::new(
                Category::UnknownReference,
                alloc::format!(
                    "cannot deserialize the reference *{}; call Value::resolve_refs first",
                    label
                ),
                0,
                0,
            )),
            Value::Undefined => Err(Error::new(
                Category::UnknownReference,
                "undefined sentinel has no Core value",
                0,
                0,
            )),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        match self {
            Value::Null => visitor.visit_none(),
            other => visitor.visit_some(other),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        match self {
            // externally tagged: a node (Variant{..}/Variant(..)) or a bare
            // string (unit variant).
            Value::Node(node) => visitor.visit_enum(EnumDeserializer {
                variant: node.name.clone(),
                value: Some(Value::Node(node)),
            }),
            Value::String(s) => visitor.visit_enum(EnumDeserializer {
                variant: s,
                value: None,
            }),
            other => Err(Error::new(
                Category::Serde,
                alloc::format!("cannot deserialize enum from {}", other.kind()),
                0,
                0,
            )),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        // A RawValue asks for the value as AXON source text rather than as
        // something Serde can model. `self` is already the parsed subtree, so
        // writing it back out is exact, and nothing crosses Serde's data model
        // on the way — which is what keeps a decimal a decimal and a tuple a
        // tuple.
        if name == crate::raw::RAW_VALUE_TOKEN {
            let text = crate::writer::to_string(&self)?;
            return visitor.visit_string(text);
        }
        visitor.visit_newtype_struct(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct
        identifier ignored_any
    }
}

impl<'de> IntoDeserializer<'de, Error> for Value {
    type Deserializer = Value;
    fn into_deserializer(self) -> Value {
        self
    }
}

struct SeqDeserializer {
    iter: alloc::vec::IntoIter<Value>,
}
impl SeqDeserializer {
    fn new(items: Vec<Value>) -> Self {
        SeqDeserializer {
            iter: items.into_iter(),
        }
    }
}
impl<'de> SeqAccess<'de> for SeqDeserializer {
    type Error = Error;
    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        match self.iter.next() {
            Some(v) => seed.deserialize(v).map(Some),
            None => Ok(None),
        }
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

struct MapDeserializer {
    iter: alloc::vec::IntoIter<(Value, Value)>,
    value: Option<Value>,
}
impl MapDeserializer {
    fn new(pairs: Vec<(Value, Value)>) -> Self {
        MapDeserializer {
            iter: pairs.into_iter(),
            value: None,
        }
    }
}
impl<'de> MapAccess<'de> for MapDeserializer {
    type Error = Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>> {
        match self.iter.next() {
            Some((k, v)) => {
                self.value = Some(v);
                seed.deserialize(k).map(Some)
            }
            None => Ok(None),
        }
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value> {
        let v = self
            .value
            .take()
            .ok_or_else(|| Error::new(Category::Serde, "value without key", 0, 0))?;
        seed.deserialize(v)
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

/// Deserialize a node as a map of its attributes (children ignored for struct
/// mapping; use `Value` directly for full-fidelity access).
struct NodeDeserializer {
    iter: alloc::vec::IntoIter<(String, Value)>,
    value: Option<Value>,
}
impl NodeDeserializer {
    fn new(node: crate::value::Node) -> Self {
        NodeDeserializer {
            iter: node.attributes.into_iter(),
            value: None,
        }
    }
}
impl<'de> MapAccess<'de> for NodeDeserializer {
    type Error = Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>> {
        match self.iter.next() {
            Some((k, v)) => {
                self.value = Some(v);
                seed.deserialize(Value::String(k)).map(Some)
            }
            None => Ok(None),
        }
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value> {
        let v = self
            .value
            .take()
            .ok_or_else(|| Error::new(Category::Serde, "value without key", 0, 0))?;
        seed.deserialize(v)
    }
}

struct EnumDeserializer {
    variant: String,
    value: Option<Value>,
}
impl<'de> EnumAccess<'de> for EnumDeserializer {
    type Error = Error;
    type Variant = VariantDeserializer;
    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self::Variant)> {
        let v = seed.deserialize(Value::String(self.variant))?;
        Ok((v, VariantDeserializer { value: self.value }))
    }
}

struct VariantDeserializer {
    value: Option<Value>,
}
impl<'de> VariantAccess<'de> for VariantDeserializer {
    type Error = Error;
    fn unit_variant(self) -> Result<()> {
        // A unit variant is a bare name; refuse to silently discard a payload
        // (`Variant{..}` / `Variant(..)`) the caller thought they were reading.
        match self.value {
            None => Ok(()),
            Some(Value::Node(node)) if node.attributes.is_empty() && node.children.is_empty() => {
                Ok(())
            }
            Some(_) => Err(Error::new(
                Category::Serde,
                "unexpected payload for unit variant",
                0,
                0,
            )),
        }
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value> {
        match self.value {
            Some(Value::Node(node)) if node.attributes.is_empty() && node.children.len() == 1 => {
                let mut children = node.children;
                seed.deserialize(children.pop().unwrap())
            }
            // Any other shape would either drop attributes or reinterpret a
            // multi-child node as the payload itself -- fail instead.
            Some(_) => Err(Error::new(
                Category::Serde,
                "expected a newtype variant node with exactly one value",
                0,
                0,
            )),
            None => Err(Error::new(
                Category::Serde,
                "missing newtype variant value",
                0,
                0,
            )),
        }
    }
    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value> {
        match self.value {
            Some(Value::Node(node)) if node.attributes.is_empty() => {
                visitor.visit_seq(SeqDeserializer::new(node.children))
            }
            _ => Err(Error::new(
                Category::Serde,
                "expected tuple variant node",
                0,
                0,
            )),
        }
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        match self.value {
            Some(Value::Node(node)) if node.children.is_empty() => {
                visitor.visit_map(NodeDeserializer::new(*node))
            }
            _ => Err(Error::new(
                Category::Serde,
                "expected struct variant node",
                0,
                0,
            )),
        }
    }
}

fn decimal_to_string(d: &Decimal) -> String {
    crate::writer::to_string(&Value::Decimal(d.clone())).unwrap_or_default()
}
fn temporal_to_string(t: &Temporal) -> String {
    crate::writer::to_string(&Value::Temporal(t.clone())).unwrap_or_default()
}
