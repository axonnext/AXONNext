//! Envelope-validated AXON streams.

use crate::error::{Category, Error};
use crate::value::{Decimal, NodeStyle, Temporal, Value, Zone};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::iter::FusedIterator;

/// An EDIFACT-inspired stream validator for AXON.
///
/// The Python reference is an iterable whose every `iter(envelope)` call creates
/// a new, fused generator session over shared underlying state.  Rust exposes
/// that model through [`Self::session`] / [`Self::iter`].  For source
/// compatibility this type also remains an [`Iterator`]; direct `next` calls
/// form one implicit fused session, and a later explicit session can resume the
/// shared parent after an error just as a fresh Python generator does.
pub struct EnvelopeStream<I> {
    iter: I,
    in_stream: bool,
    count: usize,
    direct_done: bool,
}

/// One generator-like pass over an [`EnvelopeStream`].
///
/// An error closes this session but does not poison or reset its parent.  Drop
/// the session and call [`EnvelopeStream::session`] again to resume from the
/// shared underlying iterator and envelope state.
pub struct EnvelopeSession<'a, I> {
    stream: &'a mut EnvelopeStream<I>,
    done: bool,
}

impl<I> EnvelopeStream<I>
where
    I: Iterator<Item = Result<Value, Error>>,
{
    /// Wrap an AXON value/error iterator in an envelope validator.
    pub fn new(iter: I) -> Self {
        Self {
            iter,
            in_stream: false,
            count: 0,
            direct_done: false,
        }
    }

    /// Start a fresh generator-like session over the shared parent state.
    pub fn session(&mut self) -> EnvelopeSession<'_, I> {
        EnvelopeSession {
            stream: self,
            done: false,
        }
    }

    /// Alias for [`Self::session`], matching Python's `iter(envelope)` concept.
    pub fn iter(&mut self) -> EnvelopeSession<'_, I> {
        self.session()
    }

    /// Whether a `StreamStart` has been consumed without a successful matching
    /// `StreamEnd`.
    pub fn in_stream(&self) -> bool {
        self.in_stream
    }

    /// Number of records observed since the current envelope's `StreamStart`.
    pub fn record_count(&self) -> usize {
        self.count
    }

    fn next_shared(&mut self) -> Option<Result<Value, Error>> {
        loop {
            let item = match self.iter.next() {
                Some(Ok(value)) => value,
                Some(Err(error)) => return Some(Err(error)),
                None => {
                    return if self.in_stream {
                        Some(Err(Error::new(
                            Category::UnexpectedEnd,
                            "Stream ended abruptly without a StreamEnd envelope node",
                            0,
                            0,
                        )))
                    } else {
                        None
                    };
                }
            };

            if let Value::Node(ref node) = item {
                if node.name == "StreamStart" {
                    if self.in_stream {
                        return Some(Err(Error::new(
                            Category::UnexpectedToken,
                            "Received StreamStart but a stream is already open",
                            0,
                            0,
                        )));
                    }
                    self.in_stream = true;
                    self.count = 0;
                    continue;
                }
                if node.name == "StreamEnd" {
                    if !self.in_stream {
                        return Some(Err(Error::new(
                            Category::UnexpectedToken,
                            "Received StreamEnd without a prior StreamStart",
                            0,
                            0,
                        )));
                    }

                    // Python uses `expected is not None and expected != count`.
                    // That means explicit null disables the check and numeric
                    // equality crosses bool/int/float/Decimal kinds.
                    if let Some(expected) = node
                        .attributes
                        .iter()
                        .find_map(|(key, value)| (key == "count").then_some(value))
                        && !python_count_matches(expected, self.count)
                    {
                        return Some(Err(Error::new(
                            Category::UnexpectedToken,
                            alloc::format!(
                                "StreamEnd count mismatch: expected {}, got {}",
                                count_display(expected),
                                self.count
                            ),
                            0,
                            0,
                        )));
                    }

                    // Deliberately occurs only after a successful check.  On a
                    // mismatch Python leaves the parent stream open.
                    self.in_stream = false;
                    continue;
                }
            }

            if !self.in_stream {
                return Some(Err(Error::new(
                    Category::UnexpectedToken,
                    alloc::format!(
                        "Received data outside of a stream envelope: {}",
                        count_display(&item)
                    ),
                    0,
                    0,
                )));
            }

            let Some(count) = self.count.checked_add(1) else {
                return Some(Err(Error::new(
                    Category::ResourceLimitExceeded,
                    "stream record count exceeds this platform's limit",
                    0,
                    0,
                )));
            };
            self.count = count;
            return Some(Ok(item));
        }
    }
}

