//! Direct executor for every operation in the normative AXON 2026 registry and
//! the complete RFC 8785 Appendix B binary64 corpus.

use serde_axon::cst::parse_cst;
use serde_axon::migration::{apply_migration_edits, migrate, migrate_selective};
use serde_axon::parser::Options;
use serde_axon::schema::{
    PathSegment, SchemaRegistry2026, ValidationOptions, validate2026, validation_report2026_with,
};
use serde_axon::{
    Category, GraphDocument, GraphObject, GraphValue, Value, from_slice_stream,
    from_str_stream_with, iter_stream_with, verify_canonical, writer,
};
use serde_json::{Value as Json, json};

fn fixture() -> Json {
    serde_json::from_str(include_str!("normative.json")).expect("normative.json parses")
}

fn options_from(vector: &Json) -> Options {
    let raw = vector
        .get("options")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let mut options = if raw.get("compat").and_then(Json::as_bool) == Some(true) {
        Options::compat()
    } else {
        Options::default()
    };
    for (name, value) in raw {
        let boolean = || {
            value
                .as_bool()
                .unwrap_or_else(|| panic!("{name} must be bool"))
        };
        let limit = || {
            value
                .as_u64()
                .unwrap_or_else(|| panic!("{name} must be integer")) as usize
        };
        match name.as_str() {
            "compat" => {}
            "graph" => options.graph = boolean(),
            "doc_link" => options.doc_link = boolean(),
            "scale_significant" => options.scale_significant = boolean(),
            "canonical_verify" => options.canonical_verify = boolean(),
            "compatibility" => options.compatibility = boolean(),
            "bare_temporals" => options.bare_temporals = boolean(),
            "raw_fraction" => options.raw_fraction = boolean(),
            "legacy_binding" => options.legacy_binding = boolean(),
            "indented_bodies" => options.indented_bodies = boolean(),
            "lax_numbers" => options.lax_numbers = boolean(),
            "lax_temporals" => options.lax_temporals = boolean(),
            "legacy_escapes" => options.legacy_escapes = boolean(),
            "legacy_binary" => options.legacy_binary = boolean(),
            "hash_underscore_comment" => options.hash_underscore_comment = boolean(),
            "last_wins" => options.last_wins = boolean(),
            "set_dedup" => options.duplicate_set_dedup = boolean(),
            "undefined_ref_sentinel" => options.undefined_ref_sentinel = boolean(),
            "python_name_classes" => options.python_name_classes = boolean(),
            "lax_strings" => options.lax_strings = boolean(),
            "allow_header_profiles" => options.allow_header_profiles = boolean(),
            "tz_names" => options.tz_names = boolean(),
            "numeric_separators" => options.numeric_separators = boolean(),
            "max_depth" => options.limits.max_depth = limit() as u32,
            "max_values" => options.limits.max_values = limit(),
            "max_string" => options.limits.max_string = limit(),
            "max_binary" => options.limits.max_binary = limit(),
            "max_number" => options.limits.max_number = limit(),
            "max_anchors" => options.limits.max_anchors = limit(),
            "max_references" => options.limits.max_references = limit(),
            "max_label" => options.limits.max_label = limit(),
            "max_bare_name" => options.limits.max_bare_name = limit(),
            "max_quoted_name" => options.limits.max_quoted_name = limit(),
            "max_temporal" => options.limits.max_temporal = limit(),
            "max_container" => options.limits.max_container = limit(),
            "max_header_require" => options.limits.max_header_require = limit(),
            "max_cst_tokens" => options.limits.max_cst_tokens = limit(),
            "max_input" => options.limits.max_input = limit(),
            other => panic!("unhandled normative option {other}"),
        }
    }
    options
}

fn json_to_axon(value: &Json) -> Value {
    match value {
        Json::Null => Value::Null,
        Json::Bool(value) => Value::Bool(*value),
        Json::Number(value) => {
            if value.is_i64() || value.is_u64() {
                Value::Int(value.to_string())
            } else {
                Value::Float(value.as_f64().expect("finite JSON float"))
            }
        }
        Json::String(value) => Value::String(value.clone()),
        Json::Array(values) => Value::List(values.iter().map(json_to_axon).collect()),
        Json::Object(values) => Value::Map(
            values
                .iter()
                .map(|(key, value)| (Value::String(key.clone()), json_to_axon(value)))
                .collect(),
        ),
    }
}

