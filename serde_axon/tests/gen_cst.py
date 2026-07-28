#!/usr/bin/env python3
"""Generate CST ground truth from the pyaxon 2026 reference.

For each (profile, input) case this records the exact token stream produced by
`parse_cst2026` -- every token's kind, text, and byte span -- so `cst.rs` can
assert serde_axon's tokenizer produces an identical stream. The token stream is
the lossless contract: concatenating token texts must reproduce the source, and
byte spans must agree across implementations.

The fixture also snapshots complete documents (production nodes, trivia,
diagnostics/fixes, and semantic ownership), indentation migration, and exact
formatter output. Node references use deterministic pre-order integer IDs.

The corpus deliberately exercises every tokenizer branch (whitespace, `#` and
`#_` comments, discard, raw strings, quoted forms, binary, temporal, the
punctuation/marker set, atoms, names) and includes multibyte UTF-8 so the byte
spans (not character offsets) are checked.

Run from the AXON Next dir with PYTHONPATH=lib:
    python serde_axon/tests/gen_cst.py serde_axon/tests/cst.json
"""

import json
import sys

from axon2.cst2026 import (
    CstNode,
    CstToken,
    format_cst2026,
    migrate_indented2026,
    parse_cst2026,
)
from axon2.v2026 import Axon2026Error, Options


def profile(name):
    if name == "core":
        return Options()
    if name == "compat":
        return Options(compatibility=True)
    if name == "legacy_binary":
        return Options(legacy_binary=True)
    if name == "hash_comment":
        return Options(hash_underscore_comment=True)
    if name == "graph":
        return Options(graph=True)
    if name == "tz_names":
        return Options(tz_names=True)
    raise ValueError(name)


CASES = [
    # whitespace + names + atoms
    ("core", "42"),
    ("core", "-17"),
    ("core", "  \t\r\n 1  "),
    ("core", "hello world"),
    ("core", "[1 2 3]"),
    ("core", "{a:1 b:2}"),
    ("core", "point{x:1 y:2}"),
    ("core", "(1 2 3)"),
    ("core", "[:]"),
    # comments
    ("core", "# a comment\n1"),
    ("core", "1 # trailing\n2"),
    # discard vs comment for #_
    ("core", "#_ 1 2"),
    ("core", "[1 #_ 2 3]"),
    ("hash_comment", "#_ this is a comment\n1"),
    ("compat", "#_ comment in compat\n2"),
    # raw strings
    ("core", 'r"x\\y"'),
    ("core", 'r#"a"b"#'),
    ("core", 'r##"a"#b"##'),
    ("core", 'r"unterminated'),
    # quoted forms
    ("core", '"hello"'),
    ("core", '"with \\" escape"'),
    ("core", "'quoted name'"),
    ("core", "`back`"),
    ("core", '"unterminated'),
    ("core", "'weird name'{x:1}"),
    # binary
    ("core", "|SGVsbG8=|"),
    ("core", "|deadbeef|"),
    ("legacy_binary", "|SGVsbG8=|"),
    ("compat", "|SGVsbG8=| rest"),
    # temporal
    ("core", "^2026-01-01"),
    ("core", "^12:00:00.5"),
    ("core", "^2026-01-01T12:00:00Z"),
    ("core", "^2026-01-01T12:00:00+05:00[America/New_York]"),
    ("core", "[^2026-01-01, 1]"),
    ("core", "^2026-01-01#comment"),
    # anchors / references / constants
    ("core", "&a 1"),
    ("core", "*b"),
    ("core", "$const"),
    ("core", "[&a point{x:1} *a]"),
    # special atoms
    ("core", "∞"),
    ("core", "-∞"),
    ("core", "?"),
    ("core", "∅"),
    ("core", "∞abc"),
    # multibyte UTF-8 (byte spans must differ from char offsets)
    ("core", "café"),
    ("core", '"café ☕ résumé"'),
    ("core", "{café:1 naïve:2}"),
    ("core", "^2026-01-01T00:00:00+00:00[Café/Zürich]"),
    ("core", "日本語 x"),
    ("core", "😀 emoji"),
    # Python's `str.isdigit()` classification is intentionally broader than
    # ASCII decimal digits. These distinguish it from both ASCII-only digit
    # checks and the still broader Unicode numeric class (`½` is numeric but
    # not a Python digit).
    ("core", "١"),
    ("core", "²"),
    ("core", "½"),
    # nested / mixed
    ("core", "axon{edition:\"2026\"} 1"),
    ("core", "{a:[1 2] b:{c:3}}"),
    ("core", "[&root [1 2 3] *root]"),
    ("core", "p{x:1 x:2}"),
    ("core", ""),
    ("core", "   "),
    ("core", "\n\n\n"),
]

