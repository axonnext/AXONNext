//! A decimal exponent must not turn a few bytes of input into gigabytes of
//! output, and must never overflow on the way there.
//!
//! `write_decimal_digits` negated its exponent to get the scale. `i32::MIN` has
//! no negation, so `1E-2147483648D` panicked in a debug build and, in release,
//! wrapped back to `i32::MIN` — which `as usize` sign-extended into roughly
//! eighteen quintillion, the number of zeroes the padding loop then tried to
//! write. In the other direction nothing overflowed at all: `1E2147483647D` is
//! a perfectly ordinary value that simply asks for two gibibytes of digits.
//!
//! `write_offset` had carried the `unsigned_abs` fix for this since audit M7,
//! and `decimal_numeric_parts` the `checked_add` fix since audit M10. This site
//! sits between them and was missed.

use std::time::Instant;

#[test]
fn the_most_negative_exponent_does_not_panic() {
    let value = serde_axon::from_str_value("1E-2147483648D").expect("parses");
    let error = serde_axon::writer::to_string(&value)
        .expect_err("an exponent this extreme is refused, not rendered");
    assert!(
        error.to_string().contains("maximum writable length"),
        "refused for the right reason: {error}"
    );
}

#[test]
fn the_most_positive_exponent_does_not_allocate_gigabytes() {
    let value = serde_axon::from_str_value("1E2147483647D").expect("parses");
    let start = Instant::now();
    let error = serde_axon::writer::to_string(&value)
        .expect_err("an exponent this extreme is refused, not rendered");
    // Refused from the exponent alone, so it cannot be slow.
    assert!(
        start.elapsed().as_millis() < 500,
        "refusal is immediate, not after building the string"
    );
    assert!(error.to_string().contains("maximum writable length"));
}

#[test]
fn the_canonical_writer_refuses_it_too() {
    // Canonical output is the cross-implementation contract, so the same bound
    // has to hold on that path or a document could be unwritable one way and
    // enormous the other.
    let value = serde_axon::from_str_value("1E2147483647D").expect("parses");
    assert!(serde_axon::writer::to_string_canonical(&value).is_err());
}

#[test]
fn ordinary_decimals_are_unaffected() {
    // The bound must not disturb anything a person would actually write.
    for text in [
        "1.50D",
        "0D",
        "-0.001D",
        "3.14159D",
        "1E10D",
        "1E-10D",
        "123456789.987654321D",
        "1E1000D",
        "1E-1000D",
        "∞D",
        "-∞D",
        "?D",
    ] {
        let value = serde_axon::from_str_value(text)
            .unwrap_or_else(|error| panic!("{text} should parse: {error}"));
        serde_axon::writer::to_string(&value)
            .unwrap_or_else(|error| panic!("{text} should write: {error}"));
    }
}

#[test]
fn the_bound_sits_between_reasonable_and_absurd() {
    // Just inside: a million-digit decimal is permitted.
    let inside = serde_axon::from_str_value("1E999000D").expect("parses");
    assert!(
        serde_axon::writer::to_string(&inside).is_ok(),
        "a decimal within the bound still writes"
    );

    // Just outside: refused rather than attempted.
    let outside = serde_axon::from_str_value("1E1000001D").expect("parses");
    assert!(
        serde_axon::writer::to_string(&outside).is_err(),
        "a decimal beyond the bound is refused"
    );
}
