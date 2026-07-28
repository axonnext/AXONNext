from collections import OrderedDict
from datetime import date
from decimal import Decimal
import json
import math
import os
from pathlib import Path
import random
import struct
import unittest

from axon2.cst2026 import parse_cst2026
from axon2.v2026 import (
    Axon2026Error, AxonSet, AxonTuple, NanoTime, Node, Options,
    canonical2026, loads2026,
)


ROOT = Path(__file__).resolve().parents[3]
SEEDS_FILE = ROOT / "conformance" / "fuzz_seeds.json"


def _random_text(rng):
    alphabet = ["", "a", "AXON", "é", "e\u0301", "😀", "line\ntext",
                "quote\"slash\\", "\t", "Canada"]
    return rng.choice(alphabet)


def _random_float(rng):
    value = struct.unpack(">d", rng.getrandbits(64).to_bytes(8, "big"))[0]
    return value


def _random_scalar(rng):
    choice = rng.randrange(11)
    if choice == 0:
        return None
    if choice == 1:
        return bool(rng.getrandbits(1))
    if choice == 2:
        return rng.randint(-(1 << 160), 1 << 160)
    if choice == 3:
        return _random_float(rng)
    if choice == 4:
        exponent = rng.randint(-12, 12)
        return Decimal(rng.randint(-(1 << 60), 1 << 60)).scaleb(exponent)
    if choice == 5:
        return _random_text(rng)
    if choice == 6:
        return rng.randbytes(rng.randrange(0, 24))
    if choice == 7:
        return date(rng.randint(1, 9999), rng.randint(1, 12), 1)
    if choice == 8:
        return NanoTime(
            rng.randrange(24), rng.randrange(60), rng.randrange(60),
            rng.randrange(1_000_000_000))
    if choice == 9:
        return math.nan
    return math.inf if rng.getrandbits(1) else -math.inf


