"""Regression coverage for the 11 July 2026 exhaustive audit."""

from datetime import time, timedelta, timezone
from io import StringIO
import math
import os
import tempfile
import unittest
import xml.etree.ElementTree as etree

import axon2
from axon2.binary2026 import BinaryAxonError, decode as decode_binary2026
from axon2.binary2026 import encode as encode_binary2026
from axon2 import (
    Axon2026Error, AxonSet, Node, Options, canonical2026, cid2026,
    diagnostics2026, document_symbols2026, dump2026, dumps2026,
    iload2026, iloads2026, loads2026, migrate2026, parse_cid2026,
    parse_cst2026, validate2026, verify_canonical2026,
)


class _OneCharacterReader:
    def __init__(self, text):
        self.text = text
        self.position = 0
        self.calls = []

    def read(self, size=-1):
        self.calls.append(size)
        if self.position >= len(self.text):
            return ""
        result = self.text[self.position:self.position + 1]
        self.position += 1
        return result


class AuditRemediationTests(unittest.TestCase):
    def assert_category(self, category, callback):
        with self.assertRaises(Axon2026Error) as caught:
            callback()
        self.assertEqual(category, caught.exception.category)
        return caught.exception

    def test_exact_node_names_are_injective_and_writer_round_trips_controls(self):
        composed = Node("é")
        decomposed = Node("e\u0301")
        self.assertNotEqual(canonical2026(composed), canonical2026(decomposed))
        self.assertNotEqual(cid2026(composed), cid2026(decomposed))
        for name in ("a\nb", "a\rb", "weird name/x", "ns/weird name"):
            encoded = canonical2026(Node(name)).decode("utf-8")
            self.assertNotIn("\n", encoded)
            self.assertNotIn("\r", encoded)
            self.assertEqual(name, loads2026(encoded)[0].name)
        self.assertEqual("weird name/x", loads2026("'weird name'/x")[0].name)
        self.assertEqual("ns/weird name", loads2026("ns/'weird name'")[0].name)

    def test_canonical_binary_large_float_uses_double_width(self):
        encoded = encode_binary2026(1e300, canonical=True)
        self.assertEqual(0xfb, encoded[0])
        self.assertEqual(9, len(encoded))
        with self.assertRaises(BinaryAxonError):
            decode_binary2026(b"\xd9\x03\xe7\xf6")

    def test_graph_define_before_use_duplicates_and_reference_only_cycle(self):
        self.assert_category(
            "unknown-reference", lambda: loads2026("*1 &1 2", options=Options(graph=True)))
        self.assert_category(
            "duplicate-anchor", lambda: loads2026("&1 [&1 2]", options=Options(graph=True)))
        self.assert_category(
            "unexpected-token", lambda: loads2026("&1 *1", options=Options(graph=True)))
        cycle = loads2026("&1 [*1]", options=Options(graph=True))[0]
        self.assertIs(cycle, cycle[0])

    def test_cyclic_set_is_canonical_and_verifiable(self):
        self.assertTrue(verify_canonical2026("&1 {*1}"))
        value = AxonSet()
        value.items.append(value)
        self.assertEqual(b"&1 {*1}", canonical2026(value))

    def test_compatibility_and_migration_are_value_faithful(self):
        compatibility = Options.compat()
        self.assertEqual("abab\\c", loads2026('"ab\\\nc"', options=compatibility)[0])
        rgb = loads2026("Rgb(1 2 3)", options=compatibility)
        self.assertEqual(None, rgb[0])
        self.assertEqual((1, 2, 3), rgb[1])
        self.assertEqual("a\nb", loads2026("'a\nb'", options=compatibility)[0].name)
        self.assertEqual(
            "a\nb", loads2026("'a\nb'", options=Options(lax_strings=True))[0].name)
        indented = loads2026(
            "person\n  name:\"Alex\"\n", options=Options(indented_bodies=True))[0]
        self.assertEqual("Alex", indented.attributes["name"])
        temporal = loads2026("^12:00:00.1234567", options=compatibility)[0]
        self.assertEqual(123_456_000, temporal.nanosecond)
        continuation = migrate2026('"ab\\\nc"')
        self.assertEqual('"abab\\\\c"', continuation)
        self.assertIn("legacy-continuation", [event.code for event in continuation.events])
        self.assertEqual("null\n(1 2 3)", migrate2026("Rgb(1 2 3)"))
        fraction = migrate2026("^12:00:00.1234567")
        self.assertEqual("^12:00:00.123456", fraction)
        self.assertEqual(
            ["temporal-fraction", "core-canonicalization"],
            [event.code for event in fraction.events])
        self.assertNotIn(
            "leading-zero-number", [event.code for event in fraction.events])
        comment = migrate2026("#_ legacy comment\n1", selective=True)
        self.assertEqual("# _ legacy comment\n1", comment)
        self.assertEqual("selective", comment.mode)
        self.assertIn(
            "hash-underscore-comment", [event.code for event in comment.events])
        binary = migrate2026("|YQ==", selective=True)
        self.assertEqual("|YQ==|", binary)
        self.assertEqual("selective", binary.mode)
        point = migrate2026("5.", selective=True)
        self.assertEqual("5.0", point)
        self.assertEqual("selective", point.mode)
        header = loads2026('axon{require:["compat"]} {a:1 a:2}')
        self.assertEqual({"a": 2}, header[0])

    def test_resource_limits_cover_writer_names_headers_references_and_reads(self):
        deep = 0
        for _ in range(1500):
            deep = [deep]
        self.assert_category("resource-limit-exceeded", lambda: canonical2026(deep))
        self.assert_category(
            "resource-limit-exceeded",
            lambda: loads2026("'abc'", options=Options(max_quoted_name=2)))
        self.assert_category(
            "resource-limit-exceeded",
            lambda: loads2026("abcd", options=Options(max_bare_name=3)))
        self.assert_category(
            "resource-limit-exceeded",
            lambda: loads2026(
                'axon{require:["graph" "cst"]} 1',
                options=Options(max_header_require=1)))
        self.assert_category(
            "resource-limit-exceeded",
            lambda: loads2026(
                "&1 1 *1 *1", options=Options(graph=True, max_references=1)))
        spy = StringIO("12345")
        self.assert_category(
            "resource-limit-exceeded",
            lambda: axon2.load2026(spy, options=Options(max_input=4)))

    def test_iterators_yield_before_later_errors_and_stream_in_chunks(self):
        iterator = iloads2026("1 [")
        self.assertEqual(1, next(iterator))
        self.assert_category("unexpected-end", lambda: next(iterator))

        reader = _OneCharacterReader("1 2 [")
        stream = iload2026(reader)
        self.assertEqual(1, next(stream))
        self.assertLess(reader.position, len(reader.text))
        self.assertEqual(2, next(stream))
        self.assert_category("unexpected-end", lambda: next(stream))
        self.assertTrue(all(size != -1 for size in reader.calls))

    def test_diagnostic_spans_are_utf8_bytes(self):
        source = '"é" @'
        expected = source.encode("utf-8").index(b"@")
        error = self.assert_category("unexpected-token", lambda: loads2026(source))
        self.assertEqual(expected, error.offset)
        self.assertEqual(expected, diagnostics2026(source)[0]["start_byte"])

    def test_cst_productions_and_symbols_are_cycle_safe(self):
        document = parse_cst2026("outer((inner))")
        def nodes(node):
            yield node
            for child in getattr(node, "children", ()):
                if hasattr(child, "children"):
                    yield from nodes(child)
        kinds = {child.kind for child in nodes(document.root)}
        self.assertTrue({"stream-value", "node", "node-name", "body", "atom"}
                        <= kinds)
        self.assertEqual(1, len(document.root.semantic_value))
        self.assertEqual(1, len(document.semantic_nodes))
        trivia_document = parse_cst2026("a # tail\n b")
        semantic_tokens = [token for token in trivia_document.tokens if not token.trivia]
        self.assertTrue(any(item.kind == "comment"
                            for item in semantic_tokens[0].trailing_trivia))
        self.assertTrue(semantic_tokens[1].leading_trivia)
        self.assertEqual(
            ["outer", "inner"],
            [symbol["name"] for symbol in document_symbols2026("outer((inner))")])
        symbols = document_symbols2026("&1 [*1]", options=Options(graph=True))
        self.assertEqual([], symbols)
        self.assertEqual([1], loads2026('axon{require:["cst"]} 1'))

    def test_schema_depth_regex_and_nan_semantics_are_safe(self):
        cyclic = {}
        cyclic["items"] = cyclic
        issues = validate2026([], cyclic)
        self.assertEqual("resource-limit-exceeded", issues[0].code)
        unsafe = validate2026("a" * 100, {"kind": "string", "pattern": "(a+)+$"})
        self.assertEqual("pattern", unsafe[0].code)
        nan_issues = validate2026(math.nan, {"const": math.nan})
        self.assertEqual("const", nan_issues[0].code)

    def test_jcs_float_cid_text_and_uri_validation(self):
        self.assertEqual(b"100000000000000000000.0", canonical2026(1e20))
        self.assertEqual(b"0.000001", canonical2026(1e-6))
        self.assertEqual(b"1e-7", canonical2026(1e-7))

        cid = cid2026(1)
        alphabet = "abcdefghijklmnopqrstuvwxyz234567"
        index = alphabet.index(cid[-1])
        alias = cid[:-1] + alphabet[index + 1]
        self.assertNotEqual(cid, alias)
        with self.assertRaises(ValueError):
            parse_cid2026(alias)
        self.assert_category(
            "invalid-link",
            lambda: loads2026('link("https://exa mple/x")', options=Options(doc_link=True)))
        self.assert_category(
            "invalid-link",
            lambda: loads2026('link("https://example.invalid/%zz")', options=Options(doc_link=True)))

    def test_python_offset_seconds_are_rejected_and_dump_is_atomic(self):
        odd_offset = time(1, 2, tzinfo=timezone(timedelta(seconds=30)))
        self.assert_category("invalid-temporal", lambda: dumps2026(odd_offset))
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "value.axon")
            with open(path, "w", encoding="utf-8") as stream:
                stream.write("ORIGINAL")
            with self.assertRaises(TypeError):
                dump2026(path, object())
            with open(path, "r", encoding="utf-8") as stream:
                self.assertEqual("ORIGINAL", stream.read())
            with self.assertRaises(axon2.AxonError):
                axon2.dump(path, [object()])
            with open(path, "r", encoding="utf-8") as stream:
                self.assertEqual("ORIGINAL", stream.read())

    def test_xml_reducer_does_not_mutate_child_tail(self):
        root = etree.fromstring("<root><child/>tail</root>")
        child = root[0]
        self.assertEqual("tail", child.tail)
        axon2.dumps([root])
        self.assertEqual("tail", child.tail)


if __name__ == "__main__":
    unittest.main()
