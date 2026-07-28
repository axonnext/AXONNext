#!/usr/bin/env python3
"""Regenerate every Python-derived Rust fixture and byte-compare it in place.

Cargo tests stay Python-free. This script is the release/CI reproducibility
gate that proves the checked-in JSON still comes from the executable Python
reference rather than hand edits.
"""

from __future__ import annotations

import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile


TESTS = Path(__file__).resolve().parent
ROOT = TESTS.parents[1]
FIXTURES = (
    ("gen_normative.py", "normative.json"),
    ("gen_differential.py", "differential.json"),
    ("gen_cst.py", "cst.json"),
    ("gen_schema.py", "schema.json"),
    ("gen_surfaces.py", "surfaces.json"),
)


def main() -> int:
    environment = os.environ.copy()
    library = str(ROOT / "lib")
    environment["PYTHONPATH"] = (
        library
        if not environment.get("PYTHONPATH")
        else library + os.pathsep + environment["PYTHONPATH"]
    )
    failures = []
    with tempfile.TemporaryDirectory(prefix="axon-fixtures-") as directory:
        temporary = Path(directory)
        for generator_name, fixture_name in FIXTURES:
            generated = temporary / fixture_name
            subprocess.run(
                [sys.executable, str(TESTS / generator_name), str(generated)],
                cwd=ROOT,
                env=environment,
                check=True,
            )
            expected = TESTS / fixture_name
            actual_bytes = generated.read_bytes()
            expected_bytes = expected.read_bytes()
            digest = hashlib.sha256(actual_bytes).hexdigest()
            if actual_bytes != expected_bytes:
                failures.append(fixture_name)
                print(f"MISMATCH {fixture_name} generated-sha256={digest}")
            else:
                print(f"OK       {fixture_name} sha256={digest}")
    if failures:
        print("Regenerate mismatches with their gen_*.py scripts:")
        for fixture in failures:
            print(" -", fixture)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
