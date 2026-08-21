#!/usr/bin/env python3
"""Generate AXON Schema 2026 ground truth from the Python reference.

The fixture is intentionally independent of the Rust API.  Values and schemas
are represented as AXON source, while outcomes contain the exact normalized
form or exact structured diagnostics emitted by ``axon2.schema2026``.  It covers
schema normalization, registry governance, every validation keyword,
deterministic issue ordering/limits, local and external references, recursive
schemas, and the security-governed regular-expression subset.

Run from the AXON Next directory with ``PYTHONPATH=lib``::

    python serde_axon/tests/gen_schema.py serde_axon/tests/schema.json
"""

from collections import OrderedDict
import json
import re
import sys

from axon2.schema2026 import (
    SCHEMA_DIALECT_2026,
    SchemaRegistry2026,
    ValidationIssue,
    _SchemaResourceError,
    _ordered_issues,
    _safe_regex_search,
    normalize_schema,
    validation_report2026,
)
from axon2.v2026 import Options, dumps2026, loads2026


def options(profile):
    if profile == "core":
        return Options()
    if profile == "doc_link":
        return Options(doc_link=True)
    if profile == "graph":
        return Options(graph=True)
    if profile == "scale_significant":
        return Options(scale_significant=True)
    raise ValueError(profile)


def load_one(source, profile="core"):
    values = loads2026(source, options=options(profile))
    if len(values) != 1:
        raise AssertionError("fixture source must contain exactly one value: %r" % source)
    return values[0]


def canonical(value):
    return dumps2026([value], canonical=True, graph=True)


def error_record(exc):
    return {
        "outcome": "error",
        "error_kind": (
            "resource-limit" if isinstance(exc, _SchemaResourceError)
            else "regex" if isinstance(exc, re.error)
            else "type" if isinstance(exc, TypeError)
            else "value" if isinstance(exc, ValueError)
            else type(exc).__name__
        ),
        "error_type": type(exc).__name__,
        "message": str(exc),
    }


def issue_record(issue):
    return {
        "code": issue.code,
        "path": list(issue.path),
        "schema_path": list(issue.schema_path),
        "message": issue.message,
    }


# Successful cases exercise both accepted schema forms and every recursively
# normalised sub-schema-bearing keyword.  Error cases cover every shape check
# and both normalisation resource limits.
NORMALIZATION_CASES = [
    {"id": "empty-map", "schema": "{}"},
    {"id": "dialect", "schema": '{"$schema":"urn:axonnext:schema:2026" kind:"int"}'},
    {"id": "kind-union", "schema": '{kind:["null" "string"]}'},
    {
        "id": "mapping-recursive-keywords",
        "schema": (
            '{kind:"node" properties:{p:{kind:"int"}} '
            'items:{kind:"string"} children:{kind:"bool"} '
            'attributes:{kind:"map"} additional:{kind:"number"} '
            'prefix_items:[{kind:"int"} {kind:"string"}] '
            '"$defs":{x:{kind:"date"}}}'
        ),
    },
    {
        "id": "node-fields",
        "schema": (
            'schema{kind:"node" required:["name"] additional:false '
            'field{name:"name" kind:"string" min_length:1} '
            'field{name:"age" kind:"int" minimum:0}}'
        ),
    },
    {
        "id": "node-recursive-attributes-regression",
        "schema": (
            'schema{kind:"list" items:schema{kind:"node" '
            'attributes:schema{kind:"map" '
            'properties:{age:field{kind:"int" minimum:1}} '
            'additional:false}}}'
        ),
    },
    {"id": "annotation-keywords", "schema": '{title:"T" description:"D" "$comment":"C" examples:[1] default:1 deprecated:true}'},
    {"id": "additional-bool", "schema": '{kind:"map" additional:false}'},
    {"id": "prefix-tuple", "schema": '{kind:"tuple" prefix_items:({kind:"int"} {kind:"string"})}'},
    {"id": "nested-error-before-unknown", "schema": '{zzz:0 items:[]}'},
    {"id": "node-name-keyword-is-field-metadata", "schema": 'schema{name:"person" kind:"node"}'},
    {"id": "ordered-map-schema", "schema": '[kind:"int"]'},
    {"id": "wrong-top-level-type", "schema": "1"},
    {"id": "wrong-node-name", "schema": "not_schema{}"},
    {"id": "wrong-node-child", "schema": "schema{not_field{}}"},
    {"id": "field-without-name", "schema": 'schema{field{kind:"int"}}'},
    {"id": "unsupported-dialect", "schema": '{"$schema":"urn:other"}'},
    {"id": "id-not-string", "schema": '{"$id":1}'},
    {"id": "ref-not-string", "schema": '{"$ref":1}'},
    {"id": "properties-not-map", "schema": '{properties:[]}'},
    {"id": "items-not-schema", "schema": '{items:[]}'},
    {"id": "children-not-schema", "schema": '{children:1}'},
    {"id": "attributes-not-schema", "schema": '{attributes:false}'},
    {"id": "additional-wrong-shape", "schema": '{additional:[]}'},
    {"id": "prefix-items-wrong-shape", "schema": '{prefix_items:{kind:"int"}}'},
    {"id": "defs-not-map", "schema": '{"$defs":[]}'},
    {"id": "unknown-keyword", "schema": '{minimun:1}'},
    {"id": "kind-not-string-or-sequence", "schema": '{kind:1}'},
    {"id": "kind-empty-sequence", "schema": '{kind:[]}'},
    {"id": "unknown-kind", "schema": '{kind:"object"}'},
    {"id": "enum-not-container", "schema": '{enum:1}'},
    {"id": "required-not-sequence", "schema": '{required:1}'},
    {"id": "required-non-string", "schema": '{required:["a" 1]}'},
    {"id": "required-duplicate", "schema": '{required:["a" "a"]}'},
    {"id": "pattern-not-string", "schema": '{pattern:1}'},
    {"id": "key-pattern", "schema": '{kind:"map" key_pattern:"^[a-z][a-z0-9-]*$"}'},
    {"id": "key-pattern-not-string", "schema": '{key_pattern:1}'},
    {"id": "any-of", "schema": '{kind:"map" any_of:[{required:["a"]} {required:["b"]}]}'},
    {"id": "any-of-nested", "schema": '{any_of:[{any_of:[{kind:"int"} {kind:"float"}]} {kind:"string"}]}'},
    {"id": "any-of-empty", "schema": '{any_of:[]}'},
    {"id": "any-of-not-a-sequence", "schema": '{any_of:{}}'},
    {"id": "any-of-string", "schema": '{any_of:"x"}'},
    {"id": "any-of-branch-not-a-schema", "schema": '{any_of:[{kind:"int"} 1]}'},
    {"id": "negative-length", "schema": '{min_length:-1}'},
    {"id": "boolean-length", "schema": '{max_items:true}'},
    {"id": "string-length", "schema": '{min_children:"1"}'},
    {"id": "length-bounds-inverted", "schema": '{min_length:2 max_length:1}'},
    {
        "id": "normalization-depth-limit",
        "schema": '{items:{items:{items:{kind:"int"}}}}',
        "max_depth": 2,
    },
    {
        "id": "normalization-node-limit",
        "schema": '{properties:{a:{} b:{} c:{}}}',
        "max_nodes": 3,
    },
    {
        "id": "normalization-direct-cycle",
        "schema": '&1 {items:*1}',
        "profile": "graph",
    },
]