# Sources for the indentation->brace migration. Each is fed to
# migrate_indented2026; the ground truth is the exact migrated string (or an
# error outcome when the migration would produce invalid AXON).
MIGRATE_CASES = [
    ("core", "point\n  x:1\n  y:2\n"),
    ("core", "root\n  a:1\n  child\n    b:2\n"),
    ("core", "point\n  x:1\n  y:2\n  z:3\n"),
    ("core", "a:1\nb:2\n"),
    ("core", "config\n  # a comment\n  x:1\n\n  y:2\n"),
    ("core", "ns/node\n  x:1\n"),
    ("core", "outer\n  inner\n    deep\n      v:1\n"),
    ("core", "point\r\n  x:1\r\n  y:2\r\n"),
    ("core", "{x:1 y:2}\n"),
    ("core", "42\n"),
    ("core", "a\n  b:1\nc\n  d:2\n"),
    ("core", "\n\npoint\n  x:1\n\n"),
    ("core", "point\n\tx:1\n\ty:2\n"),
    ("core", "  point\n    x:1\n"),
    ("core", "top\n  mid1\n    x:1\n  mid2\n    y:2\n"),
]


# Full-document CST snapshots. The first five are the normative Q001/Q002/
# Q003/Q005/Q006 vectors; the remainder exercise production refinement, trivia
# attachment, discarded-value traces, headers, nodes, Unicode, attribute
# documents, and independent multi-error recovery.
DOCUMENT_CASES = [
    ("Q001", "core", '["é" @]'),
    ("Q002", "core", "[1}"),
    ("Q003", "core", "[1"),
    ("Q005", "core", "[1 2]"),
    ("Q006", "legacy_binary", "|YQ== 1"),
    ("production", "core", "{a:[1 2]}"),
    ("trivia", "core", "a # tail\n b"),
    ("discard", "core", "[1 #_ 2 3]"),
    ("header", "core", 'axon{require:["cst"]} 1'),
    ("node", "core", "outer((inner))"),
    ("unicode", "core", '# café\nétage{name:"A"}'),
    ("attribute-document", "core", "a:1 b:2"),
    ("multi-error", "core", "[1 } (2"),
]


# Exact formatter outputs from the Python reference. Each case targets a
# distinct formatter contract: comment retention, top-level stream framing,
# graph tokens, named temporal atoms, and indentation-to-brace migration.
FORMAT_CASES = [
    ("comments", "core", "{a:1 # retained\n b:[2 3]}", 2),
    ("stream", "core", "false\n{nested:(40.3)}", 2),
    ("graph", "graph", "&1 {self:*1}", 2),
    (
        "tz-name",
        "tz_names",
        "^2026-11-01T01:30:00-07:00[America/Edmonton]",
        2,
    ),
    (
        "indented",
        "core",
        'person\n  # retained\n  name:"Alex"\n  address\n    city:"Edmonton"\nnext\n',
        2,
    ),
]


def span_record(span):
    """Return the compact, stable byte/line/column representation of a span."""
    return [span.start, span.end, span.line, span.column]


