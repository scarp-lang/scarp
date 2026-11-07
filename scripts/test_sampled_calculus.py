"""Independent Decimal reference for the proposed Scarp sampled calculus kernels.

Runs only against a built compiler from the checked-out source; it is not an
oracle for an older published alpha.3 executable.
"""
import ast
from decimal import Decimal, localcontext
import json
import math
from pathlib import Path
import random
import subprocess
import sys
import tempfile
import unittest

EXECUTABLE = None


def source_call(name, x, y):
    xs = ','.join(repr(v) for v in x)
    ys = ','.join(repr(v) for v in y)
    return f'print({name}([{xs}],[{ys}]));'


def exact_integral(x, y):
    with localcontext() as context:
        context.prec = 200
        result = Decimal(0)
        for i in range(len(x) - 1):
            width = Decimal.from_float(x[i + 1]) - Decimal.from_float(x[i])
            pair = Decimal.from_float(y[i]) + Decimal.from_float(y[i + 1])
            result += width * pair / 2
        return float(result)


def exact_slopes(x, y):
    with localcontext() as context:
        context.prec = 200
        return [
            float(
                (Decimal.from_float(y[i + 1]) - Decimal.from_float(y[i])) /
                (Decimal.from_float(x[i + 1]) - Decimal.from_float(x[i]))
            )
            for i in range(len(x) - 1)
        ]


def almost_ulp(actual, expected, ulps=12):
    if not math.isfinite(actual):
        return False
    return abs(actual - expected) <= ulps * math.ulp(expected)


class SampledCalculusTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if EXECUTABLE is None:
            raise unittest.SkipTest('pass the built Scarp executable')

    def run_source(self, source, *flags, command='exec'):
        with tempfile.TemporaryDirectory(prefix='scarp-samples-') as folder:
            path = Path(folder) / 'measurements.srp'
            path.write_text(source, encoding='utf-8')
            return subprocess.run(
                [str(EXECUTABLE), command, *flags, str(path)],
                capture_output=True, check=False,
            )

    def test_integrals_against_independent_decimal(self):
        maximum = sys.float_info.max
        tiny = math.ulp(0.0)
        cases = [
            ([0.0, 1.0], [maximum, maximum]),
            ([0.0, 1.0], [tiny, tiny]),
            ([0.0, 1.0e308], [-maximum, maximum]),
            ([0.0, 1.0, 3.0], [0.0, 2.0, 6.0]),
            ([0.0, 1.0, 2.0], [1.0, 0.0, -1.0]),
        ]
        rng = random.Random(20261107)
        for exponent in [-500, -100, -1, 0, 100, 500]:
            for count in [2, 3, 7, 16]:
                x = [rng.uniform(-4.0, 4.0)]
                for _ in range(count - 1):
                    x.append(x[-1] + rng.uniform(0.25, 10.0))
                y = [math.ldexp(rng.uniform(-1.0, 1.0), exponent)
                     for _ in range(count)]
                cases.append((x, y))
        for x, y in cases:
            with self.subTest(x=x, y=y):
                expected = exact_integral(x, y)
                actual = self.run_source(source_call('sample_area', x, y))
                self.assertEqual(actual.returncode, 0, actual.stderr.decode())
                self.assertEqual(actual.stderr, b'')
                self.assertTrue(
                    almost_ulp(float(actual.stdout), expected),
                    (actual.stdout, expected),
                )

    def test_interval_derivatives_against_independent_decimal(self):
        maximum = sys.float_info.max
        cases = [
            ([0.0, 1.0e308], [-maximum, maximum]),
            ([0.0, 1.0, 3.0], [0.0, 2.0, 6.0]),
            ([0.0, 1.0, 3.0, 4.0], [0.0, 1.0, 9.0, 16.0]),
        ]
        rng = random.Random(20261107)
        for count in [2, 3, 7, 16]:
            x = [0.0]
            for _ in range(count - 1):
                x.append(x[-1] + rng.uniform(0.5, 8.0))
            y = [rng.uniform(-100.0, 100.0) for _ in range(count)]
            cases.append((x, y))
        for x, y in cases:
            with self.subTest(x=x, y=y):
                actual = self.run_source(source_call('sample_slopes', x, y))
                self.assertEqual(actual.returncode, 0, actual.stderr.decode())
                self.assertEqual(actual.stderr, b'')
                values = ast.literal_eval(actual.stdout.decode())
                expected = exact_slopes(x, y)
                self.assertEqual(len(values), len(expected))
                for value, reference in zip(values, expected):
                    self.assertTrue(almost_ulp(value, reference),
                                    (value, reference))

    def test_domain_diagnostic_is_atomic_but_static_verify_is_inert(self):
        source = 'print("hidden"); print(sample_area([0.0,0.0],[1.0,2.0]));'
        result = self.run_source(source, '--diagnostic-format=json')
        self.assertEqual((result.returncode, result.stdout), (1, b''))
        diagnostic = json.loads(result.stderr)
        self.assertEqual((diagnostic['schema_version'], diagnostic['code']),
                         (1, 'E0034'))
        self.assertIn('strictly increasing', diagnostic['message'])
        static = self.run_source(source, command='verify')
        self.assertEqual((static.returncode, static.stdout, static.stderr),
                         (0, b'', b''))

    def test_exact_payload_limit_after_vector_inputs(self):
        source = 'print(sample_slopes([0.0,1.0,3.0],[0.0,2.0,6.0]));'
        success = self.run_source(source, '--max-vector-bytes', '64')
        self.assertEqual((success.returncode, success.stdout, success.stderr),
                         (0, b'[2.0, 2.0]\n', b''))
        failure = self.run_source(
            source, '--max-vector-bytes', '63', '--diagnostic-format=json',
        )
        self.assertEqual((failure.returncode, failure.stdout), (1, b''))
        self.assertEqual(json.loads(failure.stderr)['code'], 'E0036')


if __name__ == '__main__':
    if len(sys.argv) < 2:
        raise SystemExit('usage: python scripts/test_sampled_calculus.py PATH_TO_SCARP')
    EXECUTABLE = Path(sys.argv.pop(1)).resolve()
    unittest.main()
