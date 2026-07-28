"""Execute every row in the empirical pyaxon/AXON-2026 differential ledger."""

import json
from pathlib import Path
import unittest

import axon2
from axon2 import Axon2026Error, Options, loads2026


MANIFEST = Path(__file__).parents[3] / "conformance" / "empirical_differential.json"


class EmpiricalDifferentialTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.cases = json.loads(MANIFEST.read_text(encoding="utf-8"))["cases"]

    def test_manifest_is_unique_and_substantial(self):
        ids = [case["id"] for case in self.cases]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertGreaterEqual(len(ids), 70)

    def test_historical_runtime_outcomes(self):
        mismatches = []
        for case in self.cases:
            try:
                axon2.loads(case["input"])
                actual = "ok"
            except Exception:
                actual = "error"
            if actual != case["legacy"]:
                mismatches.append((case["id"], case["legacy"], actual))
        self.assertEqual(mismatches, [])

    def test_axon_2026_outcomes_and_error_categories(self):
        mismatches = []
        for case in self.cases:
            options = Options(graph=case.get("profile") == "graph")
            try:
                loads2026(case["input"], options=options)
                actual = "ok"
            except Axon2026Error as error:
                actual = error.category
            except Exception as error:
                actual = "python:" + type(error).__name__
            if actual != case["core"]:
                mismatches.append((case["id"], case["core"], actual))
        self.assertEqual(mismatches, [])


if __name__ == "__main__":
    unittest.main()
