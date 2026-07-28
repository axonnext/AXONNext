"""Differentially verify AXON finite-binary64 rendering against V8."""

import argparse
from pathlib import Path
import shutil
import struct
import subprocess

from axon2.v2026 import canonical2026


ROOT = Path(__file__).resolve().parents[1]
MASK64 = (1 << 64) - 1
FRACTION_MASK = (1 << 52) - 1


def edge_patterns():
    """Cover both signs and every finite exponent at critical significands."""
    fractions = (
        0, 1, 2, (1 << 51) - 1, 1 << 51,
        (1 << 51) + 1, FRACTION_MASK - 1, FRACTION_MASK,
    )
    for sign in (0, 1):
        for exponent in range(0x7ff):
            prefix = (sign << 63) | (exponent << 52)
            for fraction in fractions:
                yield prefix | fraction


def random_patterns(count, seed):
    """Yield deterministic xorshift64* finite bit patterns."""
    state = seed & MASK64
    if state == 0:
        state = 0x9E3779B97F4A7C15
    produced = 0
    while produced < count:
        state ^= state >> 12
        state ^= (state << 25) & MASK64
        state ^= state >> 27
        state &= MASK64
        bits = (state * 0x2545F4914F6CDD1D) & MASK64
        if ((bits >> 52) & 0x7ff) == 0x7ff:
            continue
        produced += 1
        yield bits


def expected_jcs(bits):
    value = struct.unpack(">d", bits.to_bytes(8, "big"))[0]
    rendered = canonical2026(value).decode("utf-8")
    if value == 0.0:
        return "0"
    if rendered.endswith(".0") and "e" not in rendered:
        return rendered[:-2]
    return rendered


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--random-cases", type=int, default=100_000)
    parser.add_argument("--seed", type=lambda value: int(value, 0),
                        default=0xA10A2026D1FF3E57)
    parser.add_argument("--node", default=None)
    arguments = parser.parse_args()
    if arguments.random_cases < 0:
        parser.error("--random-cases must be non-negative")

    node = arguments.node or shutil.which("node")
    if not node:
        parser.error("Node.js is required for the independent V8 oracle")
    bits = list(edge_patterns())
    bits.extend(random_patterns(arguments.random_cases, arguments.seed))
    bit_lines = "".join("%016x\n" % value for value in bits)
    process = subprocess.run(
        [node, str(ROOT / "tools" / "check_rfc8785_node.mjs"),
         "--stdin-bits"],
        input=bit_lines, text=True, capture_output=True, check=False)
    if process.returncode:
        raise SystemExit(
            "V8 oracle failed (%d):\n%s" %
            (process.returncode, process.stderr[-8000:]))
    actual = process.stdout.splitlines()
    if len(actual) != len(bits):
        raise SystemExit(
            "V8 oracle returned %d rows for %d inputs" %
            (len(actual), len(bits)))
    for index, (pattern, observed) in enumerate(zip(bits, actual)):
        expected = expected_jcs(pattern)
        if observed != expected:
            raise SystemExit(
                "binary64 mismatch at row %d, bits %016x: AXON/JCS %r, V8 %r" %
                (index, pattern, expected, observed))
    print(
        "V8 differential verified %d finite binary64 values "
        "(%d exponent-edge + %d deterministic random)" %
        (len(bits), len(bits) - arguments.random_cases, arguments.random_cases))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
