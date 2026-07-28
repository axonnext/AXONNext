#!/usr/bin/env python3
"""Generate cross-surface parity fixtures from the Python AXON 2026 reference.

This oracle covers the public surfaces that are not naturally represented by
the text, CST, or Schema fixture families: Binary AXON, CIDs and canonical
verification, profile-sensitive and incremental text behavior, migration,
editor/LSP helpers, scientific arrays, and secured stream envelopes.

Run from the repository root with ``PYTHONPATH=lib``::

    python serde_axon/tests/gen_surfaces.py serde_axon/tests/surfaces.json
"""

from __future__ import annotations

import json
import sys

from axon2.binary2026 import decode as binary_decode
from axon2.binary2026 import encode as binary_encode
from axon2.lsp2026 import (
    diagnostics2026,
    document_symbols2026,
    formatting_edits2026,
    hover2026,
)
from axon2.scientific import from_numpy, to_numpy
from axon2.stream import EnvelopeStream
from axon2.v2026 import (
    Axon2026Error,
    Node,
    Options,
    cid2026,
    dumps2026,
    iloads2026,
    loads2026,
    migrate2026,
    parse_cid2026,
    verify_canonical2026,
    verify_cid2026,
)


REFERENCE = "pyaxon 0.11.0a13"


def options_for(profile: str) -> Options:
    if profile == "core":
        return Options()
    if profile == "graph":
        return Options(graph=True)
    if profile == "doc_link":
        return Options(doc_link=True)
    if profile == "compat":
        return Options.compat()
    if profile == "scale_significant":
        return Options(scale_significant=True)
    if profile == "canonical_verify":
        return Options(canonical_verify=True)
    if profile == "tz_names":
        return Options(tz_names=True)
    if profile == "indented_bodies":
        return Options(indented_bodies=True)
    if profile == "max_input_4":
        return Options(max_input=4)
    raise ValueError(profile)


def canonical_one(value, *, graph=False) -> str:
    return dumps2026([value], canonical=True, graph=graph)


def exception_record(exc) -> dict:
    """Keep the public error contract without depending on traceback wording."""
    record = {
        "outcome": "error",
        "category": getattr(exc, "category", type(exc).__name__),
        "message": exc.args[0] if exc.args else str(exc),
    }
    if isinstance(exc, Axon2026Error):
        record.update({
            "offset": exc.offset,
            "end_offset": exc.end_offset,
            "line": exc.line,
            "column": exc.column,
        })
    return record


def binary_vectors():
    valid_sources = [
        ("null", "core", "null"),
        ("bool", "core", "true"),
        ("small-int", "core", "23"),
        ("negative-int", "core", "-1000"),
        ("big-int", "core", "123456789012345678901234567890"),
        ("float-half", "core", "1.5"),
        ("float-single", "core", "100000.0"),
        ("float-double", "core", "1e300"),
        ("float-negative-zero", "core", "-0.0"),
        ("float-infinity", "core", "∞"),
        ("float-nan", "core", "?"),
        ("decimal", "core", "1000.350D"),
        ("decimal-infinity", "core", "∞D"),
        ("string-unicode", "core", '"café ☕"'),
        ("binary", "core", "|AAEC/w==|"),
        ("date", "core", "^2026-07-18"),
        ("time-local", "core", "^12:30:45"),
        ("time", "core", "^12:30:45.123456789Z"),
        ("datetime-local", "core", "^2026-07-18T12:30:45.500"),
        ("datetime-offset", "core", "^2026-07-18T12:30:45-06:00"),
        (
            "datetime-named-zone",
            "tz_names",
            "^2026-07-18T12:30:45-06:00[America/Edmonton]",
        ),
        ("list", "core", "[1 true null]"),
        ("tuple", "core", "(1 2 3)"),
        ("map", "core", '{z:1 a:"x"}'),
        ("ordered-map", "core", "[z:1 a:2]"),
        ("set", "core", "{3 1 2}"),
        ("node", "core", 'point{x:1 y:-2 "label"}'),
        (
            "cid-link",
            "doc_link",
            'cid("bafkreiec5tzcwyhehbpob5yir5idmjfqf2qzpiiguoia4qt5fquhrbx4ym")',
        ),
        ("uri-link", "doc_link", 'link("https://example.com/a")'),
        ("graph-shared", "graph", "[&item point{x:1} *item]"),
        ("graph-cycle", "graph", "&root [*root]"),
    ]
    cases = []
    for ident, profile, source in valid_sources:
        value = loads2026(source, options=options_for(profile))[0]
        compact = binary_encode(value)
        canonical = binary_encode(value, canonical=True)
        binary_decode(compact)
        binary_decode(canonical)
        cases.append({
            "id": ident,
            "profile": profile,
            "input": source,
            "compact_hex": compact.hex(),
            "canonical_hex": canonical.hex(),
            "canonical_text": canonical_one(value, graph=(profile == "graph")),
            "compact_decode_outcome": "accept",
            "canonical_decode_outcome": "accept",
        })
    malformed = []
    for ident, payload in [
        ("empty", b""),
        ("truncated-head", b"\x18"),
        ("truncated-text", b"\x63ab"),
        ("invalid-utf8", b"\x61\xff"),
        ("trailing-data", b"\xf6\xf6"),
        ("unknown-tag", b"\xd9\x03\xe7\xf6"),
    ]:
        record = {"id": ident, "hex": payload.hex()}
        try:
            binary_decode(payload)
            record["outcome"] = "accept"
        except Exception as exc:  # noqa: BLE001 - public error outcome is shared
            record.update(exception_record(exc))
        malformed.append(record)
    return {"cases": cases, "malformed": malformed}


