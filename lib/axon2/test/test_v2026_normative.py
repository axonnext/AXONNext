import hashlib
import json
import math
from pathlib import Path
import re
import struct
import unittest

from axon2.cst2026 import parse_cst2026
from axon2.schema2026 import (
    SchemaRegistry2026, validate2026,
)
from axon2.v2026 import (
    Axon2026Error, AxonSet, Options, UNDEFINED, apply_migration_edits2026,
    canonical2026, dumps2026, iloads2026, loads2026, migrate2026,
    migrate_selective2026, verify_canonical2026,
)


ROOT = Path(__file__).resolve().parents[3]
NORMATIVE = ROOT / "conformance" / "normative_vectors.json"
RFC8785 = ROOT / "conformance" / "rfc8785_appendix_b.json"


class NormativeVectorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.manifest = json.loads(NORMATIVE.read_text(encoding="utf-8"))
        cls.vectors = {item["id"]: item for item in cls.manifest["vectors"]}

    def test_requirement_index_is_complete_and_bidirectional(self):
        requirements = self.manifest["requirements"]
        requirement_ids = [item["id"] for item in requirements]
        self.assertEqual(len(requirement_ids), len(set(requirement_ids)))
        self.assertEqual(len(self.vectors), len(self.manifest["vectors"]))
        referenced = set()
        for requirement in requirements:
            self.assertTrue(requirement.get("vectors") or requirement.get("external"),
                            requirement["id"])
            for vector_id in requirement.get("vectors", []):
                self.assertIn(vector_id, self.vectors, requirement["id"])
                referenced.add(vector_id)
            if requirement.get("external"):
                self.assertTrue((ROOT / "conformance" / requirement["external"]).is_file())
        self.assertEqual(set(self.vectors), referenced)
        self.assertTrue((ROOT / self.manifest["specification_file"]).is_file())
        covered_errors = {
            vector.get("expect", {}).get("error")
            for vector in self.manifest["vectors"]
            if vector.get("expect", {}).get("error")
        }
        self.assertEqual(
            set(self.manifest["error_categories"]), covered_errors)

    def test_every_normative_clause_is_fingerprint_bound(self):
        index_path = ROOT / "conformance" / self.manifest["normative_clause_index"]
        index = json.loads(index_path.read_text(encoding="utf-8"))
        spec_path = ROOT / index["specification_file"]
        self.assertEqual(self.manifest["specification_revision"],
                         index["specification_revision"])

        heading_pattern = re.compile(
            r"^#{2,4}\s+([0-9]+(?:\.[0-9]+)*)(?:\.\s|\s)")
        normative_pattern = re.compile(
            r"\b(?:MUST(?:\s+NOT)?|SHOULD(?:\s+NOT)?|MAY)\b")
        section = "preamble"
        clauses = []
        for line in spec_path.read_text(encoding="utf-8").splitlines():
            heading = heading_pattern.match(line)
            if heading:
                section = heading.group(1)
            if normative_pattern.search(line):
                normalized = " ".join(line.split())
                fingerprint = hashlib.sha256(
                    (section + "\0" + normalized).encode("utf-8")).hexdigest()
                clauses.append((section, fingerprint))

        self.assertEqual(index["clause_count"], len(clauses))
        self.assertEqual(index["fingerprints"],
                         [fingerprint for _, fingerprint in clauses])
        coverage = index["section_coverage"]
        self.assertEqual(set(coverage), {section for section, _ in clauses})
        requirement_ids = {item["id"] for item in self.manifest["requirements"]}
        for section_id, entry in coverage.items():
            with self.subTest(section=section_id):
                self.assertTrue(entry.get("requirements") or entry.get("disposition"))
                for requirement_id in entry.get("requirements", []):
                    self.assertIn(requirement_id, requirement_ids)

    def test_all_normative_vectors(self):
        for vector in self.manifest["vectors"]:
            with self.subTest(vector=vector["id"]):
                self._run_vector(vector)

    def test_cst_respects_legacy_binary_profile(self):
        source = "|YQ=="
        document = parse_cst2026(source, options=Options(legacy_binary=True))
        self.assertEqual(source, document.render())
        self.assertFalse(document.diagnostics)
        self.assertEqual([b"a"], document.values)

    def _run_vector(self, vector):
        operation = vector["operation"]
        if operation == "invalid-unicode":
            with self.assertRaises(Axon2026Error) as caught:
                loads2026("\ud800")
            self.assertEqual(vector["expect"]["error"], caught.exception.category)
            return
        if operation == "load":
            self._run_load(vector)
            return
        if operation == "canonical":
            values = loads2026(vector["input"])
            self.assertEqual(1, len(values))
            self.assertEqual(vector["expect"]["canonical"],
                             canonical2026(values[0]).decode("utf-8"))
            return
        if operation == "canonical-distinct":
            first, second = vector["inputs"]
            self.assertNotEqual(canonical2026(loads2026(first)[0]),
                                canonical2026(loads2026(second)[0]))
            return
        if operation == "semantic-equivalent":
            rendered = [dumps2026(loads2026(source), canonical=True)
                        for source in vector["inputs"]]
            self.assertEqual([vector["expect"]["canonical"]] * len(rendered),
                             rendered)
            return
        if operation == "incremental":
            iterator = iloads2026(
                vector["input"],
                options=Options(**vector.get("options", {})))
            self.assertEqual(vector["expect"]["first"],
                             canonical2026(next(iterator)).decode("utf-8"))
            with self.assertRaises(Axon2026Error) as caught:
                next(iterator)
            self.assertEqual(vector["expect"]["later_error"],
                             caught.exception.category)
            return
        if operation == "canonical-equivalent":
            options = Options(**vector.get("options", {}))
            rendered = [dumps2026(
                loads2026(source, options=options), canonical=True,
                graph=options.graph) for source in vector["inputs"]]
            self.assertEqual([rendered[0]] * len(rendered), rendered)
            return
        if operation == "writer-set-duplicate":
            with self.assertRaises(Axon2026Error) as caught:
                canonical2026(AxonSet(vector["values"]))
            self.assertEqual(vector["expect"]["error"],
                             caught.exception.category)
            return
        if operation == "verify-canonical":
            self.assertEqual(vector["expect"]["valid"],
                             verify_canonical2026(vector["input"]))
            return
        if operation == "cst":
            self._run_cst(vector)
            return
        if operation == "cst-error":
            options = Options(**vector.get("options", {}))
            with self.assertRaises(Axon2026Error) as caught:
                parse_cst2026(vector["input"], options=options)
            self.assertEqual(vector["expect"]["error"], caught.exception.category)
            return
        if operation == "schema":
            issues = validate2026(
                vector["value"], vector["schema"],
                max_issues=vector.get("max_issues", 256))
            self.assertEqual(vector["expect"]["issues"],
                             [item.code for item in issues])
            return
        if operation == "schema-registry":
            registry = SchemaRegistry2026()
            registry.register(vector["identifier"], vector["module"])
            registry.freeze()
            self.assertFalse(registry.network_resolution)
            issues = validate2026(
                vector["value"], vector["schema"], registry=registry)
            self.assertEqual(vector["expect"]["issues"],
                             [item.code for item in issues])
            return
        if operation == "schema-registry-governance":
            registry = SchemaRegistry2026()
            returned = registry.register(
                vector["identifier"], vector["module"])
            registry.freeze()
            returned["minimum"] = -1
            issues = validate2026(
                0, {"$ref": vector["identifier"]}, registry=registry)
            self.assertEqual(vector["expect"]["issues"],
                             [item.code for item in issues])
            self.assertEqual(vector["expect"]["frozen"], registry.frozen)
            self.assertEqual(vector["expect"]["network_resolution"],
                             registry.network_resolution)
            with self.assertRaises(RuntimeError):
                registry.register("urn:axon:test:late", {"kind": "any"})
            return
        if operation == "schema-order":
            issues = validate2026(vector["value"], vector["schema"])
            self.assertEqual(vector["expect"]["paths"],
                             [list(item.path) for item in issues])
            return
        if operation in ("migrate", "migrate-selective"):
            result = (migrate2026(vector["input"])
                      if operation == "migrate"
                      else migrate_selective2026(vector["input"]))
            self.assertEqual(vector["expect"]["output"], str(result))
            self.assertEqual(vector["expect"].get("mode", result.mode), result.mode)
            if vector["expect"].get("edits_reproduce"):
                self.assertEqual(str(result), apply_migration_edits2026(
                    vector["input"], result.edits))
            return
        self.fail("unknown normative vector operation %r" % operation)

    def _run_load(self, vector):
        raw_options = dict(vector.get("options", {}))
        compat = raw_options.pop("compat", False)
        options = Options.compat(**raw_options) if compat else Options(**raw_options)
        expected = vector["expect"]
        if "error" in expected:
            with self.assertRaises(Axon2026Error) as caught:
                loads2026(vector["input"], options=options)
            self.assertEqual(expected["error"], caught.exception.category)
            return
        values = loads2026(vector["input"], options=options)
        if "count" in expected:
            self.assertEqual(expected["count"], len(values))
        if expected.get("undefined"):
            self.assertIs(values[0], UNDEFINED)
            return
        graph = (options.graph or expected.get("self_cycle", False)
                 or expected.get("shared", False)
                 or "&" in expected.get("canonical", ""))
        if "canonical" in expected:
            self.assertEqual(expected["canonical"],
                             dumps2026(values, canonical=True, graph=graph))
        if expected.get("shared"):
            self.assertIs(values[0], values[1])
        if expected.get("self_cycle"):
            self.assertIs(values[0], values[0][0])

    def _run_cst(self, vector):
        document = parse_cst2026(
            vector["input"], options=Options(**vector.get("options", {})))
        expected = vector["expect"]
        if expected.get("render_identical"):
            self.assertEqual(vector["input"], document.render())
        if "trace_count" in expected:
            self.assertEqual(expected["trace_count"],
                             len(document.semantic_traces))
        if "token_texts" in expected:
            self.assertEqual(expected["token_texts"],
                             [item.text for item in document.tokens])
        if "value_count" in expected:
            self.assertEqual(expected["value_count"], len(document.values))
        if "diagnostic" not in expected:
            self.assertFalse(document.diagnostics)
            return
        candidates = [item for item in document.diagnostics
                      if item.category == expected["diagnostic"]]
        self.assertTrue(candidates)
        if "start_byte" in expected:
            self.assertIn(expected["start_byte"],
                          [item.span.start for item in candidates])
        if "expected" in expected:
            candidate = next(item for item in candidates
                             if list(item.expected) == expected["expected"])
            self.assertEqual(expected["expected"], list(candidate.expected))
        else:
            candidate = candidates[0]
        if "fixed" in expected:
            self.assertTrue(candidate.fixes)
            fix = candidate.fixes[0]
            data = vector["input"].encode("utf-8")
            fixed = (data[:fix.start] + fix.replacement.encode("utf-8")
                     + data[fix.end:]).decode("utf-8")
            self.assertEqual(expected["fixed"], fixed)
            self.assertFalse(parse_cst2026(fixed).diagnostics)


class Rfc8785Binary64Tests(unittest.TestCase):
    def test_complete_appendix_b_table(self):
        corpus = json.loads(RFC8785.read_text(encoding="utf-8"))
        self.assertEqual(26, len(corpus["cases"]))
        for case in corpus["cases"]:
            with self.subTest(bits=case["bits"]):
                value = struct.unpack(">d", bytes.fromhex(case["bits"]))[0]
                rendered = canonical2026(value).decode("utf-8")
                self.assertEqual(case["axon"], rendered)
                parsed = loads2026(rendered)[0]
                if math.isnan(value):
                    self.assertTrue(math.isnan(parsed))
                else:
                    self.assertEqual(struct.pack(">d", value),
                                     struct.pack(">d", parsed))


if __name__ == "__main__":
    unittest.main()