impl<I> Iterator for EnvelopeStream<I>
where
    I: Iterator<Item = Result<Value, Error>>,
{
    type Item = Result<Value, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.direct_done {
            return None;
        }
        let next = self.next_shared();
        if next.is_none() || matches!(&next, Some(Err(_))) {
            self.direct_done = true;
        }
        next
    }
}

impl<I> FusedIterator for EnvelopeStream<I> where I: Iterator<Item = Result<Value, Error>> {}

impl<I> Iterator for EnvelopeSession<'_, I>
where
    I: Iterator<Item = Result<Value, Error>>,
{
    type Item = Result<Value, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let next = self.stream.next_shared();
        if next.is_none() || matches!(&next, Some(Err(_))) {
            self.done = true;
        }
        next
    }
}

impl<I> FusedIterator for EnvelopeSession<'_, I> where I: Iterator<Item = Result<Value, Error>> {}

fn python_count_matches(expected: &Value, count: usize) -> bool {
    match expected {
        // `dict.get("count", None)` makes absent and explicit null identical.
        Value::Null => true,
        Value::Bool(value) => usize::from(*value) == count,
        Value::Int(text) => integer_equals_count(text, count),
        Value::Float(value) => exact_nonnegative_f64_integer(*value) == Some(count as u128),
        Value::Decimal(Decimal::Finite { mantissa, exponent })
        | Value::Decimal(Decimal::ScaledFinite { mantissa, exponent }) => {
            decimal_equals_count(mantissa, *exponent, count)
        }
        Value::Decimal(Decimal::Infinity | Decimal::NegInfinity | Decimal::NaN)
        | Value::String(_)
        | Value::Bytes(_)
        | Value::Temporal(_)
        | Value::List(_)
        | Value::Tuple(_)
        | Value::Map(_)
        | Value::OrderedMap(_)
        | Value::Set(_)
        | Value::Node(_)
        | Value::Link(_)
        | Value::Anchor(_)
        | Value::Ref(_)
        | Value::Undefined => false,
    }
}

fn exact_nonnegative_f64_integer(value: f64) -> Option<u128> {
    let bits = value.to_bits();
    let negative = bits >> 63 != 0;
    let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);

    // Both signed zero spellings compare equal to Python's integer zero.
    if exponent_bits == 0 && fraction == 0 {
        return Some(0);
    }
    if negative || exponent_bits == 0 || exponent_bits == 0x7ff {
        return None;
    }

    let exponent = exponent_bits - 1023;
    if !(0..=127).contains(&exponent) {
        return None;
    }
    let significand = u128::from((1u64 << 52) | fraction);
    if exponent >= 52 {
        significand.checked_shl((exponent - 52) as u32)
    } else {
        let fractional_bits = (52 - exponent) as u32;
        let mask = (1u128 << fractional_bits) - 1;
        (significand & mask == 0).then_some(significand >> fractional_bits)
    }
}

fn integer_equals_count(text: &str, count: usize) -> bool {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let significant = digits.trim_start_matches('0');
    if significant.is_empty() {
        return count == 0;
    }
    !negative && significant == count.to_string()
}

fn decimal_equals_count(mantissa: &str, exponent: i32, count: usize) -> bool {
    let (negative, digits) = match mantissa.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, mantissa.strip_prefix('+').unwrap_or(mantissa)),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return count == 0;
    }
    if negative {
        return false;
    }

    let target = count.to_string();
    if exponent >= 0 {
        let zeros = exponent as usize;
        return digits.len().checked_add(zeros) == Some(target.len())
            && target.starts_with(digits)
            && target[digits.len()..].bytes().all(|byte| byte == b'0');
    }

    let scale = exponent.unsigned_abs() as usize;
    if scale >= digits.len()
        || !digits[digits.len() - scale..]
            .bytes()
            .all(|byte| byte == b'0')
    {
        return false;
    }
    let integer = digits[..digits.len() - scale].trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    integer == target
}

fn count_display(value: &Value) -> alloc::string::String {
    python_value_text(value, false)
}

