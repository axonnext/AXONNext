"""Executable AXON 2026 acceptance tests for the Python edition engine."""

import importlib.util
from pathlib import Path
import sys
import unittest
from collections import OrderedDict
from datetime import date, datetime, time, timezone
from decimal import Decimal


MODULE = Path(__file__).parents[1] / "v2026.py"
SPEC = importlib.util.spec_from_file_location("axon_v2026", MODULE)
axon2 = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = axon2
SPEC.loader.exec_module(axon2)


class CoreSyntaxTests(unittest.TestCase):
    def one(self, text, **kwargs):
        values = axon2.loads2026(text, **kwargs)
        self.assertEqual(len(values), 1)
        return values[0]

    def assert_category(self, category, text, **kwargs):
        with self.assertRaises(axon2.Axon2026Error) as caught:
            axon2.loads2026(text, **kwargs)
        self.assertEqual(caught.exception.category, category)

    def test_containers_and_empty_forms(self):
        self.assertEqual(self.one("[]"), [])
        self.assertEqual(self.one("[:]"), OrderedDict())
        self.assertEqual(self.one("{}"), {})
        self.assertEqual(self.one("∅"), axon2.AxonSet())
        self.assertEqual(self.one("(1 2)"), (1, 2))
        self.assertEqual(self.one("[a:1 b:2]"), OrderedDict((('a', 1), ('b', 2))))
        self.assertEqual(self.one("{a:1 b:2}"), {'a': 1, 'b': 2})
        self.assertEqual(self.one("{1 2 3}"), axon2.AxonSet([1, 2, 3]))

    def test_container_shape_and_duplicates_are_strict(self):
        self.assert_category("unexpected-token", "{a:1 2}")
        self.assert_category("unexpected-token", "{1 b:2}")
        self.assert_category("duplicate-key", "{a:1 a:2}")
        self.assert_category("duplicate-set-element", "{1 2 2}")
        compat = axon2.Options.compat()
        self.assertEqual(self.one("{a:1 a:2}", options=compat), {'a': 2})
        self.assertEqual(self.one("{1 2 2}", options=compat), axon2.AxonSet([1, 2]))

    def test_comments_commas_and_discard(self):
        self.assertEqual(self.one("[1, 2, 3,]"), [1, 2, 3])
        self.assertEqual(self.one("# comment\n[1 #_ 2 3]"), [1, 3])
        self.assert_category("unexpected-token", "[1,,2]")

    def test_numbers(self):
        self.assertEqual(self.one("1_000"), 1000)
        self.assertEqual(self.one("1000.350D"), Decimal("1000.350"))
        self.assert_category("invalid-number", "007")
        self.assert_category("unexpected-token", "+42")
        self.assert_category("invalid-number", "5.")
        self.assert_category("unknown-constant", "$pi")
        self.assertEqual(self.one("$Inf"), float("inf"))

    def test_temporals_are_caret_typed_and_strict(self):
        self.assertEqual(self.one("^2026-07-10"), date(2026, 7, 10))
        self.assertEqual(self.one("^9:00"), axon2.NanoTime(9, 0))
        self.assertEqual(self.one("^12:00:00.5"), axon2.NanoTime(12, 0, 0, 500_000_000))
        self.assertEqual(self.one("^12:00:00.123456789").nanosecond, 123_456_789)
        value = self.one("^2026-07-10T09:30Z")
        self.assertEqual(value, axon2.NanoDateTime(date(2026, 7, 10), axon2.NanoTime(9, 30, offset_minutes=0, zulu=True)))
        self.assert_category("invalid-temporal", "2026-07-10")
        self.assert_category("invalid-temporal", "^2026-02-30")
        self.assert_category("invalid-temporal", "^24:00")
        compat = axon2.Options.compat()
        self.assertEqual(self.one("2012-12-31", options=compat), date(2012, 12, 31))

    def test_strings_namespaces_and_new_node_bodies(self):
        self.assertEqual(self.one(r'"a\nb"'), "a\nb")
        self.assertEqual(self.one(r'r"x\y"'), r"x\y")
        node = self.one("geo/point(10 20)")
        self.assertEqual((node.name, node.style, node.children), ("geo/point", "tuple", [10, 20]))
        node = self.one("Tags[\"a\" \"b\"]")
        self.assertEqual((node.style, node.children), ("list", ["a", "b"]))
        tree = self.one("tree{id:1 leaf{id:2 \"A\"}}")
        self.assertEqual(tree.attributes['id'], 1)
        self.assertEqual(tree.children[0].name, "leaf")
        self.assert_category("unexpected-token", 'tree{"A" id:1}')

    def test_binary_has_real_termination_and_round_trips(self):
        for payload in (b"ABC", b"Hello", b"", bytes(range(32))):
            encoded = axon2.dumps2026([payload])
            self.assertTrue(encoded.startswith("|") and encoded.endswith("|"))
            self.assertEqual(self.one(encoded), payload)
        self.assertEqual(self.one("|SGVsbG8|"), b"Hello")
        self.assert_category("unexpected-end", "|SGVsbG8=")

    def test_graph_profile_resolves_identity_and_cycles(self):
        options = axon2.Options(graph=True)
        values = axon2.loads2026("&1 {id:1} *1", options=options)
        self.assertIs(values[0], values[1])
        cycle = self.one("&root {self:*root}", options=options)
        self.assertIs(cycle, cycle['self'])
        self.assert_category("duplicate-anchor", "&1 1 &1 2", options=options)
        self.assert_category("unknown-reference", "*missing", options=options)

    def test_stream_header_and_attribute_document(self):
        values = axon2.loads2026('Event{a:1} Event{a:2}')
        self.assertEqual(len(values), 2)
        attrs = self.one('application:"myapp"\ndebug:true')
        self.assertEqual(attrs, OrderedDict((('application', 'myapp'), ('debug', True))))
        values = axon2.loads2026('axon{edition:"2026" require:[]} Event{id:1}')
        self.assertEqual(len(values), 1)
        self.assertEqual(values[0].name, 'Event')
        values = axon2.loads2026('1 axon{x:1}')
        self.assertEqual(len(values), 2)

    def test_canonical_output(self):
        self.assertEqual(axon2.canonical2026({'b': 2, 'a': 1}), b'{a:1 b:2}')
        self.assertEqual(axon2.canonical2026(axon2.AxonSet([2, 1])), b'{1 2}')
        self.assertEqual(axon2.canonical2026(Decimal('1000.350')), b'1000.35D')
        self.assertEqual(axon2.canonical2026(axon2.NanoTime(9, 0)), b'^09:00:00')
        self.assertEqual(axon2.canonical2026(b'ABC'), b'|QUJD|')
        self.assertEqual(axon2.migrate2026('007'), '7')
        self.assertTrue(axon2.verify_canonical2026('{a:1 b:2}'))
        self.assertFalse(axon2.verify_canonical2026('{b:2 a:1}'))


