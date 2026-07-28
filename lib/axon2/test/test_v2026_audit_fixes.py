import unittest
from datetime import time, timezone

from axon2.v2026 import (
    Axon2026Error, AxonTuple, Options, dumps2026, iloads2026, loads2026,
    verify_canonical2026,
)


class AuditFixes2026Tests(unittest.TestCase):
    def category(self, text, **kwargs):
        with self.assertRaises(Axon2026Error) as caught:
            loads2026(text, **kwargs)
        return caught.exception.category

    def test_core_lexical_boundaries(self):
        self.assertEqual("unexpected-token", self.category("1\u00a02"))
        self.assertEqual("unexpected-token", self.category("1,2"))
        self.assertEqual("unexpected-token", self.category("∞x"))
        self.assertEqual("invalid-escape", self.category('"a\x01b"'))
        self.assertEqual(loads2026("'a\\u{41}'")[0].name, "aA")
        self.assertEqual([item.name for item in loads2026("r# note\nx")], ["r", "x"])

    def test_kind_aware_sets_and_tuple_cycles(self):
        self.assertEqual(len(loads2026("{1 1.0}")[0]), 2)
        self.assertEqual(len(loads2026("{true 1}")[0]), 2)
        value = loads2026("&1 (1 *1)", options=Options(graph=True))[0]
        self.assertIsInstance(value, AxonTuple)
        self.assertIs(value[1], value)

    def test_cycle_writer_is_safe(self):
        cycle = []
        cycle.append(cycle)
        with self.assertRaises(Axon2026Error):
            dumps2026(cycle, graph=False)
        encoded = dumps2026(cycle, canonical=True, graph=True)
        self.assertTrue(verify_canonical2026(encoded))

    def test_limits_streaming_and_bridge_zone(self):
        self.assertEqual(list(iloads2026("1 2")), [1, 2])
        self.assertEqual("resource-limit-exceeded", self.category(
            "[1 2]", options=Options(max_container=1)))
        self.assertEqual("^12:35:00+00:00", dumps2026(time(12, 35, tzinfo=timezone.utc)))


if __name__ == "__main__":
    unittest.main()