def normalization_records():
    records = []
    for case in NORMALIZATION_CASES:
        profile = case.get("profile", "core")
        record = dict(case)
        try:
            result = normalize_schema(
                load_one(case["schema"], profile),
                max_depth=case.get("max_depth", 128),
                max_nodes=case.get("max_nodes", 1048576),
            )
        except (TypeError, ValueError, re.error) as exc:
            record.update(error_record(exc))
        else:
            record.update({"outcome": "ok", "normalized": canonical(result)})
        records.append(record)
    return records


REGISTRY_IDENTIFIER_CASES = [
    ("urn", "urn:axon:test:module"),
    ("https", "https://example.com/schema"),
    ("custom", "axon+schema:module/v1"),
    ("empty", ""),
    ("non-string", 7),
    ("non-ascii", "urn:axon:café"),
    ("space", "urn:axonnext:test module"),
    ("control", "urn:axonnext:test\nmodule"),
    ("relative", "schema/module"),
    ("fragment", "urn:axonnext:test#part"),
    ("empty-fragment", "urn:x#"),
    ("empty-http-authority", "http://"),
    ("numeric-scheme", "1:x"),
    ("invalid-uri", "http://[broken"),
    ("invalid-bracketed-host", "http://[notipv6]"),
    ("valid-ipv6-host", "http://[::1]/schema"),
]


def registry_identifier_records():
    records = []
    for case_id, identifier in REGISTRY_IDENTIFIER_CASES:
        record = {"id": case_id, "identifier": identifier}
        try:
            result = SchemaRegistry2026.validate_identifier(identifier)
        except (TypeError, ValueError) as exc:
            record.update(error_record(exc))
        else:
            record.update({"outcome": "ok", "normalized": result})
        records.append(record)
    return records


REGISTRY_CONFIG_CASES = [
    ("default", {}),
    ("bounded", {"max_modules": 2, "max_depth": 3, "max_nodes": 4}),
    ("zero-modules", {"max_modules": 0}),
    ("zero-depth", {"max_depth": 0}),
    ("zero-nodes", {"max_nodes": 0}),
    ("string-modules", {"max_modules": "1"}),
]


def registry_config_records():
    records = []
    for case_id, config in REGISTRY_CONFIG_CASES:
        record = {"id": case_id, "config": config}
        try:
            registry = SchemaRegistry2026(**config)
        except (TypeError, ValueError) as exc:
            record.update(error_record(exc))
        else:
            record.update({
                "outcome": "ok",
                "frozen": registry.frozen,
                "network_resolution": registry.network_resolution,
                "length": len(registry),
            })
        records.append(record)
    return records


