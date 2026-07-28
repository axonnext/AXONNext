//! Differential CST tokenizer conformance: for every case in `cst.json`
//! (generated from the pyaxon 2026 reference by `gen_cst.py`), assert that
//! serde_axon's tokenizer produces an *identical* token stream -- the same
//! kinds, texts, and byte spans (start/end/line/column), and the same
//! trivia flags. Also assert the lossless invariant `render() == source`.
//!
//! The token stream is the lossless contract for the CST: concatenating token
//! texts reproduces the source exactly, and byte spans agree across
//! implementations. Comparing against reference-derived ground truth catches
//! any tokenizer divergence immediately.

use serde_axon::cst::{CstDocument, CstElement, Span, format_cst, migrate_indented, parse_cst};
use serde_axon::parser::Options;

use serde_json::{Value as J, json};

fn cases() -> J {
    let raw = include_str!("cst.json");
    serde_json::from_str(raw).expect("cst.json parses")
}

/// Map a named profile to serde_axon [`Options`], matching `gen_cst.py`.
fn options_for(profile: &str) -> Options {
    match profile {
        "core" => Options::default(),
        "compat" => Options {
            compatibility: true,
            ..Options::default()
        },
        "legacy_binary" => Options {
            legacy_binary: true,
            ..Options::default()
        },
        "hash_comment" => Options {
            hash_underscore_comment: true,
            ..Options::default()
        },
        "graph" => Options {
            graph: true,
            ..Options::default()
        },
        "tz_names" => Options {
            tz_names: true,
            ..Options::default()
        },
        other => panic!("unknown profile in cst.json: {other}"),
    }
}

fn span_json(span: Span) -> J {
    json!([span.start, span.end, span.line, span.column])
}

/// Convert the arena-backed Rust tree to the reference fixture's stable
/// pre-order IDs. Arena allocation order is deliberately not part of the API.
fn stable_document_snapshot(case_id: &str, profile: &str, input: &str, doc: &CstDocument) -> J {
    let mut order = Vec::new();
    let mut remap = vec![usize::MAX; doc.nodes.len()];
    let mut stack = vec![doc.root];
    while let Some(node_id) = stack.pop() {
        assert!(node_id < doc.nodes.len(), "CST node index out of range");
        if remap[node_id] != usize::MAX {
            continue;
        }
        remap[node_id] = order.len();
        order.push(node_id);
        for child in doc.nodes[node_id].children.iter().rev() {
            if let CstElement::Node(child_id) = child {
                stack.push(*child_id);
            }
        }
    }
    assert_eq!(
        order.len(),
        doc.nodes.len(),
        "all arena nodes must be reachable from the document root"
    );

    let tokens: Vec<J> = doc
        .tokens
        .iter()
        .map(|token| {
            json!({
                "kind": token.kind.to_string(),
                "text": token.text,
                "span": span_json(token.span),
                "trivia": token.trivia,
                "leading": token.leading_trivia,
                "trailing": token.trailing_trivia,
            })
        })
        .collect();

    let nodes: Vec<J> = order
        .iter()
        .map(|&arena_id| {
            let node = &doc.nodes[arena_id];
            let children: Vec<J> = node
                .children
                .iter()
                .map(|child| match child {
                    CstElement::Token(token_id) => json!(["token", token_id]),
                    CstElement::Node(node_id) => {
                        assert_ne!(remap[*node_id], usize::MAX);
                        json!(["node", remap[*node_id]])
                    }
                })
                .collect();
            json!({
                "id": remap[arena_id],
                "kind": node.kind.to_string(),
                "span": span_json(node.span),
                "children": children,
            })
        })
        .collect();

    let diagnostics: Vec<J> = doc
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let fixes: Vec<J> = diagnostic
                .fixes
                .iter()
                .map(|fix| {
                    json!({
                        "title": fix.title,
                        "start": fix.start,
                        "end": fix.end,
                        "replacement": fix.replacement,
                    })
                })
                .collect();
            json!({
                "category": diagnostic.category,
                "message": diagnostic.message,
                "span": span_json(diagnostic.span),
                "expected": diagnostic.expected,
                "fixes": fixes,
            })
        })
        .collect();

    let semantic_nodes: Vec<usize> = doc
        .semantic_nodes
        .iter()
        .map(|&node_id| {
            assert_ne!(remap[node_id], usize::MAX);
            remap[node_id]
        })
        .collect();
    let semantic_traces: Vec<J> = doc
        .semantic_traces
        .iter()
        .map(|trace| {
            assert_ne!(remap[trace.owner], usize::MAX);
            json!({
                "span": span_json(trace.span),
                "owner": remap[trace.owner],
            })
        })
        .collect();

    json!({
        "id": case_id,
        "profile": profile,
        "input": input,
        "render": doc.render(),
        "tokens": tokens,
        "root": remap[doc.root],
        "nodes": nodes,
        "diagnostics": diagnostics,
        "value_count": doc.values.len(),
        "semantic_nodes": semantic_nodes,
        "semantic_traces": semantic_traces,
    })
}

