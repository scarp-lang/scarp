"""Independent binary64 RMS oracle using Python's high-precision Decimal."""
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


class RmsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if EXECUTABLE is None:
            raise unittest.SkipTest('pass the built Scarp executable')

    def run_source(self, source, *flags, command='exec'):
        with tempfile.TemporaryDirectory(prefix='scarp-rms-') as folder:
            path = Path(folder) / 'signal.srp'
            path.write_text(source, encoding='utf-8')
            return subprocess.run([str(EXECUTABLE), command, *flags, str(path)],
                                  capture_output=True, check=False)

    def test_extreme_and_generated_values_against_decimal(self):
        maximum = sys.float_info.max
        tiny = math.ulp(0.0)
        cases = [[maximum, -maximum], [maximum, 0.0], [1e308, -1e308],
                 [tiny, -tiny], [tiny, 0.0], [0.0, -0.0], [3.0, 4.0],
                 [maximum] + [tiny] * 63, [sys.float_info.min] * 8]
        rng = random.Random(20261105)
        for exponent in [-1073, -1022, -500, -1, 0, 500, 1023]:
            for count in [1, 2, 17, 127]:
                cases.append([math.ldexp(rng.uniform(-1, 1), exponent)
                              for _ in range(count)])
        for values in cases:
            with self.subTest(count=len(values), maximum=max(map(abs, values))):
                source = 'print(rms([' + ','.join(map(repr, values)) + ']));'
                result = self.run_source(source)
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                self.assertEqual(result.stderr, b'')
                actual = float(result.stdout)
                with localcontext() as context:
                    context.prec = 200
                    exact = sum((Decimal.from_float(x) ** 2 for x in values), Decimal(0))
                    expected = float((exact / len(values)).sqrt())
                self.assertTrue(math.isfinite(actual) and actual >= 0)
                self.assertLessEqual(actual, max(map(abs, values)))
                self.assertLessEqual(abs(actual - expected), 6 * math.ulp(expected))

    def test_empty_domain_json_is_atomic_and_static_verification_is_inert(self):
        source = 'print("OUT"); eprint("ERR"); print(rms([]));'
        result = self.run_source(source, '--diagnostic-format=json')
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, b'')
        diagnostic = json.loads(result.stderr)
        self.assertEqual((diagnostic['schema_version'], diagnostic['code']), (1, 'E0034'))
        a, b = diagnostic['primary']['span']
        self.assertEqual(source.encode()[a:b], b'[]')
        self.assertIn('nonempty', diagnostic['message'])
        static = self.run_source(source, command='verify')
        self.assertEqual((static.returncode, static.stdout, static.stderr), (0, b'', b''))

    def test_maximum_length_and_exact_payload_boundary(self):
        source = 'print(rms(fill(1e308,65536)) == 1e308);'
        result = self.run_source(source, '--max-steps', '400000', '--max-vector-bytes', '524288')
        self.assertEqual((result.returncode, result.stdout, result.stderr), (0, b'true\n', b''))
        failed = self.run_source(source, '--max-steps', '400000', '--max-vector-bytes', '524287',
                                 '--diagnostic-format=json')
        self.assertEqual((failed.returncode, failed.stdout), (1, b''))
        self.assertEqual(json.loads(failed.stderr)['code'], 'E0036')


if __name__ == '__main__':
    if len(sys.argv) < 2:
        raise SystemExit('usage: python scripts/test_rms.py PATH_TO_SCARP')
    EXECUTABLE = Path(sys.argv.pop(1)).resolve()
    unittest.main()