def cid_vectors():
    cases = []
    for ident, source in [
        ("null", "null"),
        ("map-order", "{z:1 a:2}"),
        ("unicode", '"café"'),
        ("decimal", "1000.350D"),
        ("node", "point{x:1 y:2}"),
    ]:
        value = loads2026(source)[0]
        text = cid2026(value)
        cases.append({
            "id": ident,
            "input": source,
            "cid": text,
            "digest_hex": parse_cid2026(text).hex(),
            "verify": verify_cid2026(text, value),
        })
    good = cases[0]["cid"]
    invalid = []
    for ident, text in [
        ("empty", ""),
        ("wrong-multibase", "z" + good[1:]),
        ("uppercase", good.upper()),
        ("padding", good + "="),
        ("truncated", good[:-1]),
        ("garbage", "not-a-cid"),
    ]:
        try:
            parse_cid2026(text)
            outcome = "accept"
        except Exception:  # noqa: BLE001
            outcome = "error"
        invalid.append({"id": ident, "input": text, "outcome": outcome})
    return {"cases": cases, "invalid": invalid}


def canonical_verification_vectors():
    sources = [
        ("null", "null"),
        ("stream", "1\n2"),
        ("trailing-newline", "1\n"),
        ("map-sorted", "{a:2 z:1}"),
        ("map-unsorted", "{z:1 a:2}"),
        ("set-sorted", "{1 2 3}"),
        ("decimal-normal", "1000.35D"),
        ("decimal-scale", "1000.350D"),
        ("graph-canonical-stream", "&1 []\n*1"),
        ("graph-label-stream", "&item []\n*item"),
    ]
    return [
        {"id": ident, "input": source, "expected": verify_canonical2026(source)}
        for ident, source in sources
    ]


