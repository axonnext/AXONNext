//! Binary AXON -- a CBOR (RFC 8949) profile of the AXON 2026 value model.
//!
//! [`encode`] produces well-formed CBOR; [`encode_canonical`] produces
//! deterministic, CID-ready **Canonical Binary AXON** (shortest-form floats,
//! sorted map/attribute keys, decimal normal form). [`decode`] reconstructs a
//! [`Value`]. Standard CBOR tags are reused where they exist (2/3 bignum, 4
//! decimal, 258 set, 28/29 value sharing, 42 CID, 32 URI); a provisional AXON
//! tag block covers the rest (720 node, 724 tuple, 725 ordered map, 727
//! temporal, 731 decimal-special). This is a faithful port of the reference
//! `binary2026` codec: encoding a value produces byte-identical output to the
//! reference (verified by the differential vectors). Decoding additionally
//! hardens against malformed input -- length fields are bounds-checked and the
//! decoder uses an explicit frame stack, so adversarial nesting cannot exhaust
//! the Rust call stack. [`decode_with`] additionally supports a caller-selected
//! nesting limit.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::error::{Category, Error};
use crate::value::{
    Anchor, Date, DateTime, Decimal, Link, Node, NodeStyle, Temporal, Time, Value, Zone,
};

const T_NODE: u64 = 720;
const T_TUPLE: u64 = 724;
const T_OMAP: u64 = 725;
const T_TEMP: u64 = 727;
const T_DECSP: u64 = 731;
const T_BIGPOS: u64 = 2;
const T_BIGNEG: u64 = 3;
const T_DECFRAC: u64 = 4;
const T_SET: u64 = 258;
const T_SHARE: u64 = 28;
const T_SHAREREF: u64 = 29;
const T_CID: u64 = 42;
const T_URI: u64 = 32;

/// Default maximum encoding/decoding depth (§16).
pub const DEFAULT_MAX_DEPTH: u32 = 128;

// The Python reference decoder imposes its own codec-level guard at
// `binary2026.DEFAULT_MAX_DEPTH` (128) and raises `BinaryAxonError` past it, so
// the default decode envelope here is the same section-16 bound both text
// parsers and both binary encoders use. Callers who need a different profile
// select it explicitly through `decode_with`.

fn err(msg: &str) -> Error {
    Error::new(Category::Serde, msg, 0, 0)
}

// ---- encoding primitives -------------------------------------------------- //