REGISTRY_SCENARIOS = [
    {
        "id": "register-freeze-and-alias-isolation",
        "steps": [
            {"op": "register", "identifier": "urn:axon:test:positive", "schema": '{"$id":"urn:axon:test:positive" kind:"int" minimum:1}', "mutate_return": {"minimum": -10}},
            {"op": "get", "identifier": "urn:axon:test:positive", "mutate_return": {"minimum": -20}},
            {"op": "snapshot", "mutate_schema": {"identifier": "urn:axon:test:positive", "key": "minimum", "value": -30}},
            {"op": "freeze"},
            {"op": "validate", "value": "0", "schema": '{"$ref":"urn:axon:test:positive"}'},
            {"op": "register", "identifier": "urn:axon:test:late", "schema": '{kind:"any"}'},
        ],
    },
    {
        "id": "duplicate-and-replace",
        "steps": [
            {"op": "register", "identifier": "urn:axon:test:value", "schema": '{kind:"int" minimum:0}'},
            {"op": "register", "identifier": "urn:axon:test:value", "schema": '{kind:"int" minimum:10}'},
            {"op": "register", "identifier": "urn:axon:test:value", "schema": '{kind:"int" minimum:10}', "replace": True},
            {"op": "validate", "value": "5", "schema": '{"$ref":"urn:axon:test:value"}'},
        ],
    },
    {
        "id": "declared-id-mismatch",
        "steps": [
            {"op": "register", "identifier": "urn:axon:test:actual", "schema": '{"$id":"urn:axon:test:other" kind:"any"}'},
        ],
    },
    {
        "id": "module-limit",
        "config": {"max_modules": 1},
        "steps": [
            {"op": "register", "identifier": "urn:axon:test:one", "schema": '{kind:"any"}'},
            {"op": "register", "identifier": "urn:axon:test:two", "schema": '{kind:"any"}'},
            {"op": "register", "identifier": "urn:axon:test:one", "schema": '{kind:"int"}', "replace": True},
        ],
    },
    {
        "id": "normalization-limits",
        "config": {"max_depth": 1, "max_nodes": 2},
        "steps": [
            {"op": "register", "identifier": "urn:axon:test:deep", "schema": '{items:{items:{kind:"int"}}}'},
            {"op": "register", "identifier": "urn:axon:test:wide", "schema": '{properties:{a:{} b:{}}}'},
        ],
    },
    {
        "id": "external-reference-chain",
        "steps": [
            {"op": "register", "identifier": "urn:axon:test:number", "schema": '{"$id":"urn:axon:test:number" kind:"integer" minimum:1}'},
            {"op": "register", "identifier": "urn:axon:test:alias", "schema": '{"$id":"urn:axon:test:alias" "$ref":"urn:axon:test:number"}'},
            {"op": "validate", "value": "0", "schema": '{"$ref":"urn:axon:test:alias"}'},
        ],
    },
]


def report_record(value_source, schema_source, registry, *, value_profile="core", max_depth=128, max_issues=256):
    report = validation_report2026(
        load_one(value_source, value_profile),
        load_one(schema_source),
        registry=registry,
        max_depth=max_depth,
        max_issues=max_issues,
    )
    return {
        "valid": report.valid,
        "issues": [issue_record(issue) for issue in report.issues],
        "counts": [[code, count] for code, count in report.counts],
        "truncated": report.truncated,
    }


def registry_scenario_records():
    records = []
    for scenario in REGISTRY_SCENARIOS:
        registry = SchemaRegistry2026(**scenario.get("config", {}))
        result = {key: value for key, value in scenario.items() if key != "steps"}
        output_steps = []
        for source_step in scenario["steps"]:
            step = dict(source_step)
            try:
                if step["op"] == "register":
                    returned = registry.register(
                        step["identifier"],
                        load_one(step["schema"]),
                        replace=step.get("replace", False),
                    )
                    step["result"] = {"outcome": "ok", "schema": canonical(returned)}
                    if "mutate_return" in step:
                        returned.update(step["mutate_return"])
                elif step["op"] == "get":
                    returned = registry[step["identifier"]]
                    step["result"] = {"outcome": "ok", "schema": canonical(returned)}
                    if "mutate_return" in step:
                        returned.update(step["mutate_return"])
                elif step["op"] == "snapshot":
                    snapshot = registry.snapshot()
                    step["result"] = {
                        "outcome": "ok",
                        "modules": [[identifier, canonical(schema)] for identifier, schema in snapshot.items()],
                    }
                    mutation = step.get("mutate_schema")
                    if mutation:
                        snapshot[mutation["identifier"]][mutation["key"]] = mutation["value"]
                elif step["op"] == "freeze":
                    registry.freeze()
                    step["result"] = {"outcome": "ok", "frozen": registry.frozen}
                elif step["op"] == "validate":
                    step["result"] = {
                        "outcome": "ok",
                        "report": report_record(step["value"], step["schema"], registry),
                    }
                else:
                    raise AssertionError("unknown registry operation %r" % step["op"])
            except (KeyError, RuntimeError, TypeError, ValueError) as exc:
                step["result"] = error_record(exc)
            output_steps.append(step)
        result["steps"] = output_steps
        result["final"] = {
            "length": len(registry),
            "frozen": registry.frozen,
            "network_resolution": registry.network_resolution,
            "items": [[identifier, canonical(schema)] for identifier, schema in registry.items()],
        }
        records.append(result)
    return records