def text_profile_vectors():
    """Exact text outcomes for profile-sensitive values outside the core corpus."""
    sources = [
        ("scale-time-short", "scale_significant", "^12:00:00.5"),
        ("scale-time-long", "scale_significant", "^12:00:00.500"),
        ("scale-time-zero", "scale_significant", "^12:00:00.000"),
        (
            "scale-datetime",
            "scale_significant",
            "^2026-07-18T12:30:45.500000000Z",
        ),
        ("scale-decimal", "scale_significant", "1.0D 1.00D"),
        (
            "tz-ambiguous-summer",
            "tz_names",
            "^2026-11-01T01:30:00-06:00[America/Edmonton]",
        ),
        (
            "tz-ambiguous-winter",
            "tz_names",
            "^2026-11-01T01:30:00-07:00[America/Edmonton]",
        ),
        (
            "tz-nonexistent",
            "tz_names",
            "^2026-03-08T02:30:00-07:00[America/Edmonton]",
        ),
        (
            "tz-offset-mismatch",
            "tz_names",
            "^2026-07-18T12:30:45-07:00[America/Edmonton]",
        ),
        (
            "tz-profile-required",
            "core",
            "^2026-07-18T12:30:45-06:00[America/Edmonton]",
        ),
        ("uri-https", "doc_link", 'link("https://example.com/a?b=c#d")'),
        ("uri-mailto", "doc_link", 'link("mailto:person@example.com")'),
        ("uri-urn", "doc_link", 'link("urn:example:animal:ferret:nose")'),
        ("uri-relative", "doc_link", 'link("relative/path")'),
        ("uri-space", "doc_link", 'link("https://example.com/a b")'),
        ("uri-bad-percent", "doc_link", 'link("https://example.com/%zz")'),
        ("uri-nonascii", "doc_link", 'link("https://example.com/caf\u00e9")'),
        ("uri-bad-port", "doc_link", 'link("https://example.com:99999/a")'),
        ("uri-http-no-host", "doc_link", 'link("https:/path")'),
    ]
    records = []
    for ident, profile, source in sources:
        record = {"id": ident, "profile": profile, "input": source}
        try:
            values = loads2026(source, options=options_for(profile))
            record.update({
                "outcome": "accept",
                "count": len(values),
                "compact": dumps2026(values, graph=(profile == "graph")),
            })
            try:
                record["canonical"] = {
                    "outcome": "accept",
                    "text": dumps2026(values, canonical=True, graph=True),
                }
            except Exception as exc:  # noqa: BLE001
                record["canonical"] = exception_record(exc)
        except Exception as exc:  # noqa: BLE001
            record.update(exception_record(exc))
        records.append(record)
    return records


def incremental_vectors():
    """Exercise iloads2026's prefix, preprocessing, atomicity, and limits."""
    sources = [
        ("prefix-before-error", "core", "1 ["),
        ("attribute-document-atomic", "core", "a:1 b:2"),
        ("indented-preprocessing", "indented_bodies", "person\n  age:3\n"),
        ("canonical-valid", "canonical_verify", "{a:1}"),
        ("canonical-invalid", "canonical_verify", "{ a:1 }"),
        ("max-input-preflight", "max_input_4", "12345"),
        ("discard-then-value", "core", "#_ 1 2"),
        (
            "header-profile",
            "core",
            'axon{require:["doc-link"]} link("https://example.com/a")',
        ),
        ("compat-numbers", "compat", "007 5."),
    ]
    records = []
    for ident, profile, source in sources:
        iterator = iter(iloads2026(source, options=options_for(profile)))
        yielded = []
        terminal = {"outcome": "end"}
        while True:
            try:
                value = next(iterator)
                yielded.append(canonical_one(value, graph=True))
            except StopIteration:
                break
            except Exception as exc:  # noqa: BLE001
                terminal = exception_record(exc)
                if isinstance(exc, Axon2026Error):
                    terminal.update({"line": exc.line, "column": exc.column})
                break
        fused = None
        if terminal["outcome"] == "error":
            try:
                next(iterator)
                fused = False
            except StopIteration:
                fused = True
            except Exception:
                fused = False
        records.append({
            "id": ident,
            "profile": profile,
            "input": source,
            "yielded": yielded,
            "terminal": terminal,
            "fused_after_error": fused,
        })
    return records


