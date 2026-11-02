"""Independent schema, byte-span, and scalar-position checks against the CLI."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

EXECUTABLE = None
ROOT = Path(__file__).resolve().parents[1]
FLAG = "--diagnostic-format=json"


def invoke(*args, cwd=ROOT):
    return subprocess.run([str(EXECUTABLE), *args], cwd=cwd, capture_output=True, check=False)


class DiagnosticJsonTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if EXECUTABLE is None:
            raise unittest.SkipTest("pass the built Scarp executable to this script")

    def parse(self, result, status=1):
        self.assertEqual(result.returncode, status, result.stderr.decode("utf-8"))
        self.assertEqual(result.stdout, b"")
        self.assertTrue(result.stderr.endswith(b"\n"))
        obj = json.loads(result.stderr)
        self.assertEqual(set(obj), {"schema_version", "severity", "code", "message", "span_encoding", "column_encoding", "position_base", "primary", "related", "help"})
        self.assertEqual((obj["schema_version"], obj["severity"], obj["span_encoding"], obj["column_encoding"], obj["position_base"]), (1, "error", "utf-8-bytes", "unicode-scalar", 1))
        return obj

    def location(self, record, folder):
        self.assertEqual(set(record), {"path", "span", "start", "end"})
        data = (Path(folder) / record["path"]).read_bytes()
        start, end = record["span"]
        self.assertTrue(0 <= start <= end <= len(data))
        for offset, position in [(start, record["start"]), (end, record["end"])]:
            prefix = data[:offset].decode("utf-8")
            self.assertEqual(position, {"line": prefix.count("\n") + 1, "column": len(prefix.rsplit("\n", 1)[-1]) + 1})
        return data[start:end].decode("utf-8")

    def test_unicode_crlf_imported_runtime_error_keeps_file_local_coordinates(self):
        with tempfile.TemporaryDirectory(prefix="scarp-diagnostic-") as folder:
            root = Path(folder)
            (root / "main.srp").write_bytes(b'import "lib.srp" as lib; print("OUT"); eprint("ERR"); print(lib.f());')
            (root / "lib.srp").write_bytes('// café🌍\r\npub pure funcra f()->integer{return 1/0;}'.encode("utf-8"))
            for mode in ["run", "check"]:
                obj = self.parse(invoke(mode, FLAG, "main.srp", cwd=folder))
                self.assertEqual(obj["code"], "E0008")
                self.assertEqual(Path(obj["primary"]["path"]).name, "lib.srp")
                self.assertEqual(self.location(obj["primary"], folder), "/")
                self.assertNotIn("OUT", json.dumps(obj))
                self.assertNotIn("ERR", json.dumps(obj))

    def test_related_import_locations_have_independent_source_owners(self):
        with tempfile.TemporaryDirectory(prefix="scarp-diagnostic-") as folder:
            root = Path(folder)
            (root / "main.srp").write_text('import "lib.srp" as lib; print(lib.hidden());', encoding="utf-8")
            (root / "lib.srp").write_text('funcra hidden()->integer{return 1;}', encoding="utf-8")
            obj = self.parse(invoke("modules", "--json", FLAG, "main.srp", cwd=folder))
            self.location(obj["primary"], folder)
            self.assertGreater(len(obj["related"]), 0)
            owners = {Path(obj["primary"]["path"]).name}
            for note in obj["related"]:
                self.location(note["location"], folder)
                owners.add(Path(note["location"]["path"]).name)
            self.assertEqual(owners, {"main.srp", "lib.srp"})

    def test_usage_and_io_errors_have_no_source_codes_or_coordinates(self):
        for args in [("run", FLAG), ("run", FLAG, "--max-text-bytes", "-1", "missing.srp"), ("modules", FLAG, FLAG, "missing.srp")]:
            obj = self.parse(invoke(*args), status=2)
            self.assertIsNone(obj["code"])
            self.assertIsNone(obj["primary"])
        with tempfile.TemporaryDirectory(prefix="scarp-diagnostic-") as folder:
            obj = self.parse(invoke("tokens", FLAG, "missing.srp", cwd=folder))
            self.assertIsNone(obj["code"])
            self.assertIsNone(obj["primary"])

    def test_metadata_json_and_diagnostic_json_control_separate_streams(self):
        result = invoke("modules", FLAG, "--json", "examples/inspection_json.srp")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, b"")
        self.assertEqual(json.loads(result.stdout)["schema_version"], 1)
        plain = invoke("run", "examples/text_construction.srp")
        structured = invoke("run", FLAG, "examples/text_construction.srp")
        self.assertEqual((plain.returncode, plain.stdout, plain.stderr), (structured.returncode, structured.stdout, structured.stderr))

    def test_text_allocation_failure_is_structured_at_the_constructor(self):
        with tempfile.TemporaryDirectory(prefix="scarp-diagnostic-") as folder:
            Path(folder, "text.srp").write_text('bind a=text_repeat("x",2); print(a);', encoding="utf-8")
            for mode in ["run", "check"]:
                obj = self.parse(invoke(mode, "--max-text-bytes", "2", FLAG, "text.srp", cwd=folder))
                self.assertEqual(obj["code"], "E0048")
                self.assertEqual(self.location(obj["primary"], folder), 'text_repeat("x",2)')
                self.assertIn("--max-text-bytes", obj["help"])

    def test_lexer_eof_positions_follow_scalar_columns(self):
        with tempfile.TemporaryDirectory(prefix="scarp-diagnostic-") as folder:
            Path(folder, "eof.srp").write_bytes('print("é🌍'.encode("utf-8"))
            obj = self.parse(invoke("tokens", FLAG, "eof.srp", cwd=folder))
            self.location(obj["primary"], folder)
            self.assertIsNotNone(obj["code"])

    def test_intentional_error_example_matches_the_versioned_snapshot(self):
        obj = self.parse(invoke("check", FLAG, "examples/diagnostics/type_error.srp"))
        expected = json.loads((ROOT / "examples/diagnostics/type_error.diagnostic.json").read_bytes())
        self.location(obj["primary"], ROOT)
        obj["primary"]["path"] = "type_error.srp"
        for note in obj["related"]:
            self.location(note["location"], ROOT)
            note["location"]["path"] = "type_error.srp"
        self.assertEqual(obj, expected)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: test_diagnostic_json.py <built-scarp-executable>")
    EXECUTABLE = Path(sys.argv.pop()).resolve()
    unittest.main()