# Each case is (id, value AXON, schema AXON, optional settings).  The corpus
# includes a passing and/or failing witness for every public constraint and all
# value kinds/aliases exposed by the reference validator.
VALIDATION_CASES = [
    # Kinds and aliases.
    ("kind-any", "p{x:1}", '{}', {}),
    ("kind-null", "null", '{kind:"null"}', {}),
    ("kind-bool", "true", '{kind:"bool"}', {}),
    ("kind-boolean-alias", "false", '{kind:"boolean"}', {}),
    ("kind-int", "1", '{kind:"int"}', {}),
    ("kind-integer-alias", "-1", '{kind:"integer"}', {}),
    ("kind-float", "1.5", '{kind:"float"}', {}),
    ("kind-decimal", "1.5D", '{kind:"decimal"}', {}),
    ("kind-scaled-decimal", "1.50D", '{kind:"decimal"}', {"value_profile": "scale_significant"}),
    ("kind-number-int", "1", '{kind:"number"}', {}),
    ("kind-number-float", "1.5", '{kind:"number"}', {}),
    ("kind-number-decimal", "1.5D", '{kind:"number"}', {}),
    ("kind-string", '"x"', '{kind:"string"}', {}),
    ("kind-bytes", "|YQ==|", '{kind:"bytes"}', {}),
    ("kind-binary-alias", "|YQ==|", '{kind:"binary"}', {}),
    ("kind-date", "^2026-01-02", '{kind:"date"}', {}),
    ("kind-time", "^12:34:56.7Z", '{kind:"time"}', {}),
    ("kind-datetime", "^2026-01-02T12:34:56Z", '{kind:"datetime"}', {}),
    ("kind-node-brace", "p{x:1}", '{kind:"node"}', {}),
    ("kind-set", "{1 2}", '{kind:"set"}', {}),
    ("kind-map", "{a:1}", '{kind:"map"}', {}),
    ("kind-ordered-map", "[a:1]", '{kind:"ordered-map"}', {}),
    ("kind-mapping-map", "{a:1}", '{kind:"mapping"}', {}),
    ("kind-mapping-ordered", "[a:1]", '{kind:"mapping"}', {}),
    ("kind-list", "[1 2]", '{kind:"list"}', {}),
    ("kind-tuple", "(1 2)", '{kind:"tuple"}', {}),
    ("kind-sequence-list", "[1]", '{kind:"sequence"}', {}),
    ("kind-sequence-tuple", "(1)", '{kind:"sequence"}', {}),
    ("kind-collection-set", "{1}", '{kind:"collection"}', {}),
    ("kind-collection-map", "{a:1}", '{kind:"collection"}', {}),
    ("kind-link", 'link("https://example.com/x")', '{kind:"link"}', {"value_profile": "doc_link"}),
    ("kind-union-pass", '"x"', '{kind:["null" "string"]}', {}),
    ("kind-failure", '"1"', '{kind:["integer" "boolean"]}', {}),
    # Equality and numeric constraints.
    ("const-pass", "1", '{const:1}', {}),
    ("const-strict-type", "1.0", '{const:1}', {}),
    ("const-nan", "?", '{const:?}', {}),
    ("const-negative-zero", "-0.0", '{const:0.0}', {}),
    ("const-nested-int-float", "[1]", '{const:[1.0]}', {}),
    ("const-nested-bool-int", "[true]", '{const:[1]}', {}),
    ("const-map-order-insensitive", "{a:1 b:2}", '{const:{b:2 a:1}}', {}),
    ("const-ordered-map-order-sensitive", "[a:1 b:2]", '{const:[b:2 a:1]}', {}),
    ("const-decimal-scale-core", "1.0D", '{const:1.00D}', {}),
    ("const-scaled-decimal-distinct", "1.50D", '{const:1.50D}', {"value_profile": "scale_significant"}),
    ("enum-scaled-decimal-distinct", "1.50D", '{enum:[1.50D]}', {"value_profile": "scale_significant"}),
    ("const-nested-scaled-decimal-distinct", "[1.50D]", '{const:[1.50D]}', {"value_profile": "scale_significant"}),
    ("const-nested-scaled-vs-int-distinct", "[1.0D]", '{const:[1]}', {"value_profile": "scale_significant"}),
    ("enum-list-pass", '"b"', '{enum:["a" "b"]}', {}),
    ("enum-set-pass", "2", '{enum:{1 2 3}}', {}),
    ("enum-failure", "4", '{enum:(1 2 3)}', {}),
    ("minimum-pass", "1", '{kind:"number" minimum:1}', {}),
    ("minimum-failure", "0", '{kind:"number" minimum:1}', {}),
    ("maximum-failure", "2", '{kind:"number" maximum:1}', {}),
    ("exclusive-minimum-failure", "1", '{kind:"number" exclusive_minimum:1}', {}),
    ("exclusive-maximum-failure", "1", '{kind:"number" exclusive_maximum:1}', {}),
    ("decimal-range", "1.25D", '{kind:"decimal" minimum:1 maximum:1.2D}', {}),
    ("schema-comparison", "1", '{kind:"number" minimum:"zero" maximum:2}', {}),
    ("exact-float-decimal-maximum", "1.1", '{maximum:1.1D}', {}),
    ("exact-float-decimal-minimum", "1.1", '{minimum:1.1D}', {}),
    ("exact-float-large-int", "9007199254740992.0", '{minimum:9007199254740993}', {}),
    ("float-nan-bounds", "?", '{minimum:0 maximum:1 exclusive_minimum:0 exclusive_maximum:1}', {}),
    ("decimal-nan-bound", "?D", '{minimum:0}', {}),
    ("prior-issue-before-schema-comparison", "5", '{maximum:4 exclusive_minimum:"x"}', {}),
    ("numeric-keyword-ignored-for-string", '"x"', '{minimum:10}', {}),
    # Unicode scalar lengths and patterns.
    ("min-length", '""', '{kind:"string" min_length:1}', {}),
    ("max-length", '"ab"', '{kind:"string" max_length:1}', {}),
    ("unicode-length-codepoint", '"😀"', '{kind:"string" min_length:1 max_length:1}', {}),
    ("decomposed-length", '"é"', '{kind:"string" max_length:1}', {}),
    ("pattern-pass", '"AXON"', '{kind:"string" pattern:"^[A-Z]+$"}', {}),
    ("pattern-no-match", '"axon"', '{kind:"string" pattern:"^[A-Z]+$"}', {}),
    ("pattern-invalid", '"x"', '{kind:"string" pattern:"["}', {}),
    ("pattern-unsafe", '"aaaa"', '{kind:"string" pattern:"(a+)+$"}', {}),
    # key_pattern: the mapping-key counterpart of pattern.
    ("key-pattern-pass", '{en:"Hello" "fr-CA":"Bonjour"}', '{kind:"map" key_pattern:"^[A-Za-z][A-Za-z0-9-]*$"}', {}),
    ("key-pattern-fail", '{"not a tag!":"x"}', '{kind:"map" key_pattern:"^[A-Za-z][A-Za-z0-9-]*$"}', {}),
    ("key-pattern-unsafe", '{a:1}', '{kind:"map" key_pattern:"(a+)+$"}', {}),
    ("key-pattern-with-additional", '{en:""}', '{kind:"map" key_pattern:"^[A-Za-z][A-Za-z0-9-]*$" additional:{kind:"string" min_length:1}}', {}),
    ("key-pattern-ignored-for-non-mapping", '"text"', '{key_pattern:"^[a-z]+$"}', {}),
    # any_of: alternation. A rejected alternative must contribute no diagnostics
    # of its own -- exactly one `any-of` issue, never one per alternative.
    ("any-of-first-branch", '{startDate:"a"}', '{kind:"map" any_of:[{required:["startDate"]} {required:["endDate"]}]}', {}),
    ("any-of-later-branch", '{endDate:"b"}', '{kind:"map" any_of:[{required:["startDate"]} {required:["endDate"]}]}', {}),
    ("any-of-both-branches", '{startDate:"a" endDate:"b"}', '{kind:"map" any_of:[{required:["startDate"]} {required:["endDate"]}]}', {}),
    ("any-of-no-branch", '{}', '{kind:"map" any_of:[{required:["startDate"]} {required:["endDate"]}]}', {}),
    ("any-of-composes-with-siblings", '{a:1}', '{kind:"map" required:["id"] any_of:[{required:["a"]} {required:["b"]}]}', {}),
    ("any-of-sibling-and-branch-both-fail", '{}', '{kind:"map" required:["id"] any_of:[{required:["a"]} {required:["b"]}]}', {}),
    ("any-of-by-kind", '"text"', '{any_of:[{kind:"int"} {kind:"string"}]}', {}),
    ("any-of-by-kind-fail", 'true', '{any_of:[{kind:"int"} {kind:"string"}]}', {}),
    ("any-of-nested", '2.5', '{any_of:[{any_of:[{kind:"int"} {kind:"float"}]} {kind:"string"}]}', {}),
    ("any-of-single-branch", '1', '{any_of:[{kind:"string"}]}', {}),
    # Collection size, tuple prefix, and homogeneous tail items.
    ("min-items", "[]", '{kind:"list" min_items:1}', {}),
    ("max-items", "[1 2]", '{kind:"list" max_items:1}', {}),
    ("prefix-items", '(1 2)', '{kind:"tuple" prefix_items:[{kind:"int"} {kind:"string"}]}', {}),
    ("items-after-prefix", '(1 "x" -1)', '{kind:"tuple" prefix_items:[{kind:"int"} {kind:"string"}] items:{kind:"int" minimum:0}}', {}),
    ("list-items-two-paths", '[0 0]', '{kind:"list" items:{kind:"int" minimum:1}}', {}),
    ("set-items", '{-1 2}', '{kind:"set" items:{kind:"int" minimum:0}}', {}),
    # Mapping properties, required, additional=false/schema, and stable order.
    ("mapping-valid", '{name:"Alex" age:27}', '{kind:"map" required:["name" "age"] additional:false properties:{name:{kind:"string" min_length:1} age:{kind:"int" minimum:0 maximum:130}}}', {}),
    ("mapping-all-failures", '{name:"" age:200 extra:true}', '{kind:"map" required:["name" "age" "missing"] additional:false properties:{name:{kind:"string" min_length:1} age:{kind:"int" minimum:0 maximum:130}}}', {}),
    ("additional-schema", '{a:1 b:"bad"}', '{kind:"map" properties:{a:{kind:"int"}} additional:{kind:"int"}}', {}),
    ("ordered-mapping", '[a:0 b:"bad"]', '{kind:"ordered-map" required:["c"] properties:{a:{minimum:1}} additional:{kind:"int"}}', {}),
    ("stable-path-order", '{z:"" a:-1}', '{kind:"map" properties:{z:{kind:"string" min_length:1} a:{kind:"int" minimum:0}}}', {}),
    ("stable-code-order", "-1", '{const:5 enum:[3 4] minimum:0}', {}),
    # Nodes: name/style, property sugar, explicit attributes, children and arity.
    ("node-name", 'other{x:1}', '{kind:"node" name:"person"}', {}),
    ("node-style", 'person(1)', '{kind:"node" style:"brace"}', {}),
    ("node-properties-sugar", 'person{name:"" age:-1}', '{kind:"node" required:["name" "age"] properties:{name:{kind:"string" min_length:1} age:{kind:"int" minimum:0}}}', {}),
    ("node-root-required-is-ignored", 'person{}', '{kind:"node" required:["name"]}', {}),
    ("node-root-additional-is-ignored", 'person{extra:true}', '{kind:"node" additional:false}', {}),
    ("node-explicit-empty-properties-block-sugar", 'person{age:-1}', '{kind:"node" properties:{age:{minimum:0}} attributes:{properties:{}}}', {}),
    ("node-attributes", 'person{name:"Alex" extra:true}', '{kind:"node" attributes:{kind:"map" required:["name"] properties:{name:{kind:"string"}} additional:false}}', {}),
    ("node-children", 'person("ok" 0)', '{kind:"node" style:"tuple" children:{kind:"string" min_length:1}}', {}),
    ("node-min-children", 'person()', '{kind:"node" min_children:1}', {}),
    ("node-max-children", 'person[1 2]', '{kind:"node" max_children:1}', {}),
    ("node-form-recursive-regression", '[person{age:0}]', 'schema{kind:"list" items:schema{kind:"node" attributes:schema{kind:"map" properties:{age:field{kind:"int" minimum:1}} additional:false}}}', {}),
    # Local/external refs, pointer escaping and cycles.
    ("local-ref-pass", '{count:2}', '{kind:"map" properties:{count:{"$ref":"#/$defs/positive"}} "$defs":{positive:{kind:"int" minimum:1}}}', {}),
    ("local-ref-fail", '{count:0}', '{kind:"map" properties:{count:{"$ref":"#/$defs/positive"}} "$defs":{positive:{kind:"int" minimum:1}}}', {}),
    ("pointer-escape-slash", '"x"', '{"$defs":{"a/b":{kind:"int"}} "$ref":"#/$defs/a~1b"}', {}),
    ("pointer-escape-tilde", '"x"', '{"$defs":{"a~b":{kind:"int"}} "$ref":"#/$defs/a~0b"}', {}),
    ("pointer-list-index", '"x"', '{prefix_items:[{kind:"int"}] "$ref":"#/prefix_items/0"}', {}),
    ("pointer-empty-fragment-cycle", "1", '{"$ref":"#" kind:"int"}', {}),
    ("pointer-slash-is-root", "1", '{"$ref":"#/" kind:"int"}', {}),
    ("pointer-skips-empty-components", '"x"', '{"$defs":{x:{kind:"int"}} "$ref":"#//$defs//x"}', {}),
    ("pointer-leading-zero-index", '"x"', '{prefix_items:[{kind:"int"}] "$ref":"#/prefix_items/00"}', {}),
    ("pointer-arabic-index", '"x"', '{prefix_items:[{kind:"int"}] "$ref":"#/prefix_items/٠"}', {}),
    ("pointer-superscript-index", "1", '{prefix_items:[{kind:"int"}] "$ref":"#/prefix_items/²"}', {}),
    ("pointer-replacement-order", '"x"', '{"$defs":{"~1":{kind:"int"}} "$ref":"#/$defs/~01"}', {}),
    ("ref-sibling-kind", "1", '{"$defs":{x:{kind:"int"}} "$ref":"#/$defs/x" kind:"string"}', {}),
    ("unresolved-external", "1", '{"$ref":"https://example.com/schema"}', {}),
    ("unresolved-fragment", "1", '{"$ref":"#not-a-pointer"}', {}),
    ("unresolved-bad-escape", "1", '{"$ref":"#/$defs/a~2b" "$defs":{a:{}}}', {}),
    ("unresolved-index", "1", '{prefix_items:[{kind:"int"}] "$ref":"#/prefix_items/9"}', {}),
    ("unresolved-huge-index", "1", '{prefix_items:[{kind:"int"}] "$ref":"#/prefix_items/9999999999999999999"}', {}),
    ("external-ref-fail", "0", '{"$ref":"urn:axon:test:positive"}', {"registry": [["urn:axon:test:positive", '{"$id":"urn:axon:test:positive" kind:"int" minimum:1}']]}),
    ("external-ref-cycle", "1", '{"$ref":"urn:axon:test:a"}', {"registry": [["urn:axon:test:a", '{"$id":"urn:axon:test:a" "$ref":"urn:axon:test:b"}'], ["urn:axon:test:b", '{"$id":"urn:axon:test:b" "$ref":"urn:axon:test:a"}']]}),
    ("recursive-list-ref", '[[["bad"]]]', '{"$defs":{list:{kind:"list" items:{"$ref":"#/$defs/list"}}} "$ref":"#/$defs/list"}', {}),
    # Graph-profile anchors and references validate as their denoted values.
    ("graph-root-anchor-transparent", '&a [1]', '{kind:"list" items:{kind:"int"}}', {"value_profile": "graph"}),
    ("graph-shared-ref-transparent", '[&a {x:1} *a]', '{kind:"list" items:{kind:"map" properties:{x:{kind:"int"}}}}', {"value_profile": "graph"}),
    ("graph-nested-const-transparent", '[&a {x:1} *a]', '{const:[{x:1} {x:1}]}', {"value_profile": "graph"}),
    ("graph-nested-enum-transparent", '[&a {x:1} *a]', '{enum:[[{x:1} {x:1}]]}', {"value_profile": "graph"}),
    ("graph-cycle-transparent", '&a [*a]', '{"$defs":{list:{kind:"list" items:{"$ref":"#/$defs/list"}}} "$ref":"#/$defs/list"}', {"value_profile": "graph"}),
    # Invalid schemas and deterministic resource/issue limits.
    ("invalid-schema", "1", '{properties:[]}', {}),
    ("normalization-resource-limit", "[]", '{items:{items:{items:{kind:"int"}}}}', {"max_depth": 2}),
    ("validation-depth-limit", '[[[[1]]]]', '{"$defs":{list:{kind:"list" items:{"$ref":"#/$defs/list"}}} "$ref":"#/$defs/list"}', {"max_depth": 6}),
    ("self-ref-depth-one", "1", '{"$ref":"#"}', {"max_depth": 1}),
    ("self-ref-depth-two", "1", '{"$ref":"#"}', {"max_depth": 2}),
    ("issue-limit-two", '{a:0 b:0 c:0}', '{kind:"map" properties:{a:{minimum:1} b:{minimum:1} c:{minimum:1}}}', {"max_issues": 2}),
    ("issue-limit-one", '{a:0 b:0}', '{kind:"map" properties:{a:{minimum:1} b:{minimum:1}}}', {"max_issues": 1}),
]


