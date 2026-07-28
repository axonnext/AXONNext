//! The AXON 2026 semantic value model.
//!
//! [`Value`] is a faithful, lossless representation of every AXON kind, produced
//! by the parser ([`crate::from_str`]) and consumed by the writer
//! ([`crate::to_string`]). It is the pivot the Serde `Serializer`/`Deserializer`
//! map through, and it preserves the distinctions AXON is careful about: tuple
//! vs list, ordered map vs map, the node body kind, `Z` vs an explicit offset,
//! and decimal scale.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::error::{Category, Error};

/// Recursion bound for the direct `Value` walks, matching the section-16
/// default the parser and writer enforce. A parsed value is already within this
/// bound; the guard covers values a caller builds programmatically and hands
/// straight to [`Value::resolve_refs`], which would otherwise recurse until the
/// call stack was exhausted.
const DEFAULT_MAX_DEPTH: u32 = 128;

/// An exact base-10 decimal: `sign * coefficient * 10^exponent`, arbitrary
/// precision, with scale preserved (trailing zeros are significant unless
/// canonicalised). Specials are represented explicitly.
#[derive(Clone, Debug)]
pub enum Decimal {
    /// `mantissa * 10^exponent`. `mantissa` is a decimal digit string with an
    /// optional leading `-`, kept as text to preserve arbitrary precision
    /// without pulling in a bignum dependency in the core.
    Finite {
        /// Decimal digit string with an optional leading `-`.
        mantissa: String,
        /// Base-10 exponent (negative for fractional scale).
        exponent: i32,
    },
    /// A finite decimal parsed under the `scale-significant` profile.  This is
    /// deliberately a distinct semantic kind: unlike an ordinary decimal,
    /// its exponent participates in equality and Canonical AXON must reject it.
    ScaledFinite {
        /// Decimal digit string with an optional leading `-`.
        mantissa: String,
        /// Base-10 exponent, retained exactly.
        exponent: i32,
    },
    /// Positive decimal infinity (`∞D`).
    Infinity,
    /// Negative decimal infinity (`-∞D`).
    NegInfinity,
    /// Decimal not-a-number (`?D`).
    NaN,
}

impl PartialEq for Decimal {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Decimal::Finite {
                    mantissa: left,
                    exponent: left_exp,
                },
                Decimal::Finite {
                    mantissa: right,
                    exponent: right_exp,
                },
            ) => decimal_numeric_parts(left, *left_exp) == decimal_numeric_parts(right, *right_exp),
            (
                Decimal::ScaledFinite {
                    mantissa: left,
                    exponent: left_exp,
                },
                Decimal::ScaledFinite {
                    mantissa: right,
                    exponent: right_exp,
                },
            ) => {
                left_exp == right_exp && decimal_signed_digits(left) == decimal_signed_digits(right)
            }
            (Decimal::Infinity, Decimal::Infinity)
            | (Decimal::NegInfinity, Decimal::NegInfinity) => true,
            // Python Decimal NaN follows numeric NaN equality.
            (Decimal::NaN, Decimal::NaN) => false,
            _ => false,
        }
    }
}

fn decimal_signed_digits(text: &str) -> (bool, String) {
    let negative = text.starts_with('-');
    let digits = text.trim_start_matches('-').trim_start_matches('0');
    if digits.is_empty() {
        (false, "0".to_string())
    } else {
        (negative, digits.to_string())
    }
}

fn decimal_numeric_parts(text: &str, mut exponent: i32) -> (bool, String, i32) {
    let (negative, mut digits) = decimal_signed_digits(text);
    if digits == "0" {
        return (false, digits, 0);
    }
    while digits.ends_with('0') {
        // Only strip a trailing zero if the exponent can actually be
        // incremented; saturating at i32::MAX would collapse distinct boundary
        // values (mantissa "10" and "1" at i32::MAX differ by 10x) (audit M10).
        let Some(next) = exponent.checked_add(1) else {
            break;
        };
        digits.pop();
        exponent = next;
    }
    (negative, digits, exponent)
}

