"""Regression tests for deterministic native Scarp release archives."""

import importlib.util
from pathlib import Path
import hashlib
import os
import tarfile
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location("builder", Path(__file__).with_name("build-release-archive.py"))
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class DeterministicArchiveTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.directory = Path(self.tmp.name)
        self.package = self.directory / "scarp-0.0.6-alpha.18-x86_64-unknown-linux-gnu"
        (self.package / "docs").mkdir(parents=True)
        (self.package / "scarp").write_bytes(b"native-binary\0\x01")
        (self.package / "docs" / "overview.md").write_text("Scarp \u00e9\n", encoding="utf-8")
        (self.package / "BUILD-INFO.txt").write_text("commit=abc\n", encoding="utf-8")

    def test_tar_rebuild_is_byte_identical_and_executable(self):
        first, second = (self.directory / "first.tar.gz", self.directory / "second.tar.gz")
        builder.build(self.package, first)
        os.utime(self.package / "scarp", (100000, 100000))
        os.utime(self.package / "docs" / "overview.md", (200000, 200000))
        builder.build(self.package, second)
        self.assertEqual(digest(first), digest(second))
        with tarfile.open(first, "r:gz") as result:
            self.assertEqual(result.getmember(self.package.name + "/scarp").mode, 0o755)
            self.assertEqual(result.getmember(self.package.name + "/scarp").mtime, 0)
            self.assertEqual(result.extractfile(self.package.name + "/docs/overview.md").read(), "Scarp \u00e9\n".encode())

    def test_zip_rebuild_is_byte_identical_and_contains_canonical_paths(self):
        first, second = (self.directory / "first.zip", self.directory / "second.zip")
        builder.build(self.package, first)
        os.utime(self.package / "scarp", (100000, 100000))
        builder.build(self.package, second)
        self.assertEqual(digest(first), digest(second))
        with zipfile.ZipFile(first) as result:
            self.assertEqual(result.read(self.package.name + "/scarp"), b"native-binary\0\x01")
            self.assertEqual(result.getinfo(self.package.name + "/scarp").date_time, builder.ZIP_DATE)

    def test_rejects_existing_output(self):
        archive = self.directory / "existing.zip"
        builder.build(self.package, archive)
        with self.assertRaises(FileExistsError):
            builder.build(self.package, archive)

    def test_rejects_symlinks(self):
        link = self.package / "docs" / "outside.md"
        try:
            link.symlink_to(self.directory / "external")
        except (OSError, NotImplementedError):
            self.skipTest("creating symlinks is restricted on this platform")
        with self.assertRaisesRegex(ValueError, "unsupported package entry"):
            builder.build(self.package, self.directory / "blocked.tar.gz")


if __name__ == "__main__":
    unittest.main()
