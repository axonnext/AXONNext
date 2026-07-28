"""Run the deterministic long AXON 2026 property/mutation battery."""

import argparse
import os
from pathlib import Path
import sys
import unittest


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--iterations", type=int, default=5000,
                        help="generated values per checked-in seed")
    arguments = parser.parse_args()
    if arguments.iterations < 1:
        parser.error("--iterations must be positive")
    os.environ["AXON_FUZZ_ITERATIONS"] = str(arguments.iterations)
    suite = unittest.defaultTestLoader.loadTestsFromName(
        "axon2.test.test_v2026_fuzz")
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
