#!/usr/bin/env python3
"""Generate differential conformance ground truth from pyaxon 2026.

For each (input, profile) case this records what the *reference* implementation
does -- accept with specific canonical bytes, or reject with a specific error
category -- so `differential.rs` can assert serde_axon agrees exactly. Canonical
bytes are the cross-implementation contract (§14), so comparing them catches
both value-model and writer divergences in one check.

The audit's root-cause finding was that both implementations passed their own
suites because neither corpus covered the divergent surfaces. This corpus is
built specifically FROM those surfaces (the 12 audited divergences + graph),
and is regenerated from pyaxon rather than hand-written, so it cannot drift
from the reference.

Run from the axon2026_a13 dir with PYTHONPATH=lib:
    python .../gen_differential.py .../tests/differential.json
"""

import json
import sys

from axon2.v2026 import Options, loads2026, dumps2026


def profile(name):
    if name == "core":
        return Options()
    if name == "doc_link":
        return Options(doc_link=True)
    if name == "graph":
        return Options(graph=True)
    if name == "compat":
        return Options(compatibility=True)
    if name == "tz_names":
        return Options(tz_names=True)
    if name == "canonical_verify":
        return Options(canonical_verify=True)
    if name == "scale_significant":
        return Options(scale_significant=True)
    if name == "numeric_separators_off":
        return Options(numeric_separators=False)
    if name == "compat_bundle":
        return Options.compat()
    if name == "header_profiles_off":
        return Options(allow_header_profiles=False)
    raise ValueError(name)


def compact_outcome(values, *, graph):
    """Record the exact observable result of compact graph policy."""
    try:
        return {
            "outcome": "accept",
            "text": dumps2026(values, canonical=False, graph=graph),
        }
    except Exception as exc:  # noqa: BLE001 - writer categories are the oracle
        category = getattr(exc, "category", None)
        if category is None:
            category = str(exc).split()[0] if str(exc) else "error"
        return {
            "outcome": "error",
            "category": category,
            "message": exc.args[0] if exc.args else str(exc),
        }


# (profile, input) -- chosen to hit every surface the audit flagged plus the
# baseline, so a regression on any fix reappears here immediately.
CASES = [
    # F-M1 duplicate keys
    ("core", "[a:1 a:2]"),
    ("core", "{a:1 a:2}"),
    ("core", "p{x:1 x:2}"),
    ("core", "[a:1 b:2]"),
    ("compat", "{a:1 a:2}"),          # last_wins
    # F-M2 temporal validity
    ("core", "^2026-02-30"),
    ("core", "^2026-04-31"),
    ("core", "^2024-02-29"),          # valid leap day
    ("core", "^0000-01-01"),
    ("core", "^2026-01-01"),
    ("core", "^12:00+00:60"),
    ("core", "^12:00+00:59"),
    ("core", "^12:5"),
    ("core", "^12:05"),
    ("core", "^12:00:00.5"),
    # F-M3 doc-link validation
    ("doc_link", 'cid("notacid")'),
    ("doc_link", 'link("not a uri")'),
    ("doc_link", 'cid("bafkreiec5tzcwyhehbpob5yir5idmjfqf2qzpiiguoia4qt5fquhrbx4ym")'),
    ("doc_link", 'link("https://example.com/a")'),
    # F-M4 discard inside containers / pairs
    ("core", "[1 #_ 2 3]"),
    ("core", "{a:1 #_ b:2 c:3}"),
    ("core", "(1 #_ 2 3)"),
    ("core", "#_ 1 2"),
    # F-M5 raw strings, quoted node names, header consumption
    ("core", 'r"x\\y"'),
    ("core", 'r#"a"b"#'),
    ("core", "'weird name'{x:1}"),
    ("core", 'axon{edition:"2026"} 1'),
    ("core", 'axon{edition:"2026" require:["graph"]} &a 1'),
    ("core", 'axon{edition:"2026" require:["graph"]}\n&a []\n*a'),
    ("core", 'axon{edition:"2026" require:["graph"]}\n&a [*a]'),
    # F-M6 canonical set sort + JCS float layout
    ("core", "{2 1}"),
    ("core", "{3 1 2}"),
    ("core", "1e21"),
    ("core", "1e20"),
    ("core", "1e-7"),
    ("core", "1e-6"),
    ("core", "3.14"),
    ("core", "100.0"),
    ("core", "1e300"),
    ("core", "-0.0"),
    # F-L1 -inf adjacency
    ("core", "∞abc"),
    # F-L2 category drift
    ("core", "{null:1}"),
    # Graph profile
    ("graph", "[&a point{x:1} *a]"),
    ("graph", "[&a 1 &a 2]"),
    ("graph", "[*b]"),
    ("graph", "&root [1 2 3]"),
    ("graph", "&a []\n*a"),
    ("graph", "&1 []\n*1"),
    ("graph", "&a [*a]"),
    # Remaining profile switches and policy boundaries.
    ("tz_names", "^2026-11-01T01:30:00-07:00[America/Edmonton]"),
    ("core", "^2026-11-01T01:30:00-07:00[America/Edmonton]"),
    ("canonical_verify", "{a:1}"),
    ("canonical_verify", "{ a:1 }"),
    ("scale_significant", "1.00D"),
    ("numeric_separators_off", "1_000"),
    ("compat_bundle", "007"),
    ("compat_bundle", "12:30"),
    ("compat_bundle", "|YQ=="),
    ("header_profiles_off", 'axon{edition:"2026" require:["graph"]} &a 1'),
    # Baseline sanity
    ("core", "42"),
    ("core", "-17"),
    ("core", '"hello"'),
    ("core", "[1 2 3]"),
    ("core", "point{x:1 y:2}"),
    ("core", "1000.350D"),
    ("core", "|SGVsbG8=|"),
    ("core", "∅"),
    ("core", "[:]"),
    # Top-level attribute documents (§11.3): a document of all pairs is one
    # ordered map; mixing pairs and values, or a leading/duplicate/discard
    # anomaly, follows the reference exactly.
    ("core", "a:1 b:2"),
    ("core", "a:1"),
    ("core", "a:1\n\nb:2"),
    ("core", "a:1, b:2"),
    ("core", "a:1,b:2"),
    ("core", "a:1 b:2,"),
    ("core", "a:1,"),
    ("core", "a:1,,b:2"),
    ("core", ",a:1"),
    ("core", "a:1 2"),
    ("core", "3 a:1"),
    ("core", "a:1 b:2 3"),
    ("core", "a:1 a:2"),
    ("compat", "a:1 a:2"),
    ("core", "a:1 #_ b:2 c:3"),
    ("core", "#_a:1"),
    ('core', '"k":1 "m":2'),
    ("core", "1 2 3"),
    ("core", "p{x:1} a:1"),
    # Adversarial attribute-document boundaries: reserved words and quoted
    # names are values, not keys; nested container values remain valid.
    ("core", "true:1"),
    ("core", "null:1 a:2"),
    ("core", "true:1 b:2"),
    ("core", "'q name':1 b:2"),
    ("core", "a:1 true:2"),
    ("core", "a:[1 2] b:{c:3}"),
    ("core", "a:1 b"),
]


