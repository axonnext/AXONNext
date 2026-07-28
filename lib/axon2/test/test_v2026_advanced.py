"""Advanced AXON 2026 tests: CST, schemas, nanoseconds, and graphs."""

from collections import OrderedDict
from datetime import date
from decimal import Decimal
import unittest

from axon2 import (
    AxonSet, NanoDateTime, NanoTime, Node, Options, ScaledDecimal,
    SCHEMA_DIALECT_2026,
    canonical2026, dumps2026, load_schema2026, loads2026,
    cid2026, diagnostics2026, document_symbols2026, format_cst2026,
    formatting_edits2026, hover2026, migrate_indented2026, parse_cst2026,
    validate2026, verify_cid2026,
)


class CstTests(unittest.TestCase):
    def test_cst_has_production_tree_and_recovers_multiple_delimiters(self):
        document = parse_cst2026('{a:[1 2]}')
        self.assertEqual(document.root.kind, 'document')
        def nodes(node):
            yield node
            for child in getattr(node, 'children', ()):
                if hasattr(child, 'children'):
                    yield from nodes(child)
        brace = next(child for child in nodes(document.root)
                     if getattr(child, 'kind', None) == 'brace')
        self.assertTrue(any(getattr(child, 'kind', None) == 'bracket'
                            for child in nodes(brace)))
        self.assertEqual(''.join(token.text for token in document.root.tokens()),
                         '{a:[1 2]}')
        broken = parse_cst2026('[1 } (2')
        self.assertGreaterEqual(len(broken.diagnostics), 2)
        self.assertEqual(broken.render(), '[1 } (2')

    def test_lsp_helpers_use_byte_offsets(self):
        source = 'étage{name:"A"}'
        hover = hover2026(source, 1)
        self.assertEqual(hover['text'], 'étage')
        self.assertEqual(hover['end_byte'], len('étage'.encode('utf-8')))
        self.assertEqual(document_symbols2026(source)[0]['name'], 'étage')
        self.assertTrue(formatting_edits2026(source))
        self.assertEqual(diagnostics2026('[1,,2]')[0]['code'], 'unexpected-token')

    def test_formatter_preserves_comments_and_semantics(self):
        source = '{a:1 # retained\n b:[2 3]}'
        formatted = format_cst2026(source, indent=2)
        self.assertIn('# retained', formatted)
        self.assertEqual(loads2026(formatted), loads2026(source))
        self.assertIn('\n  ', formatted)
        stream = 'false\n{nested:(40.3)}'
        self.assertEqual(loads2026(format_cst2026(stream)), loads2026(stream))
        self.assertEqual(loads2026('[false {}]'), [[False, {}]])

    def test_formatter_preserves_graph_and_named_temporal_atoms(self):
        graph_options = Options(graph=True)
        formatted_graph = format_cst2026('&1 {self:*1}', options=graph_options)
        graph = loads2026(formatted_graph, options=graph_options)[0]
        self.assertIs(graph, graph['self'])
        zone_options = Options(tz_names=True)
        temporal = '^2026-11-01T01:30:00-07:00[America/Edmonton]'
        formatted_temporal = format_cst2026(temporal, options=zone_options)
        self.assertEqual(loads2026(formatted_temporal, options=zone_options),
                         loads2026(temporal, options=zone_options))

    def test_indented_body_migration_preserves_comments_and_nesting(self):
        source = 'person\n  # retained\n  name:"Alex"\n  address\n    city:"Edmonton"\nnext\n'
        migrated = migrate_indented2026(source)
        self.assertIn('# retained', migrated)
        self.assertIn('person{', migrated)
        self.assertIn('address{', migrated)
        values = loads2026(migrated)
        self.assertEqual(values[0].attributes['name'], 'Alex')
        self.assertEqual(values[0].children[0].attributes['city'], 'Edmonton')
        self.assertEqual(values[1].name, 'next')

    def test_every_byte_round_trips_and_spans_are_utf8_bytes(self):
        source = '# café\nétage { name: "A" # tail\n value: 1_000 }\n'
        cst = parse_cst2026(source)
        self.assertEqual(cst.render(), source)
        self.assertEqual(cst.bytes(), source.encode('utf-8'))
        self.assertTrue(any(token.kind == 'comment' for token in cst.tokens))
        name = next(token for token in cst.tokens if token.text == 'étage')
        self.assertEqual(source.encode('utf-8')[name.span.start:name.span.end].decode(), 'étage')

    def test_invalid_input_is_retained_with_diagnostic(self):
        source = '[1,,2] # still here\n'
        cst = parse_cst2026(source)
        self.assertEqual(cst.render(), source)
        self.assertEqual(cst.diagnostics[0].category, 'unexpected-token')

    def test_byte_span_edit_reparses_without_touching_other_bytes(self):
        source = '{a:1  # keep\n b:2}'
        cst = parse_cst2026(source)
        token = next(token for token in cst.tokens if token.text == '1')
        edited = cst.replace(token.span.start, token.span.end, '1000D')
        self.assertEqual(edited.render(), '{a:1000D  # keep\n b:2}')
        self.assertFalse(edited.diagnostics)


