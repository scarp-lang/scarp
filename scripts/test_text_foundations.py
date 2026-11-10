#!/usr/bin/env python3
"""Independent seeded Python oracle for the unreleased scalar/text intrinsics.

Run after cargo build: python3 scripts/test_text_foundations.py --scarp target/debug/scarp
Uses only the Python standard library; creates no repository output files.
"""

import argparse
import math
import random
import struct
import subprocess
import tempfile
from pathlib import Path


def quoted(text):
    # Scarp's existing escapes; leave real UTF-8 scalars intact.
    return '"' + (
        text.replace('\\', '\\\\')
        .replace('"', '\\"')
        .replace('\n', '\\n')
        .replace('\r', '\\r')
        .replace('\t', '\\t')
    ) + '"'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--scarp', required=True, type=Path)
    args = parser.parse_args()
    binary = args.scarp.resolve(strict=True)
    rng = random.Random(20261109)
    lines, expected = [], []

    def add(expression, value, floating=False):
        lines.append('print(' + expression + ');')
        expected.append((floating, value))

    integers = [-2**63, 2**63 - 1, 0, -1, 1]
    integers += [rng.randrange(-2**63, 2**63) for _ in range(200)]
    for value in integers:
        literal = '(-9223372036854775807-1)' if value == -2**63 else str(value)
        add('integer_to_text(' + literal + ')', str(value))
        add('text_to_integer(' + quoted(str(value)) + ')', str(value))

    for _ in range(200):
        value = struct.unpack('!d', rng.getrandbits(64).to_bytes(8, 'big'))[0]
        if math.isfinite(value):
            add('float_to_text(text_to_float(' + quoted(repr(value)) + '))',
                struct.pack('!d', value), floating=True)

    # Explicit Unicode White_Space subset. Python str.isspace additionally
    # accepts U+001C..U+001F, so using Python strip() would be the wrong oracle.
    whitespace = [' ', '\t', '\r', '\u0085', '\u00a0', '\u1680', '\u2003',
                  '\u2028', '\u202f', '\u205f', '\u3000']
    alphabet = ['a', 'b', 'é', 'ह', '🌍', '\u0301', '\u200b', '\ufeff']
    for _ in range(200):
        text = ''.join(rng.choice(alphabet) for _ in range(rng.randrange(0, 30)))
        pattern = ''.join(rng.choice(alphabet) for _ in range(rng.randrange(1, 4)))
        replacement = ''.join(rng.choice(alphabet) for _ in range(rng.randrange(0, 4)))
        values = ','.join(quoted(s) for s in [text, pattern, replacement])
        add('text_replace(' + values + ')', text.replace(pattern, replacement))
        padded = rng.choice(whitespace) + text + rng.choice(whitespace)
        add('text_trim(' + quoted(padded) + ')', text)

    with tempfile.TemporaryDirectory(prefix='scarp-text-oracle-') as directory:
        source = Path(directory) / 'text oracle.srp'
        source.write_text('\n'.join(lines), encoding='utf-8')
        result = subprocess.run([str(binary), 'exec', str(source)],
                                capture_output=True, check=True)
        if result.stderr or not result.stdout.endswith(b'\n'):
            raise AssertionError('successful exec must have empty stderr and final LF')
        actual = result.stdout.decode('utf-8').split('\n')[:-1]
        if len(actual) != len(expected):
            raise AssertionError(('output count', len(actual), len(expected)))
        for index, (got, (floating, want)) in enumerate(zip(actual, expected)):
            comparable = struct.pack('!d', float(got)) if floating else got
            if comparable != want:
                raise AssertionError((index, lines[index], got, want))
        for mode in ['verify', 'check']:
            checked = subprocess.run([str(binary), mode, str(source)],
                                     capture_output=True, check=True)
            if checked.stdout or checked.stderr:
                raise AssertionError(mode + ' unexpectedly wrote output')
    print('Python oracle passed: ' + str(len(expected)) +
          ' scalar/text results; static verify and evaluating check parity')


if __name__ == '__main__':
    main()