def validation_records():
    records = []
    for case_id, value_source, schema_source, settings in VALIDATION_CASES:
        registry = SchemaRegistry2026()
        for identifier, module_source in settings.get("registry", []):
            registry.register(identifier, load_one(module_source))
        record = {
            "id": case_id,
            "value": value_source,
            "schema": schema_source,
            "value_profile": settings.get("value_profile", "core"),
        }
        if "registry" in settings:
            record["registry"] = [
                {"identifier": identifier, "schema": module_source}
                for identifier, module_source in settings["registry"]
            ]
        for key in ("max_depth", "max_issues"):
            if key in settings:
                record[key] = settings[key]
        record["report"] = report_record(
            value_source,
            schema_source,
            registry,
            value_profile=record["value_profile"],
            max_depth=settings.get("max_depth", 128),
            max_issues=settings.get("max_issues", 256),
        )
        records.append(record)
    return records


def issue_ordering_record():
    source = [
        ValidationIssue("z-code", ("z",), "last", ("properties", "z")),
        ValidationIssue("minimum", ("a",), "low", ("properties", "a", "minimum")),
        ValidationIssue("minimum", ("a",), "low", ("properties", "a", "minimum")),
        ValidationIssue("enum", ("a",), "enum", ("properties", "a", "enum")),
        ValidationIssue("kind", (1,), "integer path", ("items",)),
        ValidationIssue("kind", ("1",), "string path", ("properties", "1")),
        ValidationIssue("a-code", (), "root", ()),
    ]
    return {
        "input": [issue_record(issue) for issue in source],
        "ordered_unique": [issue_record(issue) for issue in _ordered_issues(source)],
    }