class NanosecondTests(unittest.TestCase):
    def test_nine_digits_are_semantic_and_canonical(self):
        value = loads2026('^2026-07-10T12:00:00.123456789-06:00')[0]
        self.assertIsInstance(value, NanoDateTime)
        self.assertEqual(value.clock_time.nanosecond, 123_456_789)
        self.assertEqual(value.clock_time.offset_minutes, -360)
        self.assertEqual(canonical2026(value), b'^2026-07-10T12:00:00.123456789-06:00')
        self.assertEqual(value.to_datetime().microsecond, 123456)

    def test_rfc9557_named_zone_profile_and_offset_validation(self):
        text = ('axon{edition:"2026" require:["tz-names"]} '
                '^2026-07-10T12:30:00-06:00[America/Edmonton]')
        value = loads2026(text)[0]
        self.assertEqual(value.zone_name, 'America/Edmonton')
        self.assertEqual(value.clock_time.offset_minutes, -360)
        self.assertEqual(dumps2026(value), '^2026-07-10T12:30:00-06:00[America/Edmonton]')
        with self.assertRaises(Exception) as cm:
            loads2026('^2026-07-10T12:30:00-07:00[America/Edmonton]',
                      options=Options(tz_names=True))
        self.assertEqual(cm.exception.category, 'invalid-temporal')

    def test_raw_fraction_compatibility_flag_is_independent(self):
        core = loads2026('^12:00:00.5')[0]
        legacy = loads2026('^12:00:00.5', options=Options(raw_fraction=True))[0]
        self.assertEqual(core.nanosecond, 500_000_000)
        self.assertEqual(legacy.nanosecond, 5_000)

    def test_named_zone_ambiguous_and_nonexistent_wall_times(self):
        options = Options(tz_names=True)
        summer_side = loads2026(
            '^2026-11-01T01:30:00-06:00[America/Edmonton]', options=options)[0]
        winter_side = loads2026(
            '^2026-11-01T01:30:00-07:00[America/Edmonton]', options=options)[0]
        self.assertEqual(summer_side.to_zoned_datetime().fold, 0)
        self.assertEqual(winter_side.to_zoned_datetime().fold, 1)
        with self.assertRaises(Exception) as cm:
            loads2026('^2026-03-08T02:30:00-07:00[America/Edmonton]', options=options)
        self.assertEqual(cm.exception.category, 'invalid-temporal')


class SchemaTests(unittest.TestCase):
    def test_schema_dialect_local_and_external_references(self):
        schema = {
            '$schema': SCHEMA_DIALECT_2026,
            '$defs': {'positive': {'kind': 'integer', 'minimum': 1}},
            'kind': 'map',
            'properties': {
                'count': {'$ref': '#/$defs/positive'},
                'code': {'$ref': 'urn:example:code'},
            },
            'required': ['count', 'code'],
        }
        registry = {'urn:example:code': {'kind': 'string', 'pattern': '^[A-Z]+$'}}
        self.assertEqual([], validate2026({'count': 2, 'code': 'AXON'}, schema,
                                          registry=registry))
        issues = validate2026({'count': 0, 'code': 'bad'}, schema, registry=registry)
        # Diagnostics are ordered by stable data path, independent of mapping
        # insertion order: ``code`` precedes ``count``.
        self.assertEqual(['pattern', 'minimum'], [issue.code for issue in issues])
        unresolved = validate2026(1, {'$ref': 'urn:missing'})
        self.assertEqual('unresolved-ref', unresolved[0].code)

    def test_schema_reports_repeated_interned_values_at_each_path(self):
        issues = validate2026([0, 0], {'kind': 'list', 'items': {
            'kind': 'integer', 'minimum': 1,
        }})
        self.assertEqual([(0,), (1,)], [issue.path for issue in issues])

    def test_public_kind_names_and_numeric_union(self):
        self.assertEqual([], validate2026(3, {"kind": "integer"}))
        self.assertEqual([], validate2026(True, {"kind": "boolean"}))
        self.assertEqual([], validate2026(Decimal("1.5"), {"kind": "number"}))
        self.assertEqual("kind", validate2026("3", {"kind": "number"})[0].code)

    def test_mapping_constraints_and_structured_paths(self):
        schema = {
            'kind': 'map',
            'required': ['name', 'age'],
            'additional': False,
            'properties': {
                'name': {'kind': 'string', 'min_length': 1},
                'age': {'kind': 'int', 'minimum': 0, 'maximum': 130},
            },
        }
        self.assertEqual(validate2026({'name': 'Alex', 'age': 27}, schema), [])
        issues = validate2026({'name': '', 'age': 200, 'extra': True}, schema)
        self.assertEqual({issue.code for issue in issues}, {'min-length', 'maximum', 'additional'})
        self.assertIn(('age',), [issue.path for issue in issues])

    def test_node_schema_from_axon(self):
        schema = load_schema2026('''schema{
          kind:"node" name:"person" required:["name" "age"] additional:false
          field{name:"name" kind:"string" min_length:1}
          field{name:"age" kind:"int" minimum:0 maximum:130}}
        ''')
        person = loads2026('person{name:"Alex" age:27}')[0]
        self.assertEqual(validate2026(person, schema), [])
        bad = loads2026('person{name:"" age:-1}')[0]
        self.assertEqual({issue.code for issue in validate2026(bad, schema)}, {'min-length', 'minimum'})

    def test_tuple_prefix_and_set_items(self):
        schema = {'kind': 'tuple', 'prefix_items': [{'kind': 'int'}, {'kind': 'string'}], 'max_items': 2}
        self.assertEqual(validate2026((1, 'x'), schema), [])
        self.assertTrue(validate2026((1, 2), schema))
        set_schema = {'kind': 'set', 'items': {'kind': 'int', 'minimum': 0}}
        self.assertEqual(validate2026(AxonSet([1, 2]), set_schema), [])


