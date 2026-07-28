#!/usr/bin/env python3
"""Vendor the normative registries into a deterministic Rust test fixture.

The Rust test executor consumes a package-local copy so the complete normative
registry and RFC 8785 Appendix B remain available in published crate sources.
The source hashes make the copy auditable rather than relying on a version
label that can drift independently of the files.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sys


TESTS = Path(__file__).resolve().parent
ROOT = TESTS.parents[1]
NORMATIVE = ROOT / "conformance" / "normative_vectors.json"
RFC8785 = ROOT / "conformance" / "rfc8785_appendix_b.json"


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    payload = {
        "note": "verbatim normative registries executed by serde_axon/tests/normative.rs",
        "sources": {
            "normative_vectors.json": digest(NORMATIVE),
            "rfc8785_appendix_b.json": digest(RFC8785),
        },
        "normative": json.loads(NORMATIVE.read_text(encoding="utf-8")),
        "rfc8785": json.loads(RFC8785.read_text(encoding="utf-8")),
    }
    rendered = json.dumps(payload, indent=1, ensure_ascii=False) + "\n"
    if len(sys.argv) > 1:
        Path(sys.argv[1]).write_text(rendered, encoding="utf-8", newline="\n")
    else:
        sys.stdout.buffer.write(rendered.encode("utf-8"))


if __name__ == "__main__":
    main()