/// A time zone designation on a temporal value. Distinguishes local time (no
/// zone), UTC-designated `Z`, a numeric offset in minutes, and a named IANA zone
/// (with its offset) -- mirroring AXON §6.6.
#[derive(Clone, Debug, PartialEq)]
pub enum Zone {
    /// No offset -- a local (floating) time.
    Local,
    /// The `Z` designator (UTC), kept distinct from `Offset(0)`.
    Zulu,
    /// A fixed offset from UTC, in minutes (may be negative).
    Offset(i32),
    /// A named IANA zone with its resolved offset in minutes (`tz-names` profile).
    Named {
        /// Resolved offset from UTC, in minutes.
        offset: i32,
        /// IANA zone name (e.g., `America/Edmonton`).
        name: String,
    },
}

/// Wall-clock time with nanosecond precision and a zone designation.
#[derive(Clone, Debug, PartialEq)]
pub struct Time {
    /// Hour of day, 0-23.
    pub hour: u8,
    /// Minute, 0-59.
    pub minute: u8,
    /// Second, 0-59.
    pub second: u8,
    /// Sub-second nanoseconds, 0-999_999_999.
    pub nanosecond: u32,
    /// Significant fractional-second width (1..=9) under the
    /// `scale-significant` profile; otherwise `None`.
    pub fraction_digits: Option<u8>,
    /// Time-zone designation.
    pub zone: Zone,
}

/// A calendar date (no time component).
#[derive(Clone, Debug, PartialEq)]
pub struct Date {
    /// Calendar year.
    pub year: i32,
    /// Month, 1-12.
    pub month: u8,
    /// Day of month, 1-31.
    pub day: u8,
}

/// A date and time together.
#[derive(Clone, Debug, PartialEq)]
pub struct DateTime {
    /// The calendar date component.
    pub date: Date,
    /// The wall-clock time component.
    pub time: Time,
}

/// A temporal value: one of the three AXON temporal kinds.
#[derive(Clone, Debug, PartialEq)]
pub enum Temporal {
    /// A calendar date with no time component.
    Date(Date),
    /// A wall-clock time with no date component.
    Time(Time),
    /// A date and time together.
    DateTime(DateTime),
}

/// The body form of a [`Node`], preserving the syntactic distinction between the
/// four node shapes (a unit node is not the same value as an empty-tuple node).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeStyle {
    /// `Name` -- no body.
    Unit,
    /// `Name{ .. }` -- attributes and/or children.
    Brace,
    /// `Name( .. )` -- a tuple body.
    Tuple,
    /// `Name[ .. ]` -- a list body.
    List,
}

/// A named/tagged node: `Name`, `Name{attrs children}`, `Name(values)`, or
/// `Name[values]`. `name` may be namespaced as `ns/name`.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    /// The node name; may be namespaced as `ns/name`.
    pub name: String,
    /// The body form (unit / brace / tuple / list).
    pub style: NodeStyle,
    /// Attribute key/value pairs (brace bodies only), order preserved.
    pub attributes: Vec<(String, Value)>,
    /// Children (brace bodies) or positional values (tuple/list bodies).
    pub children: Vec<Value>,
}

/// A content-addressed or URI link (the Document Link profile).
#[derive(Clone, Debug, PartialEq)]
pub enum Link {
    /// A CIDv1 content identifier (text form).
    Cid(String),
    /// A URI.
    Uri(String),
}

/// An anchored value: `&label value` under the Graph profile (§10).
#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    /// The label bound by this anchor, without the leading `&`.
    pub label: String,
    /// The value the label is bound to.
    pub value: Value,
}

