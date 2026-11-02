"""Independent JSON/schema/source-span checks against a built development CLI."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

EXECUTABLE = None
ROOT = Path(__file__).resolve().parents[1]

def invoke(*arguments, cwd=ROOT):
    return subprocess.run([str(EXECUTABLE), *arguments], cwd=cwd, capture_output=True, check=False)

class InspectionJsonTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if EXECUTABLE is None:
            raise unittest.SkipTest("pass the built Scarp executable to this script")

    def parse(self, *arguments, cwd=ROOT):
        result = invoke("modules", "--json", *arguments, cwd=cwd)
        self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8"))
        self.assertEqual(result.stderr, b"")
        self.assertTrue(result.stdout.endswith(b"\n"))
        return json.loads(result.stdout)

    def test_complete_graph_and_file_local_spans(self):
        obj = self.parse("examples/constant_expressions.srp")
        self.assertEqual(set(obj), {"schema_version", "entry", "modules", "constant_dependencies"})
        self.assertEqual((obj["schema_version"], obj["entry"]), (1, 0))
        modules = obj["modules"]
        self.assertEqual([module["id"] for module in modules], [0, 1, 2])
        sources = [(ROOT / module["path"]).read_bytes() for module in modules]
        def token(source, span):
            start, end = span
            self.assertTrue(0 <= start <= end <= len(sources[source]))
            return sources[source][start:end].decode("utf-8")
        for module in modules:
            source = module["id"]
            for record in module["imports"]:
                self.assertEqual(token(source, record["path_span"]), '"' + record["path"] + '"')
                self.assertEqual(token(source, record["alias_span"]), record["alias"])
                self.assertTrue(token(source, record["span"]).startswith("import "))
            self.assertEqual(module["dependencies"], list(dict.fromkeys(record["target"] for record in module["imports"])))
            for symbol in module["symbols"]:
                self.assertEqual(token(source, symbol["span"]), symbol["name"])
                self.assertNotIn("value", symbol)
                for parameter in symbol.get("parameters", []):
                    self.assertEqual(token(source, parameter["span"]), parameter["name"])
                    self.assertIsInstance(parameter["has_default"], bool)
                    self.assertNotIn("default", parameter)
        self.assertGreater(len(obj["constant_dependencies"]), 0)
        for edge in obj["constant_dependencies"]:
            self.assertTrue(token(edge["source"], edge["declaration"]).isidentifier())
            self.assertTrue(token(edge["target"], edge["target_declaration"]).isidentifier())
            if edge["source"] != edge["target"]:
                self.assertIn(".", token(edge["source"], edge["span"]))

    def test_unicode_crlf_and_dash_paths(self):
        with tempfile.TemporaryDirectory(prefix="scarp-json-") as folder:
            path = Path(folder) / "-caf\u00e9.srp"
            path.write_bytes('// caf\u00e9 \U0001f30d\r\npub bind answer: integer=6*7;'.encode("utf-8"))
            obj = self.parse("--", path.name, cwd=folder)
            module = obj["modules"][0]
            self.assertEqual(module["path"], path.name)
            symbol = module["symbols"][0]
            self.assertEqual(path.read_bytes()[slice(*symbol["span"])], b"answer")

    def test_failed_source_has_no_partial_json(self):
        result = invoke("modules", "--json", "tests/fixtures/constants/division.srp")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, b"")
        self.assertTrue(result.stderr.startswith(b"error[E0008]"))

    def test_json_is_repeatable_and_old_text_mode_still_works(self):
        first = invoke("modules", "--json", "examples/inspection_json.srp")
        second = invoke("modules", "--json", "examples/inspection_json.srp")
        self.assertEqual(first.returncode, 0)
        self.assertEqual(first.stdout, second.stdout)
        text = invoke("modules", "examples/inspection_json.srp")
        self.assertTrue(text.stdout.startswith(b"modules: 1, imports: 0\n"))

if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: python test_inspection_json.py <scarp executable>")
    EXECUTABLE = Path(sys.argv.pop()).resolve()
    unittest.main()