class Fixes011a3TestCase(unittest.TestCase):
    """0.11.0a3: header enabling (D-2), numeric adjacency (D-1), UAX #31 names."""

    def test_header_enables_and_is_consumed(self):
        vals = axon2.loads2026('axon{edition:"2026" require:["graph"]} &1 {v:*1}')
        self.assertEqual(len(vals), 1)
        self.assertIs(vals[0]["v"], vals[0])

    def test_header_policy_forbid(self):
        with self.assertRaises(axon2.Axon2026Error) as cm:
            axon2.loads2026('axon{require:["graph"]} 1',
                            options=axon2.Options(allow_header_profiles=False))
        self.assertEqual(cm.exception.category, "unsupported-profile-feature")

    def test_header_unknown_profile(self):
        with self.assertRaises(axon2.Axon2026Error) as cm:
            axon2.loads2026('axon{require:["future-profile"]} 1')
        self.assertEqual(cm.exception.category, "unsupported-profile-feature")

    def test_numeric_adjacency(self):
        for text in ("1__0", "1abc", "10Dx"):
            with self.assertRaises(axon2.Axon2026Error) as cm:
                axon2.loads2026(text)
            self.assertEqual(cm.exception.category, "unexpected-token", text)

    def test_numeric_adjacency_compat_shape(self):
        vals = axon2.loads2026("1__0", options=axon2.Options(compatibility=True))
        self.assertEqual(vals[0], 1)
        self.assertEqual(vals[1].name, "__0")

    def test_uax31_names(self):
        self.assertEqual(axon2.loads2026("étage")[0].name, "étage")
        self.assertEqual(axon2.loads2026("µ")[0].name, "µ")

    def test_core_binding_does_not_cross_newline(self):
        values = axon2.loads2026('person\n{x:1}')
        self.assertEqual(values[0].style, 'unit')
        self.assertEqual(values[1], {'x': 1})
        self.assertEqual(axon2.loads2026('person {x:1}')[0].style, 'brace')
        compat = axon2.loads2026('person\n{x:1}', options=axon2.Options.compat())
        self.assertEqual(compat[0].style, 'brace')
        self.assertEqual(axon2.loads2026('false\n{x:1}'), [False, {'x': 1}])


# Keep this last: an earlier `unittest.main()` exits before the classes below it
# are defined, so running the file directly would silently skip them.
if __name__ == "__main__":
    unittest.main()