fn canonical_stream(values: &[Value]) -> String {
    writer::to_string_stream_canonical(values).expect("parsed normative values are writable")
}

fn category_result(
    id: &str,
    expected: &str,
    result: serde_axon::Result<Vec<Value>>,
    failures: &mut Vec<String>,
) {
    match result {
        Err(error) if error.category().as_str() == expected => {}
        Err(error) => failures.push(format!(
            "[{id}] error category {} != {expected}",
            error.category().as_str()
        )),
        Ok(values) => failures.push(format!(
            "[{id}] expected {expected}, accepted {}",
            canonical_stream(&values)
        )),
    }
}

fn path_json(path: &[PathSegment]) -> Json {
    Json::Array(
        path.iter()
            .map(|segment| match segment {
                PathSegment::Index(index) => json!(index),
                PathSegment::Key(Value::String(key)) => json!(key),
                PathSegment::Key(Value::Int(value)) => value
                    .parse::<i64>()
                    .map_or_else(|_| json!(value), |value| json!(value)),
                PathSegment::Key(value) => json!(
                    writer::to_string_canonical(value)
                        .expect("schema issue paths contain writable values")
                ),
            })
            .collect(),
    )
}

fn mutate_minimum(value: &mut Value) -> bool {
    let pairs = match value {
        Value::Map(pairs) | Value::OrderedMap(pairs) => pairs,
        _ => return false,
    };
    let Some((_, minimum)) = pairs
        .iter_mut()
        .find(|(key, _)| matches!(key, Value::String(key) if key == "minimum"))
    else {
        return false;
    };
    *minimum = Value::Int("-1".to_string());
    true
}

fn execute_load(vector: &Json, failures: &mut Vec<String>) {
    let id = vector["id"].as_str().unwrap();
    let input = vector["input"].as_str().unwrap();
    let options = options_from(vector);
    let expected = &vector["expect"];
    let result = from_str_stream_with(input, options);
    if let Some(category) = expected.get("error").and_then(Json::as_str) {
        category_result(id, category, result, failures);
        return;
    }
    let values = match result {
        Ok(values) => values,
        Err(error) => {
            failures.push(format!("[{id}] rejected with {error}"));
            return;
        }
    };
    if let Some(count) = expected.get("count").and_then(Json::as_u64)
        && values.len() != count as usize
    {
        failures.push(format!("[{id}] count {} != {count}", values.len()));
    }
    if let Some(canonical) = expected.get("canonical").and_then(Json::as_str) {
        let got = canonical_stream(&values);
        if got != canonical {
            failures.push(format!("[{id}] canonical {got:?} != {canonical:?}"));
        }
    }
    if expected.get("undefined").and_then(Json::as_bool) == Some(true)
        && values.as_slice() != [Value::Undefined]
    {
        failures.push(format!(
            "[{id}] expected the UNDEFINED sentinel, got {values:?}"
        ));
    }
    if expected.get("shared").and_then(Json::as_bool) == Some(true) {
        match GraphDocument::from_values(&values) {
            Ok(graph)
                if graph.roots().len() >= 2
                    && GraphDocument::same_identity(&graph.roots()[0], &graph.roots()[1]) => {}
            Ok(graph) => failures.push(format!(
                "[{id}] roots do not retain shared identity: {:?}",
                graph.roots()
            )),
            Err(error) => failures.push(format!("[{id}] graph resolution failed: {error}")),
        }
    }
    if expected.get("self_cycle").and_then(Json::as_bool) == Some(true) {
        match GraphDocument::from_values(&values) {
            Ok(graph) => {
                let cyclic = graph.roots().first().is_some_and(|root| {
                    let GraphValue::Object(root_id) = root else {
                        return false;
                    };
                    matches!(
                        graph.object(*root_id),
                        Some(GraphObject::List(items))
                            if items.first() == Some(&GraphValue::Object(*root_id))
                    )
                });
                if !cyclic {
                    failures.push(format!("[{id}] resolved root is not self-cyclic"));
                }
            }
            Err(error) => failures.push(format!("[{id}] graph resolution failed: {error}")),
        }
    }
}