def main():
    out = []
    for prof_name, text in CASES:
        opts = profile(prof_name)
        record = {"profile": prof_name, "input": text}
        try:
            values = loads2026(text, options=opts)
        except Exception as exc:  # noqa: BLE001 - want the category, whatever it is
            category = getattr(exc, "category", None)
            if category is None:
                category = str(exc).split()[0] if str(exc) else "error"
            record["outcome"] = "error"
            record["category"] = category
            record["message"] = exc.args[0] if exc.args else str(exc)
            for field in ("offset", "end_offset", "line", "column"):
                if hasattr(exc, field):
                    record[field] = getattr(exc, field)
        else:
            record["outcome"] = "accept"
            record["count"] = len(values)
            # Canonical bytes are the contract. Scale-significant values are
            # intentionally outside Canonical AXON, so that profile is locked
            # by parsing + compact output instead.
            if prof_name != "scale_significant":
                record["canonical"] = dumps2026(
                    values, canonical=True, graph=True)
            # Compact Graph policy is independently observable. For profile or
            # header-enabled graph documents, lock both Python modes: graph=False
            # expands acyclic sharing and rejects cycles, while graph=True emits
            # deterministic identity markers. Other cases retain the historical
            # default compact oracle field.
            header_graph = "require" in text and '"graph"' in text
            if prof_name == "graph" or header_graph:
                record["compact_modes"] = {
                    "graph_false": compact_outcome(values, graph=False),
                    "graph_true": compact_outcome(values, graph=True),
                }
            else:
                record["compact"] = dumps2026(values)
        out.append(record)
    payload = {"note": "differential ground truth from pyaxon 0.11.0a13", "cases": out}
    text = json.dumps(payload, indent=1, ensure_ascii=False) + "\n"
    if len(sys.argv) > 1:
        with open(sys.argv[1], "w", encoding="utf-8", newline="\n") as fd:
            fd.write(text)
    else:
        # Force UTF-8 even on a cp1252 console.
        sys.stdout.buffer.write(text.encode("utf-8"))


if __name__ == "__main__":
    main()