class CanonicalGraphTests(unittest.TestCase):
    def test_shared_value_gets_first_encounter_label(self):
        shared = {'v': 1}
        text = dumps2026([shared, [shared, shared]], canonical=True, graph=True)
        self.assertEqual(text, '&1 {v:1}\n[*1 *1]')
        values = loads2026(text, options=Options(graph=True))
        self.assertIs(values[0], values[1][0])
        self.assertIs(values[1][0], values[1][1])

    def test_cycle_is_canonical_and_round_trips_identity(self):
        cycle = {}
        cycle['self'] = cycle
        text = dumps2026([cycle], canonical=True, graph=True)
        self.assertEqual(text, '&1 {self:*1}')
        back = loads2026(text, options=Options(graph=True))[0]
        self.assertIs(back, back['self'])

    def test_canonical_labels_follow_sorted_map_traversal(self):
        left, right = {'n': 1}, {'n': 2}
        value = {'z': right, 'a': left, 'again': left, 'zagain': right}
        self.assertEqual(
            dumps2026([value], canonical=True, graph=True),
            '{a:&1 {n:1} again:*1 z:&2 {n:2} zagain:*2}',
        )


class ProfileTests(unittest.TestCase):
    def test_canonical_verify_profile_and_composition(self):
        self.assertEqual(loads2026(
            'axon{require:["canonical-verify"]} {a:1 b:2}'), [{'a': 1, 'b': 2}])
        with self.assertRaises(Exception) as cm:
            loads2026('axon{require:["canonical-verify"]} {b:2 a:1}')
        self.assertEqual(cm.exception.category, 'unexpected-token')
        with self.assertRaises(Exception) as cm:
            loads2026('axon{require:["canonical-verify" "scale-significant"]} 1.0D')
        self.assertEqual(cm.exception.category, 'unsupported-profile-feature')

    def test_cid_generation_parsing_and_verification(self):
        value = {'name': 'AXON', 'version': 2026}
        cid = cid2026(value)
        self.assertTrue(cid.startswith('bafkrei'))
        self.assertTrue(verify_cid2026(cid, value))
        self.assertFalse(verify_cid2026(cid, {'name': 'other'}))
        link = loads2026('cid("%s")' % cid, options=Options(doc_link=True))[0]
        self.assertTrue(verify_cid2026(link, value))
        with self.assertRaises(Exception) as cm:
            loads2026('cid("bafy-invalid")', options=Options(doc_link=True))
        self.assertEqual(cm.exception.category, 'invalid-link')

    def test_scale_significant_decimal_and_temporal_semantics(self):
        options = Options(scale_significant=True)
        first, second = loads2026('1.0D 1.00D', options=options)
        self.assertIsInstance(first, ScaledDecimal)
        self.assertNotEqual(first, second)
        self.assertEqual(dumps2026([first, second]), '1.0D\n1.00D')
        short, long = loads2026('^12:00:00.5 ^12:00:00.500', options=options)
        self.assertNotEqual(short, long)
        self.assertEqual(dumps2026([short, long]),
                         '^12:00:00.5\n^12:00:00.500')
        with self.assertRaises(Exception) as cm:
            canonical2026(first)
        self.assertEqual(cm.exception.category, 'unsupported-profile-feature')

    def test_doc_link_is_gated_and_header_enabled(self):
        target = cid2026('test target')
        ordinary = loads2026('cid("%s")' % target)[0]
        self.assertIsInstance(ordinary, Node)
        linked = loads2026('cid("%s")' % target, options=Options(doc_link=True))[0]
        self.assertEqual((linked.kind, linked.target), ('cid', target))
        header = loads2026(
            'axon{require:["doc-link"]} link("https://example.invalid/x")')[0]
        self.assertEqual(header.kind, 'link')

    def test_individual_compatibility_flags_do_not_enable_bundle(self):
        self.assertEqual(loads2026('007', options=Options(lax_numbers=True)), [7])
        with self.assertRaises(Exception):
            loads2026('007', options=Options(legacy_escapes=True))
        self.assertEqual(loads2026('"x\\q"', options=Options(legacy_escapes=True)),
                         ['x\\q'])
        self.assertEqual(loads2026('#_ ignored\n1',
                                   options=Options(hash_underscore_comment=True)), [1])


if __name__ == '__main__':
    unittest.main()
