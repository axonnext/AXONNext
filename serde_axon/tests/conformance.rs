//! Conformance tests: assert the parser's outcome for each vector against the
//! ground truth captured from the pyaxon 0.11.0a13 reference implementation
//! (`tests/vectors.json`). Accept vectors must parse to the described value;
//! error vectors must fail with the described category.

use serde_axon::parser::Options;
use serde_axon::value::{Decimal, Link, Node, NodeStyle, Temporal, Value, Zone};
use serde_axon::{Category, from_str_stream, from_str_value_with};

use serde_json::Value as J;

fn vectors() -> J {
    let raw = include_str!("vectors.json");
    serde_json::from_str(raw).expect("vectors.json parses")
}

/// Compare a parsed [`Value`] against a `{"k":..}` ground-truth description.
fn matches(v: &Value, d: &J) -> bool {
    let k = d["k"].as_str().unwrap();
    match (k, v) {
        ("null", Value::Null) => true,
        ("bool", Value::Bool(b)) => *b == d["v"].as_bool().unwrap(),
        ("int", Value::Int(s)) => s == d["v"].as_str().unwrap(),
        ("float", Value::Float(f)) => match d["v"].as_str().unwrap() {
            "nan" => f.is_nan(),
            "inf" => *f == f64::INFINITY,
            "-inf" => *f == f64::NEG_INFINITY,
            other => other.parse::<f64>().map(|x| x == *f).unwrap_or(false),
        },
        ("decimal", Value::Decimal(dec)) => decimal_matches(dec, d["v"].as_str().unwrap()),
        ("string", Value::String(s)) => s == d["v"].as_str().unwrap(),
        ("bytes", Value::Bytes(b)) => {
            let want: Vec<u8> = d["v"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap() as u8)
                .collect();
            *b == want
        }
        ("date", Value::Temporal(Temporal::Date(dt))) => {
            let a = d["date"].as_array().unwrap();
            dt.year == a[0].as_i64().unwrap() as i32
                && dt.month == a[1].as_u64().unwrap() as u8
                && dt.day == a[2].as_u64().unwrap() as u8
        }
        ("time", Value::Temporal(Temporal::Time(t))) => time_matches(t, d),
        ("datetime", Value::Temporal(Temporal::DateTime(dt))) => {
            let a = d["date"].as_array().unwrap();
            dt.date.year == a[0].as_i64().unwrap() as i32
                && dt.date.month == a[1].as_u64().unwrap() as u8
                && dt.date.day == a[2].as_u64().unwrap() as u8
                && time_matches(&dt.time, d)
        }
        ("list", Value::List(items)) => seq_matches(items, &d["items"]),
        ("tuple", Value::Tuple(items)) => seq_matches(items, &d["items"]),
        ("set", Value::Set(items)) => seq_matches(items, &d["items"]),
        ("map", Value::Map(pairs)) => pairs_matches(pairs, &d["pairs"]),
        ("omap", Value::OrderedMap(pairs)) => pairs_matches(pairs, &d["pairs"]),
        ("node", Value::Node(node)) => node_matches(node, d),
        ("link", Value::Link(link)) => {
            let kind = d["kind"].as_str().unwrap();
            let target = d["target"].as_str().unwrap();
            match link {
                Link::Cid(s) => kind == "cid" && s == target,
                Link::Uri(s) => kind == "uri" && s == target,
            }
        }
        _ => false,
    }
}

fn decimal_matches(dec: &Decimal, want: &str) -> bool {
    match (dec, want) {
        (Decimal::NaN, "nan") => true,
        (Decimal::Infinity, "inf") => true,
        (Decimal::NegInfinity, "-inf") => true,
        (Decimal::Finite { .. }, w) => {
            // compare via the canonical text the writer produces (without the D)
            let s = serde_axon::writer::to_string(&Value::Decimal(dec.clone())).unwrap();
            let s = s.trim_end_matches('D');
            s == w
        }
        _ => false,
    }
}

fn time_matches(t: &serde_axon::value::Time, d: &J) -> bool {
    let a = d["time"].as_array().unwrap();
    let ok = t.hour == a[0].as_u64().unwrap() as u8
        && t.minute == a[1].as_u64().unwrap() as u8
        && t.second == a[2].as_u64().unwrap() as u8
        && t.nanosecond == a[3].as_u64().unwrap() as u32;
    let tz_ok = match &d["tz"] {
        J::Null => matches!(t.zone, Zone::Local),
        J::String(s) if s == "Z" => matches!(t.zone, Zone::Zulu),
        J::Number(n) => matches!(t.zone, Zone::Offset(m) if m as i64 == n.as_i64().unwrap()),
        _ => false,
    };
    ok && tz_ok
}

