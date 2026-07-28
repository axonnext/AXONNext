//! Generated cross-surface parity checks against `surfaces.json`.
//!
//! The fixture is produced by `gen_surfaces.py` from the Python AXON 2026
//! reference. Robustness-only adversarial tests remain in `robustness.rs`;
//! this file checks public, observable cross-implementation behaviour.

use serde_axon::binary;
use serde_axon::lsp::{DocumentSymbol, LspDiagnostic, LspHover, LspTextEdit, SymbolPathSegment};
use serde_axon::migration::{apply_migration_edits, migrate_with};
use serde_axon::parser::Options;
use serde_axon::scientific::{ArrayDtype, ArrayOrder, ScientificArray};
use serde_axon::stream::EnvelopeStream;
use serde_axon::value::Value;
use serde_axon::{
    cid, diagnostics, document_symbols, formatting_edits, from_str_stream_with, hover, iter_stream,
    iter_stream_with, parse_cid, verify_canonical, verify_cid, writer,
};
use serde_json::{Value as J, json};

fn fixture() -> J {
    serde_json::from_str(include_str!("surfaces.json")).expect("surfaces.json parses")
}

fn options_for(profile: &str) -> Options {
    match profile {
        "core" => Options::default(),
        "graph" => Options {
            graph: true,
            ..Options::default()
        },
        "doc_link" => Options {
            doc_link: true,
            ..Options::default()
        },
        "compat" => Options::compat(),
        "scale_significant" => Options {
            scale_significant: true,
            ..Options::default()
        },
        "canonical_verify" => Options {
            canonical_verify: true,
            ..Options::default()
        },
        "tz_names" => Options {
            tz_names: true,
            ..Options::default()
        },
        "indented_bodies" => Options {
            indented_bodies: true,
            ..Options::default()
        },
        "max_input_4" => {
            let mut options = Options::default();
            options.limits.max_input = 4;
            options
        }
        other => panic!("unknown surfaces profile: {other}"),
    }
}

fn decode_hex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2), "hex length must be even");
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => panic!("invalid hex digit"),
            };
            digit(pair[0]) << 4 | digit(pair[1])
        })
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