/// Write a CBOR head: `major << 5 | argument`, with the argument inline or in
/// 1/2/4/8 following big-endian bytes.
fn head(out: &mut Vec<u8>, major: u8, n: u64) {
    let m = major << 5;
    if n < 24 {
        out.push(m | n as u8);
    } else if n < 256 {
        out.push(m | 24);
        out.push(n as u8);
    } else if n < 65536 {
        out.push(m | 25);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else if n < 0x1_0000_0000 {
        out.push(m | 26);
        out.extend_from_slice(&(n as u32).to_be_bytes());
    } else {
        out.push(m | 27);
        out.extend_from_slice(&n.to_be_bytes());
    }
}

/// The minimal big-endian magnitude of `v` (at least one byte).
fn magnitude_bytes(v: u128) -> Vec<u8> {
    if v == 0 {
        return alloc::vec![0];
    }
    let full = v.to_be_bytes();
    let first = full.iter().position(|&b| b != 0).unwrap_or(15);
    full[first..].to_vec()
}

/// Write a CBOR tag head.
fn tag(out: &mut Vec<u8>, t: u64) {
    head(out, 6, t);
}

/// True when `s` is a well-formed AXON integer literal: an optional leading `-`
/// followed by one or more ASCII digits. The bignum fallback in [`enc_int`]
/// interprets each byte as `b - b'0'`, so a non-digit byte would underflow;
/// callers that accept a caller-built [`Value::Int`] validate with this first.
fn is_valid_int_text(s: &str) -> bool {
    let digits = s.strip_prefix('-').unwrap_or(s);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Encode an arbitrary-precision integer (stored as decimal text) as CBOR:
/// major 0/1 when it fits in 64 bits, else a bignum (tag 2/3).
///
/// The caller must ensure `s` is well-formed ([`is_valid_int_text`]); every
/// value produced by the parser, the deserializer, or the decoder already is.
fn enc_int(out: &mut Vec<u8>, s: &str) {
    let neg = s.starts_with('-');
    let digits = s.trim_start_matches('-');
    // Python's ``int`` has no negative zero.  Decimal preserves the sign of a
    // zero coefficient, however, and the reference Binary encoder converts
    // that coefficient through ``int`` before writing it.  Treat every all-zero
    // spelling as CBOR unsigned zero before entering the negative-integer path;
    // otherwise `-0D` would attempt `0u128 - 1` and panic in debug builds.
    if digits.bytes().all(|byte| byte == b'0') {
        head(out, 0, 0);
        return;
    }
    if let Ok(mag) = digits.parse::<u128>() {
        if !neg {
            if mag <= u64::MAX as u128 {
                head(out, 0, mag as u64);
            } else {
                tag(out, T_BIGPOS);
                let body = magnitude_bytes(mag);
                head(out, 2, body.len() as u64);
                out.extend_from_slice(&body);
            }
        } else {
            let m = mag - 1; // -1 - i, with i = -mag
            if m <= u64::MAX as u128 {
                head(out, 1, m as u64);
            } else {
                tag(out, T_BIGNEG);
                let body = magnitude_bytes(m);
                head(out, 2, body.len() as u64);
                out.extend_from_slice(&body);
            }
        }
    } else {
        // Beyond u128: fall back to decimal-string bignum arithmetic.
        let (t, body) = if !neg {
            (T_BIGPOS, dec_to_be_bytes(digits))
        } else {
            (T_BIGNEG, dec_to_be_bytes(&dec_sub1(digits)))
        };
        tag(out, t);
        head(out, 2, body.len() as u64);
        out.extend_from_slice(&body);
    }
}

/// Decimal-digit string -> minimal big-endian bytes (base-10 -> base-256).
fn dec_to_be_bytes(digits: &str) -> Vec<u8> {
    let mut num: Vec<u8> = digits.bytes().map(|b| b - b'0').collect();
    let mut out: Vec<u8> = Vec::new();
    while num.iter().any(|&d| d != 0) {
        let mut rem = 0u16;
        for d in num.iter_mut() {
            let cur = rem * 10 + *d as u16;
            *d = (cur / 256) as u8;
            rem = cur % 256;
        }
        out.push(rem as u8);
        while num.first() == Some(&0) && num.len() > 1 {
            num.remove(0);
        }
    }
    if out.is_empty() {
        out.push(0);
    }
    out.reverse();
    out
}

/// Big-endian bytes -> decimal-digit string (base-256 -> base-10).
fn be_bytes_to_dec(bytes: &[u8]) -> String {
    let mut digits: Vec<u8> = alloc::vec![0];
    for &byte in bytes {
        let mut carry = byte as u16;
        for d in digits.iter_mut() {
            let cur = *d as u16 * 256 + carry;
            *d = (cur % 10) as u8;
            carry = cur / 10;
        }
        while carry > 0 {
            digits.push((carry % 10) as u8);
            carry /= 10;
        }
    }
    while digits.len() > 1 && *digits.last().unwrap() == 0 {
        digits.pop();
    }
    digits.iter().rev().map(|d| (d + b'0') as char).collect()
}

/// Subtract one from a non-negative decimal-digit string.
fn dec_sub1(digits: &str) -> String {
    let mut d: Vec<u8> = digits.bytes().map(|b| b - b'0').collect();
    let mut i = d.len();
    while i > 0 {
        i -= 1;
        if d[i] > 0 {
            d[i] -= 1;
            break;
        }
        d[i] = 9;
    }
    let mut s: String = d.iter().map(|x| (x + b'0') as char).collect();
    let trimmed = s.trim_start_matches('0');
    s = if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    };
    s
}

// ---- IEEE 754 half-precision (for canonical shortest floats) -------------- //

/// Convert an `f32` (already known to represent the value exactly) to the bits
/// of the nearest `f16`, or `None` if it cannot be represented.
fn f32_to_f16_bits(value: f32) -> Option<u16> {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x7f_ffff;
    if exp == 0xff {
        // inf/nan
        let m = if mant != 0 { 0x200 } else { 0 };
        return Some(sign | 0x7c00 | m);
    }
    if exp == 0 {
        // f32 zero -> half zero; a nonzero f32 subnormal can't be an exact half.
        return if mant == 0 { Some(sign) } else { None };
    }
    let unbiased = exp - 127;
    if unbiased > 15 {
        return None; // overflow to inf -- not an exact half of a finite value
    }
    if unbiased >= -14 {
        // normal half
        if mant & 0x1fff != 0 {
            return None; // would lose precision
        }
        let h_exp = ((unbiased + 15) as u16) << 10;
        let h_mant = (mant >> 13) as u16;
        Some(sign | h_exp | h_mant)
    } else if unbiased >= -24 {
        // subnormal half
        let full_mant = mant | 0x80_0000;
        let shift = (-14 - unbiased) as u32 + 13;
        if shift < 32 && full_mant & ((1u32 << shift) - 1) == 0 {
            Some(sign | (full_mant >> shift) as u16)
        } else {
            None
        }
    } else {
        None
    }
}

/// Expand `f16` bits to an `f64`.
fn f16_to_f64(h: u16) -> f64 {
    let sign = (h >> 15) & 1;
    let exp = (h >> 10) & 0x1f;
    let mant = h & 0x3ff;
    let val: f64 = if exp == 0 {
        (mant as f64) * pow2(-24)
    } else if exp == 0x1f {
        if mant == 0 { f64::INFINITY } else { f64::NAN }
    } else {
        (1.0 + mant as f64 / 1024.0) * pow2(exp as i32 - 15)
    };
    if sign == 1 { -val } else { val }
}

/// Return an exact, normal `f64` power of two without relying on `std` math.
fn pow2(exp: i32) -> f64 {
    debug_assert!((-1022..=1023).contains(&exp));
    f64::from_bits(((exp + 1023) as u64) << 52)
}

/// Non-canonical float: NaN/Inf as canonical half, everything else full double.
fn enc_float_full(out: &mut Vec<u8>, v: f64) {
    if v.is_nan() {
        out.extend_from_slice(&[0xf9, 0x7e, 0x00]);
    } else if v == f64::INFINITY {
        out.extend_from_slice(&[0xf9, 0x7c, 0x00]);
    } else if v == f64::NEG_INFINITY {
        out.extend_from_slice(&[0xf9, 0xfc, 0x00]);
    } else {
        out.push(0xfb);
        out.extend_from_slice(&v.to_bits().to_be_bytes());
    }
}

/// Canonical float: shortest of half/single/double that round-trips.
fn enc_float_shortest(out: &mut Vec<u8>, v: f64) {
    if v.is_nan() {
        out.extend_from_slice(&[0xf9, 0x7e, 0x00]);
        return;
    }
    if v == f64::INFINITY {
        out.extend_from_slice(&[0xf9, 0x7c, 0x00]);
        return;
    }
    if v == f64::NEG_INFINITY {
        out.extend_from_slice(&[0xf9, 0xfc, 0x00]);
        return;
    }
    let f = v as f32;
    if f as f64 == v {
        if let Some(h) = f32_to_f16_bits(f)
            && f16_to_f64(h) == v
        {
            out.push(0xf9);
            out.extend_from_slice(&h.to_be_bytes());
            return;
        }
        out.push(0xfa);
        out.extend_from_slice(&f.to_bits().to_be_bytes());
        return;
    }
    out.push(0xfb);
    out.extend_from_slice(&v.to_bits().to_be_bytes());
}

// ---- encoder -------------------------------------------------------------- //

struct Encoder {
    canonical: bool,
    max_depth: u32,
    /// Labels that are referenced by a `*label` somewhere.
    referenced: Vec<String>,
    /// Referenced labels in first-encounter order -> their share index.
    kept: Vec<String>,
}

impl Encoder {
    fn enc(&mut self, out: &mut Vec<u8>, v: &Value, depth: u32) -> Result<(), Error> {
        if depth > self.max_depth {
            return Err(err("maximum encoding depth exceeded"));
        }
        match v {
            Value::Ref(label) => {
                tag(out, T_SHAREREF);
                let idx = self
                    .kept
                    .iter()
                    .position(|l| l == label)
                    .ok_or_else(|| err("reference to an undefined anchor"))?;
                enc_int(out, &idx.to_string());
            }
            Value::Anchor(a) => {
                if self.referenced.contains(&a.label) {
                    tag(out, T_SHARE);
                    self.enc(out, &a.value, depth)?;
                } else {
                    // Unreferenced anchor is inlined (matches canonical text).
                    self.enc(out, &a.value, depth)?;
                }
            }
            Value::Null => out.push(0xf6),
            Value::Bool(true) => out.push(0xf5),
            Value::Bool(false) => out.push(0xf4),
            Value::Int(s) => {
                if !is_valid_int_text(s) {
                    return Err(err("invalid integer text"));
                }
                enc_int(out, s);
            }
            Value::Float(f) => {
                if self.canonical {
                    enc_float_shortest(out, *f);
                } else {
                    enc_float_full(out, *f);
                }
            }
            Value::Decimal(d) => self.enc_decimal(out, d)?,
            Value::Undefined => {
                return Err(err("undefined sentinel has no Binary AXON spelling"));
            }
            Value::String(s) => {
                let b = s.as_bytes();
                head(out, 3, b.len() as u64);
                out.extend_from_slice(b);
            }
            Value::Bytes(b) => {
                head(out, 2, b.len() as u64);
                out.extend_from_slice(b);
            }
            Value::Temporal(t) => self.enc_temporal(out, t, depth)?,
            Value::Link(link) => match link {
                Link::Cid(s) => {
                    tag(out, T_CID);
                    let mut payload = alloc::vec![0u8];
                    payload.extend_from_slice(s.as_bytes());
                    head(out, 2, payload.len() as u64);
                    out.extend_from_slice(&payload);
                }
                Link::Uri(s) => {
                    tag(out, T_URI);
                    let b = s.as_bytes();
                    head(out, 3, b.len() as u64);
                    out.extend_from_slice(b);
                }
            },
            Value::List(items) => self.enc_array(out, items, depth)?,
            Value::Tuple(items) => {
                tag(out, T_TUPLE);
                self.enc_array(out, items, depth)?;
            }
            Value::Set(items) => {
                tag(out, T_SET);
                let mut encoded: Vec<Vec<u8>> = Vec::with_capacity(items.len());
                for it in items {
                    let mut e = Vec::new();
                    self.enc(&mut e, it, depth + 1)?;
                    encoded.push(e);
                }
                if self.canonical {
                    encoded.sort();
                }
                head(out, 4, encoded.len() as u64);
                for e in &encoded {
                    out.extend_from_slice(e);
                }
            }
            Value::Map(pairs) => self.enc_map(out, pairs, depth)?,
            Value::OrderedMap(pairs) => {
                tag(out, T_OMAP);
                head(out, 4, pairs.len() as u64);
                for (k, val) in pairs {
                    head(out, 4, 2);
                    self.enc(out, k, depth + 1)?;
                    self.enc(out, val, depth + 1)?;
                }
            }
            Value::Node(n) => self.enc_node(out, n, depth)?,
        }
        Ok(())
    }

    fn enc_array(&mut self, out: &mut Vec<u8>, items: &[Value], depth: u32) -> Result<(), Error> {
        head(out, 4, items.len() as u64);
        for it in items {
            self.enc(out, it, depth + 1)?;
        }
        Ok(())
    }

    fn enc_map(
        &mut self,
        out: &mut Vec<u8>,
        pairs: &[(Value, Value)],
        depth: u32,
    ) -> Result<(), Error> {
        let mut entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(pairs.len());
        for (k, val) in pairs {
            let mut ke = Vec::new();
            let mut ve = Vec::new();
            self.enc(&mut ke, k, depth + 1)?;
            self.enc(&mut ve, val, depth + 1)?;
            entries.push((ke, ve));
        }
        if self.canonical {
            entries.sort_by(|a, b| a.0.cmp(&b.0));
        }
        head(out, 5, entries.len() as u64);
        for (k, val) in &entries {
            out.extend_from_slice(k);
            out.extend_from_slice(val);
        }
        Ok(())
    }

    fn enc_node(&mut self, out: &mut Vec<u8>, n: &Node, depth: u32) -> Result<(), Error> {
        let bk: u64 = match n.style {
            NodeStyle::Unit => 0,
            NodeStyle::Brace => 1,
            NodeStyle::Tuple => 2,
            NodeStyle::List => 3,
        };
        tag(out, T_NODE);
        head(out, 4, 3);
        enc_int(out, &bk.to_string());
        let name = n.name.as_bytes();
        head(out, 3, name.len() as u64);
        out.extend_from_slice(name);
        match n.style {
            NodeStyle::Unit => out.push(0xf6),
            NodeStyle::Brace => {
                head(out, 4, 2);
                // attribute map
                let mut entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(n.attributes.len());
                for (k, val) in &n.attributes {
                    let mut ke = Vec::new();
                    let kb = k.as_bytes();
                    head(&mut ke, 3, kb.len() as u64);
                    ke.extend_from_slice(kb);
                    let mut ve = Vec::new();
                    self.enc(&mut ve, val, depth + 1)?;
                    entries.push((ke, ve));
                }
                if self.canonical {
                    entries.sort_by(|a, b| a.0.cmp(&b.0));
                }
                head(out, 5, entries.len() as u64);
                for (k, val) in &entries {
                    out.extend_from_slice(k);
                    out.extend_from_slice(val);
                }
                self.enc_array(out, &n.children, depth)?;
            }
            NodeStyle::Tuple | NodeStyle::List => self.enc_array(out, &n.children, depth)?,
        }
        Ok(())
    }

    fn enc_decimal(&mut self, out: &mut Vec<u8>, d: &Decimal) -> Result<(), Error> {
        match d {
            Decimal::ScaledFinite { .. } => {
                return Err(err(
                    "scale-significant decimals are incompatible with Binary AXON",
                ));
            }
            Decimal::NaN => {
                tag(out, T_DECSP);
                enc_int(out, "2");
            }
            Decimal::Infinity => {
                tag(out, T_DECSP);
                enc_int(out, "0");
            }
            Decimal::NegInfinity => {
                tag(out, T_DECSP);
                enc_int(out, "1");
            }
            Decimal::Finite { mantissa, exponent } => {
                if !is_valid_int_text(mantissa) {
                    return Err(err("invalid decimal mantissa"));
                }
                let (mut mant, mut exp) = (mantissa.clone(), *exponent);
                if self.canonical {
                    // strip trailing zeros while exponent < 0
                    let neg = mant.starts_with('-');
                    let mut digits = mant.trim_start_matches('-').to_string();
                    while exp < 0 && digits.len() > 1 && digits.ends_with('0') {
                        digits.pop();
                        exp += 1;
                    }
                    if digits == "0" {
                        // 0 * 10^e -- leave exponent as-is per reference (mant!=0 guard)
                    }
                    mant = if neg {
                        alloc::format!("-{}", digits)
                    } else {
                        digits
                    };
                }
                tag(out, T_DECFRAC);
                head(out, 4, 2);
                enc_int(out, &exp.to_string());
                enc_int(out, &mant);
            }
        }
        Ok(())
    }

    fn enc_temporal(&mut self, out: &mut Vec<u8>, t: &Temporal, depth: u32) -> Result<(), Error> {
        tag(out, T_TEMP);
        let mut arr: Vec<TzOr> = Vec::new();
        match t {
            Temporal::Date(d) => {
                arr.push(TzOr::Int(0));
                arr.push(TzOr::Int(d.year as i64));
                arr.push(TzOr::Int(d.month as i64));
                arr.push(TzOr::Int(d.day as i64));
            }
            Temporal::Time(tm) => {
                arr.push(TzOr::Int(1));
                arr.push(TzOr::Int(tm.hour as i64));
                arr.push(TzOr::Int(tm.minute as i64));
                arr.push(TzOr::Int(tm.second as i64));
                arr.push(TzOr::Int(tm.nanosecond as i64));
                arr.push(tz_slot(&tm.zone));
            }
            Temporal::DateTime(dt) => {
                arr.push(TzOr::Int(2));
                arr.push(TzOr::Int(dt.date.year as i64));
                arr.push(TzOr::Int(dt.date.month as i64));
                arr.push(TzOr::Int(dt.date.day as i64));
                arr.push(TzOr::Int(dt.time.hour as i64));
                arr.push(TzOr::Int(dt.time.minute as i64));
                arr.push(TzOr::Int(dt.time.second as i64));
                arr.push(TzOr::Int(dt.time.nanosecond as i64));
                arr.push(tz_slot(&dt.time.zone));
            }
        }
        head(out, 4, arr.len() as u64);
        for item in &arr {
            item.encode(out);
        }
        let _ = depth;
        Ok(())
    }
}

/// A temporal array element: an integer, the `Z` marker, `null` (local), or a
/// `[offset, name]` named-zone pair.
enum TzOr {
    Int(i64),
    Zulu,
    Local,
    Named(i64, String),
}

impl TzOr {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            TzOr::Int(i) => enc_int(out, &i.to_string()),
            TzOr::Zulu => {
                head(out, 3, 1);
                out.push(b'Z');
            }
            TzOr::Local => out.push(0xf6),
            TzOr::Named(off, name) => {
                head(out, 4, 2);
                enc_int(out, &off.to_string());
                let b = name.as_bytes();
                head(out, 3, b.len() as u64);
                out.extend_from_slice(b);
            }
        }
    }
}