def _random_value(rng, depth):
    if depth <= 0 or rng.random() < 0.48:
        return _random_scalar(rng)
    choice = rng.randrange(6)
    size = rng.randrange(0, 5)
    if choice == 0:
        return [_random_value(rng, depth - 1) for _ in range(size)]
    if choice == 1:
        return AxonTuple([_random_value(rng, depth - 1) for _ in range(size)])
    if choice == 2:
        result = {}
        for index in range(size):
            result["k%d_%d" % (depth, index)] = _random_value(rng, depth - 1)
        return result
    if choice == 3:
        result = OrderedDict()
        for index in range(size):
            result["o%d_%d" % (depth, index)] = _random_value(rng, depth - 1)
        return result
    if choice == 4:
        # Set elements use distinct primitive kinds/spellings so the generated
        # value itself always satisfies Core's duplicate rule.
        primitives = []
        for index in range(size):
            primitives.extend((index, "s%d" % index, Decimal(index)))
        return AxonSet(primitives[:size])
    attributes = OrderedDict(
        ("a%d" % index, _random_value(rng, depth - 1))
        for index in range(size // 2))
    children = [_random_value(rng, depth - 1) for _ in range(size - len(attributes))]
    return Node(rng.choice(("Node", "geo/point", "étage")), "brace",
                attributes, children)


class GeneratedPropertyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.config = json.loads(SEEDS_FILE.read_text(encoding="utf-8"))

    def test_seeded_generated_values_round_trip_canonically(self):
        configured = self.config["iterations_per_seed"]
        iterations = int(os.environ.get("AXON_FUZZ_ITERATIONS", configured))
        depth = self.config["max_generated_depth"]
        for seed in self.config["property_seeds"]:
            rng = random.Random(seed)
            for index in range(iterations):
                with self.subTest(seed=seed, index=index):
                    value = _random_value(rng, depth)
                    first = canonical2026(value)
                    parsed = loads2026(
                        first.decode("utf-8"), options=Options(graph=True))[0]
                    second = canonical2026(parsed)
                    self.assertEqual(first, second)

    def test_seeded_mutations_never_escape_the_error_contract(self):
        known = {
            "invalid-unicode", "invalid-escape", "invalid-name",
            "invalid-number", "invalid-decimal", "invalid-temporal",
            "invalid-binary", "invalid-link", "unexpected-token",
            "unexpected-end", "duplicate-key", "duplicate-set-element",
            "unknown-reference", "duplicate-anchor", "unknown-constant",
            "resource-limit-exceeded", "unsupported-profile-feature",
        }
        for seed in self.config["mutation_seeds"]:
            rng = random.Random(seed)
            for source in self.config["mutation_inputs"]:
                for index in range(80):
                    mutated = self._mutate(source, rng)
                    with self.subTest(seed=seed, source=source[:12], index=index):
                        cst = parse_cst2026(mutated)
                        self.assertEqual(mutated, cst.render())
                        try:
                            values = loads2026(
                                mutated,
                                options=Options(
                                    graph=True, doc_link=True, tz_names=True))
                            for value in values:
                                canonical = canonical2026(value)
                                reparsed = loads2026(
                                    canonical.decode("utf-8"),
                                    options=Options(
                                        graph=True, doc_link=True, tz_names=True))[0]
                                self.assertEqual(canonical, canonical2026(reparsed))
                        except Axon2026Error as exc:
                            self.assertIn(exc.category, known)
                        except RecursionError as exc:
                            self.fail("mutation escaped resource limits: %s" % exc)

    def _mutate(self, source, rng):
        operation = rng.randrange(5)
        if not source:
            return rng.choice(self.config["insertions"])
        position = rng.randrange(len(source) + 1)
        if operation == 0 and position < len(source):
            return source[:position] + source[position + 1:]
        if operation == 1:
            return (source[:position] + rng.choice(self.config["insertions"])
                    + source[position:])
        if operation == 2 and position < len(source):
            return (source[:position] + rng.choice("{}[]():,#&*?0a\"")
                    + source[position + 1:])
        if operation == 3:
            return source[:position]
        end = min(len(source), position + rng.randrange(0, 5))
        return source[:end] + source[position:end] + source[end:]


class MalformedInputTests(unittest.TestCase):
    def test_checked_in_malformed_corpus_has_exact_categories(self):
        corpus = json.loads(SEEDS_FILE.read_text(encoding="utf-8"))
        self.assertGreaterEqual(len(corpus["malformed"]), 30)
        for case in corpus["malformed"]:
            with self.subTest(case=case["id"]):
                options = Options(**case.get("options", {}))
                with self.assertRaises(Axon2026Error) as caught:
                    loads2026(case["input"], options=options)
                self.assertEqual(case["error"], caught.exception.category)


class GraphIsomorphismTests(unittest.TestCase):
    def test_distinct_empty_tuples_do_not_gain_false_shared_identity(self):
        value = loads2026("(() ())", options=Options(graph=True))[0]
        self.assertIsNot(value[0], value[1])
        self.assertEqual(b"(() ())", canonical2026(value))

    def test_allocation_and_insertion_order_do_not_change_canonical_graph(self):
        shared_left = [Node("Leaf", "brace", OrderedDict((('v', 1),)), [])]
        left = {"z": shared_left, "a": shared_left}

        shared_right = [Node("Leaf", "brace", OrderedDict((('v', 1),)), [])]
        right = {}
        right["a"] = shared_right
        right["z"] = shared_right
        self.assertEqual(canonical2026(left), canonical2026(right))

        cycle_left = []
        cycle_left.append({"z": 2, "self": cycle_left, "a": 1})
        cycle_right = []
        body = {"a": 1, "self": cycle_right, "z": 2}
        cycle_right.append(body)
        self.assertEqual(canonical2026(cycle_left), canonical2026(cycle_right))

    def test_generated_isomorphic_graphs_and_round_trip_identity(self):
        rng = random.Random(20260711)
        for index in range(250):
            shared_a = [rng.randrange(1 << 30), "v%d" % index]
            shared_b = list(shared_a)
            left = {"z": shared_a, "a": [shared_a, {"k": shared_a}]}
            right = {"a": [shared_b, {"k": shared_b}], "z": shared_b}
            first = canonical2026(left)
            second = canonical2026(right)
            self.assertEqual(first, second)
            parsed = loads2026(first.decode("utf-8"), options=Options(graph=True))[0]
            self.assertIs(parsed["z"], parsed["a"][0])
            self.assertIs(parsed["z"], parsed["a"][1]["k"])
            self.assertEqual(first, canonical2026(parsed))


if __name__ == "__main__":
    unittest.main()