/// Python's f-strings use `str(value)` at the top level. Container and
/// dataclass representations recursively use `repr`, which differs for text,
/// decimals, and dates. Keep that distinction explicit instead of leaking the
/// Rust `Debug` representation into the public stream error contract.
fn python_value_text(value: &Value, nested_repr: bool) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Int(text) => text.clone(),
        Value::Float(value) => python_float_text(*value),
        Value::Decimal(Decimal::Finite { mantissa, exponent }) => {
            let text = python_decimal_text(mantissa, *exponent);
            if nested_repr {
                alloc::format!("Decimal({})", python_string_repr(&text))
            } else {
                text
            }
        }
        Value::Decimal(Decimal::ScaledFinite { mantissa, exponent }) => {
            let decimal = python_decimal_text(mantissa, *exponent);
            alloc::format!(
                "ScaledDecimal(value=Decimal({}), exponent={exponent})",
                python_string_repr(&decimal)
            )
        }
        Value::Decimal(Decimal::Infinity) => python_decimal_special("Infinity", nested_repr),
        Value::Decimal(Decimal::NegInfinity) => python_decimal_special("-Infinity", nested_repr),
        Value::Decimal(Decimal::NaN) => python_decimal_special("NaN", nested_repr),
        Value::String(text) if nested_repr => python_string_repr(text),
        Value::String(text) => text.clone(),
        Value::Bytes(bytes) => python_bytes_repr(bytes),
        Value::Temporal(temporal) => python_temporal_text(temporal, nested_repr),
        Value::List(items) => python_sequence_repr("[", "]", items),
        Value::Tuple(items) => {
            alloc::format!("AxonTuple(items={})", python_sequence_repr("[", "]", items))
        }
        Value::Map(pairs) => python_mapping_repr("", pairs),
        Value::OrderedMap(pairs) => python_mapping_repr("OrderedDict", pairs),
        Value::Set(items) => {
            alloc::format!("AxonSet(items={})", python_sequence_repr("[", "]", items))
        }
        Value::Node(node) => {
            let style = match node.style {
                NodeStyle::Unit => "unit",
                NodeStyle::Brace => "brace",
                NodeStyle::Tuple => "tuple",
                NodeStyle::List => "list",
            };
            let attributes: Vec<(Value, Value)> = node
                .attributes
                .iter()
                .map(|(key, value)| (Value::String(key.clone()), value.clone()))
                .collect();
            alloc::format!(
                "Node({}, {style}, attrs={}, children={})",
                python_string_repr(&node.name),
                python_mapping_repr("OrderedDict", &attributes),
                python_sequence_repr("[", "]", &node.children)
            )
        }
        Value::Link(crate::value::Link::Cid(target)) => {
            alloc::format!("Link(kind='cid', target={})", python_string_repr(target))
        }
        Value::Link(crate::value::Link::Uri(target)) => {
            alloc::format!("Link(kind='uri', target={})", python_string_repr(target))
        }
        // Python resolves anchors before values reach EnvelopeStream, so an
        // anchor itself is transparent for display purposes.
        Value::Anchor(anchor) => python_value_text(&anchor.value, nested_repr),
        // A valid Python graph never exposes `_Ref` after resolution. Retain
        // its private dataclass spelling for a hand-built structural value.
        Value::Ref(label) => {
            alloc::format!("_Ref(label={})", python_string_repr(label))
        }
        Value::Undefined => "??".to_string(),
    }
}

fn python_decimal_special(text: &str, nested_repr: bool) -> String {
    if nested_repr {
        alloc::format!("Decimal({})", python_string_repr(text))
    } else {
        text.to_string()
    }
}

fn python_float_text(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    let raw = alloc::format!("{value:?}");
    let Some(index) = raw.find('e') else {
        return raw;
    };
    let coefficient = &raw[..index];
    let exponent = &raw[index + 1..];
    let (sign, digits) = if let Some(digits) = exponent.strip_prefix('-') {
        ('-', digits)
    } else if let Some(digits) = exponent.strip_prefix('+') {
        ('+', digits)
    } else {
        ('+', exponent)
    };
    if digits.len() < 2 {
        alloc::format!("{coefficient}e{sign}0{digits}")
    } else {
        alloc::format!("{coefficient}e{sign}{digits}")
    }
}

