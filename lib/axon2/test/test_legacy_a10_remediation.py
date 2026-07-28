import os
from pathlib import Path
import subprocess
import sys
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[3]


class NativeHistoricalRegressionTests(unittest.TestCase):
    def run_isolated(self, source):
        environment = os.environ.copy()
        environment["PYTHONPATH"] = str(ROOT / "lib")
        process = subprocess.run(
            [sys.executable, "-c", textwrap.dedent(source)],
            cwd=ROOT, env=environment, capture_output=True, text=True,
            check=False)
        self.assertEqual(
            0, process.returncode,
            "native subprocess failed (%d)\nstdout=%s\nstderr=%s" % (
                process.returncode, process.stdout, process.stderr))

    def test_ordered_map_factory_tuple_update_pop_and_views(self):
        self.run_isolated("""
            from axon2.odict import OrderedDict, odict
            made = odict([('a', 1), ('b', 2)])
            assert list(made.items()) == [('a', 1), ('b', 2)]
            updated = OrderedDict()
            updated.update((('a', 1), ('b', 2)))
            assert list(updated.items()) == [('a', 1), ('b', 2)]
            assert ('a', 1) in updated.items()
            assert updated.keys() != ['a', 'b', 'c']
            reordered = OrderedDict((('b', 2), ('a', 1)))
            assert updated.keys() == reordered.keys()
            assert updated.items() == reordered.items()
            assert updated.keys() == {'a', 'b'}
            values = updated.values()
            assert values == values
            assert values != updated.values()
            assert updated == {'a': 1, 'b': 2}
            assert updated != [('a', 1), ('b', 2)]
            assert updated.pop('a') == 1
            assert list(updated.items()) == [('b', 2)]
            assert 'a' not in updated
        """)

    def test_registration_resets_constants_and_empty_stream(self):
        self.run_isolated("""
            import axon2
            import axon2._dumper as dumper
            class Thing: pass
            @axon2.factory('thing')
            def make(attrs, vals): return Thing()
            assert isinstance(axon2.loads('thing{}', mode='strict')[0], Thing)
            axon2.reset_factory()
            try:
                axon2.loads('thing{}', mode='strict')
            except axon2.AxonError:
                pass
            else:
                raise AssertionError('factory reset did not clear registration')
            axon2.defname('Answer', 42)
            assert axon2.loads('$Answer') == [42]
            dumper.reduce(Thing, lambda value: axon2.node('thing'))
            dumper.reset_reduce()
            assert Thing not in dumper.reduce_dict
            assert axon2.dumps([]) == ''
        """)

    def test_anchor_followed_by_eof_comment_is_categorized_not_native_crash(self):
        self.run_isolated("""
            import axon2
            for source in ('&hOY#', '&4é#comment', '&label#'):
                try:
                    axon2.loads(source)
                except axon2.AxonError:
                    pass
                else:
                    raise AssertionError(
                        'missing anchored value did not raise AxonError: %r' % source)
        """)


if __name__ == "__main__":
    unittest.main()