def migration_vectors():
    sources = [
        ("canonical-leading-zero", "canonical", False, "007"),
        ("canonical-temporal", "canonical", False, "007\n12:30"),
        ("selective-leading-zero", "selective", False, "# keep\n007\n"),
        ("selective-unchanged", "selective", False, "# keep\n7\n"),
        ("selective-comment-opener", "selective", False, "#_ legacy comment\n1"),
        ("selective-trailing-point", "selective", False, "5."),
        ("selective-binary", "selective", False, "|YQ=="),
        ("selective-fraction", "selective", False, "^12:00:00.5"),
        ("selective-continuation", "selective", False, '"ab\\\nc"'),
        ("selective-indented", "selective", False, "person\n  name:\"Alex\"\n"),
        ("fallback-duplicate-map", "selective", False, "{a:1 a:2}"),
        ("fallback-duplicate-set", "selective", False, "{1 1}"),
        ("legacy-binding", "canonical", False, "Rgb(1 2 3)"),
        ("graph-stream-canonical", "canonical", True, "&item []\n*item"),
        ("graph-stream-selective", "selective", True, "&item []\n*item"),
        ("unicode-byte-edits", "selective", False, '"é"\n007\n12:30'),
        ("unknown-reference", "canonical", True, "*missing"),
    ]
    records = []
    for ident, operation, graph, source in sources:
        record = {
            "id": ident,
            "operation": operation,
            "graph": graph,
            "input": source,
        }
        try:
            result = migrate2026(
                source, graph=graph, selective=(operation == "selective"))
            record.update({
                "outcome": "accept",
                "output": str(result),
                "mode": result.mode,
                "events": [
                    {
                        "code": event.code,
                        "message": event.message,
                        "offset": event.offset,
                    }
                    for event in result.events
                ],
                "edits": [
                    {
                        "code": edit.code,
                        "start": edit.start,
                        "end": edit.end,
                        "replacement": edit.replacement,
                    }
                    for edit in result.edits
                ],
            })
        except Exception as exc:  # noqa: BLE001
            record.update(exception_record(exc))
        records.append(record)
    return records


def lsp_vectors():
    diagnostics = []
    for ident, profile, source in [
        ("double-comma", "core", "[1,,2]"),
        ("missing-close", "core", "[1"),
        ("multibyte-error", "core", '["é" @]'),
        ("clean", "core", "point{x:1}"),
    ]:
        diagnostics.append({
            "id": ident,
            "profile": profile,
            "input": source,
            "result": diagnostics2026(source, options=options_for(profile)),
        })

    hover = []
    hover_source = 'étage{name:"A"} # tail'
    for ident, offset in [
        ("multibyte-interior", 1),
        ("attribute-name", len("étage{".encode("utf-8"))),
        ("trivia", len('étage{name:"A"}'.encode("utf-8"))),
        ("eof", len(hover_source.encode("utf-8"))),
    ]:
        hover.append({
            "id": ident,
            "profile": "core",
            "input": hover_source,
            "byte_offset": offset,
            "result": hover2026(hover_source, offset),
        })

    formatting = []
    for ident, source, indent in [
        ("nested-comment", "{a:1 # keep\n b:[2 3]}", 2),
        ("stream", "false\n{nested:(40.3)}", 4),
        ("indent-zero", "{a:[1 2] b:3}", 0),
    ]:
        formatting.append({
            "id": ident,
            "profile": "core",
            "input": source,
            "indent": indent,
            "result": formatting_edits2026(source, indent=indent),
        })

    symbols = []
    for ident, profile, source in [
        ("nested", "core", "outer{list:[leaf] child inner{x:1}}"),
        ("map-path", "core", "{root:item{nested}}"),
        ("malformed", "core", "outer{[}"),
        ("graph-cycle", "graph", "&a [outer *a]"),
    ]:
        symbols.append({
            "id": ident,
            "profile": profile,
            "input": source,
            "result": document_symbols2026(
                source, options=options_for(profile)),
        })
    return {
        "diagnostics": diagnostics,
        "hover": hover,
        "formatting": formatting,
        "symbols": symbols,
    }


