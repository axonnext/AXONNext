//! Differential conformance: for every case in `differential.json` (generated
//! from the pyaxon 2026 reference by `gen_differential.py`), assert that
//! serde_axon reaches the *same* outcome -- the same accept/reject decision, the
//! same canonical bytes on accept, and the same error category on reject.
//!
//! The audit's root cause was that each implementation only ran its own suite,
//! so divergences on unshared surfaces stayed invisible. This test closes that
//! by feeding one reference-derived corpus to serde_axon and comparing against
//! ground truth, so any regression on a parity fix fails here immediately.

use serde_axon::parser::{Limits, Options};
use serde_axon::{from_str_stream_with, writer};

use serde_json::Value as J;

fn cases() -> J {
    let raw = include_str!("differential.json");
    serde_json::from_str(raw).expect("differential.json parses")
}

/// Map a named profile to serde_axon [`Options`], matching `gen_differential.py`.
fn options_for(profile: &str) -> Options {
    let base = Options {
        limits: Limits::default(),
        ..Options::default()
    };
    match profile {
        "core" => base,
        "doc_link" => Options {
            doc_link: true,
            ..base
        },
        "graph" => Options {
            graph: true,
            ..base
        },
        // The generator uses `Options(compatibility=True)` -- the single field,
        // which is a master switch only for the flags gated `or compatibility`
        // in the reference, NOT the whole bundle. So this is deliberately the
        // one-field form, not `Options::compat()` (the header-expanded bundle).
        "compat" => Options {
            compatibility: true,
            ..base
        },
        "tz_names" => Options {
            tz_names: true,
            ..base
        },
        "canonical_verify" => Options {
            canonical_verify: true,
            ..base
        },
        "scale_significant" => Options {
            scale_significant: true,
            ..base
        },
        "numeric_separators_off" => Options {
            numeric_separators: false,
            ..base
        },
        "compat_bundle" => Options::compat(),
        "header_profiles_off" => Options {
            allow_header_profiles: false,
            ..base
        },
        other => panic!("unknown profile in differential.json: {other}"),
    }
}

#[test]
fn matches_reference() {
    let data = cases();
    let mut failures = Vec::new();

    for case in data["cases"].as_array().unwrap() {
        let profile = case["profile"].as_str().unwrap();
        let input = case["input"].as_str().unwrap();
        let opts = options_for(profile);
        let outcome = case["outcome"].as_str().unwrap();

        match (outcome, from_str_stream_with(input, opts)) {
            ("accept", Ok(values)) => {
                // Compare canonical bytes: the cross-implementation contract.
                if let Some(want) = case["canonical"].as_str() {
                    match writer::to_string_stream_canonical(&values) {
                        Ok(rendered) if rendered == want => {}
                        Ok(rendered) => failures.push(format!(
                            "[{profile}] {input:?}: canonical mismatch\n     got: {rendered:?}\n    want: {want:?}"
                        )),
                        Err(error) => failures.push(format!(
                            "[{profile}] {input:?}: canonical write failed: {error}"
                        )),
                    }
                }
                if let Some(count) = case["count"].as_u64()
                    && values.len() as u64 != count
                {
                    failures.push(format!(
                        "[{profile}] {input:?}: value count {} != reference {count}",
                        values.len()
                    ));
                }
                if let Some(want) = case["compact"].as_str() {
                    let compact = writer::to_string_stream(&values).unwrap();
                    if compact != want {
                        failures.push(format!(
                            "[{profile}] {input:?}: compact mismatch\n     got: {compact:?}\n    want: {want:?}"
                        ));
                    }
                }
                if let Some(modes) = case["compact_modes"].as_object() {
                    for (mode, graph) in [("graph_false", false), ("graph_true", true)] {
                        let expected = &modes[mode];
                        let actual =
                            writer::to_string_stream_with(&values, writer::WriteOptions { graph });
                        match (expected["outcome"].as_str().unwrap(), actual) {
                            ("accept", Ok(text)) if text == expected["text"].as_str().unwrap() => {}
                            ("accept", Ok(text)) => failures.push(format!(
                                "[{profile}] {input:?}: compact {mode} mismatch\n     got: {text:?}\n    want: {:?}",
                                expected["text"].as_str().unwrap()
                            )),
                            ("accept", Err(error)) => failures.push(format!(
                                "[{profile}] {input:?}: compact {mode} failed: {error}"
                            )),
                            ("error", Err(error))
                                if error.category().as_str()
                                    == expected["category"].as_str().unwrap()
                                    && error.message == expected["message"].as_str().unwrap() => {}
                            ("error", Err(error)) => failures.push(format!(
                                "[{profile}] {input:?}: compact {mode} error ({}, {:?}) != ({}, {:?})",
                                error.category().as_str(),
                                error.message,
                                expected["category"].as_str().unwrap(),
                                expected["message"].as_str().unwrap()
                            )),
                            ("error", Ok(text)) => failures.push(format!(
                                "[{profile}] {input:?}: compact {mode} should fail, wrote {text:?}"
                            )),
                            (other, _) => panic!("unknown compact outcome: {other}"),
                        }
                    }
                }
            }
            ("accept", Err(e)) => failures.push(format!(
                "[{profile}] {input:?}: reference accepts, serde_axon rejected with {}",
                e.category().as_str()
            )),
            ("error", Ok(v)) => failures.push(format!(
                "[{profile}] {input:?}: reference rejects ({}), serde_axon accepted {v:?}",
                case["category"].as_str().unwrap()
            )),
            ("error", Err(e)) => {
                let want = case["category"].as_str().unwrap();
                let message_matches = case["message"]
                    .as_str()
                    .is_none_or(|message| e.message == message);
                let offset_matches = case["offset"]
                    .as_u64()
                    .is_none_or(|offset| e.offset == offset as usize);
                let end_matches = case["end_offset"]
                    .as_u64()
                    .is_none_or(|offset| e.end_offset == offset as usize);
                let line_matches = case["line"]
                    .as_u64()
                    .is_none_or(|line| e.line == line as u32);
                let column_matches = case["column"]
                    .as_u64()
                    .is_none_or(|column| e.column == column as u32);
                if e.category().as_str() != want
                    || !message_matches
                    || !offset_matches
                    || !end_matches
                    || !line_matches
                    || !column_matches
                {
                    failures.push(format!(
                        "[{profile}] {input:?}: error ({}, {:?}, {}..{}, {}:{}) != reference {}",
                        e.category().as_str(),
                        e.message,
                        e.offset,
                        e.end_offset,
                        e.line,
                        e.column,
                        case
                    ));
                }
            }
            (other, _) => panic!("unknown outcome in differential.json: {other}"),
        }
    }

    assert!(
        failures.is_empty(),
        "{} differential mismatch(es):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
