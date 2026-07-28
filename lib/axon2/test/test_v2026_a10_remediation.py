from collections import OrderedDict
import io
from pathlib import Path
import subprocess
import sys
import unittest

from axon2.cst2026 import CstNode, parse_cst2026
from axon2.schema2026 import SchemaRegistry2026, validate2026
from axon2.v2026 import (
    Axon2026Error, MigrationEdit, Node, Options,
    apply_migration_edits2026, canonical2026, dumps2026, iload2026,
    load2026, loads2026,
)


ROOT = Path(__file__).resolve().parents[3]


class SchemaGovernanceRegressionTests(unittest.TestCase):
    def test_invalid_shapes_and_pointers_are_categorized(self):
        malformed = [
            {"properties": []},
            {"$defs": []},
            {"kind": 1},
            {"kind": [1]},
            {"enum": 1},
            {"kind": "string", "min_length": "x"},
            {"required": 1},
        ]
        for schema in malformed:
            with self.subTest(schema=schema):
                self.assertEqual(
                    ["invalid-schema"],
                    [issue.code for issue in validate2026("x", schema)])
        schema = {
            "prefix_items": [{"kind": "int"}],
            "$ref": "#/prefix_items/9",
        }
        self.assertEqual(
            ["unresolved-ref"],
            [issue.code for issue in validate2026(1, schema)])
        schema["$ref"] = "#/prefix_items/" + "9" * 5000
        self.assertEqual(
            ["unresolved-ref"],
            [issue.code for issue in validate2026(1, schema)])

    def test_frozen_registry_does_not_expose_mutable_aliases(self):
        registry = SchemaRegistry2026()
        returned = registry.register(
            "urn:axon:test:frozen", {"kind": "int", "minimum": 1})
        registry.freeze()
        returned["minimum"] = -1
        fetched = registry["urn:axon:test:frozen"]
        fetched["minimum"] = -2
        snapshot = registry.snapshot()
        snapshot["urn:axon:test:frozen"]["minimum"] = -3
        self.assertEqual(
            ["minimum"],
            [issue.code for issue in validate2026(
                0, {"$ref": "urn:axon:test:frozen"}, registry=registry)])
        with self.assertRaises(RuntimeError):
            registry.register("urn:axon:test:late", {"kind": "any"})

    def test_polynomial_regex_shape_is_rejected(self):
        issues = validate2026(
            "a" * 500,
            {"kind": "string", "pattern": "a*a*a*a*b"})
        self.assertEqual(["pattern"], [issue.code for issue in issues])
        atomic_issues = validate2026(
            "a" * 500,
            {"kind": "string", "pattern": "(?>a+)b"})
        self.assertEqual(["pattern"], [issue.code for issue in atomic_issues])
        nested_atomic_issues = validate2026(
            "a" * 500,
            {"kind": "string", "pattern": "^(?>(a+)+)$"})
        self.assertEqual(["pattern"], [issue.code for issue in nested_atomic_issues])
        oversized_fixed = validate2026(
            "aaaa",
            {"kind": "string", "pattern": "a{500000}b"})
        self.assertIn("unsafe or invalid pattern", oversized_fixed[0].message)
        cumulative_fixed = validate2026(
            "aaaa",
            {"kind": "string", "pattern": "a{4096}" * 100 + "b"})
        self.assertIn("unsafe or invalid pattern", cumulative_fixed[0].message)
        anchored_fixed = validate2026(
            "a",
            {"kind": "string", "pattern": "^a{500000}b$"})
        self.assertEqual("string does not match pattern", anchored_fixed[0].message)
        self.assertEqual([], validate2026(
            "ABC", {"kind": "string", "pattern": "^[A-Z]+$"}))


