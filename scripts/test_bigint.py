#!/usr/bin/env python3
"""Seeded, independent Python-integer oracle; Python is never a runtime dependency.

Run after cargo build: python3 scripts/test_bigint.py target/debug/scarp
Use --large for controlled 100,000/1,000,000-digit linear-operation checks.
"""
import argparse
import random
import subprocess
import sys
import tempfile
from pathlib import Path


def execute(binary, source, expected, *, digits=10000, steps=50000000):
    with tempfile.TemporaryDirectory(prefix="scarp-bigint-oracle-") as directory:
        path = Path(directory) / "oracle.srp"
        path.write_text(source, encoding="utf-8")
        flags = ["--max-bigint-digits", str(digits), "--max-steps", str(steps)]
        actual = subprocess.run([str(binary), "exec", *flags, str(path)],
                                capture_output=True, timeout=60, check=False)
        assert actual.returncode == 0, actual.stderr.decode("utf-8", errors="replace")
        assert not actual.stderr, actual.stderr
        assert actual.stdout == expected.encode("ascii"), "Python/Scarp arithmetic disagreement"
        checked = subprocess.run([str(binary), "check", *flags, str(path)],
                                 capture_output=True, timeout=60, check=False)
        assert checked.returncode == 0, checked.stderr
        assert checked.stdout == b"" and checked.stderr == b"", checked
        verified = subprocess.run([str(binary), "verify", str(path)],
                                  capture_output=True, timeout=60, check=False)
        assert verified.returncode == 0 and verified.stdout == b"" and verified.stderr == b"", verified


def quotient(a, b):
    # Python // floors; Scarp integer division truncates toward zero.
    q = abs(a) // abs(b)
    return -q if (a < 0) != (b < 0) else q


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--large", action="store_true")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    rng = random.Random(0x5343415250424947)
    pairs = []
    # Base-10^9 limb edges, both signs, long quotient, near-equal operands,
    # and exact division. No expected result uses the Scarp algorithm.
    for exponent in [1, 9, 18, 27, 99, 100, 257, 1000]:
        for offset in [-1, 0, 1]:
            a = 10 ** exponent + offset
            b = 10 ** max(1, exponent // 2) + 1
            pairs.extend([(a, b), (-a, b), (a, -b), (-a, -b), (a*b, b), (a, a)])
    for _ in range(160):
        a = rng.randrange(-(10 ** rng.randrange(1, 601)), 10 ** rng.randrange(1, 601))
        b = 0
        while b == 0:
            b = rng.randrange(-(10 ** rng.randrange(1, 601)), 10 ** rng.randrange(1, 601))
        pairs.append((a, b))
    for start in range(0, len(pairs), 24):
        source, expected = [], []
        for a, b in pairs[start:start+24]:
            q = quotient(a, b)
            r = a - q*b
            source.append(f'{{ bind a=bigint("{a}"); bind b=bigint("{b}"); '
                          'print(a+b,a-b,a*b,a/b,a%b,a<b,a<=b,a>b,a>=b,a==b,a!=b); }')
            values = [a+b, a-b, a*b, q, r, a < b, a <= b, a > b, a >= b, a == b, a != b]
            expected.append(" ".join(str(value).lower() for value in values) + "\n")
        execute(binary, "\n".join(source), "".join(expected))
    print(f"Python oracle passed: {len(pairs)} pairs, {len(pairs)*11} arithmetic/comparison results; exec/check parity and static verify")
    if args.large:
        # Explicitly bounded exception to Python's decimal-conversion guard.
        # Only these generated fixtures use the larger controlled limit.
        if hasattr(sys, "set_int_max_str_digits"):
            sys.set_int_max_str_digits(1000100)
        for count in [19, 100, 1000, 10000, 100000, 1000000]:
            text = "8" * count
            number = int(text)
            source = (f'bind a=bigint("{text}"); bind one=bigint("1"); '
                      'print(a+one); print(a-one); print(a/one); print(a%one); print(a==a);')
            expected = f"{number+1}\n{number-1}\n{number}\n0\ntrue\n"
            execute(binary, source, expected, digits=max(10000,count), steps=10000000)
            print(f"Large linear-operation oracle passed: {count} decimal digits")


if __name__ == "__main__":
    main()