fn tz_slot(zone: &Zone) -> TzOr {
    match zone {
        Zone::Named { offset, name, .. } => TzOr::Named(*offset as i64, name.clone()),
        Zone::Zulu => TzOr::Zulu,
        Zone::Offset(o) => TzOr::Int(*o as i64),
        Zone::Local => TzOr::Local,
    }
}

/// Collect every `*label` reachable from `v`.
fn collect_refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Ref(label) => {
            if !out.iter().any(|l| l == label) {
                out.push(label.clone());
            }
        }
        Value::Anchor(a) => collect_refs(&a.value, out),
        Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
            items.iter().for_each(|x| collect_refs(x, out))
        }
        Value::Map(p) | Value::OrderedMap(p) => p.iter().for_each(|(k, val)| {
            collect_refs(k, out);
            collect_refs(val, out);
        }),
        Value::Node(n) => {
            n.attributes
                .iter()
                .for_each(|(_, val)| collect_refs(val, out));
            n.children.iter().for_each(|c| collect_refs(c, out));
        }
        _ => {}
    }
}

/// Assign share indices to referenced anchors in pre-order (their tag-28 order).
fn assign_shares(v: &Value, referenced: &[String], kept: &mut Vec<String>) {
    if let Value::Anchor(a) = v
        && referenced.contains(&a.label)
        && !kept.contains(&a.label)
    {
        kept.push(a.label.clone());
    }
    match v {
        Value::Anchor(a) => assign_shares(&a.value, referenced, kept),
        Value::List(items) | Value::Tuple(items) | Value::Set(items) => items
            .iter()
            .for_each(|x| assign_shares(x, referenced, kept)),
        Value::Map(p) | Value::OrderedMap(p) => p.iter().for_each(|(k, val)| {
            assign_shares(k, referenced, kept);
            assign_shares(val, referenced, kept);
        }),
        Value::Node(n) => {
            n.attributes
                .iter()
                .for_each(|(_, val)| assign_shares(val, referenced, kept));
            n.children
                .iter()
                .for_each(|c| assign_shares(c, referenced, kept));
        }
        _ => {}
    }
}

/// Encode a value to Binary AXON bytes (non-canonical).
pub fn encode(value: &Value) -> Result<Vec<u8>, Error> {
    encode_with(value, false, DEFAULT_MAX_DEPTH)
}

/// Encode a value to Canonical Binary AXON bytes (deterministic, CID-ready).
pub fn encode_canonical(value: &Value) -> Result<Vec<u8>, Error> {
    encode_with(value, true, DEFAULT_MAX_DEPTH)
}

/// Encode with explicit canonical flag and depth limit.
pub fn encode_with(value: &Value, canonical: bool, max_depth: u32) -> Result<Vec<u8>, Error> {
    // Normalize the graph exactly as canonical text does: only composite,
    // referenced anchors keep identity (tag 28/29); scalar and unreferenced
    // anchors are inlined. This matches the reference, whose `_find_referenced`
    // only ever tracks containers.
    let value = crate::writer::normalize_graph(value, canonical)?;
    let value = &value;
    let mut referenced = Vec::new();
    collect_refs(value, &mut referenced);
    let mut kept = Vec::new();
    assign_shares(value, &referenced, &mut kept);
    let mut enc = Encoder {
        canonical,
        max_depth,
        referenced,
        kept,
    };
    let mut out = Vec::new();
    enc.enc(&mut out, value, 0)?;
    Ok(out)
}

// ---- decoder -------------------------------------------------------------- //

struct Decoder<'a> {
    b: &'a [u8],
    i: usize,
    /// Number of tag-28 shares seen so far (their labels are their index).
    share_count: usize,
    /// Maximum accepted nesting depth. Parsing itself is iterative; this is a
    /// compatibility/application budget rather than a call-stack safeguard.
    max_depth: u32,
}

/// A suspended composite while its children are decoded. Keeping these frames
/// on the heap rather than on the Rust call stack makes decoder depth depend
/// only on available input/memory (or the explicit `decode_with` budget).
enum DecodeFrame {
    Array {
        remaining: usize,
        items: Vec<Value>,
        child_depth: u32,
    },
    Map {
        remaining: usize,
        pairs: Vec<(Value, Value)>,
        key: Option<Value>,
        child_depth: u32,
    },
    Tag {
        tag: u64,
        share_label: Option<String>,
    },
}

impl<'a> Decoder<'a> {
    /// Bytes not yet consumed. `self.i <= self.b.len()` is an invariant, so this
    /// never underflows and is a safe upper bound on any remaining length.
    fn remaining(&self) -> usize {
        self.b.len() - self.i
    }

    fn u(&mut self, n: usize) -> Result<u64, Error> {
        // `self.i + n` could overflow `usize` on an adversarial length, wrapping
        // below `b.len()` and bypassing the check -- compare against the
        // remaining bytes instead, which cannot overflow.
        if n > self.remaining() {
            return Err(err("unexpected end of binary input"));
        }
        let mut v: u64 = 0;
        for _ in 0..n {
            v = (v << 8) | self.b[self.i] as u64;
            self.i += 1;
        }
        Ok(v)
    }

    fn head(&mut self) -> Result<(u8, u8, u64), Error> {
        if self.i >= self.b.len() {
            return Err(err("unexpected end of binary input"));
        }
        let ib = self.b[self.i];
        self.i += 1;
        let (major, ai) = (ib >> 5, ib & 31);
        let val = match ai {
            0..=23 => ai as u64,
            24 => self.u(1)?,
            25 => self.u(2)?,
            26 => self.u(4)?,
            27 => self.u(8)?,
            _ => return Err(err("indefinite/reserved additional-info")),
        };
        Ok((major, ai, val))
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        // See `u`: compare against remaining bytes so an adversarial `n` cannot
        // overflow the bounds check and panic the following slice.
        if n > self.remaining() {
            return Err(err("unexpected end of binary input"));
        }
        let s = &self.b[self.i..self.i + n];
        self.i += n;
        Ok(s)
    }

    /// A CBOR container declaring `count` elements needs at least `count` more
    /// bytes (every element is one byte or more), so a count exceeding the
    /// remaining input is malformed. Rejecting it early both fails fast and keeps
    /// any subsequent pre-allocation bounded by the real input size, never by an
    /// attacker-declared length.
    fn checked_count(&self, count: u64) -> Result<usize, Error> {
        if count > self.remaining() as u64 {
            return Err(err("container length exceeds remaining input"));
        }
        Ok(count as usize)
    }