class CstRegressionTests(unittest.TestCase):
    def test_deep_input_is_owned_without_python_recursion(self):
        source = "[" * 5000 + "]" * 5000
        document = parse_cst2026(
            source,
            options=Options(
                max_depth=128, max_input=20_000, max_cst_tokens=20_000))
        self.assertEqual(source, document.render())
        self.assertIn(
            "resource-limit-exceeded",
            [item.category for item in document.diagnostics])
        self.assertEqual(10_000, sum(1 for _ in document.root.tokens()))

    def test_profile_tokenization_and_stream_trace_are_exact(self):
        document = parse_cst2026(
            "|YQ== 1", options=Options(legacy_binary=True))
        self.assertEqual(["|YQ==", " ", "1"],
                         [item.text for item in document.tokens])
        self.assertEqual([b"a", 1], document.values)
        self.assertEqual(2, len(document.semantic_nodes))

        document = parse_cst2026(
            "#_comment\n1", options=Options(hash_underscore_comment=True))
        self.assertTrue(document.tokens[0].trivia)
        self.assertEqual("comment", document.tokens[0].kind)

        document = parse_cst2026("Foo\n{a:1}")
        self.assertEqual(2, len(document.values))
        self.assertEqual(2, len(document.semantic_nodes))

        document = parse_cst2026("1 #_ 2 3")
        self.assertEqual([1, 3], document.values)
        self.assertEqual([1, 3],
                         [node.semantic_value for node in document.semantic_nodes])

    def test_nested_and_discarded_values_have_semantic_traces(self):
        document = parse_cst2026("[1 #_ 2 3]")
        self.assertEqual(4, len(document.semantic_traces))
        self.assertEqual(
            [(1, 2), (6, 7), (8, 9), (0, 10)],
            [(trace.span.start, trace.span.end)
             for trace in document.semantic_traces])
        self.assertTrue(all(isinstance(trace.node, CstNode)
                            for trace in document.semantic_traces))

    def test_byte_edits_require_valid_bounds_and_scalar_boundaries(self):
        document = parse_cst2026("é")
        for start, end in ((-1, 0), (2, 1), (99, 100), (1, 1)):
            with self.subTest(start=start, end=end):
                with self.assertRaises(ValueError):
                    document.replace(start, end, "x")
        with self.assertRaises(ValueError):
            apply_migration_edits2026(
                "é", [MigrationEdit("test", 1, 1, "x")])


class PublicContractRegressionTests(unittest.TestCase):
    def test_reserved_names_and_attribute_key_columns_round_trip(self):
        self.assertEqual(b'{"null":1}', canonical2026({"null": 1}))
        node = Node(
            "null", "brace",
            OrderedDict((("has space", 1), ("null", 2))), [])
        rendered = canonical2026(node).decode("utf-8")
        self.assertEqual("'null'{'has space':1 'null':2}", rendered)
        self.assertEqual(rendered, dumps2026(loads2026(rendered), canonical=True))
        with self.assertRaises(Axon2026Error) as caught:
            loads2026("{null:1}")
        self.assertEqual("unexpected-token", caught.exception.category)

    def test_canonical_key_validation_precedes_sorting(self):
        bad = [
            {"\ud800": 1},
            Node("N", "brace", OrderedDict((("\ud800", 1),)), []),
        ]
        for value in bad:
            with self.subTest(value=type(value).__name__):
                with self.assertRaises(Axon2026Error) as caught:
                    canonical2026(value)
                self.assertEqual("invalid-unicode", caught.exception.category)
        with self.assertRaises(TypeError):
            canonical2026(Node(
                "N", "brace", OrderedDict(((1, 1),)), []))

    def test_file_like_decode_errors_are_categorized(self):
        for incremental in (False, True):
            stream = io.TextIOWrapper(
                io.BytesIO(b"1\n\xff"), encoding="utf-8")
            with self.subTest(incremental=incremental):
                with self.assertRaises(Axon2026Error) as caught:
                    if incremental:
                        list(iload2026(stream))
                    else:
                        load2026(stream)
                self.assertEqual("invalid-unicode", caught.exception.category)

    def test_release_gate_injects_source_layout(self):
        code = (
            "import importlib.util,pathlib;"
            "p=pathlib.Path('tools/release_gate.py');"
            "s=importlib.util.spec_from_file_location('gate',p);"
            "m=importlib.util.module_from_spec(s);s.loader.exec_module(m);"
            "r=m.run('import',[__import__('sys').executable,'-c','import axon2']);"
            "raise SystemExit(r['returncode'])"
        )
        process = subprocess.run(
            [sys.executable, "-c", code], cwd=ROOT,
            env={"PATH": str(Path(sys.executable).parent)},
            capture_output=True, text=True, check=False)
        self.assertEqual(0, process.returncode, process.stderr)

    def test_release_gate_builds_generated_c_before_importing_tests(self):
        source = (ROOT / "tools" / "release_gate.py").read_text(
            encoding="utf-8")
        build = source.index('("build-extensions"')
        tests = source.index('("unittest"')
        self.assertLess(build, tests)
        self.assertIn('"build_ext", "--inplace"', source)
        self.assertIn("if (ROOT / target).exists()", source)

    def test_binary64_differential_is_a_permanent_matrix_gate(self):
        workflow = (ROOT / ".github" / "workflows" /
                    "python-reference.yml").read_text(encoding="utf-8")
        self.assertIn("check_rfc8785_differential.py --random-cases 100000",
                      workflow)
        oracle = (ROOT / "tools" / "check_rfc8785_node.mjs").read_text(
            encoding="utf-8")
        self.assertIn("--stdin-bits", oracle)


if __name__ == "__main__":
    unittest.main()