# Direct safe-regex oracle.  In addition to security boundaries, these cases
# pin Python's Unicode and flag semantics so a Rust engine cannot silently use
# ASCII-only classes or a subtly different case-folding model.
REGEX_CASES = [
    ("empty", "", "anything"),
    ("literal-match", "axon", "pyaxon"),
    ("literal-miss", "axon", "AXON"),
    ("dot-newline-default", "^a.b$", "a\nb"),
    ("dotall", "(?s)^a.b$", "a\nb"),
    ("multiline", "(?m)^b$", "a\nb\nc"),
    ("alternation", "cat|dog", "a dog"),
    ("character-range", "^[A-Z]{3}$", "ABC"),
    ("negated-class", "^[^0-9]{2}$", "éΩ"),
    ("unicode-digit", r"^\d+$", "١٢٣"),
    ("unicode-digit-superscript", r"^\d+$", "²"),
    ("ascii-digit-flag", r"(?a)^\d+$", "١"),
    ("unicode-word-latin", r"^\w+$", "café"),
    ("unicode-word-cjk", r"^\w+$", "日本語"),
    ("unicode-word-combining-mark", r"^\w+$", "é"),
    ("unicode-space-nbsp", r"^\s$", " "),
    ("unicode-space-em", r"^\s$", " "),
    ("ascii-space-flag", r"(?a)^\s$", " "),
    ("word-boundary", r"\bcafé\b", "un café ici"),
    ("unicode-ignorecase-kelvin", "(?i)^[a-z]+$", "K"),
    ("unicode-ignorecase-long-s", "(?i)^[a-z]+$", "ſ"),
    ("unicode-ignorecase-dotless-i", "(?i)^[a-z]+$", "ı"),
    ("ascii-ignorecase-kelvin", "(?ai)^[a-z]+$", "K"),
    ("scoped-ignorecase", "^(?i:axon)$", "AxOn"),
    ("verbose", "(?x)^ a [ ] b $", "a b"),
    ("absolute-anchor", r"\Aaxon\Z", "axon"),
    ("fixed-repeat-unanchored", "a{2}", "baac"),
    ("fixed-repeat-unanchored-limit", "a{4096}", "a"),
    ("fixed-repeat-cumulative-limit", "a{2048}a{2048}", "a"),
    ("fixed-branch-cumulative-limit", "a{2048}|b{2048}", "c"),
    ("oversized-fixed-unanchored", "a{4097}", "a"),
    ("oversized-fixed-failing-suffix", "a{500000}b", "aaaa"),
    ("oversized-fixed-anchored", "^a{500000}b$", "a"),
    ("oversized-fixed-cumulative", "a{4096}" * 100 + "b", "a"),
    ("oversized-fixed-branches", "a{2049}|b{2049}", "c"),
    ("oversized-fixed-cumulative-anchored", "^" + "a{4096}" * 100 + "b$", "a"),
    ("fixed-repeat-with-variable", "^a+b{2}$", "aaabb"),
    ("bounded-variable", "^a{1,3}$", "aaa"),
    ("lazy-repeat", "^a+?$", "aaa"),
    ("possessive-repeat", "^a++$", "aaa"),
    ("possessive-repeat-commits", "^a++a$", "aa"),
    ("named-capture", "^(?P<word>[A-Z]{2})$", "AX"),
    ("capturing-hidden-start-anchor", "(^a+)", "aaa"),
    ("noncapturing-start-anchor", "(?:^a+)", "aaa"),
    ("scoped-hidden-start-anchor", "(?i:^a+)", "AAA"),
    ("atomic-group", "^(?>a+)b$", "aaab"),
    ("atomic-hidden-start-anchor", "(?>^a+)", "aaa"),
    ("atomic-alternation-commits", "(?>a|ab)c", "abc"),
    ("atomic-unanchored-variable", "(?>a+)b", "aaaa"),
    ("atomic-nested-repeat", "^(?>(a+)+)$", "aaaa"),
    ("ignorecase-sharp-s-is-not-single-s", "(?i)^ß$", "S"),
    ("invalid-unclosed-class", "[", "x"),
    ("invalid-unclosed-group", "(abc", "abc"),
    ("invalid-escape", r"\k", "k"),
    ("backreference", r"^(a)\1$", "aa"),
    ("positive-lookahead", "^(?=a)a$", "a"),
    ("negative-lookahead", "^(?!b)a$", "a"),
    ("positive-lookbehind", "(?<=a)b", "ab"),
    ("conditional", "^(a)?(?(1)b|c)$", "ab"),
    ("nested-repeat", "^(a+)+$", "aaaa"),
    ("nested-fixed-repeat", "^(a{2}){3}$", "aaaaaa"),
    ("branch-in-repeat", "^(?:ab|cd)+$", "abcd"),
    ("unanchored-variable", "a+", "aaaa"),
    ("multiple-variable", "^a*b*$", "aaabbb"),
    ("branch-two-variable-paths", "^(?:a+|b+)$", "aaa"),
    ("anchored-one-variable", "^[A-Z]+$", "AXON"),
    ("start-string-one-variable", r"\A\w+", "axon!"),
    ("end-only-not-start", "a+$", "aaaa"),
    ("length-1024", "x" * 1024, "x" * 1024),
    ("length-1025", "x" * 1025, "x" * 1025),
]