fn python_decimal_text(mantissa: &str, exponent: i32) -> String {
    let negative = mantissa.starts_with('-');
    let digits = mantissa.trim_start_matches('-');
    let digits = if digits.is_empty() { "0" } else { digits };
    let adjusted = i64::try_from(digits.len()).unwrap_or(i64::MAX) + i64::from(exponent) - 1;
    let mut output = String::new();
    if negative {
        output.push('-');
    }

    if exponent <= 0 && adjusted >= -6 {
        let point = i64::try_from(digits.len()).unwrap_or(i64::MAX) + i64::from(exponent);
        if point > 0 {
            let point = usize::try_from(point).unwrap_or(digits.len());
            output.push_str(&digits[..point]);
            if point < digits.len() {
                output.push('.');
                output.push_str(&digits[point..]);
            }
        } else {
            output.push_str("0.");
            for _ in 0..point.unsigned_abs() {
                output.push('0');
            }
            output.push_str(digits);
        }
    } else {
        output.push(digits.as_bytes()[0] as char);
        if digits.len() > 1 {
            output.push('.');
            output.push_str(&digits[1..]);
        }
        output.push('E');
        if adjusted >= 0 {
            output.push('+');
        }
        output.push_str(&adjusted.to_string());
    }
    output
}

fn python_sequence_repr(open: &str, close: &str, items: &[Value]) -> String {
    let mut output = open.to_string();
    for (index, item) in items.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        output.push_str(&python_value_text(item, true));
    }
    output.push_str(close);
    output
}

fn python_mapping_repr(prefix: &str, pairs: &[(Value, Value)]) -> String {
    let mut body = String::from("{");
    for (index, (key, value)) in pairs.iter().enumerate() {
        if index != 0 {
            body.push_str(", ");
        }
        body.push_str(&python_value_text(key, true));
        body.push_str(": ");
        body.push_str(&python_value_text(value, true));
    }
    body.push('}');
    if prefix.is_empty() {
        body
    } else if pairs.is_empty() {
        alloc::format!("{prefix}()")
    } else {
        alloc::format!("{prefix}({body})")
    }
}

fn python_string_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut output = String::new();
    output.push(quote);
    for character in text.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '\t' => output.push_str("\\t"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            character if character == quote => {
                output.push('\\');
                output.push(character);
            }
            character if python_character_is_printable(character) => output.push(character),
            character => push_python_escape(&mut output, u32::from(character)),
        }
    }
    output.push(quote);
    output
}

fn python_bytes_repr(bytes: &[u8]) -> String {
    let quote = if bytes.contains(&b'\'') && !bytes.contains(&b'"') {
        b'"'
    } else {
        b'\''
    };
    let mut output = String::from("b");
    output.push(quote as char);
    for byte in bytes {
        match *byte {
            b'\\' => output.push_str("\\\\"),
            b'\t' => output.push_str("\\t"),
            b'\n' => output.push_str("\\n"),
            b'\r' => output.push_str("\\r"),
            byte if byte == quote => {
                output.push('\\');
                output.push(byte as char);
            }
            0x20..=0x7e => output.push(*byte as char),
            byte => output.push_str(&alloc::format!("\\x{byte:02x}")),
        }
    }
    output.push(quote as char);
    output
}

fn python_character_is_printable(character: char) -> bool {
    character == ' '
        || (!character.is_control()
            && !character.is_whitespace()
            && !matches!(
                u32::from(character),
                0x00ad
                    | 0x061c
                    | 0x180e
                    | 0x200b..=0x200f
                    | 0x202a..=0x202e
                    | 0x2060..=0x206f
                    | 0xfeff
                    | 0xfff9..=0xfffb
            ))
}

fn push_python_escape(output: &mut String, codepoint: u32) {
    if codepoint <= 0xff {
        output.push_str(&alloc::format!("\\x{codepoint:02x}"));
    } else if codepoint <= 0xffff {
        output.push_str(&alloc::format!("\\u{codepoint:04x}"));
    } else {
        output.push_str(&alloc::format!("\\U{codepoint:08x}"));
    }
}