fn execute_cst(vector: &Json, failures: &mut Vec<String>) {
    let id = vector["id"].as_str().unwrap();
    let source = vector["input"].as_str().unwrap();
    let options = options_from(vector);
    let expected = &vector["expect"];
    let document = match parse_cst(source, &options) {
        Ok(document) => document,
        Err(error) => {
            failures.push(format!("[{id}] CST returned hard error {error}"));
            return;
        }
    };
    if expected.get("render_identical").and_then(Json::as_bool) == Some(true)
        && document.render() != source
    {
        failures.push(format!("[{id}] CST render is not lossless"));
    }
    if let Some(count) = expected.get("trace_count").and_then(Json::as_u64)
        && document.semantic_traces.len() != count as usize
    {
        failures.push(format!(
            "[{id}] trace count {} != {count}",
            document.semantic_traces.len()
        ));
    }
    if let Some(count) = expected.get("value_count").and_then(Json::as_u64)
        && document.values.len() != count as usize
    {
        failures.push(format!(
            "[{id}] value count {} != {count}",
            document.values.len()
        ));
    }
    if let Some(texts) = expected.get("token_texts") {
        let got: Vec<_> = document
            .tokens
            .iter()
            .map(|token| token.text.as_str())
            .collect();
        let want: Vec<_> = texts
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        if got != want {
            failures.push(format!("[{id}] CST token texts differ"));
        }
    }
    let Some(category) = expected.get("diagnostic").and_then(Json::as_str) else {
        if !document.diagnostics.is_empty() {
            failures.push(format!("[{id}] unexpected CST diagnostics"));
        }
        return;
    };
    let candidates: Vec<_> = document
        .diagnostics
        .iter()
        .filter(|item| item.category == category)
        .collect();
    if candidates.is_empty() {
        failures.push(format!("[{id}] missing CST diagnostic {category}"));
        return;
    }
    if let Some(start) = expected.get("start_byte").and_then(Json::as_u64)
        && !candidates
            .iter()
            .any(|item| item.span.start == start as usize)
    {
        failures.push(format!(
            "[{id}] CST diagnostic does not start at byte {start}"
        ));
    }
    let candidate = if let Some(expected_tokens) = expected.get("expected") {
        let want: Vec<_> = expected_tokens
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap())
            .collect();
        match candidates.iter().find(|item| {
            item.expected
                .iter()
                .map(String::as_str)
                .eq(want.iter().copied())
        }) {
            Some(candidate) => *candidate,
            None => {
                failures.push(format!("[{id}] CST expected-token set differs"));
                candidates[0]
            }
        }
    } else {
        candidates[0]
    };
    if let Some(fixed) = expected.get("fixed").and_then(Json::as_str) {
        let Some(fix) = candidate.fixes.first() else {
            failures.push(format!("[{id}] missing CST fix"));
            return;
        };
        match fix.apply(&document, &Options::default()) {
            Ok(result) if result.render() == fixed && result.diagnostics.is_empty() => {}
            Ok(result) => failures.push(format!(
                "[{id}] CST fix produced {:?} with {} diagnostics",
                result.render(),
                result.diagnostics.len()
            )),
            Err(error) => failures.push(format!("[{id}] CST fix failed: {error}")),
        }
    }
}