def regex_records():
    records = []
    for case_id, pattern, value in REGEX_CASES:
        record = {"id": case_id, "pattern": pattern, "value": value}
        try:
            matched = _safe_regex_search(pattern, value)
        except (TypeError, ValueError, re.error) as exc:
            record.update(error_record(exc))
        else:
            record.update({"outcome": "match" if matched else "no-match"})
        records.append(record)
    return records


def main():
    payload = OrderedDict([
        ("note", "AXON Schema 2026 ground truth from pyaxon; values and schemas are AXON source and issue paths retain typed string/integer segments"),
        ("dialect", SCHEMA_DIALECT_2026),
        ("normalization", normalization_records()),
        ("registry", OrderedDict([
            ("identifiers", registry_identifier_records()),
            ("config", registry_config_records()),
            ("scenarios", registry_scenario_records()),
        ])),
        ("validation", validation_records()),
        ("issue_ordering", issue_ordering_record()),
        ("safe_regex", regex_records()),
    ])
    text = json.dumps(payload, indent=1, ensure_ascii=False) + "\n"
    if len(sys.argv) > 1:
        with open(sys.argv[1], "w", encoding="utf-8", newline="\n") as stream:
            stream.write(text)
    else:
        sys.stdout.buffer.write(text.encode("utf-8"))


if __name__ == "__main__":
    main()
