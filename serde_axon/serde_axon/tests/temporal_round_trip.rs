//! A temporal at the end of a list must survive its own writer.
//!
//! The temporal scanner consumed `]` unconditionally, so `[^2026-07-01]` ate
//! the list's closing bracket and failed as "invalid time". The writer emits
//! exactly that shape — `to_string_canonical` included — so the crate could not
//! re-read its own output whenever a list ended with a temporal.
//!
//! That reaches the canonical form, and therefore CIDs: canonical bytes that
//! cannot be parsed back break the round-trip the content identifier rests on.
//! These tests are written against the writer rather than against fixed strings
//! so they keep testing the real thing if the spelling ever changes.

use serde_axon::{Value, from_str_value, writer};

fn round_trips(document: &str) {
    let value =
        from_str_value(document).unwrap_or_else(|error| panic!("{document} should parse: {error}"));

    let readable = writer::to_string(&value).expect("readable output");
    from_str_value(&readable)
        .unwrap_or_else(|error| panic!("writer produced unparseable output {readable}: {error}"));

    let canonical = writer::to_string_canonical(&value).expect("canonical output");
    let reparsed = from_str_value(&canonical)
        .unwrap_or_else(|error| panic!("canonical output {canonical} did not parse back: {error}"));

    // Canonical form is idempotent, so re-canonicalizing must not move the
    // bytes; if it did, the CID would depend on how many times a document had
    // been through the writer.
    let again = writer::to_string_canonical(&reparsed).expect("canonical output");
    assert_eq!(canonical, again, "canonical bytes moved on a second pass");
}

#[test]
fn a_temporal_closing_a_list_round_trips() {
    round_trips("[^2026-07-01]");
    round_trips("[^2026-07-01T12:00:00Z]");
    round_trips("[^2026-07-01T12:00-06:00]");
}

#[test]
fn a_temporal_closing_other_containers_round_trips() {
    // These always worked -- `}` and `)` were never in the temporal character
    // set -- and are here so a future change cannot quietly break them while
    // fixing the bracket case.
    round_trips("{a:^2026-07-01T12:00:00Z}");
    round_trips("(^2026-07-01T12:00:00Z)");
}

#[test]
fn a_nested_list_of_temporals_round_trips() {
    round_trips("[[^2026-07-01] [^2026-07-02]]");
    round_trips("{when:[^2026-07-01T00:00:00Z]}");
}

#[test]
fn the_writers_output_is_always_readable_again() {
    // The property that was actually violated, stated directly: whatever the
    // writer emits, the parser accepts.
    let value = from_str_value("[^2026-07-01 ^2026-07-02 ]").expect("parses");
    for text in [
        writer::to_string(&value).expect("readable"),
        writer::to_string_canonical(&value).expect("canonical"),
    ] {
        assert!(
            from_str_value(&text).is_ok(),
            "the writer emitted something the parser rejects: {text}"
        );
    }
}

#[test]
fn a_bare_temporal_is_still_refused() {
    // The fix must not have loosened the Core rule that a temporal carries `^`.
    let error = from_str_value("[2026-07-01]").expect_err("a bare date is not Core AXON");
    assert!(
        error.to_string().contains("bare temporal"),
        "expected the bare-temporal tripwire, got: {error}"
    );
}

#[test]
fn a_list_of_plain_values_is_unaffected() {
    // The scanner change touches `[` and `]`; these confirm ordinary documents
    // did not move.
    for document in ["[1 2 3]", "[\"a\" \"b\"]", "[[1] [2]]", "{a:[1 2]}"] {
        let value = from_str_value(document).unwrap_or_else(|e| panic!("{document}: {e}"));
        assert!(matches!(value, Value::List(_) | Value::Map(_)));
        round_trips(document);
    }
}