    fn item(&mut self, root_depth: u32) -> Result<Value, Error> {
        let mut frames = Vec::new();
        let mut depth = root_depth;

        loop {
            if depth > self.max_depth {
                return Err(err("maximum decoding depth exceeded"));
            }

            let ib = self
                .b
                .get(self.i)
                .copied()
                .ok_or_else(|| err("unexpected end"))?;
            let major = ib >> 5;
            let mut value = if major == 7 {
                let ai = ib & 31;
                self.i += 1;
                match ai {
                    20 => Value::Bool(false),
                    21 => Value::Bool(true),
                    22 => Value::Null,
                    25 => {
                        let bits = self.u(2)? as u16;
                        float_value(f16_to_f64(bits))
                    }
                    26 => {
                        let bytes = self.take(4)?;
                        let arr: [u8; 4] = bytes.try_into().unwrap();
                        float_value(f32::from_be_bytes(arr) as f64)
                    }
                    27 => {
                        let bytes = self.take(8)?;
                        let arr: [u8; 8] = bytes.try_into().unwrap();
                        float_value(f64::from_be_bytes(arr))
                    }
                    _ => return Err(err("unsupported simple/float value")),
                }
            } else {
                let (major, _, argument) = self.head()?;
                match major {
                    0 => Value::Int(argument.to_string()),
                    1 => Value::Int(neg_int_string(argument)),
                    2 => {
                        let len = usize::try_from(argument)
                            .map_err(|_| err("byte string length is too large"))?;
                        Value::Bytes(self.take(len)?.to_vec())
                    }
                    3 => {
                        let len = usize::try_from(argument)
                            .map_err(|_| err("text string length is too large"))?;
                        let bytes = self.take(len)?;
                        let text = core::str::from_utf8(bytes).map_err(|_| err("invalid UTF-8"))?;
                        Value::String(text.to_string())
                    }
                    4 => {
                        let count = self.checked_count(argument)?;
                        if count == 0 {
                            Value::List(Vec::new())
                        } else {
                            let child_depth = depth
                                .checked_add(1)
                                .ok_or_else(|| err("maximum decoding depth exceeded"))?;
                            frames.push(DecodeFrame::Array {
                                remaining: count,
                                items: Vec::with_capacity(count),
                                child_depth,
                            });
                            depth = child_depth;
                            continue;
                        }
                    }
                    5 => {
                        let count = self.checked_count(argument)?;
                        if count == 0 {
                            Value::Map(Vec::new())
                        } else {
                            let child_depth = depth
                                .checked_add(1)
                                .ok_or_else(|| err("maximum decoding depth exceeded"))?;
                            frames.push(DecodeFrame::Map {
                                remaining: count,
                                pairs: Vec::with_capacity(count),
                                key: None,
                                child_depth,
                            });
                            depth = child_depth;
                            continue;
                        }
                    }
                    6 => {
                        if !matches!(
                            argument,
                            T_BIGPOS
                                | T_BIGNEG
                                | T_DECFRAC
                                | T_DECSP
                                | T_TEMP
                                | T_NODE
                                | T_TUPLE
                                | T_OMAP
                                | T_SET
                                | T_CID
                                | T_URI
                                | T_SHARE
                                | T_SHAREREF
                        ) {
                            return Err(err("unsupported CBOR tag"));
                        }
                        let share_label = if argument == T_SHARE {
                            let label = self.share_count.to_string();
                            self.share_count = self
                                .share_count
                                .checked_add(1)
                                .ok_or_else(|| err("share table is too large"))?;
                            Some(label)
                        } else {
                            None
                        };
                        frames.push(DecodeFrame::Tag {
                            tag: argument,
                            share_label,
                        });
                        // CBOR tags decorate an item without increasing AXON's
                        // container nesting depth, matching the reference.
                        continue;
                    }
                    _ => return Err(err("unexpected CBOR major type")),
                }
            };

            // Feed a completed value into suspended parents until either the
            // root is complete or another child must be read.
            loop {
                match frames.pop() {
                    None => return Ok(value),
                    Some(DecodeFrame::Tag { tag, share_label }) => {
                        value = self.apply_tag(tag, share_label, value)?;
                    }
                    Some(DecodeFrame::Array {
                        mut remaining,
                        mut items,
                        child_depth,
                    }) => {
                        items.push(value);
                        remaining -= 1;
                        if remaining == 0 {
                            value = Value::List(items);
                        } else {
                            frames.push(DecodeFrame::Array {
                                remaining,
                                items,
                                child_depth,
                            });
                            depth = child_depth;
                            break;
                        }
                    }
                    Some(DecodeFrame::Map {
                        mut remaining,
                        mut pairs,
                        key,
                        child_depth,
                    }) => match key {
                        None => {
                            frames.push(DecodeFrame::Map {
                                remaining,
                                pairs,
                                key: Some(value),
                                child_depth,
                            });
                            depth = child_depth;
                            break;
                        }
                        Some(key) => {
                            python_mapping_insert(&mut pairs, key, value)?;
                            remaining -= 1;
                            if remaining == 0 {
                                value = Value::Map(pairs);
                            } else {
                                frames.push(DecodeFrame::Map {
                                    remaining,
                                    pairs,
                                    key: None,
                                    child_depth,
                                });
                                depth = child_depth;
                                break;
                            }
                        }
                    },
                }
            }
        }
    }

    fn apply_tag(
        &mut self,
        tag: u64,
        share_label: Option<String>,
        payload: Value,
    ) -> Result<Value, Error> {
        match tag {
            T_BIGPOS | T_BIGNEG => {
                let raw = python_byte_iterable(payload)?;
                let magnitude = be_bytes_to_dec(&raw);
                if tag == T_BIGPOS {
                    Ok(Value::Int(magnitude))
                } else {
                    Ok(Value::Int(alloc::format!("-{}", dec_add1(&magnitude))))
                }
            }
            T_DECFRAC => decode_decimal_fraction(payload),
            T_DECSP => {
                let code = python_small_index(&payload, 2)
                    .ok_or_else(|| err("unknown decimal-special code"))?;
                Ok(Value::Decimal(match code {
                    0 => Decimal::Infinity,
                    1 => Decimal::NegInfinity,
                    2 => Decimal::NaN,
                    _ => unreachable!("decimal-special code is bounded"),
                }))
            }
            T_TEMP => decode_temporal_payload(payload),
            T_NODE => decode_node_payload(payload),
            T_TUPLE => Ok(Value::Tuple(python_iterable(payload)?)),
            T_OMAP => {
                let entries = python_iterable(payload)?;
                let mut pairs = Vec::with_capacity(entries.len());
                for entry in entries {
                    let mut pair = python_iterable(entry)?;
                    if pair.len() != 2 {
                        return Err(err("omap entry must contain exactly two values"));
                    }
                    let value = pair.pop().unwrap();
                    let key = pair.pop().unwrap();
                    python_mapping_insert(&mut pairs, key, value)?;
                }
                Ok(Value::OrderedMap(pairs))
            }
            T_SET => Ok(Value::Set(python_iterable(payload)?)),
            T_CID => match payload {
                Value::Bytes(bytes) => {
                    let text = bytes.strip_prefix(&[0]).unwrap_or(&bytes);
                    Ok(Value::Link(Link::Cid(
                        core::str::from_utf8(text)
                            .map_err(|_| err("invalid CID text"))?
                            .to_string(),
                    )))
                }
                Value::String(text) => Ok(Value::Link(Link::Cid(text))),
                _ => Err(err("CID payload is not representable as text")),
            },
            T_URI => match payload {
                Value::String(text) => Ok(Value::Link(Link::Uri(text))),
                _ => Err(err("URI payload must be text")),
            },
            T_SHARE => Ok(Value::Anchor(Box::new(Anchor {
                label: share_label.ok_or_else(|| err("missing share label"))?,
                value: payload,
            }))),
            T_SHAREREF => {
                let index = python_share_index(&payload)?;
                Ok(Value::Ref(index.to_string()))
            }
            _ => Err(err("unsupported CBOR tag")),
        }
    }
}

fn decode_temporal_payload(payload: Value) -> Result<Value, Error> {
    let arr = match payload {
        Value::List(items) => items,
        _ => return Err(err("expected a CBOR array")),
    };
    let kind = python_small_index(arr.first().ok_or_else(|| err("empty temporal"))?, 2)
        .ok_or_else(|| err("unknown temporal kind"))?;
    match kind {
        0 => {
            if arr.len() != 4 {
                return Err(err("bad date array"));
            }
            Ok(Value::Temporal(Temporal::Date(Date {
                year: i32_of(&arr[1], "date year")?,
                month: u8_of(&arr[2], "date month")?,
                day: u8_of(&arr[3], "date day")?,
            })))
        }
        1 => {
            if arr.len() != 6 {
                return Err(err("bad time array"));
            }
            Ok(Value::Temporal(Temporal::Time(Time {
                hour: u8_of(&arr[1], "time hour")?,
                minute: u8_of(&arr[2], "time minute")?,
                second: u8_of(&arr[3], "time second")?,
                nanosecond: u32_of(&arr[4], "time nanosecond")?,
                fraction_digits: None,
                zone: zone_of(&arr[5])?,
            })))
        }
        2 => {
            if arr.len() != 9 {
                return Err(err("bad datetime array"));
            }
            Ok(Value::Temporal(Temporal::DateTime(DateTime {
                date: Date {
                    year: i32_of(&arr[1], "datetime year")?,
                    month: u8_of(&arr[2], "datetime month")?,
                    day: u8_of(&arr[3], "datetime day")?,
                },
                time: Time {
                    hour: u8_of(&arr[4], "datetime hour")?,
                    minute: u8_of(&arr[5], "datetime minute")?,
                    second: u8_of(&arr[6], "datetime second")?,
                    nanosecond: u32_of(&arr[7], "datetime nanosecond")?,
                    fraction_digits: None,
                    zone: zone_of(&arr[8])?,
                },
            })))
        }
        _ => Err(err("unknown temporal kind")),
    }
}

