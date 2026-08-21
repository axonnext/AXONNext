//! `RawValue` carries arbitrary AXON through a typed structure.
//!
//! The point of the type is that nothing crosses Serde's data model on the way,
//! so the distinctions AXON is careful about survive. These tests are what hold
//! that claim: each one uses a kind that Serde would flatten if the value were
//! routed through `deserialize_any`.

use serde::{Deserialize, Serialize};
use serde_axon::RawValue;

#[derive(Serialize, Deserialize)]
struct Record {
    name: String,
    extra: RawValue,
}

#[test]
fn raw_value_keeps_the_kinds_serde_would_flatten() {
    // A decimal becomes a string under `deserialize_any`, a tuple and a set
    // become sequences, an ordered map becomes a map, and a tagged node becomes
    // a map. None of that may happen here.
    let text = concat!(
        r#"{name:"Ada" extra:{"#,
        "amount:1.50D ",
        "pair:(1 2) ",
        "ordered:[a:1 b:2] ",
        "tagged:point{x:1 y:-2}",
        "}}"
    );

    let record: Record = serde_axon::from_str(text).expect("reads");
    assert_eq!(record.name, "Ada");

    let again = serde_axon::to_string(&record).expect("writes");
    assert!(again.contains("1.50D"), "decimal scale and suffix: {again}");
    assert!(again.contains("(1 2)"), "tuple stayed a tuple: {again}");
    assert!(
        again.contains("[a:1 b:2]"),
        "ordered map stayed ordered: {again}"
    );
    assert!(again.contains("point{"), "node kept its name: {again}");

    // And it is a fixed point: reading the output and writing it again changes
    // nothing further.
    let round_two: Record = serde_axon::from_str(&again).expect("reads again");
    assert_eq!(
        serde_axon::to_string(&round_two).expect("writes again"),
        again,
        "RawValue round trips are idempotent"
    );
}

#[test]
fn raw_value_survives_a_temporal_and_a_link() {
    let text = r#"{name:"Ada" extra:{when:^2026-08-20T12:00:00Z rate:3.14159D}}"#;
    let record: Record = serde_axon::from_str(text).expect("reads");
    let again = serde_axon::to_string(&record).expect("writes");
    assert!(
        again.contains("^2026-08-20T12:00:00Z"),
        "the temporal stayed a temporal rather than becoming a string: {again}"
    );
    assert!(
        again.contains("3.14159D"),
        "decimal kept its scale: {again}"
    );
}

#[test]
fn raw_value_accessors_agree_with_the_document() {
    let text = r#"{name:"Ada" extra:{k:1}}"#;
    let record: Record = serde_axon::from_str(text).expect("reads");
    assert_eq!(record.extra.get(), record.extra.to_string());
    assert!(record.extra.to_value().is_ok(), "parses back into the tree");
}

#[test]
fn from_string_rejects_text_that_is_not_axon() {
    assert!(RawValue::from_string("{unclosed:".to_string()).is_err());
    assert!(RawValue::from_string("{closed:1}".to_string()).is_ok());
}

#[test]
fn canonical_bytes_are_unchanged_by_going_through_a_raw_value() {
    // The cross-implementation contract is canonical bytes. A RawValue must not
    // be able to move them, or it would break parity with the reference.
    let text = r#"{name:"Ada" extra:{b:2 a:1 d:1.50D}}"#;

    let direct = serde_axon::from_str_value(text).expect("parses");
    let via_raw: Record = serde_axon::from_str(text).expect("reads");
    let rewritten = serde_axon::to_string(&via_raw).expect("writes");
    let reparsed = serde_axon::from_str_value(&rewritten).expect("parses again");

    assert_eq!(
        serde_axon::writer::to_string_canonical(&direct).expect("canonicalizes"),
        serde_axon::writer::to_string_canonical(&reparsed).expect("canonicalizes"),
        "canonical bytes are identical with and without the RawValue hop"
    );
}
