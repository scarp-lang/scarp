"""Offline consistency check for the current Scarp release documentation.

Run: python3 -m unittest scripts/test_release_docs.py
This checks the package-version anchor and deliberately excludes immutable
historical release notes and version-comparison tables.
"""

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]


def package_version():
    cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version = "([^"]+)"$', cargo, re.MULTILINE)
    if match is None:
        raise AssertionError("Cargo.toml package version not found")
    return match.group(1)


class ReleaseDocConsistencyTests(unittest.TestCase):
    def test_current_release_anchors(self):
        version = package_version()
        needles = {
            "README.md": (f"**{version}**", f"latest published prerelease is\n[{version}]"),
            "docs/README.md": (f"**{version} prerelease**", f"latest published prerelease is\n[{version}]"),
            "docs/compatibility.md": (f"published in {version}", f"latest published prerelease is\n[{version}]"),
            "docs/reference/cli.md": (f"(v{version})",),
            "docs/development/feature-matrix.md": (f"Cargo declares {version}",),
            "CHANGELOG.md": (f"## {version} — prerelease",),
        }
        for filename, expected in needles.items():
            with self.subTest(file=filename):
                text = (ROOT / filename).read_text(encoding="utf-8")
                for needle in expected:
                    self.assertIn(needle, text, f"update {filename} when changing the package release anchor")

    def test_no_historical_release_claimed_as_latest(self):
        version = package_version()
        for filename in ("README.md", "docs/README.md", "docs/compatibility.md"):
            with self.subTest(file=filename):
                text = (ROOT / filename).read_text(encoding="utf-8")
                matches = re.findall(r"latest published prerelease is\s*\[([^]]+)\]", text)
                self.assertEqual(matches, [version])


if __name__ == "__main__":
    unittest.main()