fn python_temporal_text(temporal: &Temporal, nested_repr: bool) -> String {
    match temporal {
        Temporal::Date(date) if !nested_repr => {
            alloc::format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)
        }
        Temporal::Date(date) => {
            alloc::format!("datetime.date({}, {}, {})", date.year, date.month, date.day)
        }
        Temporal::Time(time) => python_nano_time_repr(time),
        Temporal::DateTime(datetime) => {
            let zone_name = match &datetime.time.zone {
                Zone::Named { name, .. } => python_string_repr(name),
                _ => "None".to_string(),
            };
            alloc::format!(
                "NanoDateTime(calendar_date=datetime.date({}, {}, {}), clock_time={}, zone_name={zone_name})",
                datetime.date.year,
                datetime.date.month,
                datetime.date.day,
                python_nano_time_repr(&datetime.time)
            )
        }
    }
}

fn python_nano_time_repr(time: &crate::value::Time) -> String {
    let (offset, zulu) = match time.zone {
        Zone::Local => ("None".to_string(), false),
        Zone::Zulu => ("0".to_string(), true),
        Zone::Offset(offset) | Zone::Named { offset, .. } => (offset.to_string(), false),
    };
    let fraction = time
        .fraction_digits
        .map_or_else(|| "None".to_string(), |digits| digits.to_string());
    let zulu = if zulu { "True" } else { "False" };
    alloc::format!(
        "NanoTime(hour={}, minute={}, second={}, nanosecond={}, offset_minutes={offset}, zulu={zulu}, fraction_digits={fraction})",
        time.hour,
        time.minute,
        time.second,
        time.nanosecond
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;

    fn marker(name: &str, count: Option<Value>) -> Value {
        Value::Node(Box::new(crate::value::Node {
            name: name.to_string(),
            style: if count.is_some() {
                NodeStyle::Brace
            } else {
                NodeStyle::Unit
            },
            attributes: count
                .into_iter()
                .map(|value| ("count".to_string(), value))
                .collect(),
            children: Vec::new(),
        }))
    }

    fn terminal_message(values: Vec<Value>) -> String {
        let mut stream = EnvelopeStream::new(values.into_iter().map(Ok));
        loop {
            match stream.next() {
                Some(Ok(_)) => {}
                Some(Err(error)) => return error.message,
                None => panic!("test stream ended without an error"),
            }
        }
    }

    #[test]
    fn python_integer_equality_is_exact_beyond_f64_precision() {
        assert_eq!(exact_nonnegative_f64_integer(-0.0), Some(0));
        assert_eq!(exact_nonnegative_f64_integer(1.0), Some(1));
        assert_eq!(exact_nonnegative_f64_integer(1.5), None);
        assert_eq!(exact_nonnegative_f64_integer(f64::NAN), None);
        assert_eq!(exact_nonnegative_f64_integer(f64::INFINITY), None);
        assert_eq!(
            exact_nonnegative_f64_integer(9_007_199_254_740_992.0),
            Some(9_007_199_254_740_992)
        );

        assert!(integer_equals_count("-0", 0));
        assert!(integer_equals_count("0001", 1));
        assert!(!integer_equals_count("-1", 1));

        assert!(decimal_equals_count("100", -2, 1));
        assert!(decimal_equals_count("-0", -100, 0));
        assert!(!decimal_equals_count("15", -1, 1));
        assert!(!decimal_equals_count("1", 100, 1));
    }

    #[test]
    fn byte_count_mismatch_uses_python_bytes_text() {
        let message = terminal_message(alloc::vec![
            marker("StreamStart", None),
            Value::Int("1".to_string()),
            marker("StreamEnd", Some(Value::Bytes(b"x".to_vec()))),
        ]);
        assert_eq!(message, "StreamEnd count mismatch: expected b'x', got 1");
    }

    #[test]
    fn outside_empty_list_uses_python_container_text() {
        let message = terminal_message(alloc::vec![Value::List(Vec::new())]);
        assert_eq!(message, "Received data outside of a stream envelope: []");
    }

    #[test]
    fn composite_count_mismatch_uses_recursive_python_repr() {
        let count = Value::Map(alloc::vec![(
            Value::String("a".to_string()),
            Value::List(alloc::vec![Value::String("x".to_string())]),
        )]);
        let message = terminal_message(alloc::vec![
            marker("StreamStart", None),
            Value::Null,
            marker("StreamEnd", Some(count)),
        ]);
        assert_eq!(
            message,
            "StreamEnd count mismatch: expected {'a': ['x']}, got 1"
        );
    }
}