def scientific_vectors():
    try:
        import numpy as np
    except ImportError as exc:  # pragma: no cover - generator prerequisite
        raise RuntimeError("numpy is required to generate scientific fixtures") from exc

    values = {
        "u8": [0, 255],
        "u16": [0, 65535],
        "u32": [0, 4294967295],
        "u64": [0, 18446744073709551615],
        "i8": [-128, 127],
        "i16": [-32768, 32767],
        "i32": [-2147483648, 2147483647],
        "i64": [-9223372036854775808, 9223372036854775807],
        "f16": [1.5, -2.0],
        "f32": [1.5, -2.0],
        "f64": [3.25, -0.0],
        "c64": [1 + 2j, -3 + 0.5j],
        "c128": [1 + 2j, -3 + 0.5j],
    }
    dtype_names = {
        "u8": "uint8", "u16": "uint16", "u32": "uint32", "u64": "uint64",
        "i8": "int8", "i16": "int16", "i32": "int32", "i64": "int64",
        "f16": "float16", "f32": "float32", "f64": "float64",
        "c64": "complex64", "c128": "complex128",
    }
    cases = []
    for tag, items in values.items():
        array = np.asarray(items, dtype=dtype_names[tag])
        node = from_numpy(array)
        decoded = to_numpy(node)
        cases.append({
            "id": tag,
            "tag": tag,
            "element_size": int(array.dtype.itemsize),
            "shape": list(array.shape),
            "order": node.attributes.get("order", "C"),
            "data_hex": node.attributes["data"].hex(),
            "source": canonical_one(node),
            "roundtrip_data_hex": decoded.tobytes(
                order=node.attributes.get("order", "C")).hex(),
        })

    fortran = np.asfortranarray(np.asarray([[1, 2, 3], [4, 5, 6]], dtype="int32"))
    node = from_numpy(fortran)
    cases.append({
        "id": "fortran-i32",
        "tag": "i32",
        "element_size": 4,
        "shape": list(fortran.shape),
        "order": "F",
        "data_hex": node.attributes["data"].hex(),
        "source": canonical_one(node),
        "roundtrip_data_hex": to_numpy(node).tobytes(order="F").hex(),
    })

    edge_sources = [
        ("default-empty-f32", "Array{shape:[0]}"),
        ("default-type", "Array{shape:[1] data:|AAAAAA==|}"),
        ("default-shape-scalar", 'Array{type:"u8" data:|Bw==|}'),
        ("child-ignored", 'Array{shape:[1] type:"u8" data:|Bw==| null}'),
        ("integer-shape", 'Array{shape:2 type:"u8" data:|AQI=|}'),
        ("null-shape-flattens", 'Array{shape:null type:"u8" data:|AQI=|}'),
        ("tuple-shape", 'Array{shape:(1 2) type:"u8" data:|AQI=|}'),
        ("inferred-shape", 'Array{shape:[-1 2] type:"u8" data:|AQIDBA==|}'),
        ("lowercase-order", 'Array{shape:[2] type:"u8" data:|AQI=| order:"f"}'),
        ("automatic-order", 'Array{shape:[2] type:"u8" data:|AQI=| order:"A"}'),
        ("null-order", 'Array{shape:[2] type:"u8" data:|AQI=| order:null}'),
    ]
    numpy_to_tag = {name: tag for tag, name in dtype_names.items()}
    edge = []
    for ident, source in edge_sources:
        record = {"id": ident, "source": source}
        value = loads2026(source)[0]
        try:
            decoded = to_numpy(value)
            order = value.attributes.get("order", "C")
            resolved_order = "F" if isinstance(order, str) and order.lower() == "f" else "C"
            record.update({
                "outcome": "accept",
                "dtype": numpy_to_tag[decoded.dtype.name],
                "shape": list(decoded.shape),
                "order": resolved_order,
                "data_hex": value.attributes.get("data", b"").hex(),
            })
        except Exception as exc:  # noqa: BLE001
            record.update(exception_record(exc))
        edge.append(record)

    invalid_sources = [
        ("wrong-node", "NotArray{shape:[1] type:\"u8\" data:|AA==|}"),
        ("unknown-dtype", "Array{shape:[1] type:\"x8\" data:|AA==|}"),
        ("shape-entry", "Array{shape:[\"x\"] type:\"u8\" data:|AA==|}"),
        ("short-data", "Array{shape:[2 2] type:\"f32\" data:|AACAPwAAAEA=|}"),
        ("wrong-data", "Array{shape:[1] type:\"u8\" data:\"x\"}"),
        ("bad-order", "Array{shape:[1] type:\"u8\" data:|AA==| order:\"X\"}"),
    ]
    invalid = []
    for ident, source in invalid_sources:
        value = loads2026(source)[0]
        record = {"id": ident, "source": source}
        try:
            to_numpy(value)
            record["outcome"] = "accept"
        except Exception as exc:  # noqa: BLE001
            record.update(exception_record(exc))
        invalid.append(record)
    return {"cases": cases, "edge": edge, "invalid": invalid}