fn decode_node_payload(payload: Value) -> Result<Value, Error> {
    let arr = python_iterable(payload)?;
    if arr.len() != 3 {
        return Err(err("node must be [bk, name, payload]"));
    }
    let bk = python_small_index(&arr[0], 3).ok_or_else(|| err("unknown node body kind"))?;
    let name = match &arr[1] {
        Value::String(text) => text.clone(),
        _ => return Err(err("node name must be text")),
    };
    let (style, attributes, children) = match bk {
        0 => (NodeStyle::Unit, Vec::new(), Vec::new()),
        1 => {
            let body = python_iterable(arr[2].clone())?;
            if body.len() != 2 {
                return Err(err("brace node payload must be [attrs, children]"));
            }
            let attributes = python_mapping_entries(body[0].clone())?
                .into_iter()
                .map(|(key, value)| match key {
                    Value::String(key) => Ok((key, value)),
                    _ => Err(err("attribute key must be text")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let children = python_iterable(body[1].clone())?;
            (NodeStyle::Brace, attributes, children)
        }
        2 | 3 => {
            let children = python_iterable(arr[2].clone())?;
            (
                if bk == 2 {
                    NodeStyle::Tuple
                } else {
                    NodeStyle::List
                },
                Vec::new(),
                children,
            )
        }
        _ => return Err(err("unknown node body kind")),
    };
    Ok(Value::Node(Box::new(Node {
        name,
        style,
        attributes,
        children,
    })))
}

/// Python's CBOR decoder feeds several tagged constructors through ordinary
/// Python iteration (`tuple(payload)`, `OrderedDict(payload)`, and
/// `int.from_bytes(payload)`).  Keep those acceptance rules for the iterable
/// AXON shapes representable by [`Value`].
fn python_iterable(value: Value) -> Result<Vec<Value>, Error> {
    match value {
        Value::Anchor(anchor) => python_iterable(anchor.value),
        Value::List(items) | Value::Tuple(items) | Value::Set(items) => Ok(items),
        Value::Map(pairs) | Value::OrderedMap(pairs) => {
            Ok(pairs.into_iter().map(|(key, _)| key).collect())
        }
        Value::String(text) => Ok(text
            .chars()
            .map(|character| Value::String(character.to_string()))
            .collect()),
        Value::Bytes(bytes) => Ok(bytes
            .into_iter()
            .map(|byte| Value::Int(byte.to_string()))
            .collect()),
        _ => Err(err("tag payload is not iterable")),
    }
}

fn python_byte_iterable(value: Value) -> Result<Vec<u8>, Error> {
    if let Value::Bytes(bytes) = value {
        return Ok(bytes);
    }
    python_iterable(value)?
        .into_iter()
        .map(|item| match item {
            Value::Bool(value) => Ok(u8::from(value)),
            Value::Int(text) => text
                .parse::<u16>()
                .ok()
                .and_then(|value| u8::try_from(value).ok())
                .ok_or_else(|| err("bignum iterable item is not a byte")),
            _ => Err(err("bignum iterable item is not an integer")),
        })
        .collect()
}

/// Mirror `exp, mant = payload; Decimal(mant).scaleb(exp)` for every decoded
/// Python numeric/string shape whose result is representable by `Decimal`.
fn decode_decimal_fraction(payload: Value) -> Result<Value, Error> {
    let mut pair = python_iterable(payload)?;
    if pair.len() != 2 {
        return Err(err("decfrac payload must contain exactly two values"));
    }
    let mantissa = pair.pop().unwrap();
    let scale = python_decimal_scale(&pair.pop().unwrap())?;
    let decimal = python_decimal_constructor(mantissa)?;
    Ok(Value::Decimal(match decimal {
        Decimal::Finite { mantissa, exponent } | Decimal::ScaledFinite { mantissa, exponent } => {
            Decimal::Finite {
                mantissa,
                exponent: exponent
                    .checked_add(scale)
                    .ok_or_else(|| err("decfrac exponent is out of range"))?,
            }
        }
        special => special,
    }))
}

fn python_decimal_scale(value: &Value) -> Result<i32, Error> {
    let value = match value {
        Value::Anchor(anchor) => &anchor.value,
        value => value,
    };
    match value {
        Value::Bool(value) => Ok(i32::from(*value)),
        Value::Int(text) => text
            .parse::<i32>()
            .map_err(|_| err("decfrac exponent is out of range")),
        Value::Decimal(Decimal::Finite { mantissa, exponent })
        | Value::Decimal(Decimal::ScaledFinite { mantissa, exponent }) => {
            finite_decimal_to_i32(mantissa, *exponent)
        }
        _ => Err(err("decfrac exponent must be an integer")),
    }
}

fn finite_decimal_to_i32(mantissa: &str, exponent: i32) -> Result<i32, Error> {
    let negative = mantissa.starts_with('-');
    let mut digits = mantissa.trim_start_matches('-').to_string();
    if exponent < 0 {
        let remove = usize::try_from(exponent.unsigned_abs())
            .map_err(|_| err("decfrac exponent is out of range"))?;
        if remove > digits.len()
            || !digits[digits.len() - remove..]
                .bytes()
                .all(|byte| byte == b'0')
        {
            return Err(err("decfrac exponent must be integral"));
        }
        digits.truncate(digits.len() - remove);
        if digits.is_empty() {
            digits.push('0');
        }
    } else {
        let append =
            usize::try_from(exponent).map_err(|_| err("decfrac exponent is out of range"))?;
        if digits.len().saturating_add(append) > 11 {
            return Err(err("decfrac exponent is out of range"));
        }
        digits.extend(core::iter::repeat_n('0', append));
    }
    if negative && digits != "0" {
        digits.insert(0, '-');
    }
    digits
        .parse::<i32>()
        .map_err(|_| err("decfrac exponent is out of range"))
}

fn python_decimal_constructor(value: Value) -> Result<Decimal, Error> {
    match value {
        Value::Anchor(anchor) => python_decimal_constructor(anchor.value),
        Value::Bool(value) => Ok(Decimal::Finite {
            mantissa: if value { "1" } else { "0" }.to_string(),
            exponent: 0,
        }),
        Value::Int(mantissa) => Ok(Decimal::Finite {
            mantissa,
            exponent: 0,
        }),
        Value::Float(value) => decimal_from_float(value),
        Value::Decimal(Decimal::ScaledFinite { mantissa, exponent }) => {
            Ok(Decimal::Finite { mantissa, exponent })
        }
        Value::Decimal(decimal) => Ok(decimal),
        Value::String(text) => parse_python_decimal_text(&text),
        Value::Bytes(bytes) => parse_python_decimal_text(
            core::str::from_utf8(&bytes).map_err(|_| err("invalid decimal byte string"))?,
        ),
        _ => Err(err("decfrac mantissa cannot be converted to Decimal")),
    }
}

fn decimal_from_float(value: f64) -> Result<Decimal, Error> {
    if value.is_nan() {
        return Ok(Decimal::NaN);
    }
    if value == f64::INFINITY {
        return Ok(Decimal::Infinity);
    }
    if value == f64::NEG_INFINITY {
        return Ok(Decimal::NegInfinity);
    }
    if value == 0.0 {
        return Ok(Decimal::Finite {
            mantissa: if value.is_sign_negative() { "-0" } else { "0" }.to_string(),
            exponent: 0,
        });
    }
    match float_number(value) {
        PythonNumber::Finite {
            negative,
            digits,
            exponent,
        } => Ok(Decimal::Finite {
            mantissa: if negative {
                alloc::format!("-{digits}")
            } else {
                digits
            },
            exponent: exponent
                .try_into()
                .map_err(|_| err("float-derived decimal exponent is out of range"))?,
        }),
        _ => unreachable!("non-finite floats were handled above"),
    }
}

fn parse_python_decimal_text(text: &str) -> Result<Decimal, Error> {
    let compact: String = text
        .trim()
        .chars()
        .filter(|character| *character != '_')
        .collect();
    let lower = compact.to_ascii_lowercase();
    match lower.as_str() {
        "inf" | "+inf" | "infinity" | "+infinity" => Ok(Decimal::Infinity),
        "-inf" | "-infinity" => Ok(Decimal::NegInfinity),
        "nan" | "+nan" | "-nan" => Ok(Decimal::NaN),
        _ => parse_python_finite_decimal(&compact),
    }
}

fn parse_python_finite_decimal(text: &str) -> Result<Decimal, Error> {
    let (negative, unsigned) = if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else {
        (false, text.strip_prefix('+').unwrap_or(text))
    };
    let (coefficient, extra_exponent) = match unsigned.find(['e', 'E']) {
        Some(index) => {
            let exponent = unsigned[index + 1..]
                .parse::<i32>()
                .map_err(|_| err("invalid decimal exponent"))?;
            (&unsigned[..index], exponent)
        }
        None => (unsigned, 0),
    };
    let mut digits = String::new();
    let mut fraction = 0i32;
    let mut saw_dot = false;
    for byte in coefficient.bytes() {
        match byte {
            b'0'..=b'9' => {
                digits.push(byte as char);
                if saw_dot {
                    fraction = fraction
                        .checked_add(1)
                        .ok_or_else(|| err("decimal scale is out of range"))?;
                }
            }
            b'.' if !saw_dot => saw_dot = true,
            _ => return Err(err("invalid decimal text")),
        }
    }
    if digits.is_empty() {
        return Err(err("invalid decimal text"));
    }
    while digits.len() > 1 && digits.starts_with('0') {
        digits.remove(0);
    }
    if negative {
        digits.insert(0, '-');
    }
    Ok(Decimal::Finite {
        mantissa: digits,
        exponent: extra_exponent
            .checked_sub(fraction)
            .ok_or_else(|| err("decimal scale is out of range"))?,
    })
}

fn python_mapping_entries(value: Value) -> Result<Vec<(Value, Value)>, Error> {
    match value {
        Value::Anchor(anchor) => python_mapping_entries(anchor.value),
        Value::Map(pairs) | Value::OrderedMap(pairs) => Ok(pairs),
        other => {
            let entries = python_iterable(other)?;
            let mut pairs = Vec::with_capacity(entries.len());
            for entry in entries {
                let mut pair = python_iterable(entry)?;
                if pair.len() != 2 {
                    return Err(err("mapping entry must contain exactly two values"));
                }
                let value = pair.pop().unwrap();
                let key = pair.pop().unwrap();
                python_mapping_insert(&mut pairs, key, value)?;
            }
            Ok(pairs)
        }
    }
}

/// Insert with Python-dict semantics: keys must be hashable, numerically equal
/// keys alias across bool/int/float/decimal domains, the first key object keeps
/// its position, and the final value wins.
fn python_mapping_insert(
    pairs: &mut Vec<(Value, Value)>,
    key: Value,
    value: Value,
) -> Result<(), Error> {
    if !python_hashable(&key) {
        return Err(err("unhashable Binary AXON map key"));
    }
    if let Some((_, slot)) = pairs
        .iter_mut()
        .find(|(known, _)| python_key_equal(known, &key))
    {
        *slot = value;
    } else {
        pairs.push((key, value));
    }
    Ok(())
}

fn python_hashable(value: &Value) -> bool {
    match value {
        Value::Anchor(anchor) => python_hashable(&anchor.value),
        Value::Null
        | Value::Bool(_)
        | Value::Int(_)
        | Value::Float(_)
        | Value::Decimal(_)
        | Value::String(_)
        | Value::Bytes(_)
        | Value::Ref(_)
        | Value::Undefined => true,
        Value::Tuple(items) => items.iter().all(python_hashable),
        Value::Temporal(_)
        | Value::List(_)
        | Value::Map(_)
        | Value::OrderedMap(_)
        | Value::Set(_)
        | Value::Node(_)
        | Value::Link(_) => false,
    }
}

fn python_key_equal(left: &Value, right: &Value) -> bool {
    let left = match left {
        Value::Anchor(anchor) => &anchor.value,
        value => value,
    };
    let right = match right {
        Value::Anchor(anchor) => &anchor.value,
        value => value,
    };
    if let (Some(left), Some(right)) = (python_number(left), python_number(right)) {
        return left != PythonNumber::NaN && left == right;
    }
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::String(left), Value::String(right)) => left == right,
        (Value::Bytes(left), Value::Bytes(right)) => left == right,
        (Value::Tuple(left), Value::Tuple(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| python_key_equal(left, right))
        }
        // Decoder reference markers are ordinary identity-hashed Python
        // objects while a mapping is built, so two distinct markers never
        // collapse merely because they carry the same share index.
        (Value::Ref(_), Value::Ref(_)) => false,
        (Value::Undefined, Value::Undefined) => true,
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PythonNumber {
    Finite {
        negative: bool,
        digits: String,
        exponent: i64,
    },
    PosInfinity,
    NegInfinity,
    NaN,
}

fn python_number(value: &Value) -> Option<PythonNumber> {
    match value {
        Value::Bool(value) => Some(decimal_number(if *value { "1" } else { "0" }, 0)),
        Value::Int(text) => Some(decimal_number(text, 0)),
        Value::Float(value) => Some(float_number(*value)),
        Value::Decimal(decimal) => Some(match decimal {
            Decimal::Finite { mantissa, exponent }
            | Decimal::ScaledFinite { mantissa, exponent } => {
                decimal_number(mantissa, i64::from(*exponent))
            }
            Decimal::Infinity => PythonNumber::PosInfinity,
            Decimal::NegInfinity => PythonNumber::NegInfinity,
            Decimal::NaN => PythonNumber::NaN,
        }),
        _ => None,
    }
}

fn decimal_number(text: &str, mut exponent: i64) -> PythonNumber {
    let negative = text.starts_with('-');
    let mut digits = text
        .trim_start_matches('-')
        .trim_start_matches('0')
        .to_string();
    if digits.is_empty() {
        return PythonNumber::Finite {
            negative: false,
            digits: "0".to_string(),
            exponent: 0,
        };
    }
    while digits.ends_with('0') {
        digits.pop();
        exponent += 1;
    }
    PythonNumber::Finite {
        negative,
        digits,
        exponent,
    }
}

fn float_number(value: f64) -> PythonNumber {
    if value.is_nan() {
        return PythonNumber::NaN;
    }
    if value == f64::INFINITY {
        return PythonNumber::PosInfinity;
    }
    if value == f64::NEG_INFINITY {
        return PythonNumber::NegInfinity;
    }
    if value == 0.0 {
        return decimal_number("0", 0);
    }

    let bits = value.to_bits();
    let negative = bits >> 63 != 0;
    let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (mantissa, exponent_two) = if exponent_bits == 0 {
        (fraction, 1 - 1023 - 52)
    } else {
        ((1u64 << 52) | fraction, exponent_bits - 1023 - 52)
    };
    let mut digits = mantissa.to_string();
    let exponent_ten = if exponent_two >= 0 {
        for _ in 0..exponent_two {
            multiply_decimal_digits(&mut digits, 2);
        }
        0
    } else {
        for _ in 0..-exponent_two {
            multiply_decimal_digits(&mut digits, 5);
        }
        i64::from(exponent_two)
    };
    let signed = if negative {
        alloc::format!("-{digits}")
    } else {
        digits
    };
    decimal_number(&signed, exponent_ten)
}

fn multiply_decimal_digits(digits: &mut String, factor: u8) {
    let mut bytes = core::mem::take(digits).into_bytes();
    let mut carry = 0u16;
    for digit in bytes.iter_mut().rev() {
        let product = u16::from(*digit - b'0') * u16::from(factor) + carry;
        *digit = (product % 10) as u8 + b'0';
        carry = product / 10;
    }
    while carry != 0 {
        bytes.insert(0, (carry % 10) as u8 + b'0');
        carry /= 10;
    }
    *digits = String::from_utf8(bytes).expect("decimal multiplication stays ASCII");
}

fn python_small_index(value: &Value, maximum: usize) -> Option<usize> {
    (0..=maximum).find(|candidate| python_key_equal(value, &Value::Int(candidate.to_string())))
}

fn python_share_index(value: &Value) -> Result<i64, Error> {
    match value {
        Value::Bool(value) => Ok(i64::from(*value)),
        Value::Int(text) => text
            .parse::<i64>()
            .map_err(|_| err("share reference index is out of range")),
        _ => Err(err("share reference index must be an integer")),
    }
}

fn i32_of(value: &Value, label: &str) -> Result<i32, Error> {
    int_of(value)?
        .try_into()
        .map_err(|_| err(&alloc::format!("{label} is out of range")))
}

fn u32_of(value: &Value, label: &str) -> Result<u32, Error> {
    int_of(value)?
        .try_into()
        .map_err(|_| err(&alloc::format!("{label} is out of range")))
}

fn u8_of(value: &Value, label: &str) -> Result<u8, Error> {
    int_of(value)?
        .try_into()
        .map_err(|_| err(&alloc::format!("{label} is out of range")))
}

fn float_value(f: f64) -> Value {
    Value::Float(f)
}

/// Decimal string for `-1 - val` (CBOR major-1 negatives).
fn neg_int_string(val: u64) -> String {
    // -1 - val; val is up to u64::MAX so use u128 to avoid overflow.
    let magnitude = val as u128 + 1;
    alloc::format!("-{}", magnitude)
}

fn dec_add1(digits: &str) -> String {
    let mut d: Vec<u8> = digits.bytes().map(|b| b - b'0').collect();
    let mut i = d.len();
    let mut carry = 1u8;
    while i > 0 && carry > 0 {
        i -= 1;
        let s = d[i] + carry;
        d[i] = s % 10;
        carry = s / 10;
    }
    let mut s = String::new();
    if carry > 0 {
        s.push((carry + b'0') as char);
    }
    for x in d {
        s.push((x + b'0') as char);
    }
    s
}

fn int_of(v: &Value) -> Result<i64, Error> {
    match v {
        Value::Int(s) => s.parse::<i64>().map_err(|_| err("integer out of range")),
        _ => Err(err("expected an integer")),
    }
}

fn zone_of(v: &Value) -> Result<Zone, Error> {
    match v {
        Value::Null => Ok(Zone::Local),
        Value::String(s) if s == "Z" => Ok(Zone::Zulu),
        Value::Int(s) => Ok(Zone::Offset(
            s.parse::<i32>().map_err(|_| err("bad offset"))?,
        )),
        Value::List(kv) if kv.len() == 2 => {
            let off = i32_of(&kv[0], "named-zone offset")?;
            let name = match &kv[1] {
                Value::String(s) => s.clone(),
                _ => return Err(err("zone name must be text")),
            };
            Ok(Zone::Named {
                offset: off,
                name,
                zulu: false,
            })
        }
        _ => Err(err("invalid time-zone slot")),
    }
}

/// Decode Binary AXON bytes to a [`Value`]. Shares (tag 28) decode to
/// [`Value::Anchor`] labeled by share index; references (tag 29) to
/// [`Value::Ref`] -- the structural form, matching the text parser.
///
/// Decoding is hardened against malformed input: length fields are validated
/// against the remaining bytes (no allocation is sized by an attacker-declared
/// length), and an explicit frame stack avoids call-stack exhaustion. The
/// default depth is [`DEFAULT_MAX_DEPTH`], matching section 16, the encoders,
/// and the Python reference decoder, so both implementations accept and reject
/// the same documents; use [`decode_with`] to select a different profile.
pub fn decode(buf: &[u8]) -> Result<Value, Error> {
    decode_impl(buf, DEFAULT_MAX_DEPTH)
}

/// Decode with an explicit maximum nesting depth. Passing
/// [`DEFAULT_MAX_DEPTH`] matches the encoder's default hardening guard.
pub fn decode_with(buf: &[u8], max_depth: u32) -> Result<Value, Error> {
    decode_impl(buf, max_depth)
}

fn decode_impl(buf: &[u8], max_depth: u32) -> Result<Value, Error> {
    let mut d = Decoder {
        b: buf,
        i: 0,
        share_count: 0,
        max_depth,
    };
    let value = d.item(0)?;
    if d.i != buf.len() {
        return Err(err("trailing bytes after value"));
    }
    normalize_share_references(value, d.share_count)
}

/// Validate tag-29 indices only after the whole document has been decoded.
/// This is important for parity with Python list indexing: self-references,
/// forward references, booleans, and negative indices may all be valid once
/// the final tag-28 count is known. The Rust value model keeps graph markers
/// structurally, so successful negative indices are normalized to their
/// non-negative share labels.
fn normalize_share_references(value: Value, share_count: usize) -> Result<Value, Error> {
    #[derive(Clone, Copy)]
    enum Mode {
        Full,
        MappingKey,
    }

    enum SequenceKind {
        List,
        Tuple,
        Set,
    }

    enum MappingKind {
        Map,
        OrderedMap,
    }

    enum Task {
        Visit(Value, Mode),
        BuildAnchor(String),
        BuildSequence(SequenceKind, usize),
        BuildMapping(MappingKind, usize),
        BuildNode {
            name: String,
            style: NodeStyle,
            attribute_names: Vec<String>,
            attribute_count: usize,
            child_count: usize,
        },
    }

    let total =
        i128::try_from(share_count).map_err(|_| err("share table is too large to index"))?;
    let mut tasks = alloc::vec![Task::Visit(value, Mode::Full)];
    let mut values = Vec::new();

    while let Some(task) = tasks.pop() {
        match task {
            Task::Visit(Value::Ref(mut label), Mode::Full) => {
                let index = label
                    .parse::<i128>()
                    .map_err(|_| err("share reference index is out of range"))?;
                let resolved = if index < 0 { total + index } else { index };
                if resolved < 0 || resolved >= total {
                    return Err(err("share reference index is out of range"));
                }
                label = resolved.to_string();
                values.push(Value::Ref(label));
            }
            Task::Visit(Value::Anchor(anchor), mode) => {
                let Anchor { label, value } = *anchor;
                tasks.push(Task::BuildAnchor(label));
                tasks.push(Task::Visit(
                    value,
                    if matches!(mode, Mode::MappingKey) {
                        Mode::Full
                    } else {
                        mode
                    },
                ));
            }
            Task::Visit(Value::Tuple(items), mode) => {
                let count = items.len();
                tasks.push(Task::BuildSequence(SequenceKind::Tuple, count));
                for item in items.into_iter().rev() {
                    tasks.push(Task::Visit(item, mode));
                }
            }
            Task::Visit(Value::List(items), Mode::Full) => {
                let count = items.len();
                tasks.push(Task::BuildSequence(SequenceKind::List, count));
                for item in items.into_iter().rev() {
                    tasks.push(Task::Visit(item, Mode::Full));
                }
            }
            Task::Visit(Value::Set(items), Mode::Full) => {
                let count = items.len();
                tasks.push(Task::BuildSequence(SequenceKind::Set, count));
                for item in items.into_iter().rev() {
                    tasks.push(Task::Visit(item, Mode::Full));
                }
            }
            Task::Visit(Value::Map(pairs), Mode::Full) => {
                let count = pairs.len();
                tasks.push(Task::BuildMapping(MappingKind::Map, count));
                for (key, value) in pairs.into_iter().rev() {
                    tasks.push(Task::Visit(value, Mode::Full));
                    tasks.push(Task::Visit(key, Mode::MappingKey));
                }
            }
            Task::Visit(Value::OrderedMap(pairs), Mode::Full) => {
                let count = pairs.len();
                tasks.push(Task::BuildMapping(MappingKind::OrderedMap, count));
                for (key, value) in pairs.into_iter().rev() {
                    tasks.push(Task::Visit(value, Mode::Full));
                    tasks.push(Task::Visit(key, Mode::MappingKey));
                }
            }
            Task::Visit(Value::Node(node), Mode::Full) => {
                let Node {
                    name,
                    style,
                    attributes,
                    children,
                } = *node;
                let attribute_count = attributes.len();
                let child_count = children.len();
                let mut attribute_names = Vec::with_capacity(attribute_count);
                let mut attribute_values = Vec::with_capacity(attribute_count);
                for (name, value) in attributes {
                    attribute_names.push(name);
                    attribute_values.push(value);
                }
                tasks.push(Task::BuildNode {
                    name,
                    style,
                    attribute_names,
                    attribute_count,
                    child_count,
                });
                for child in children.into_iter().rev() {
                    tasks.push(Task::Visit(child, Mode::Full));
                }
                for value in attribute_values.into_iter().rev() {
                    tasks.push(Task::Visit(value, Mode::Full));
                }
            }
            Task::Visit(value, _) => values.push(value),
            Task::BuildAnchor(label) => {
                let value = values.pop().expect("anchor child was visited");
                values.push(Value::Anchor(Box::new(Anchor { label, value })));
            }
            Task::BuildSequence(kind, count) => {
                let start = values.len() - count;
                let items = values.split_off(start);
                values.push(match kind {
                    SequenceKind::List => Value::List(items),
                    SequenceKind::Tuple => Value::Tuple(items),
                    SequenceKind::Set => Value::Set(items),
                });
            }
            Task::BuildMapping(kind, count) => {
                let start = values.len() - count * 2;
                let flattened = values.split_off(start);
                let mut pairs = Vec::with_capacity(count);
                let mut iter = flattened.into_iter();
                while let Some(key) = iter.next() {
                    let value = iter.next().expect("mapping value was visited");
                    pairs.push((key, value));
                }
                values.push(match kind {
                    MappingKind::Map => Value::Map(pairs),
                    MappingKind::OrderedMap => Value::OrderedMap(pairs),
                });
            }
            Task::BuildNode {
                name,
                style,
                attribute_names,
                attribute_count,
                child_count,
            } => {
                let start = values.len() - attribute_count - child_count;
                let mut node_values = values.split_off(start);
                let children = node_values.split_off(attribute_count);
                let attributes = attribute_names.into_iter().zip(node_values).collect();
                values.push(Value::Node(Box::new(Node {
                    name,
                    style,
                    attributes,
                    children,
                })));
            }
        }
    }

    debug_assert_eq!(values.len(), 1);
    Ok(values.pop().expect("root value was rebuilt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        fn nibble(byte: u8) -> u8 {
            match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => panic!("invalid test hex"),
            }
        }

        assert_eq!(text.len() % 2, 0, "test hex must contain whole bytes");
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| nibble(pair[0]) << 4 | nibble(pair[1]))
            .collect()
    }

    fn finite(mantissa: &str, exponent: i32) -> Value {
        Value::Decimal(Decimal::Finite {
            mantissa: mantissa.to_string(),
            exponent,
        })
    }

    #[test]
    fn decimal_negative_zero_is_byte_exact_and_never_panics() {
        for (value, expected) in [
            (finite("-0", 0), "c4820000"),
            (finite("-0", -1), "c4822000"),
            (finite("-0", -2), "c4822100"),
        ] {
            assert_eq!(encode(&value).unwrap(), hex(expected));
            assert_eq!(encode_canonical(&value).unwrap(), hex(expected));
        }

        assert!(
            encode(&Value::Decimal(Decimal::ScaledFinite {
                mantissa: "10".to_string(),
                exponent: -1,
            }))
            .is_err()
        );
        assert!(encode(&Value::Undefined).is_err());
    }

    #[test]
    fn compact_and_canonical_encodings_match_reference_bytes() {
        let float = Value::Float(1.5);
        assert_eq!(encode(&float).unwrap(), hex("fb3ff8000000000000"));
        assert_eq!(encode_canonical(&float).unwrap(), hex("f93e00"));

        let decimal = finite("1000350", -3);
        assert_eq!(encode(&decimal).unwrap(), hex("c482221a000f439e"));
        assert_eq!(encode_canonical(&decimal).unwrap(), hex("c482211a000186c3"));

        let map = Value::Map(alloc::vec![
            (Value::String("z".to_string()), Value::Int("1".to_string())),
            (Value::String("a".to_string()), Value::Int("2".to_string())),
        ]);
        assert_eq!(encode(&map).unwrap(), hex("a2617a01616102"));
        assert_eq!(encode_canonical(&map).unwrap(), hex("a2616102617a01"));
    }

    #[test]
    fn decimal_special_codes_use_python_numeric_lookup() {
        assert!(matches!(
            decode(&hex("d902dbf4")).unwrap(),
            Value::Decimal(Decimal::Infinity)
        ));
        assert!(matches!(
            decode(&hex("d902dbfb3ff0000000000000")).unwrap(),
            Value::Decimal(Decimal::NegInfinity)
        ));
        assert!(matches!(
            decode(&hex("d902db02")).unwrap(),
            Value::Decimal(Decimal::NaN)
        ));
        assert!(decode(&hex("d902db03")).is_err());
        assert!(decode(&hex("d902db63626164")).is_err());
    }

    #[test]
    fn decimal_fraction_uses_python_constructor_and_scaleb_shapes() {
        for (encoded, expected) in [
            ("c482f501", finite("1", 1)),
            ("c48200f4", finite("0", 0)),
            ("c48200fb3ff8000000000000", finite("15", -1)),
            ("c4820063312e35", finite("15", -1)),
            ("c4420001", finite("1", 0)),
            ("c4d902d4820001", finite("1", 0)),
            ("c48200c482210f", finite("15", -2)),
        ] {
            assert_eq!(decode(&hex(encoded)).unwrap(), expected);
        }
        assert!(matches!(
            decode(&hex("c48200d902db00")).unwrap(),
            Value::Decimal(Decimal::Infinity)
        ));
        assert!(decode(&hex("c482fb000000000000000001")).is_err());
    }

    #[test]
    fn standard_maps_follow_python_hash_and_duplicate_rules() {
        let duplicate = decode(&hex("a2616101616102")).unwrap();
        assert_eq!(
            duplicate,
            Value::Map(alloc::vec![(
                Value::String("a".to_string()),
                Value::Int("2".to_string()),
            )])
        );

        assert!(decode(&hex("a1810001")).is_err());

        // False, integer zero, negative float zero, and Decimal zero are one
        // Python mapping key. The first key object remains and the last value
        // wins.
        let numeric = decode(&hex("a4f4010002f9800003c482000004")).unwrap();
        assert_eq!(
            numeric,
            Value::Map(alloc::vec![(
                Value::Bool(false),
                Value::Int("4".to_string()),
            )])
        );

        // Binary float 0.1 is not exactly Decimal 0.1.
        let unequal = decode(&hex("a2fb3fb999999999999a01c482200102")).unwrap();
        assert!(matches!(unequal, Value::Map(ref pairs) if pairs.len() == 2));

        let tuple_key = decode(&hex("a1d902d4810102")).unwrap();
        assert!(matches!(tuple_key, Value::Map(ref pairs) if pairs.len() == 1));

        // Python's resolver intentionally skips mapping keys, so an otherwise
        // undefined reference marker used only as a key remains accepted.
        let reference_key = decode(&hex("a1d81d0001")).unwrap();
        assert!(matches!(
            reference_key,
            Value::Map(ref pairs) if matches!(&pairs[0].0, Value::Ref(label) if label == "0")
        ));
    }

    #[test]
    fn ordered_maps_apply_the_same_duplicate_semantics() {
        let value = decode(&hex("d902d5828261610182616102")).unwrap();
        assert_eq!(
            value,
            Value::OrderedMap(alloc::vec![(
                Value::String("a".to_string()),
                Value::Int("2".to_string()),
            )])
        );
    }

    #[test]
    fn tagged_iterable_payloads_match_python_constructors() {
        assert_eq!(decode(&hex("c280")).unwrap(), Value::Int("0".to_string()));
        assert_eq!(decode(&hex("c28101")).unwrap(), Value::Int("1".to_string()));
        assert_eq!(decode(&hex("c380")).unwrap(), Value::Int("-1".to_string()));

        assert_eq!(
            decode(&hex("d902d4626162")).unwrap(),
            Value::Tuple(alloc::vec![
                Value::String("a".to_string()),
                Value::String("b".to_string()),
            ])
        );
        assert_eq!(
            decode(&hex("d90102420102")).unwrap(),
            Value::Set(alloc::vec![
                Value::Int("1".to_string()),
                Value::Int("2".to_string()),
            ])
        );
        assert_eq!(
            decode(&hex("d82a6178")).unwrap(),
            Value::Link(Link::Cid("x".to_string()))
        );
        assert!(decode(&hex("d82a01")).is_err());
    }

    #[test]
    fn share_references_are_validated_after_the_full_share_table() {
        let self_cycle = decode(&hex("d81c81d81d00")).unwrap();
        assert!(matches!(
            self_cycle,
            Value::Anchor(ref anchor)
                if anchor.label == "0"
                    && matches!(&anchor.value, Value::List(items)
                        if matches!(&items[0], Value::Ref(label) if label == "0"))
        ));

        let forward = decode(&hex("82d81d00d81c80")).unwrap();
        assert!(matches!(
            forward,
            Value::List(ref items)
                if matches!(&items[0], Value::Ref(label) if label == "0")
                    && matches!(&items[1], Value::Anchor(anchor)
                        if anchor.label == "0" && matches!(&anchor.value, Value::List(values) if values.is_empty()))
        ));

        let negative = decode(&hex("82d81d20d81c80")).unwrap();
        assert!(matches!(
            negative,
            Value::List(ref items) if matches!(&items[0], Value::Ref(label) if label == "0")
        ));

        assert!(decode(&hex("d81d00")).is_err());
        assert!(decode(&hex("82d81c80d81d01")).is_err());
    }

    #[test]
    fn temporal_decode_preserves_core_profile_fraction_state_and_checks_ranges() {
        let value = decode(&hex("d902d786010c181e182d187bf6")).unwrap();
        assert!(matches!(
            value,
            Value::Temporal(Temporal::Time(Time {
                hour: 12,
                minute: 30,
                second: 45,
                nanosecond: 123,
                fraction_digits: None,
                zone: Zone::Local,
            }))
        ));

        // The Value model cannot represent a 256 month; reject instead of the
        // old wrapping cast to zero.
        assert!(decode(&hex("d902d784000019010001")).is_err());
        assert!(decode(&hex("c4821a8000000000")).is_err());
    }

    /// The default decode envelope must be the section-16 bound the Python
    /// reference decoder also enforces (`binary2026.DEFAULT_MAX_DEPTH`), so a
    /// document either decodes in both implementations or in neither. A depth
    /// inside the old 129..=997 window is the regression case: it decoded here
    /// while the reference rejected it.
    #[test]
    fn default_decode_matches_the_reference_depth_envelope() {
        let at_limit = {
            let mut nested = alloc::vec![0x81; DEFAULT_MAX_DEPTH as usize];
            nested.push(0xf6);
            nested
        };
        assert!(decode(&at_limit).is_ok());

        for depth in [DEFAULT_MAX_DEPTH as usize + 1, 500, 997] {
            let mut nested = alloc::vec![0x81; depth];
            nested.push(0xf6);
            assert!(
                decode(&nested).is_err(),
                "depth {depth} is rejected by the Python reference and must be rejected here",
            );
            // The deeper envelope stays available, explicitly opted into.
            assert!(decode_with(&nested, 997).is_ok());
        }
    }

    /// Depth far beyond the default envelope is decoded from an explicit frame
    /// stack, so raising the limit costs heap rather than call stack. Run
    /// through `decode_with`, since `decode` now stops at [`DEFAULT_MAX_DEPTH`]
    /// to stay in step with the reference implementation.
    #[test]
    fn ordinary_decode_uses_an_explicit_stack_at_reference_recursion_depth() {
        const DEPTH: usize = 997;
        let mut nested = alloc::vec![0x81; DEPTH];
        nested.push(0xf6);

        let value = decode_with(&nested, DEPTH as u32).expect("explicit stack reaches depth 997");
        let mut cursor = &value;
        for _ in 0..DEPTH {
            cursor = match cursor {
                Value::List(items) if items.len() == 1 => &items[0],
                _ => panic!("decoded nesting shape changed"),
            };
        }
        assert!(matches!(cursor, Value::Null));

        assert!(decode_with(&nested, DEPTH as u32).is_ok());
        assert!(decode_with(&nested, DEPTH as u32 - 1).is_err());

        let mut beyond_limit = alloc::vec![0x81; DEPTH + 1];
        beyond_limit.push(0xf6);
        assert!(
            decode_with(&beyond_limit, DEPTH as u32).is_err(),
            "one frame past the requested limit must be rejected"
        );

        nested.pop();
        assert!(
            decode_with(&nested, DEPTH as u32).is_err(),
            "truncated deep input must fail"
        );

        // Post-decode share validation is iterative as well; otherwise this
        // valid forward/self-reference shape would merely move the overflow
        // from CBOR parsing to the normalization pass.
        let mut shared = alloc::vec![0xd8, 0x1c];
        shared.extend(core::iter::repeat_n(0x81, DEPTH));
        shared.extend_from_slice(&[0xd8, 0x1d, 0x00]);
        let shared =
            decode_with(&shared, DEPTH as u32 + 1).expect("deep share normalization is stack-safe");
        let mut cursor = match &shared {
            Value::Anchor(anchor) if anchor.label == "0" => &anchor.value,
            _ => panic!("share wrapper changed"),
        };
        for _ in 0..DEPTH {
            cursor = match cursor {
                Value::List(items) if items.len() == 1 => &items[0],
                _ => panic!("decoded shared nesting shape changed"),
            };
        }
        assert!(matches!(cursor, Value::Ref(label) if label == "0"));
    }

    #[test]
    fn malformed_branches_return_errors() {
        for bytes in [
            hex(""),
            hex("18"),
            hex("63ab"),
            hex("61ff"),
            hex("f6f6"),
            hex("d903e7f6"),
            hex("9f"),
            hex("f8"),
            hex("fb0000"),
            hex("1bffffffffffffffff00"),
            hex("d902d48300"),
            hex("d902d58100"),
        ] {
            assert!(decode(&bytes).is_err(), "accepted malformed {bytes:02x?}");
        }
    }
}
