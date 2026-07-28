"""Smoke-test the installed AXON 2026 distribution, outside the source tree."""

import axon2


if axon2.__version__ != "1.0.0":
    raise SystemExit("unexpected installed version: %s" % axon2.__version__)

value = axon2.loads2026("{b:2 a:1}")[0]
if axon2.canonical2026(value) != b"{a:1 b:2}":
    raise SystemExit("installed canonical writer smoke test failed")

print("installed pyaxon 1.0.0 smoke test passed")
