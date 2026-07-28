"""Regression tests for the legacy-engine C-stack-overflow DoS (M-LEGACY-1).

The legacy `Loader`/`Dumper` are compiled `cdef` classes whose container methods
recurse on the C stack, so before the depth guard a deeply nested document sent to
the public `axon2.loads`/`axon2.dumps` overflowed the C stack and crashed the whole
process (`STATUS_STACK_OVERFLOW`, uncatchable), rather than raising a normal
`AxonError`. Each case below runs in an isolated subprocess and asserts the process
exits 0: a regression would exit with a hard crash code and fail the assertion,
without taking the test runner down with it.
"""
import os
from pathlib import Path
import subprocess
import sys
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[3]


class LegacyDepthLimitTests(unittest.TestCase):
    def run_isolated(self, source):
        environment = os.environ.copy()
        environment["PYTHONPATH"] = str(ROOT / "lib")
        process = subprocess.run(
            [sys.executable, "-c", textwrap.dedent(source)],
            cwd=ROOT, env=environment, capture_output=True, text=True,
            check=False)
        self.assertEqual(
            0, process.returncode,
            "legacy subprocess crashed or failed (%d)\nstdout=%s\nstderr=%s" % (
                process.returncode, process.stdout, process.stderr))

    def test_loads_deeply_nested_openers_raise_axonerror_not_crash(self):
        # Each container opener recurses on the C stack in the legacy loader.
        for opener in "[{(":
            self.run_isolated("""
                import axon2
                from axon2.errors import AxonError
                try:
                    axon2.loads(%r * 100000)
                    raise SystemExit("deeply nested input was accepted")
                except AxonError:
                    pass  # clean, catchable rejection -- correct
            """ % opener)

    def test_dumps_deeply_nested_structure_raises_axonerror_not_crash(self):
        self.run_isolated("""
            import axon2
            from axon2.errors import AxonError
            deep = 0
            for _ in range(100000):
                deep = [deep]
            try:
                axon2.dumps([deep])
                raise SystemExit("deeply nested structure was serialised")
            except AxonError:
                pass  # clean, catchable rejection -- correct
        """)

    def test_normal_and_within_limit_use_is_unaffected(self):
        self.run_isolated("""
            import axon2
            data = [1, 2.5, "hi", True, None, {"a": 1, "b": [1, 2, 3]}, (1, 2)]
            assert axon2.loads(axon2.dumps(data)) == data
            nested = 0
            for _ in range(100):          # comfortably within the depth limit
                nested = [nested]
            assert axon2.loads(axon2.dumps([nested])) == [nested]
        """)


if __name__ == "__main__":
    unittest.main()