#[test]
fn binary_matches_reference_bytes_and_decode_outcomes() {
    let data = fixture();
    let mut failures = Vec::new();
    for case in data["binary"]["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let source = case["input"].as_str().unwrap();
        let profile = case["profile"].as_str().unwrap();
        let values = match from_str_stream_with(source, options_for(profile)) {
            Ok(values) if values.len() == 1 => values,
            Ok(values) => {
                failures.push(format!("[{id}] expected one value, got {}", values.len()));
                continue;
            }
            Err(error) => {
                failures.push(format!("[{id}] parse failed: {error}"));
                continue;
            }
        };
        let value = &values[0];
        match binary::encode(value) {
            Ok(bytes) if encode_hex(&bytes) == case["compact_hex"].as_str().unwrap() => {}
            Ok(bytes) => failures.push(format!(
                "[{id}] compact bytes {} != {}",
                encode_hex(&bytes),
                case["compact_hex"].as_str().unwrap()
            )),
            Err(error) => failures.push(format!("[{id}] compact encode failed: {error}")),
        }
        let canonical = match binary::encode_canonical(value) {
            Ok(bytes) => bytes,
            Err(error) => {
                failures.push(format!("[{id}] canonical encode failed: {error}"));
                continue;
            }
        };
        let want = case["canonical_hex"].as_str().unwrap();
        let oracle_canonical = decode_hex(want);
        if encode_hex(&canonical) != want {
            failures.push(format!(
                "[{id}] canonical bytes {} != {want}",
                encode_hex(&canonical)
            ));
        }
        for (form, bytes, outcome_key) in [
            (
                "compact",
                decode_hex(case["compact_hex"].as_str().unwrap()),
                "compact_decode_outcome",
            ),
            (
                "canonical",
                oracle_canonical.clone(),
                "canonical_decode_outcome",
            ),
        ] {
            let decoded = binary::decode(&bytes);
            let expected_accept = case[outcome_key].as_str().unwrap() == "accept";
            if decoded.is_ok() != expected_accept {
                failures.push(format!(
                    "[{id}] {form} decode acceptance mismatch: {decoded:?}"
                ));
                continue;
            }
            if let Ok(decoded) = decoded {
                match writer::to_string_canonical(&decoded) {
                    Ok(rendered) if rendered == case["canonical_text"].as_str().unwrap() => {}
                    Ok(rendered) => failures.push(format!(
                        "[{id}] {form} decoded canonical text {rendered:?} != {:?}",
                        case["canonical_text"].as_str().unwrap()
                    )),
                    Err(error) => failures.push(format!(
                        "[{id}] {form} decoded value is not text-writable: {error}"
                    )),
                }
                match binary::encode_canonical(&decoded) {
                    Ok(reencoded) if reencoded == oracle_canonical => {}
                    Ok(reencoded) => failures.push(format!(
                        "[{id}] {form} decode/canonicalize changed {} to {}",
                        encode_hex(&oracle_canonical),
                        encode_hex(&reencoded)
                    )),
                    Err(error) => {
                        failures.push(format!("[{id}] {form} re-encode failed: {error}"));
                    }
                }
            }
        }
    }

    for case in data["binary"]["malformed"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let bytes = decode_hex(case["hex"].as_str().unwrap());
        let accepted = binary::decode(&bytes).is_ok();
        let expected = case["outcome"].as_str().unwrap() == "accept";
        if accepted != expected {
            failures.push(format!("[{id}] malformed decode acceptance mismatch"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} binary surface mismatch(es):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn cid_and_canonical_verification_match_reference() {
    let data = fixture();
    let mut failures = Vec::new();
    for case in data["cid"]["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let values =
            from_str_stream_with(case["input"].as_str().unwrap(), Options::default()).unwrap();
        let value = &values[0];
        let got = match cid(value) {
            Ok(got) => got,
            Err(error) => {
                failures.push(format!("[{id}] CID generation failed: {error}"));
                continue;
            }
        };
        let want = case["cid"].as_str().unwrap();
        if got != want {
            failures.push(format!("[{id}] CID {got} != {want}"));
        }
        match parse_cid(&got) {
            Ok(digest) if encode_hex(&digest) == case["digest_hex"].as_str().unwrap() => {}
            Ok(digest) => failures.push(format!(
                "[{id}] digest {} != {}",
                encode_hex(&digest),
                case["digest_hex"].as_str().unwrap()
            )),
            Err(error) => failures.push(format!("[{id}] CID parse failed: {error}")),
        }
        if verify_cid(&got, value) != case["verify"].as_bool().unwrap() {
            failures.push(format!("[{id}] CID verification mismatch"));
        }
    }
    for case in data["cid"]["invalid"].as_array().unwrap() {
        let accepted = parse_cid(case["input"].as_str().unwrap()).is_ok();
        let expected = case["outcome"].as_str().unwrap() == "accept";
        if accepted != expected {
            failures.push(format!(
                "[{}] invalid CID acceptance mismatch",
                case["id"].as_str().unwrap()
            ));
        }
    }
    for case in data["canonical_verification"].as_array().unwrap() {
        let got = verify_canonical(case["input"].as_str().unwrap());
        if got != case["expected"].as_bool().unwrap() {
            failures.push(format!(
                "[{}] canonical verification {got} != {}",
                case["id"].as_str().unwrap(),
                case["expected"].as_bool().unwrap()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} CID/canonical mismatch(es):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn profile_sensitive_text_matches_reference_exactly() {
    let data = fixture();
    let mut failures = Vec::new();
    for case in data["text_profiles"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let parsed = from_str_stream_with(
            case["input"].as_str().unwrap(),
            options_for(case["profile"].as_str().unwrap()),
        );
        match (case["outcome"].as_str().unwrap(), parsed) {
            ("accept", Ok(values)) => {
                if values.len() != case["count"].as_u64().unwrap() as usize {
                    failures.push(format!("[{id}] parsed value count mismatch"));
                }
                match writer::to_string_stream(&values) {
                    Ok(compact) if compact == case["compact"].as_str().unwrap() => {}
                    Ok(compact) => failures.push(format!(
                        "[{id}] compact text {compact:?} != {:?}",
                        case["compact"].as_str().unwrap()
                    )),
                    Err(error) => failures.push(format!("[{id}] compact write failed: {error}")),
                }
                match (
                    case["canonical"]["outcome"].as_str().unwrap(),
                    writer::to_string_stream_canonical(&values),
                ) {
                    ("accept", result) => match result {
                        Ok(canonical)
                            if canonical == case["canonical"]["text"].as_str().unwrap() => {}
                        Ok(canonical) => failures.push(format!(
                            "[{id}] canonical text {canonical:?} != {:?}",
                            case["canonical"]["text"].as_str().unwrap()
                        )),
                        Err(error) => {
                            failures.push(format!("[{id}] canonical write failed: {error}"));
                        }
                    },
                    ("error", Err(error))
                        if error.category().as_str()
                            == case["canonical"]["category"].as_str().unwrap()
                            && error.message == case["canonical"]["message"].as_str().unwrap() => {}
                    ("error", Err(error)) => failures.push(format!(
                        "[{id}] canonical error ({}, {:?}) != ({}, {:?})",
                        error.category().as_str(),
                        error.message,
                        case["canonical"]["category"].as_str().unwrap(),
                        case["canonical"]["message"].as_str().unwrap()
                    )),
                    ("error", Ok(canonical)) => failures.push(format!(
                        "[{id}] reference canonical writer errors, Rust wrote {canonical:?}"
                    )),
                    (other, _) => panic!("unknown canonical text outcome: {other}"),
                }
            }
            ("accept", Err(error)) => {
                failures.push(format!("[{id}] reference accepts, Rust errored: {error}"));
            }
            ("error", Err(error)) => {
                if error.category().as_str() != case["category"].as_str().unwrap()
                    || error.message != case["message"].as_str().unwrap()
                    || case
                        .get("offset")
                        .is_some_and(|value| value.as_u64().unwrap() as usize != error.offset)
                    || case
                        .get("end_offset")
                        .is_some_and(|value| value.as_u64().unwrap() as usize != error.end_offset)
                    || case
                        .get("line")
                        .is_some_and(|value| value.as_u64().unwrap() as u32 != error.line)
                    || case
                        .get("column")
                        .is_some_and(|value| value.as_u64().unwrap() as u32 != error.column)
                {
                    failures.push(format!(
                        "[{id}] error ({}, {:?}, {}..{}, {}:{}) != {}",
                        error.category().as_str(),
                        error.message,
                        error.offset,
                        error.end_offset,
                        error.line,
                        error.column,
                        case
                    ));
                }
            }
            ("error", Ok(values)) => failures.push(format!(
                "[{id}] reference errors, Rust parsed {:?}",
                writer::to_string_stream(&values)
            )),
            (other, _) => panic!("unknown text profile outcome: {other}"),
        }
    }
    assert!(
        failures.is_empty(),
        "{} profile-sensitive text mismatch(es):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn incremental_parser_matches_reference_prefix_atomicity_and_preprocessing() {
    let data = fixture();
    let mut failures = Vec::new();
    for case in data["incremental"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let mut iterator = iter_stream_with(
            case["input"].as_str().unwrap(),
            options_for(case["profile"].as_str().unwrap()),
        );
        let mut yielded = Vec::new();
        let mut terminal = json!({"outcome": "end"});
        loop {
            match iterator.next() {
                Some(Ok(value)) => match writer::to_string_canonical(&value) {
                    Ok(rendered) => yielded.push(rendered),
                    Err(error) => {
                        failures.push(format!("[{id}] yielded value is not writable: {error}"));
                        break;
                    }
                },
                Some(Err(error)) => {
                    terminal = json!({
                        "outcome": "error",
                        "category": error.category().as_str(),
                        "message": error.message,
                        "offset": error.offset,
                        "end_offset": error.end_offset,
                        "line": error.line,
                        "column": error.column,
                    });
                    break;
                }
                None => break,
            }
        }
        let expected_yielded = case["yielded"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        if yielded != expected_yielded {
            failures.push(format!(
                "[{id}] yielded {yielded:?} != {expected_yielded:?}"
            ));
        }
        if terminal != case["terminal"] {
            failures.push(format!(
                "[{id}] terminal {terminal} != {}",
                case["terminal"]
            ));
        }
        if case["terminal"]["outcome"] == "error" {
            let fused = iterator.next().is_none() && iterator.next().is_none();
            if fused != case["fused_after_error"].as_bool().unwrap() {
                failures.push(format!("[{id}] fused-after-error mismatch"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} incremental parser mismatch(es):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn migration_events_json(result: &serde_axon::MigrationResult) -> J {
    J::Array(
        result
            .events
            .iter()
            .map(|event| {
                json!({
                    "code": event.code,
                    "message": event.message,
                    "offset": event.offset,
                })
            })
            .collect(),
    )
}

fn migration_edits_json(result: &serde_axon::MigrationResult) -> J {
    J::Array(
        result
            .edits
            .iter()
            .map(|edit| {
                json!({
                    "code": edit.code,
                    "start": edit.start,
                    "end": edit.end,
                    "replacement": edit.replacement,
                })
            })
            .collect(),
    )
}

#[test]
fn migration_matches_reference_output_reports_and_edits() {
    let data = fixture();
    let mut failures = Vec::new();
    for case in data["migration"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let source = case["input"].as_str().unwrap();
        let graph = case["graph"].as_bool().unwrap();
        let selective = case["operation"].as_str().unwrap() == "selective";
        match (
            case["outcome"].as_str().unwrap(),
            migrate_with(source, graph, selective),
        ) {
            ("accept", Ok(result)) => {
                if result.text != case["output"].as_str().unwrap() {
                    failures.push(format!(
                        "[{id}] output {:?} != {:?}",
                        result.text,
                        case["output"].as_str().unwrap()
                    ));
                }
                if result.mode.as_str() != case["mode"].as_str().unwrap() {
                    failures.push(format!(
                        "[{id}] mode {} != {}",
                        result.mode,
                        case["mode"].as_str().unwrap()
                    ));
                }
                let events = migration_events_json(&result);
                if events != case["events"] {
                    failures.push(format!("[{id}] events {events} != {}", case["events"]));
                }
                let edits = migration_edits_json(&result);
                if edits != case["edits"] {
                    failures.push(format!("[{id}] edits {edits} != {}", case["edits"]));
                }
                if selective {
                    match apply_migration_edits(source, &result.edits) {
                        Ok(applied) if applied == result.text => {}
                        Ok(applied) => failures.push(format!(
                            "[{id}] edits produced {applied:?}, not {:?}",
                            result.text
                        )),
                        Err(error) => failures.push(format!("[{id}] edits failed: {error}")),
                    }
                } else if !result.edits.is_empty() {
                    failures.push(format!("[{id}] canonical migration returned edits"));
                }
            }
            ("accept", Err(error)) => {
                failures.push(format!("[{id}] reference accepts, Rust errored: {error}"));
            }
            ("error", Err(error)) => {
                if error.category().as_str() != case["category"].as_str().unwrap() {
                    failures.push(format!(
                        "[{id}] error {} != {}",
                        error.category().as_str(),
                        case["category"].as_str().unwrap()
                    ));
                }
                if error.message != case["message"].as_str().unwrap()
                    || error.offset != case["offset"].as_u64().unwrap() as usize
                    || error.end_offset != case["end_offset"].as_u64().unwrap() as usize
                    || error.line != case["line"].as_u64().unwrap() as u32
                    || error.column != case["column"].as_u64().unwrap() as u32
                {
                    failures.push(format!(
                        "[{id}] error detail ({:?}, {}..{}, {}:{}) != ({:?}, {}..{}, {}:{})",
                        error.message,
                        error.offset,
                        error.end_offset,
                        error.line,
                        error.column,
                        case["message"].as_str().unwrap(),
                        case["offset"].as_u64().unwrap(),
                        case["end_offset"].as_u64().unwrap(),
                        case["line"].as_u64().unwrap(),
                        case["column"].as_u64().unwrap(),
                    ));
                }
            }
            ("error", Ok(result)) => failures.push(format!(
                "[{id}] reference errors, Rust produced {:?}",
                result.text
            )),
            (other, _) => panic!("unknown migration outcome: {other}"),
        }
    }
    assert!(
        failures.is_empty(),
        "{} migration mismatch(es):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn diagnostics_json(items: &[LspDiagnostic]) -> J {
    J::Array(
        items
            .iter()
            .map(|item| {
                json!({
                    "code": item.code,
                    "message": item.message,
                    "start_byte": item.start_byte,
                    "end_byte": item.end_byte,
                    "line": item.line,
                    "character": item.character,
                    "expected": item.expected,
                    "fixes": item.fixes.iter().map(|fix| json!({
                        "title": fix.title,
                        "start_byte": fix.start_byte,
                        "end_byte": fix.end_byte,
                        "new_text": fix.new_text,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

fn hover_json(item: Option<&LspHover>) -> J {
    match item {
        None => J::Null,
        Some(item) => json!({
            "kind": item.kind,
            "text": item.text,
            "start_byte": item.start_byte,
            "end_byte": item.end_byte,
            "line": item.line,
            "character": item.character,
            "trivia": item.trivia,
        }),
    }
}

fn edits_json(items: &[LspTextEdit]) -> J {
    J::Array(
        items
            .iter()
            .map(|item| {
                json!({
                    "start_byte": item.start_byte,
                    "end_byte": item.end_byte,
                    "new_text": item.new_text,
                })
            })
            .collect(),
    )
}

fn path_value_json(value: &Value) -> J {
    match value {
        Value::Null => J::Null,
        Value::Bool(value) => J::Bool(*value),
        Value::Int(text) => text
            .parse::<i64>()
            .ok()
            .map_or_else(|| J::String(text.clone()), |value| json!(value)),
        Value::String(text) => J::String(text.clone()),
        other => J::String(
            writer::to_string_canonical(other).expect("symbol paths contain writable values"),
        ),
    }
}

fn symbols_json(items: &[DocumentSymbol]) -> J {
    J::Array(
        items
            .iter()
            .map(|item| {
                let path: Vec<J> = item
                    .path
                    .iter()
                    .map(|segment| match segment {
                        SymbolPathSegment::Index(index) => json!(index),
                        SymbolPathSegment::Key(value) => path_value_json(value),
                    })
                    .collect();
                json!({"name": item.name, "kind": item.kind, "path": path})
            })
            .collect(),
    )
}

fn scientific_typed_hex(array: &ScientificArray, tag: &str) -> Result<String, serde_axon::Error> {
    let mut bytes = Vec::new();
    macro_rules! native_bytes {
        ($values:expr) => {
            for value in $values? {
                bytes.extend_from_slice(&value.to_ne_bytes());
            }
        };
    }
    match tag {
        "u8" => bytes.extend(array.to_u8_vec()?),
        "u16" => native_bytes!(array.to_u16_vec()),
        "u32" => native_bytes!(array.to_u32_vec()),
        "u64" => native_bytes!(array.to_u64_vec()),
        "i8" => native_bytes!(array.to_i8_vec()),
        "i16" => native_bytes!(array.to_i16_vec()),
        "i32" => native_bytes!(array.to_i32_vec()),
        "i64" => native_bytes!(array.to_i64_vec()),
        "f16" => native_bytes!(array.to_f16_bits_vec()),
        "f32" => native_bytes!(array.to_f32_vec()),
        "f64" => native_bytes!(array.to_f64_vec()),
        "c64" => {
            for (real, imaginary) in array.to_c64_components_vec()? {
                bytes.extend_from_slice(&real.to_ne_bytes());
                bytes.extend_from_slice(&imaginary.to_ne_bytes());
            }
        }
        "c128" => {
            for (real, imaginary) in array.to_c128_components_vec()? {
                bytes.extend_from_slice(&real.to_ne_bytes());
                bytes.extend_from_slice(&imaginary.to_ne_bytes());
            }
        }
        other => panic!("unknown scientific dtype: {other}"),
    }
    Ok(encode_hex(&bytes))
}

fn reconstruct_scientific_array(
    array: &ScientificArray,
    tag: &str,
    shape: Vec<usize>,
    order: ArrayOrder,
) -> Result<ScientificArray, serde_axon::Error> {
    match tag {
        "u8" => ScientificArray::from_u8_vec(shape, array.to_u8_vec()?, order),
        "u16" => ScientificArray::from_u16_vec(shape, array.to_u16_vec()?, order),
        "u32" => ScientificArray::from_u32_vec(shape, array.to_u32_vec()?, order),
        "u64" => ScientificArray::from_u64_vec(shape, array.to_u64_vec()?, order),
        "i8" => ScientificArray::from_i8_vec(shape, array.to_i8_vec()?, order),
        "i16" => ScientificArray::from_i16_vec(shape, array.to_i16_vec()?, order),
        "i32" => ScientificArray::from_i32_vec(shape, array.to_i32_vec()?, order),
        "i64" => ScientificArray::from_i64_vec(shape, array.to_i64_vec()?, order),
        "f16" => ScientificArray::from_f16_bits_vec(shape, array.to_f16_bits_vec()?, order),
        "f32" => ScientificArray::from_f32_vec(shape, array.to_f32_vec()?, order),
        "f64" => ScientificArray::from_f64_vec(shape, array.to_f64_vec()?, order),
        "c64" => {
            ScientificArray::from_c64_components_vec(shape, array.to_c64_components_vec()?, order)
        }
        "c128" => {
            ScientificArray::from_c128_components_vec(shape, array.to_c128_components_vec()?, order)
        }
        other => panic!("unknown scientific dtype: {other}"),
    }
}

fn scientific_python_exception_class(error: &serde_axon::Error) -> &'static str {
    match error.category() {
        serde_axon::Category::Serde => "TypeError",
        _ => "ValueError",
    }
}

#[test]
fn lsp_helpers_match_reference_shapes_and_byte_offsets() {
    let data = fixture();
    let mut failures = Vec::new();
    for case in data["lsp"]["diagnostics"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let options = options_for(case["profile"].as_str().unwrap());
        match diagnostics(case["input"].as_str().unwrap(), &options) {
            Ok(items) if diagnostics_json(&items) == case["result"] => {}
            Ok(items) => failures.push(format!(
                "[{id}] diagnostics {} != {}",
                diagnostics_json(&items),
                case["result"]
            )),
            Err(error) => failures.push(format!("[{id}] diagnostics failed: {error}")),
        }
    }
    for case in data["lsp"]["hover"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let options = options_for(case["profile"].as_str().unwrap());
        match hover(
            case["input"].as_str().unwrap(),
            case["byte_offset"].as_u64().unwrap() as usize,
            &options,
        ) {
            Ok(item) if hover_json(item.as_ref()) == case["result"] => {}
            Ok(item) => failures.push(format!(
                "[{id}] hover {} != {}",
                hover_json(item.as_ref()),
                case["result"]
            )),
            Err(error) => failures.push(format!("[{id}] hover failed: {error}")),
        }
    }
    for case in data["lsp"]["formatting"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let options = options_for(case["profile"].as_str().unwrap());
        match formatting_edits(
            case["input"].as_str().unwrap(),
            case["indent"].as_u64().unwrap() as usize,
            &options,
        ) {
            Ok(items) if edits_json(&items) == case["result"] => {}
            Ok(items) => failures.push(format!(
                "[{id}] formatting {} != {}",
                edits_json(&items),
                case["result"]
            )),
            Err(error) => failures.push(format!("[{id}] formatting failed: {error}")),
        }
    }
    for case in data["lsp"]["symbols"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let options = options_for(case["profile"].as_str().unwrap());
        match document_symbols(case["input"].as_str().unwrap(), &options) {
            Ok(items) if symbols_json(&items) == case["result"] => {}
            Ok(items) => failures.push(format!(
                "[{id}] symbols {} != {}",
                symbols_json(&items),
                case["result"]
            )),
            Err(error) => failures.push(format!("[{id}] symbols failed: {error}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} LSP mismatch(es):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn scientific_metadata_and_layouts_match_reference() {
    let data = fixture();
    let mut failures = Vec::new();
    for case in data["scientific"]["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let values =
            match from_str_stream_with(case["source"].as_str().unwrap(), Options::default()) {
                Ok(values) => values,
                Err(error) => {
                    failures.push(format!("[{id}] parse failed: {error}"));
                    continue;
                }
            };
        let array = match ScientificArray::from_value(values[0].clone()) {
            Ok(array) => array,
            Err(error) => {
                failures.push(format!("[{id}] Array conversion failed: {error}"));
                continue;
            }
        };
        if let Err(error) = array.validate_layout() {
            failures.push(format!("[{id}] layout rejected: {error}"));
            continue;
        }
        let dtype = ArrayDtype::from_tag(case["tag"].as_str().unwrap()).unwrap();
        if array.dtype() != Some(dtype)
            || dtype.element_size() != case["element_size"].as_u64().unwrap() as usize
            || array.try_shape().unwrap()
                != case["shape"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|item| item.as_u64().unwrap() as usize)
                    .collect::<Vec<_>>()
            || array.order() != case["order"].as_str().unwrap()
            || encode_hex(array.data().unwrap()) != case["data_hex"].as_str().unwrap()
        {
            failures.push(format!("[{id}] scientific metadata/data mismatch"));
        }
        match scientific_typed_hex(&array, case["tag"].as_str().unwrap()) {
            Ok(hex) if hex == case["roundtrip_data_hex"].as_str().unwrap() => {}
            Ok(hex) => failures.push(format!(
                "[{id}] typed extraction bytes {hex} != {}",
                case["roundtrip_data_hex"].as_str().unwrap()
            )),
            Err(error) => failures.push(format!("[{id}] typed extraction failed: {error}")),
        }
        let shape = case["shape"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_u64().unwrap() as usize)
            .collect::<Vec<_>>();
        let order = if case["order"] == "F" {
            ArrayOrder::F
        } else {
            ArrayOrder::C
        };
        match reconstruct_scientific_array(&array, case["tag"].as_str().unwrap(), shape, order)
            .and_then(|rebuilt| writer::to_string_canonical(&rebuilt.into_value()))
        {
            Ok(source) if source == case["source"].as_str().unwrap() => {}
            Ok(source) => failures.push(format!(
                "[{id}] typed constructor source {source:?} != {:?}",
                case["source"].as_str().unwrap()
            )),
            Err(error) => failures.push(format!("[{id}] typed constructor failed: {error}")),
        }
    }
    for case in data["scientific"]["edge"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let value = from_str_stream_with(case["source"].as_str().unwrap(), Options::default())
            .unwrap()
            .remove(0);
        let layout = ScientificArray::from_value(value).and_then(|array| {
            let view = array.numpy_layout()?;
            Ok((
                view.dtype().tag().to_string(),
                view.shape().to_vec(),
                view.order(),
                encode_hex(view.data()),
            ))
        });
        match (case["outcome"].as_str().unwrap(), layout) {
            ("accept", Ok((dtype, shape, order, bytes))) => {
                let expected_shape = case["shape"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_u64().unwrap() as usize)
                    .collect::<Vec<_>>();
                let expected_order = if case["order"] == "F" {
                    ArrayOrder::F
                } else {
                    ArrayOrder::C
                };
                if dtype != case["dtype"].as_str().unwrap()
                    || shape != expected_shape
                    || order != expected_order
                    || bytes != case["data_hex"].as_str().unwrap()
                {
                    failures.push(format!("[{id}] numpy-compatible layout mismatch"));
                }
            }
            ("accept", Err(error)) => {
                failures.push(format!("[{id}] reference accepts, Rust errored: {error}"));
            }
            ("error", Ok(_)) => failures.push(format!("[{id}] reference rejects, Rust accepts")),
            ("error", Err(_)) => {}
            (other, _) => panic!("unknown scientific edge outcome: {other}"),
        }
    }
    for case in data["scientific"]["invalid"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let value = from_str_stream_with(case["source"].as_str().unwrap(), Options::default())
            .unwrap()
            .remove(0);
        let result = ScientificArray::from_value(value).and_then(|array| {
            array.numpy_layout()?;
            Ok(())
        });
        match (case["outcome"].as_str().unwrap(), result) {
            ("accept", Ok(())) => {}
            ("accept", Err(error)) => {
                failures.push(format!("[{id}] reference accepts, Rust errored: {error}"));
            }
            ("error", Ok(())) => failures.push(format!("[{id}] reference rejects, Rust accepts")),
            ("error", Err(error)) => {
                if scientific_python_exception_class(&error) != case["category"].as_str().unwrap()
                    || error.message != case["message"].as_str().unwrap()
                {
                    failures.push(format!(
                        "[{id}] error ({}, {:?}) != ({}, {:?})",
                        scientific_python_exception_class(&error),
                        error.message,
                        case["category"].as_str().unwrap(),
                        case["message"].as_str().unwrap(),
                    ));
                }
            }
            (other, _) => panic!("unknown scientific invalid outcome: {other}"),
        }
    }
    assert!(
        failures.is_empty(),
        "{} scientific mismatch(es):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn stream_error_code(error: &serde_axon::Error) -> &str {
    if error.message.contains("already open") {
        "nested-start"
    } else if error.message.contains("without a prior") {
        "end-without-start"
    } else if error.message.contains("count mismatch") {
        "count-mismatch"
    } else if error.message.contains("outside of a stream") {
        "outside-envelope"
    } else if error.message.contains("ended abruptly") {
        "abrupt-end"
    } else {
        error.category().as_str()
    }
}

#[test]
fn envelope_stream_matches_reference_prefix_terminal_and_fusing() {
    let data = fixture();
    let mut failures = Vec::new();
    for case in data["stream"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let mut stream = EnvelopeStream::new(iter_stream(case["input"].as_str().unwrap()));
        let mut yielded = Vec::new();
        let mut terminal = json!({"outcome": "end"});
        loop {
            match stream.next() {
                Some(Ok(value)) => match writer::to_string_canonical(&value) {
                    Ok(rendered) => yielded.push(rendered),
                    Err(error) => {
                        failures.push(format!("[{id}] yielded value is not writable: {error}"));
                        break;
                    }
                },
                Some(Err(error)) => {
                    terminal = json!({
                        "outcome": "error",
                        "code": stream_error_code(&error),
                        "message": error.message,
                    });
                    break;
                }
                None => break,
            }
        }
        if yielded
            != case["yielded"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        {
            failures.push(format!("[{id}] yielded prefix mismatch"));
        }
        if terminal != case["terminal"] {
            failures.push(format!(
                "[{id}] terminal {terminal} != {}",
                case["terminal"]
            ));
        }
        if terminal["outcome"] == "error" {
            let fused = stream.next().is_none() && stream.next().is_none();
            if fused != case["fused_after_error"].as_bool().unwrap() {
                failures.push(format!("[{id}] fused-after-error mismatch"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} stream mismatch(es):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