#[test]
fn tokenizer_matches_reference() {
    let data = cases();
    let mut failures = Vec::new();

    for case in data["cases"].as_array().unwrap() {
        let profile = case["profile"].as_str().unwrap();
        let input = case["input"].as_str().unwrap();
        let opts = options_for(profile);
        let doc = parse_cst(input, &opts).unwrap_or_else(|e| {
            panic!(
                "[{profile}] {input:?}: reference returned a document, serde_axon errored with {}",
                e.category().as_str()
            )
        });

        // Lossless invariant: rendering the token stream reproduces the source.
        if doc.render() != input {
            failures.push(format!(
                "[{profile}] {input:?}: render() != source\n     got: {:?}",
                doc.render()
            ));
            continue;
        }

        let want = case["tokens"].as_array().unwrap();
        if doc.tokens.len() != want.len() {
            failures.push(format!(
                "[{profile}] {input:?}: token count {} != reference {}\n     got: {:?}",
                doc.tokens.len(),
                want.len(),
                doc.tokens
                    .iter()
                    .map(|t| (t.kind, t.text.as_str()))
                    .collect::<Vec<_>>()
            ));
            continue;
        }

        for (idx, (got, exp)) in doc.tokens.iter().zip(want.iter()).enumerate() {
            let ek = exp["kind"].as_str().unwrap();
            let et = exp["text"].as_str().unwrap();
            let es = exp["start"].as_u64().unwrap() as usize;
            let ee = exp["end"].as_u64().unwrap() as usize;
            let el = exp["line"].as_u64().unwrap() as u32;
            let ec = exp["column"].as_u64().unwrap() as u32;
            let etr = exp["trivia"].as_bool().unwrap();
            if got.kind != ek
                || got.text != et
                || got.span.start != es
                || got.span.end != ee
                || got.span.line != el
                || got.span.column != ec
                || got.trivia != etr
            {
                failures.push(format!(
                    "[{profile}] {input:?} token #{idx}: mismatch\n     got: kind={} text={:?} span=({},{},{},{}) trivia={}\n    want: kind={} text={:?} span=({},{},{},{}) trivia={}",
                    got.kind, got.text, got.span.start, got.span.end, got.span.line, got.span.column, got.trivia,
                    ek, et, es, ee, el, ec, etr
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} CST tokenizer mismatch(es):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn documents_match_reference() {
    let data = cases();
    let mut failures = Vec::new();
    let fields = [
        "render",
        "tokens",
        "root",
        "nodes",
        "diagnostics",
        "value_count",
        "semantic_nodes",
        "semantic_traces",
    ];

    for case in data["documents"].as_array().unwrap() {
        let case_id = case["id"].as_str().unwrap();
        let profile = case["profile"].as_str().unwrap();
        let input = case["input"].as_str().unwrap();
        let opts = options_for(profile);
        let doc = match parse_cst(input, &opts) {
            Ok(doc) => doc,
            Err(error) => {
                failures.push(format!(
                    "[{case_id}/{profile}] reference returned a document, serde_axon errored with {}",
                    error.category().as_str()
                ));
                continue;
            }
        };
        let got = stable_document_snapshot(case_id, profile, input, &doc);
        for &field in &fields {
            if got[field] != case[field] {
                failures.push(format!(
                    "[{case_id}/{profile}] {field} mismatch\n     got: {}\n    want: {}",
                    serde_json::to_string_pretty(&got[field]).unwrap(),
                    serde_json::to_string_pretty(&case[field]).unwrap()
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} CST document mismatch(es):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn migrate_matches_reference() {
    let data = cases();
    let mut failures = Vec::new();

    for case in data["migrate"].as_array().unwrap() {
        let profile = case["profile"].as_str().unwrap();
        let input = case["input"].as_str().unwrap();
        let opts = options_for(profile);
        let outcome = case["outcome"].as_str().unwrap();

        match (outcome, migrate_indented(input, &opts)) {
            ("ok", Ok(got)) => {
                let want = case["output"].as_str().unwrap();
                if got != want {
                    failures.push(format!(
                        "[{profile}] {input:?}: migrate mismatch\n     got: {got:?}\n    want: {want:?}"
                    ));
                }
            }
            ("ok", Err(e)) => failures.push(format!(
                "[{profile}] {input:?}: reference migrates, serde_axon errored with {}",
                e.category().as_str()
            )),
            ("error", Ok(got)) => failures.push(format!(
                "[{profile}] {input:?}: reference errors ({}), serde_axon produced {got:?}",
                case["category"].as_str().unwrap()
            )),
            ("error", Err(e)) => {
                let want = case["category"].as_str().unwrap();
                if e.category().as_str() != want {
                    failures.push(format!(
                        "[{profile}] {input:?}: category {} != reference {want}",
                        e.category().as_str()
                    ));
                }
            }
            (other, _) => panic!("unknown outcome in cst.json migrate: {other}"),
        }
    }

    assert!(
        failures.is_empty(),
        "{} CST migrate mismatch(es):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn format_matches_reference() {
    let data = cases();
    let mut failures = Vec::new();

    for case in data["format"].as_array().unwrap() {
        let case_id = case["id"].as_str().unwrap();
        let profile = case["profile"].as_str().unwrap();
        let input = case["input"].as_str().unwrap();
        let indent = case["indent"].as_u64().unwrap() as usize;
        let outcome = case["outcome"].as_str().unwrap();
        let opts = options_for(profile);

        match (outcome, format_cst(input, indent, &opts)) {
            ("ok", Ok(got)) => {
                let want = case["output"].as_str().unwrap();
                if got != want {
                    failures.push(format!(
                        "[{case_id}/{profile}] formatter mismatch\n     got: {got:?}\n    want: {want:?}"
                    ));
                }
            }
            ("ok", Err(error)) => failures.push(format!(
                "[{case_id}/{profile}] reference formatted successfully as {:?}, serde_axon errored: {error}",
                case["output"].as_str().unwrap()
            )),
            ("error", Ok(got)) => failures.push(format!(
                "[{case_id}/{profile}] reference errored with {}, serde_axon produced {got:?}",
                case["category"].as_str().unwrap()
            )),
            ("error", Err(error)) => {
                let want = case["category"].as_str().unwrap();
                if error.category().as_str() != want {
                    failures.push(format!(
                        "[{case_id}/{profile}] category {} != reference {want}",
                        error.category().as_str()
                    ));
                }
            }
            (other, _) => panic!("unknown formatter outcome: {other}"),
        }
    }

    assert!(
        failures.is_empty(),
        "{} CST formatter mismatch(es):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn cst_token_limit_is_enforced() {
    let mut opts = Options::default();
    opts.limits.max_cst_tokens = 1;
    let error = parse_cst("1 2", &opts).expect_err("second CST token exceeds the limit");
    assert_eq!(error.category().as_str(), "resource-limit-exceeded");
}

#[test]
fn deeply_nested_source_remains_fully_owned() {
    let source = "[".repeat(5_000) + &"]".repeat(5_000);
    let mut opts = Options::default();
    opts.limits.max_depth = 128;
    opts.limits.max_input = 20_000;
    opts.limits.max_cst_tokens = 20_000;

    let doc = parse_cst(&source, &opts).expect("depth overflow is a recovering diagnostic");
    assert_eq!(doc.render(), source);
    assert!(
        doc.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.category == "resource-limit-exceeded")
    );
    assert_eq!(doc.node_tokens(doc.root).len(), 10_000);
}

#[test]
fn byte_edits_and_normative_fix_are_utf8_safe() {
    let opts = Options::default();
    let unicode = parse_cst("é", &opts).unwrap();
    assert!(unicode.replace(1, 1, "x", &opts).is_err());
    assert!(unicode.replace(2, 1, "x", &opts).is_err());
    assert!(unicode.replace(99, 100, "x", &opts).is_err());

    let source = "{a:1  # keep\n b:2}";
    let doc = parse_cst(source, &opts).unwrap();
    let token = doc.tokens.iter().find(|token| token.text == "1").unwrap();
    let edited = doc
        .replace(token.span.start, token.span.end, "1000D", &opts)
        .unwrap();
    assert_eq!(edited.render(), "{a:1000D  # keep\n b:2}");
    assert!(edited.diagnostics.is_empty());

    let broken = parse_cst("[1", &opts).unwrap();
    let diagnostic = broken
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.category == "unexpected-end")
        .unwrap();
    assert_eq!(diagnostic.expected, ["]"]);
    let fix = diagnostic
        .fixes
        .first()
        .expect("Q003 supplies an insert fix");
    let fixed = broken
        .replace(fix.start, fix.end, &fix.replacement, &opts)
        .unwrap();
    assert_eq!(fixed.render(), "[1]");
    assert!(fixed.diagnostics.is_empty());
}

#[test]
fn canonical_failures_retain_cst_traces_and_original_input_limits() {
    let canonical = Options {
        canonical_verify: true,
        ..Options::default()
    };
    let error = serde_axon::from_str_stream_with(" 1", canonical)
        .expect_err("leading whitespace is not canonical AXON");
    assert_eq!(error.category().as_str(), "unexpected-token");

    let document = parse_cst(" 1", &canonical).expect("canonical failure remains lossless");
    assert!(document.values.is_empty());
    assert_eq!(document.semantic_traces.len(), 1);
    assert_eq!(document.semantic_traces[0].span.start, 1);
    assert_eq!(document.semantic_traces[0].span.end, 2);
    assert!(
        document
            .diagnostics
            .iter()
            .any(|item| item.category == "unexpected-token")
    );

    let source = "person\n  name:\"Alex\"\n";
    let mut indented = Options {
        indented_bodies: true,
        ..Options::default()
    };
    indented.limits.max_input = source.len();
    serde_axon::from_str_stream_with(source, indented)
        .expect("max_input applies before indentation expansion");
}
