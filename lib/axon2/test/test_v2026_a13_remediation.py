# coding: utf-8
"""Regression tests for the 0.11.0a13 audit remediation.

Finding L1 (AUDIT_AXON2026_12-07-2026_0623): the schema validator silently
ignored unknown constraint keywords, so a mistyped constraint name produced a
false pass instead of rejecting invalid values.  Unknown keywords are now
rejected as an ``invalid-schema`` issue, consistent with the existing unknown
``kind`` rejection, on both dict-form and node-form schemas.  Recognised
constraints, structural keywords, and non-semantic annotation keywords continue
to behave exactly as before.
"""

import unittest

from axon2 import v2026
from axon2.schema2026 import normalize_schema, validate2026


def _codes(value, schema):
    return [issue.code for issue in validate2026(value, schema)]


class SchemaUnknownKeywordTests(unittest.TestCase):

    def test_mistyped_constraint_no_longer_false_passes(self):
        # `minimun` is a typo for `minimum`; value 5 violates the intended
        # constraint and must not silently pass.
        self.assertEqual(_codes(5, {"kind": "int", "minimun": 10}),
                         ["invalid-schema"])
        self.assertEqual(_codes("zzz", {"kind": "string", "patern": "^x$"}),
                         ["invalid-schema"])
        self.assertEqual(_codes(1, {"wibble": "z"}), ["invalid-schema"])

    def test_recognised_constraints_still_apply(self):
        self.assertEqual(_codes(5, {"kind": "int", "minimum": 10}), ["minimum"])
        self.assertEqual(_codes(12, {"kind": "int", "minimum": 10}), [])
        self.assertEqual(_codes("abc", {"kind": "string", "pattern": "^a"}), [])
        self.assertEqual(_codes(2, {"enum": [1, 2, 3]}), [])
        self.assertEqual(
            _codes({"a": 1},
                   {"kind": "map",
                    "properties": {"a": {"kind": "int"}},
                    "required": ["a"]}),
            [])

    def test_annotation_keywords_are_permitted(self):
        self.assertEqual(
            _codes(1, {"kind": "int", "title": "T", "description": "d"}), [])

    def test_node_form_schema_rejects_unknown_keyword(self):
        schema_node = v2026.loads2026('schema{kind:"int" boguskey:1}')[0]
        with self.assertRaises(ValueError):
            normalize_schema(schema_node)

    def test_node_form_schema_accepts_recognised_keywords(self):
        schema_node = v2026.loads2026('schema{kind:"int" minimum:0}')[0]
        normalized = normalize_schema(schema_node)
        self.assertEqual(normalized.get("minimum"), 0)

    def test_node_form_recursively_normalizes_nested_schema_attributes(self):
        schema_node = v2026.loads2026(
            'schema{kind:"list" items:schema{kind:"node" '
            'attributes:schema{kind:"map" '
            'properties:{age:field{kind:"int" minimum:1}} '
            'additional:false}}}')[0]

        normalized = normalize_schema(schema_node)
        self.assertIsInstance(normalized["items"], dict)
        self.assertIsInstance(normalized["items"]["attributes"], dict)
        self.assertIsInstance(
            normalized["items"]["attributes"]["properties"]["age"], dict)

        value = v2026.loads2026('[person{age:0}]')[0]
        issues = validate2026(value, schema_node)
        self.assertEqual(["minimum"], [issue.code for issue in issues])
        self.assertEqual([(0, "@", "age")], [issue.path for issue in issues])
        self.assertEqual(
            [("items", "attributes", "properties", "age", "minimum")],
            [issue.schema_path for issue in issues])


if __name__ == "__main__":
    unittest.main()