fn seq_matches(items: &[Value], want: &J) -> bool {
    let w = want.as_array().unwrap();
    items.len() == w.len() && items.iter().zip(w).all(|(v, d)| matches(v, d))
}

fn pairs_matches(pairs: &[(Value, Value)], want: &J) -> bool {
    let w = want.as_array().unwrap();
    if pairs.len() != w.len() {
        return false;
    }
    pairs.iter().zip(w).all(|((k, v), pair)| {
        let key_ok = match k {
            Value::String(s) => s == pair[0].as_str().unwrap(),
            _ => false,
        };
        key_ok && matches(v, &pair[1])
    })
}

fn node_matches(node: &Node, d: &J) -> bool {
    let name_ok = node.name == d["name"].as_str().unwrap();
    let style_ok = node.style
        == match d["style"].as_str().unwrap() {
            "unit" => NodeStyle::Unit,
            "brace" => NodeStyle::Brace,
            "tuple" => NodeStyle::Tuple,
            "list" => NodeStyle::List,
            _ => return false,
        };
    let attrs = d["attrs"].as_array().unwrap();
    let attrs_ok = node.attributes.len() == attrs.len()
        && node
            .attributes
            .iter()
            .zip(attrs)
            .all(|((k, v), pair)| k == pair[0].as_str().unwrap() && matches(v, &pair[1]));
    let children = d["children"].as_array().unwrap();
    let children_ok = node.children.len() == children.len()
        && node
            .children
            .iter()
            .zip(children)
            .all(|(v, dd)| matches(v, dd));
    name_ok && style_ok && attrs_ok && children_ok
}

fn category_of(name: &str) -> Category {
    match name {
        "invalid-unicode" => Category::InvalidUnicode,
        "unexpected-end" => Category::UnexpectedEnd,
        "unexpected-token" => Category::UnexpectedToken,
        "invalid-number" => Category::InvalidNumber,
        "invalid-temporal" => Category::InvalidTemporal,
        "invalid-escape" => Category::InvalidEscape,
        "invalid-name" => Category::InvalidName,
        "invalid-binary" => Category::InvalidBinary,
        "invalid-link" => Category::InvalidLink,
        "duplicate-key" => Category::DuplicateKey,
        "duplicate-set-element" => Category::DuplicateSetElement,
        "duplicate-anchor" => Category::DuplicateAnchor,
        "unknown-reference" => Category::UnknownReference,
        "unknown-constant" => Category::UnknownConstant,
        "unsupported-profile-feature" => Category::UnsupportedProfileFeature,
        "resource-limit-exceeded" => Category::ResourceLimitExceeded,
        other => panic!("unknown category in fixture: {other}"),
    }
}

#[test]
fn accept_vectors() {
    let vecs = vectors();
    let mut failures = Vec::new();
    for case in vecs["accept"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let want = case["values"].as_array().unwrap();
        match from_str_stream(input) {
            Ok(values) => {
                if values.len() != want.len()
                    || !values.iter().zip(want).all(|(v, d)| matches(v, d))
                {
                    failures.push(format!("{input:?}: value mismatch, got {values:?}"));
                }
            }
            Err(e) => failures.push(format!(
                "{input:?}: unexpected error {}",
                e.category().as_str()
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "accept failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn error_vectors() {
    let vecs = vectors();
    let opts = Options::default();
    let mut failures = Vec::new();
    for case in vecs["errors"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let want = category_of(case["error"].as_str().unwrap());
        match from_str_value_with(input, opts) {
            Ok(v) => failures.push(format!(
                "{input:?}: expected error {}, parsed {v:?}",
                want.as_str()
            )),
            Err(e) => {
                if e.category() != want {
                    failures.push(format!(
                        "{input:?}: expected {}, got {}",
                        want.as_str(),
                        e.category().as_str()
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "error failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn doc_link_vectors() {
    let vecs = vectors();
    let opts = Options {
        doc_link: true,
        ..Default::default()
    };
    for case in vecs["doc_link"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let want = &case["values"].as_array().unwrap()[0];
        let v = from_str_value_with(input, opts).expect("doc_link parses");
        assert!(matches(&v, want), "{input:?}: got {v:?}");
    }
}