/// A single AXON value. Every AXON 2026 kind is represented here losslessly.
///
/// # Graph profile and identity
///
/// `Value` is a tree, so the Graph profile is first represented structurally:
/// `&label value` parses to [`Value::Anchor`] and `*label` to [`Value::Ref`].
/// Use [`crate::GraphDocument::from_value`] (or
/// [`crate::resolve_graph_stream`]) for an arena view in which shared identity
/// and cycles are directly observable. [`Value::resolve_refs`] remains useful
/// when a plain, necessarily acyclic tree expansion is desired instead.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `null`.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// An arbitrary-precision integer, kept as its canonical decimal text so the
    /// core needs no bignum crate; fits-in-`i128`/`u128` accessors are provided.
    Int(String),
    /// An IEEE-754 double.
    Float(f64),
    /// An exact base-10 decimal (`d`/`D` suffix in text).
    Decimal(Decimal),
    /// A UTF-8 string.
    String(String),
    /// A byte string.
    Bytes(Vec<u8>),
    /// A temporal value.
    Temporal(Temporal),
    /// An ordered list `[ .. ]`.
    List(Vec<Value>),
    /// A tuple `( .. )` -- distinct from a list.
    Tuple(Vec<Value>),
    /// An unordered map `{ key: value .. }`.
    Map(Vec<(Value, Value)>),
    /// An ordered map `[ key: value .. ]` -- order is semantic.
    OrderedMap(Vec<(Value, Value)>),
    /// A set `{ v .. }` / `∅`.
    Set(Vec<Value>),
    /// A named/tagged node.
    Node(Box<Node>),
    /// A content-addressed or URI link.
    Link(Link),
    /// `&label value` -- an anchored value (Graph profile, §10.1).
    Anchor(Box<Anchor>),
    /// `*label` -- a reference to an anchored value (Graph profile, §10.2).
    Ref(String),
    /// The compatibility profile's undefined-reference sentinel.  It has no
    /// Core text or Binary AXON spelling and checked writers reject it.
    Undefined,
}

impl Value {
    /// Returns the value as an `i128` if it is an integer that fits.
    pub fn as_i128(&self) -> Option<i128> {
        match self {
            Value::Int(s) => s.parse().ok(),
            _ => None,
        }
    }

    /// Returns the value as a `u128` if it is a non-negative integer that fits.
    pub fn as_u128(&self) -> Option<u128> {
        match self {
            Value::Int(s) => s.parse().ok(),
            _ => None,
        }
    }

    /// Returns the `f64` if this is a float.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Returns the string slice if this is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// True for `Value::Null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// A short, stable name for the kind, used in error messages and Serde's
    /// `unexpected` reporting.
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "integer",
            Value::Float(_) => "float",
            Value::Decimal(_) => "decimal",
            Value::String(_) => "string",
            Value::Bytes(_) => "bytes",
            Value::Temporal(_) => "temporal",
            Value::List(_) => "list",
            Value::Tuple(_) => "tuple",
            Value::Map(_) => "map",
            Value::OrderedMap(_) => "ordered-map",
            Value::Set(_) => "set",
            Value::Node(_) => "node",
            Value::Link(_) => "link",
            Value::Anchor(_) => "anchor",
            Value::Ref(_) => "reference",
            Value::Undefined => "undefined",
        }
    }

    /// Collect every `&label` anchor reachable from this value, outermost
    /// first. A label bound more than once is reported by the parser as
    /// `duplicate-anchor`, so the map here is keyed by first binding.
    pub fn anchors(&self) -> Vec<(&str, &Value)> {
        let mut out = Vec::new();
        let mut stack = alloc::vec![self];
        while let Some(value) = stack.pop() {
            match value {
                Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
                    stack.extend(items.iter().rev());
                }
                Value::Map(pairs) | Value::OrderedMap(pairs) => {
                    for (key, item) in pairs.iter().rev() {
                        stack.push(item);
                        stack.push(key);
                    }
                }
                Value::Node(node) => {
                    stack.extend(node.children.iter().rev());
                    stack.extend(node.attributes.iter().rev().map(|(_, item)| item));
                }
                Value::Anchor(anchor) => {
                    out.push((anchor.label.as_str(), &anchor.value));
                    stack.push(&anchor.value);
                }
                Value::Null
                | Value::Bool(_)
                | Value::Int(_)
                | Value::Float(_)
                | Value::Decimal(_)
                | Value::String(_)
                | Value::Bytes(_)
                | Value::Temporal(_)
                | Value::Link(_)
                | Value::Ref(_)
                | Value::Undefined => {}
            }
        }
        out
    }

    /// Replace every `*label` with the value its `&label` anchor binds,
    /// producing a plain tree with the anchors stripped.
    ///
    /// This expands shared structure into repeated subtrees: it preserves what
    /// the document *denotes*, not the sharing itself. A cyclic graph has no
    /// tree expansion at all, so `&a [1 *a]` is rejected with
    /// `unknown-reference` rather than looped on -- see the type-level note on
    /// [`Value`] for why identity is not modelled directly.
    pub fn resolve_refs(&self) -> Result<Value, Error> {
        let anchors = self.anchors();
        let mut table: Vec<(String, Value)> = Vec::new();
        for (label, value) in anchors {
            if !table.iter().any(|(l, _)| l == label) {
                table.push((label.to_string(), value.clone()));
            }
        }
        let mut active: Vec<String> = Vec::new();
        resolve_in(self, &table, &mut active, 0)
    }
}