def document_record(case_id, prof_name, source):
    """Serialize one reference CST with stable pre-order node identifiers."""
    opts = profile(prof_name)
    document = parse_cst2026(source, options=opts)
    token_ids = {id(token): index for index, token in enumerate(document.tokens)}

    tokens = []
    for token in document.tokens:
        tokens.append(
            {
                "kind": token.kind,
                "text": token.text,
                "span": span_record(token.span),
                "trivia": bool(token.trivia),
                "leading": [token_ids[id(item)] for item in token.leading_trivia],
                "trailing": [token_ids[id(item)] for item in token.trailing_trivia],
            }
        )

    node_ids = {}
    nodes = []

    def visit(node):
        identity = id(node)
        known = node_ids.get(identity)
        if known is not None:
            return known
        node_id = len(nodes)
        node_ids[identity] = node_id
        # Reserve the pre-order slot before descending into children.
        nodes.append(None)
        children = []
        for child in node.children:
            if isinstance(child, CstToken):
                children.append(["token", token_ids[id(child)]])
            elif isinstance(child, CstNode):
                children.append(["node", visit(child)])
            else:
                raise TypeError("unexpected CST child %r" % (child,))
        nodes[node_id] = {
            "id": node_id,
            "kind": node.kind,
            "span": span_record(node.span),
            "children": children,
        }
        return node_id

    root_id = visit(document.root)
    diagnostics = []
    for diagnostic in document.diagnostics:
        diagnostics.append(
            {
                "category": diagnostic.category,
                "message": diagnostic.message,
                "span": span_record(diagnostic.span),
                "expected": list(diagnostic.expected),
                "fixes": [
                    {
                        "title": fix.title,
                        "start": fix.start,
                        "end": fix.end,
                        "replacement": fix.replacement,
                    }
                    for fix in diagnostic.fixes
                ],
            }
        )

    return {
        "id": case_id,
        "profile": prof_name,
        "input": source,
        "render": document.render(),
        "tokens": tokens,
        "root": root_id,
        "nodes": nodes,
        "diagnostics": diagnostics,
        "value_count": len(document.values),
        "semantic_nodes": [node_ids[id(node)] for node in document.semantic_nodes],
        "semantic_traces": [
            {
                "span": span_record(trace.span),
                "owner": node_ids[id(trace.node)],
            }
            for trace in document.semantic_traces
        ],
    }


def main():
    out = []
    for prof_name, text in CASES:
        opts = profile(prof_name)
        doc = parse_cst2026(text, options=opts)
        assert doc.render() == text, "lossless invariant broken in reference for %r" % text
        tokens = [
            {
                "kind": t.kind,
                "text": t.text,
                "start": t.span.start,
                "end": t.span.end,
                "line": t.span.line,
                "column": t.span.column,
                "trivia": bool(t.trivia),
            }
            for t in doc.tokens
        ]
        out.append({"profile": prof_name, "input": text, "tokens": tokens})

    migrate = []
    for prof_name, text in MIGRATE_CASES:
        opts = profile(prof_name)
        record = {"profile": prof_name, "input": text}
        try:
            record["outcome"] = "ok"
            record["output"] = migrate_indented2026(text, options=opts)
        except Axon2026Error as exc:
            record["outcome"] = "error"
            record["category"] = getattr(exc, "category", "error")
        out_migrate = record
        migrate.append(out_migrate)

    documents = [
        document_record(case_id, prof_name, text)
        for case_id, prof_name, text in DOCUMENT_CASES
    ]

    formatted = []
    for case_id, prof_name, text, indent in FORMAT_CASES:
        opts = profile(prof_name)
        record = {
            "id": case_id,
            "profile": prof_name,
            "input": text,
            "indent": indent,
        }
        try:
            record["outcome"] = "ok"
            record["output"] = format_cst2026(
                text, indent=indent, options=opts
            )
        except Axon2026Error as exc:
            record["outcome"] = "error"
            record["category"] = getattr(exc, "category", "error")
        formatted.append(record)

    payload = {
        "note": "CST ground truth from pyaxon cst2026; spans are [start,end,line,column] and document node IDs are stable pre-order indexes",
        "cases": out,
        "migrate": migrate,
        "documents": documents,
        "format": formatted,
    }
    text = json.dumps(payload, indent=1, ensure_ascii=False) + "\n"
    if len(sys.argv) > 1:
        with open(sys.argv[1], "w", encoding="utf-8", newline="\n") as fd:
            fd.write(text)
    else:
        sys.stdout.buffer.write(text.encode("utf-8"))


if __name__ == "__main__":
    main()
