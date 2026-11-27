"""Alpha.18 release archive inventory safety and exact-source provenance tests."""

import importlib.util
import io
from pathlib import Path
import stat
import tarfile
import unittest
import zipfile


spec = importlib.util.spec_from_file_location(
    "verify_release", Path(__file__).with_name("verify-release.py"),
)
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)
ROOT = "scarp-0.0.6-alpha.18-x86_64-unknown-linux-gnu"
SOURCES = {"README.md": "blob-id", "docs/guide.md": "blob-id"}


def valid_entries():
    return [
        (ROOT + "/", "dir"),
        (ROOT + "/docs/", "dir"),
        (ROOT + "/README.md", "file"),
        (ROOT + "/docs/guide.md", "file"),
        (ROOT + "/BUILD-INFO.txt", "file"),
        (ROOT + "/scarp", "file"),
    ]


class ArchiveInventoryTests(unittest.TestCase):
    def test_alpha7_candidates_require_exact_git_source_for_full_inventory(self):
        for version in ("0.0.7-alpha.1", "0.0.7-alpha.2", "0.0.7-alpha.3"):
            with self.subTest(version=version):
                with self.assertRaisesRegex(ValueError, "requires the exact Git source directory"):
                    release.verify(Path("unused"), version, "a" * 40)

    def test_accepts_exact_tree_and_root_directory(self):
        release.validate_archive_inventory(valid_entries(), ROOT, "scarp", SOURCES)

    def test_parent_directories_are_optional(self):
        release.validate_archive_inventory(
            [entry for entry in valid_entries() if entry[1] != "dir"], ROOT, "scarp", SOURCES,
        )

    def test_rejects_unsafe_paths_and_wrong_root(self):
        unsafe = [
            "/" + ROOT + "/extra",
            ROOT + "/../outside",
            ROOT + "/docs/./guide.md",
            ROOT + "/docs//guide.md",
            ROOT + r"\outside",
            "unrelated/README.md",
            ROOT + "/docs/../../private",
        ]
        for path in unsafe:
            with self.subTest(path=path), self.assertRaisesRegex(ValueError, "unsafe|outside"):
                release.validate_archive_inventory(
                    valid_entries() + [(path, "file")], ROOT, "scarp", SOURCES,
                )

    def test_rejects_duplicate_file_and_directory_records(self):
        for entry in ((ROOT + "/README.md", "file"), (ROOT + "/docs", "dir")):
            with self.subTest(entry=entry), self.assertRaisesRegex(ValueError, "duplicate"):
                release.validate_archive_inventory(valid_entries() + [entry], ROOT, "scarp", SOURCES)

    def test_rejects_archive_symlinks_and_special_types(self):
        for kind in ("other", "symlink", "hardlink"):
            with self.subTest(kind=kind), self.assertRaisesRegex(ValueError, "non-regular"):
                release.validate_archive_inventory(
                    valid_entries() + [(ROOT + "/alias", kind)], ROOT, "scarp", SOURCES,
                )

    def test_rejects_extra_file_or_directory(self):
        for member in ((ROOT + "/hidden.txt", "file"), (ROOT + "/unknown/", "dir")):
            with self.subTest(member=member), self.assertRaisesRegex(ValueError, "unexpected"):
                release.validate_archive_inventory(
                    valid_entries() + [member], ROOT, "scarp", SOURCES,
                )

    def test_rejects_missing_expected_source_or_executable(self):
        for missing in (ROOT + "/docs/guide.md", ROOT + "/scarp", ROOT + "/BUILD-INFO.txt"):
            with self.subTest(missing=missing), self.assertRaisesRegex(ValueError, "missing="):
                release.validate_archive_inventory(
                    [entry for entry in valid_entries() if entry[0] != missing],
                    ROOT, "scarp", SOURCES,
                )

    def test_alpha19_closed_inventory_contract_rejects_extra_and_missing_files(self):
        # The full source inventory is enforced at verify() for alpha.19.
        # A loose inventory would silently permit extra files in release archives.
        alpha19_root = ROOT.replace("alpha.18", "alpha.19")
        entries = [(name.replace(ROOT, alpha19_root), kind) for name, kind in valid_entries()]
        release.validate_archive_inventory(entries, alpha19_root, "scarp", SOURCES)
        with self.assertRaisesRegex(ValueError, "unexpected"):
            release.validate_archive_inventory(
                entries + [(alpha19_root + "/payload.txt", "file")],
                alpha19_root, "scarp", SOURCES,
            )
        with self.assertRaisesRegex(ValueError, "missing="):
            release.validate_archive_inventory(
                [entry for entry in entries if entry[0] != alpha19_root + "/scarp"],
                alpha19_root, "scarp", SOURCES,
            )

    def test_standalone_verifier_without_source_manifest_still_rejects_unsafe_members(self):
        release.validate_archive_inventory(
            valid_entries() + [(ROOT + "/extra.txt", "file")], ROOT, "scarp", {},
        )
        with self.assertRaisesRegex(ValueError, "unsafe"):
            release.validate_archive_inventory(
                valid_entries() + [(ROOT + "/../escape", "file")], ROOT, "scarp", {},
            )

    def test_zip_unix_symlink_is_not_regular_file(self):
        archive = io.BytesIO()
        with zipfile.ZipFile(archive, "w") as output:
            info = zipfile.ZipInfo(ROOT + "/link")
            info.create_system = 3
            info.external_attr = (stat.S_IFLNK | 0o777) << 16
            output.writestr(info, b"README.md")
        archive.seek(0)
        with zipfile.ZipFile(archive) as package:
            with self.assertRaisesRegex(ValueError, "non-regular"):
                release.validate_archive_inventory(
                    release.zip_archive_members(package), ROOT, "scarp", {},
                )

    def test_tar_symlink_is_not_regular_file(self):
        archive = io.BytesIO()
        with tarfile.open(fileobj=archive, mode="w") as output:
            info = tarfile.TarInfo(ROOT + "/link")
            info.type = tarfile.SYMTYPE
            info.linkname = ROOT + "/README.md"
            output.addfile(info)
        archive.seek(0)
        with tarfile.open(fileobj=archive, mode="r") as package:
            with self.assertRaisesRegex(ValueError, "non-regular"):
                release.validate_archive_inventory(
                    release.tar_archive_members(package), ROOT, "scarp", {},
                )


if __name__ == "__main__":
    unittest.main()