fn execute_schema(vector: &Json, failures: &mut Vec<String>) {
    let id = vector["id"].as_str().unwrap();
    let value = json_to_axon(&vector["value"]);
    let schema = json_to_axon(&vector["schema"]);
    let options = ValidationOptions {
        max_issues: vector
            .get("max_issues")
            .and_then(Json::as_u64)
            .map_or(256, |value| value as usize),
        ..ValidationOptions::default()
    };
    let report = validation_report2026_with(&value, &schema, None, options);
    let got: Vec<_> = report
        .issues
        .iter()
        .map(|issue| issue.code.as_str())
        .collect();
    let want: Vec<_> = vector["expect"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    if got != want {
        failures.push(format!("[{id}] schema issues {got:?} != {want:?}"));
    }
}

fn execute_vector(vector: &Json, failures: &mut Vec<String>) {
    let id = vector["id"].as_str().unwrap();
    match vector["operation"].as_str().unwrap() {
        "invalid-unicode" => match from_slice_stream(&[0xff]) {
            Err(error) if error.category() == Category::InvalidUnicode && error.offset == 0 => {}
            Err(error) => failures.push(format!(
                "[{id}] malformed UTF-8 classified as {} at byte {}",
                error.category().as_str(),
                error.offset
            )),
            Ok(values) => failures.push(format!(
                "[{id}] malformed UTF-8 unexpectedly parsed as {values:?}"
            )),
        },
        "load" => execute_load(vector, failures),
        "semantic-equivalent" => {
            let expected = vector["expect"]["canonical"].as_str().unwrap();
            for source in vector["inputs"].as_array().unwrap() {
                match from_str_stream_with(source.as_str().unwrap(), Options::default()) {
                    Ok(values) if canonical_stream(&values) == expected => {}
                    Ok(values) => failures.push(format!(
                        "[{id}] semantic equivalent rendered {:?}",
                        canonical_stream(&values)
                    )),
                    Err(error) => failures.push(format!("[{id}] parse failed: {error}")),
                }
            }
        }
        "incremental" => {
            let mut iterator =
                iter_stream_with(vector["input"].as_str().unwrap(), options_from(vector));
            match iterator.next() {
                Some(Ok(value))
                    if writer::to_string_canonical(&value).as_deref()
                        == Ok(vector["expect"]["first"].as_str().unwrap()) => {}
                other => failures.push(format!("[{id}] wrong incremental prefix: {other:?}")),
            }
            match iterator.next() {
                Some(Err(error))
                    if error.category().as_str()
                        == vector["expect"]["later_error"].as_str().unwrap() => {}
                other => failures.push(format!("[{id}] wrong incremental terminal: {other:?}")),
            }
        }
        "canonical" => {
            match from_str_stream_with(vector["input"].as_str().unwrap(), Options::default()) {
                Ok(values) if values.len() == 1 => {
                    let got = writer::to_string_canonical(&values[0])
                        .expect("parsed canonical vector is writable");
                    let want = vector["expect"]["canonical"].as_str().unwrap();
                    if got != want {
                        failures.push(format!("[{id}] canonical {got:?} != {want:?}"));
                    }
                }
                Ok(values) => {
                    failures.push(format!("[{id}] expected one value, got {}", values.len()))
                }
                Err(error) => failures.push(format!("[{id}] parse failed: {error}")),
            }
        }
        "canonical-distinct" => {
            let rendered: Vec<_> = vector["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|source| {
                    let values = from_str_stream_with(source.as_str().unwrap(), Options::default())
                        .expect("normative canonical-distinct source parses");
                    writer::to_string_canonical(&values[0])
                        .expect("parsed canonical-distinct vector is writable")
                })
                .collect();
            if rendered[0] == rendered[1] {
                failures.push(format!(
                    "[{id}] distinct values have identical canonical bytes"
                ));
            }
        }
        "canonical-equivalent" => {
            let options = options_from(vector);
            let mut rendered = Vec::new();
            for source in vector["inputs"].as_array().unwrap() {
                match from_str_stream_with(source.as_str().unwrap(), options) {
                    Ok(values) => rendered.push(canonical_stream(&values)),
                    Err(error) => failures.push(format!("[{id}] parse failed: {error}")),
                }
            }
            if rendered.windows(2).any(|pair| pair[0] != pair[1]) {
                failures.push(format!(
                    "[{id}] canonical graph equivalents differ: {rendered:?}"
                ));
            }
        }
        "writer-set-duplicate" => {
            let values = vector["values"]
                .as_array()
                .unwrap()
                .iter()
                .map(json_to_axon)
                .collect();
            match writer::to_string_canonical(&Value::Set(values)) {
                Err(error)
                    if error.category().as_str() == vector["expect"]["error"].as_str().unwrap() => {
                }
                Err(error) => failures.push(format!(
                    "[{id}] writer error {} != {}",
                    error.category().as_str(),
                    vector["expect"]["error"].as_str().unwrap()
                )),
                Ok(rendered) => failures.push(format!(
                    "[{id}] duplicate-set writer unexpectedly emitted {rendered:?}"
                )),
            }
        }
        "verify-canonical" => {
            let got = verify_canonical(vector["input"].as_str().unwrap());
            let want = vector["expect"]["valid"].as_bool().unwrap();
            if got != want {
                failures.push(format!("[{id}] verify-canonical {got} != {want}"));
            }
        }
        "cst" => execute_cst(vector, failures),
        "cst-error" => match parse_cst(vector["input"].as_str().unwrap(), &options_from(vector)) {
            Err(error)
                if error.category().as_str() == vector["expect"]["error"].as_str().unwrap() => {}
            Err(error) => failures.push(format!("[{id}] CST error category differs: {error}")),
            Ok(_) => failures.push(format!("[{id}] expected a hard CST error")),
        },
        "schema" => execute_schema(vector, failures),
        "schema-order" => {
            let value = json_to_axon(&vector["value"]);
            let schema = json_to_axon(&vector["schema"]);
            let got = Json::Array(
                validate2026(&value, &schema)
                    .iter()
                    .map(|issue| path_json(&issue.path))
                    .collect(),
            );
            if got != vector["expect"]["paths"] {
                failures.push(format!(
                    "[{id}] schema paths {got} != {}",
                    vector["expect"]["paths"]
                ));
            }
        }
        "schema-registry" => {
            let mut registry = SchemaRegistry2026::new();
            let module = json_to_axon(&vector["module"]);
            if let Err(error) = registry.register(vector["identifier"].as_str().unwrap(), &module) {
                failures.push(format!("[{id}] registry setup failed: {error}"));
                return;
            }
            registry.freeze();
            let value = json_to_axon(&vector["value"]);
            let schema = json_to_axon(&vector["schema"]);
            let report = validation_report2026_with(
                &value,
                &schema,
                Some(&registry),
                ValidationOptions::default(),
            );
            let got: Vec<_> = report
                .issues
                .iter()
                .map(|issue| issue.code.as_str())
                .collect();
            let want: Vec<_> = vector["expect"]["issues"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect();
            if got != want {
                failures.push(format!("[{id}] schema registry issues {got:?} != {want:?}"));
            }
        }
        "schema-registry-governance" => {
            let mut registry = SchemaRegistry2026::new();
            let module = json_to_axon(&vector["module"]);
            let mut returned =
                match registry.register(vector["identifier"].as_str().unwrap(), &module) {
                    Ok(returned) => returned,
                    Err(error) => {
                        failures.push(format!("[{id}] registry setup failed: {error}"));
                        return;
                    }
                };
            registry.freeze();
            if !mutate_minimum(&mut returned) {
                failures.push(format!("[{id}] could not mutate returned registry clone"));
            }
            let schema = Value::Map(vec![(
                Value::String("$ref".to_string()),
                Value::String(vector["identifier"].as_str().unwrap().to_string()),
            )]);
            let report = validation_report2026_with(
                &Value::Int("0".to_string()),
                &schema,
                Some(&registry),
                ValidationOptions::default(),
            );
            let got: Vec<_> = report
                .issues
                .iter()
                .map(|issue| issue.code.as_str())
                .collect();
            let want: Vec<_> = vector["expect"]["issues"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect();
            if got != want
                || registry.frozen() != vector["expect"]["frozen"].as_bool().unwrap()
                || registry.network_resolution()
                    != vector["expect"]["network_resolution"].as_bool().unwrap()
            {
                failures.push(format!("[{id}] registry governance state/issues differ"));
            }
            if registry
                .register("urn:axon:test:late", &Value::Map(Vec::new()))
                .is_ok()
            {
                failures.push(format!("[{id}] frozen registry accepted a late module"));
            }
        }
        "migrate" | "migrate-selective" => {
            let source = vector["input"].as_str().unwrap();
            let selective = vector["operation"] == "migrate-selective";
            let result = if selective {
                migrate_selective(source)
            } else {
                migrate(source)
            };
            match result {
                Ok(result) => {
                    let want = &vector["expect"];
                    if result.text != want["output"].as_str().unwrap()
                        || want
                            .get("mode")
                            .and_then(Json::as_str)
                            .is_some_and(|mode| result.mode.as_str() != mode)
                    {
                        failures.push(format!("[{id}] migration output/mode differs"));
                    }
                    if want.get("edits_reproduce").and_then(Json::as_bool) == Some(true)
                        && apply_migration_edits(source, &result.edits).ok().as_deref()
                            != Some(result.text.as_str())
                    {
                        failures.push(format!("[{id}] migration edits do not reproduce output"));
                    }
                }
                Err(error) => failures.push(format!("[{id}] migration failed: {error}")),
            }
        }
        other => panic!("unhandled normative operation {other} for {id}"),
    }
}

fn execute_rfc8785(data: &Json, failures: &mut Vec<String>) {
    for case in data["cases"].as_array().unwrap() {
        let bits = u64::from_str_radix(case["bits"].as_str().unwrap(), 16).unwrap();
        let value = f64::from_bits(bits);
        let rendered = match writer::to_string_canonical(&Value::Float(value)) {
            Ok(rendered) => rendered,
            Err(error) => {
                failures.push(format!(
                    "[RFC8785/{}] writing failed: {error}",
                    case["bits"].as_str().unwrap()
                ));
                continue;
            }
        };
        let want = case["axon"].as_str().unwrap();
        if rendered != want {
            failures.push(format!(
                "[RFC8785/{}] {rendered:?} != {want:?}",
                case["bits"].as_str().unwrap()
            ));
            continue;
        }
        match from_str_stream_with(&rendered, Options::default()) {
            Ok(values) if values.len() == 1 => match &values[0] {
                Value::Float(parsed) if value.is_nan() && parsed.is_nan() => {}
                Value::Float(parsed) if parsed.to_bits() == bits => {}
                parsed => failures.push(format!(
                    "[RFC8785/{}] parsed bits/value differ: {parsed:?}",
                    case["bits"].as_str().unwrap()
                )),
            },
            Ok(values) => failures.push(format!(
                "[RFC8785/{}] parsed {} values",
                case["bits"].as_str().unwrap(),
                values.len()
            )),
            Err(error) => failures.push(format!(
                "[RFC8785/{}] reparsing failed: {error}",
                case["bits"].as_str().unwrap()
            )),
        }
    }
}

#[test]
fn complete_normative_registry_and_rfc8785_match() {
    let data = fixture();
    let vectors = data["normative"]["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 125, "normative vector registry changed");
    assert_eq!(
        data["rfc8785"]["cases"].as_array().unwrap().len(),
        26,
        "RFC 8785 Appendix B corpus changed"
    );
    let mut failures = Vec::new();
    for vector in vectors {
        execute_vector(vector, &mut failures);
    }
    execute_rfc8785(&data["rfc8785"], &mut failures);
    assert!(
        failures.is_empty(),
        "{} normative parity gap(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn normative_error_registry_matches_rust_categories() {
    let data = fixture();
    let expected: Vec<_> = data["normative"]["error_categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    let actual = [
        Category::InvalidUnicode,
        Category::UnexpectedToken,
        Category::UnexpectedEnd,
        Category::InvalidNumber,
        Category::InvalidTemporal,
        Category::InvalidEscape,
        Category::InvalidName,
        Category::InvalidBinary,
        Category::InvalidLink,
        Category::DuplicateKey,
        Category::DuplicateSetElement,
        Category::DuplicateAnchor,
        Category::UnknownReference,
        Category::UnknownConstant,
        Category::UnsupportedProfileFeature,
        Category::ResourceLimitExceeded,
    ];
    assert_eq!(actual.map(Category::as_str).as_slice(), expected.as_slice());
}