def stream_error_code(exc):
    if isinstance(exc, Axon2026Error):
        return exc.category
    message = str(exc)
    for fragment, code in [
        ("already open", "nested-start"),
        ("without a prior", "end-without-start"),
        ("count mismatch", "count-mismatch"),
        ("outside of a stream", "outside-envelope"),
        ("ended abruptly", "abrupt-end"),
    ]:
        if fragment in message:
            return code
    return type(exc).__name__


def stream_vectors():
    sources = [
        ("valid-count", "StreamStart 1 2 StreamEnd{count:2}"),
        ("valid-no-count", "StreamStart 1 2 StreamEnd"),
        ("zero", "StreamStart StreamEnd{count:0}"),
        ("numeric-equal-bool", "StreamStart 1 StreamEnd{count:true}"),
        ("numeric-equal-float", "StreamStart 1 StreamEnd{count:1.0}"),
        ("numeric-equal-decimal", "StreamStart 1 StreamEnd{count:1D}"),
        ("zero-false", "StreamStart StreamEnd{count:false}"),
        ("zero-float", "StreamStart StreamEnd{count:0.0}"),
        ("null-count-empty", "StreamStart StreamEnd{count:null}"),
        ("null-count-nonempty", "StreamStart 1 StreamEnd{count:null}"),
        ("two-envelopes", "StreamStart 1 StreamEnd{count:1} StreamStart 2 StreamEnd{count:1}"),
        ("mismatch", "StreamStart 1 StreamEnd{count:2}"),
        ("mismatch-false", "StreamStart 1 StreamEnd{count:false}"),
        ("mismatch-float", "StreamStart 1 StreamEnd{count:2.0}"),
        ("mismatch-bytes", "StreamStart 1 StreamEnd{count:|eA==|}"),
        ("string-count", 'StreamStart 1 StreamEnd{count:"1"}'),
        ("negative-count", "StreamStart 1 StreamEnd{count:-1}"),
        ("outside", "0 StreamStart 1 StreamEnd"),
        ("outside-empty-list", "[] StreamStart StreamEnd"),
        ("end-without-start", "StreamEnd"),
        ("abrupt", "StreamStart 1"),
        ("nested-start", "StreamStart 1 StreamStart 2 StreamEnd"),
        ("parse-error", "StreamStart 1 ["),
    ]
    records = []
    for ident, source in sources:
        iterator = iter(EnvelopeStream(iloads2026(source)))
        yielded = []
        terminal = {"outcome": "end"}
        while True:
            try:
                item = next(iterator)
                yielded.append(canonical_one(item, graph=True))
            except StopIteration:
                break
            except Exception as exc:  # noqa: BLE001
                terminal = {
                    "outcome": "error",
                    "code": stream_error_code(exc),
                    "message": exc.args[0] if exc.args else str(exc),
                }
                break
        fused = False
        if terminal["outcome"] == "error":
            try:
                next(iterator)
            except StopIteration:
                fused = True
        records.append({
            "id": ident,
            "input": source,
            "yielded": yielded,
            "terminal": terminal,
            "fused_after_error": fused,
        })
    return records


def main():
    payload = {
        "note": "cross-surface differential ground truth from " + REFERENCE,
        "binary": binary_vectors(),
        "cid": cid_vectors(),
        "canonical_verification": canonical_verification_vectors(),
        "text_profiles": text_profile_vectors(),
        "incremental": incremental_vectors(),
        "migration": migration_vectors(),
        "lsp": lsp_vectors(),
        "scientific": scientific_vectors(),
        "stream": stream_vectors(),
    }
    text = json.dumps(payload, indent=1, ensure_ascii=False) + "\n"
    if len(sys.argv) > 1:
        with open(sys.argv[1], "w", encoding="utf-8", newline="\n") as stream:
            stream.write(text)
    else:
        sys.stdout.buffer.write(text.encode("utf-8"))


if __name__ == "__main__":
    main()