/// Executes the reference-substitution walk backing [`Value::resolve_refs`],
/// tracking the anchors currently being expanded so a cycle is reported rather
/// than followed forever, and bounding nesting at [`DEFAULT_MAX_DEPTH`].
fn resolve_in(
    v: &Value,
    table: &[(String, Value)],
    active: &mut Vec<String>,
    depth: u32,
) -> Result<Value, Error> {
    if depth > DEFAULT_MAX_DEPTH {
        return Err(Error::new(
            Category::ResourceLimitExceeded,
            "maximum reference-resolution nesting depth exceeded",
            0,
            0,
        ));
    }
    let nested = depth.saturating_add(1);
    Ok(match v {
        Value::Ref(label) => {
            if active.iter().any(|l| l == label) {
                return Err(Error::new(
                    Category::UnknownReference,
                    alloc::format!("reference *{} is cyclic and has no tree expansion", label),
                    0,
                    0,
                ));
            }
            let bound = table
                .iter()
                .find(|(l, _)| l == label)
                .map(|(_, b)| b)
                .ok_or_else(|| {
                    Error::new(
                        Category::UnknownReference,
                        alloc::format!("reference *{} has no anchor", label),
                        0,
                        0,
                    )
                })?;
            active.push(label.clone());
            let out = resolve_in(bound, table, active, nested)?;
            active.pop();
            out
        }
        Value::Anchor(a) => {
            active.push(a.label.clone());
            let out = resolve_in(&a.value, table, active, nested)?;
            active.pop();
            out
        }
        Value::List(items) => Value::List(resolve_each(items, table, active, nested)?),
        Value::Tuple(items) => Value::Tuple(resolve_each(items, table, active, nested)?),
        Value::Set(items) => Value::Set(resolve_each(items, table, active, nested)?),
        Value::Map(pairs) => Value::Map(resolve_pairs(pairs, table, active, nested)?),
        Value::OrderedMap(pairs) => Value::OrderedMap(resolve_pairs(pairs, table, active, nested)?),
        Value::Node(n) => {
            let mut attributes = Vec::with_capacity(n.attributes.len());
            for (k, val) in &n.attributes {
                attributes.push((k.clone(), resolve_in(val, table, active, nested)?));
            }
            Value::Node(Box::new(Node {
                name: n.name.clone(),
                style: n.style,
                attributes,
                children: resolve_each(&n.children, table, active, nested)?,
            }))
        }
        other => other.clone(),
    })
}

/// Executes reference substitution across a sequence of values.
fn resolve_each(
    items: &[Value],
    table: &[(String, Value)],
    active: &mut Vec<String>,
    depth: u32,
) -> Result<Vec<Value>, Error> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(resolve_in(item, table, active, depth)?);
    }
    Ok(out)
}

/// Executes reference substitution across a sequence of key/value pairs.
fn resolve_pairs(
    pairs: &[(Value, Value)],
    table: &[(String, Value)],
    active: &mut Vec<String>,
    depth: u32,
) -> Result<Vec<(Value, Value)>, Error> {
    let mut out = Vec::with_capacity(pairs.len());
    for (k, v) in pairs {
        out.push((
            resolve_in(k, table, active, depth)?,
            resolve_in(v, table, active, depth)?,
        ));
    }
    Ok(out)
}
