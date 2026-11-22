"""Regression tests for the independent package payload comparator."""

import importlib.util
from pathlib import Path
import io
import tarfile
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location("verify_package_rebuild", Path(__file__).with_name("verify-package-rebuild.py"))
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class ComparisonTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.first = self.root / "first"
        self.second = self.root / "second"
        self.first.mkdir()
        self.second.mkdir()

    def archive(self, directory, target, data, *, timestamp):
        name = "scarp-0.0.6-alpha.18-" + target
        extension = verify.TARGETS[target]
        archive = directory / (name + extension)
        if extension == ".zip":
            with zipfile.ZipFile(archive, "w") as output:
                for file, content in data.items():
                    info = zipfile.ZipInfo(name + "/" + file, date_time=(2020 + timestamp, 1, 1, 0, 0, 0))
                    output.writestr(info, content)
        else:
            with tarfile.open(archive, "w:gz") as output:
                for file, content in data.items():
                    info = tarfile.TarInfo(name + "/" + file)
                    info.size = len(content)
                    info.mtime = timestamp
                    output.addfile(info, io.BytesIO(content))
        with archive.open("rb") as content:
            checksum = verify.digest(content)
        archive.with_name(archive.name + ".sha256").write_text(f"{checksum}  {archive.name}\n")
        return archive

    def test_tar_payloads_identical_despite_metadata_changes(self):
        target = "x86_64-unknown-linux-gnu"
        data = {"scarp": b"binary", "BUILD-INFO.txt": b"commit=abc\n"}
        self.archive(self.first, target, data, timestamp=0)
        self.archive(self.second, target, data, timestamp=1)
        count, same_archive = verify.compare(self.first, self.second, target)
        self.assertEqual(count, 2)
        self.assertFalse(same_archive)

    def test_zip_payloads_identical_despite_metadata_changes(self):
        target = "x86_64-pc-windows-msvc"
        data = {"scarp.exe": b"binary", "BUILD-INFO.txt": b"commit=abc\n"}
        self.archive(self.first, target, data, timestamp=0)
        self.archive(self.second, target, data, timestamp=1)
        count, same_archive = verify.compare(self.first, self.second, target)
        self.assertEqual(count, 2)
        self.assertFalse(same_archive)

    def test_strict_mode_rejects_metadata_only_changes(self):
        target = "x86_64-pc-windows-msvc"
        self.archive(self.first, target, {"scarp.exe": b"same"}, timestamp=0)
        self.archive(self.second, target, {"scarp.exe": b"same"}, timestamp=1)
        with self.assertRaisesRegex(ValueError, "archive bytes are not reproducible"):
            verify.compare(self.first, self.second, target, require_archive_identity=True)

    def test_strict_mode_accepts_exact_archive_bytes(self):
        target = "aarch64-apple-darwin"
        self.archive(self.first, target, {"scarp": b"same"}, timestamp=0)
        self.archive(self.second, target, {"scarp": b"same"}, timestamp=0)
        count, same_archive = verify.compare(
            self.first, self.second, target, require_archive_identity=True,
        )
        self.assertEqual(count, 1)
        self.assertTrue(same_archive)

    def test_detects_byte_change(self):
        target = "aarch64-apple-darwin"
        self.archive(self.first, target, {"scarp": b"native-a"}, timestamp=0)
        self.archive(self.second, target, {"scarp": b"native-b"}, timestamp=0)
        with self.assertRaisesRegex(ValueError, "changed=.*scarp"):
            verify.compare(self.first, self.second, target)

    def test_detects_added_or_missing_member(self):
        target = "x86_64-unknown-linux-gnu"
        self.archive(self.first, target, {"scarp": b"same"}, timestamp=0)
        self.archive(self.second, target, {"scarp": b"same", "unwanted": b"extra"}, timestamp=0)
        with self.assertRaisesRegex(ValueError, "added=.*unwanted"):
            verify.compare(self.first, self.second, target)

    def test_detects_corrupt_checksum(self):
        target = "x86_64-pc-windows-msvc"
        archive = self.archive(self.first, target, {"scarp.exe": b"same"}, timestamp=0)
        self.archive(self.second, target, {"scarp.exe": b"same"}, timestamp=0)
        archive.with_name(archive.name + ".sha256").write_text("0" * 64 + "  " + archive.name + "\n")
        with self.assertRaisesRegex(ValueError, "invalid SHA-256 sidecar"):
            verify.compare(self.first, self.second, target)

    def test_rejects_tar_traversal(self):
        target = "x86_64-unknown-linux-gnu"
        self.archive(self.first, target, {"../sneaky": b"bad"}, timestamp=0)
        self.archive(self.second, target, {"../sneaky": b"bad"}, timestamp=0)
        with self.assertRaisesRegex(ValueError, "unsafe archive member"):
            verify.compare(self.first, self.second, target)


if __name__ == "__main__":
    unittest.main()
