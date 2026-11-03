"""Offline consistency check for the current Scarp release documentation.

Run: python3 -m unittest scripts/test_release_docs.py
This checks the package-version anchor and deliberately excludes immutable
historical release notes and version-comparison tables.
"""

from pathlib import Path
import re
import json
import unittest

ROOT = Path(__file__).resolve().parents[1]
PUBLISHED_VERSION = "0.0.5-alpha.3"


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
            "README.md": (f"**{version}**", f"latest published prerelease is\n[{PUBLISHED_VERSION}]"),
            "docs/README.md": (f"**{version} release candidate (unpublished)**", f"latest published prerelease is\n[{PUBLISHED_VERSION}]"),
            "docs/compatibility.md": (f"`{version}` is an unpublished prerelease candidate", f"latest published prerelease is\n[{PUBLISHED_VERSION}]"),
            "docs/reference/cli.md": (f"v{version}",),
            "docs/development/feature-matrix.md": (f"Cargo declares {version}",),
            "CHANGELOG.md": (f"## {version} — prerelease candidate (unpublished)",),
            "docs/releases/v0.0.5-alpha.4.md": (f"Scarp v{version}",),
        }
        for filename, expected in needles.items():
            with self.subTest(file=filename):
                text = (ROOT / filename).read_text(encoding="utf-8")
                for needle in expected:
                    self.assertIn(needle, text, f"update {filename} when changing the package release anchor")

    def test_no_historical_release_claimed_as_latest(self):
        version = PUBLISHED_VERSION
        self.assertNotEqual(version, package_version())
        for filename in ("README.md", "docs/README.md", "docs/compatibility.md"):
            with self.subTest(file=filename):
                text = (ROOT / filename).read_text(encoding="utf-8")
                matches = re.findall(r"latest published prerelease is\s*\[([^]]+)\]", text)
                self.assertEqual(matches, [version])


    def test_candidate_metadata_and_lockfile(self):
        version = package_version()
        metadata = json.loads(
            (ROOT / ".github/release-metadata.json").read_text(encoding="utf-8")
        )
        self.assertEqual(metadata["version"], version)
        self.assertEqual(metadata["schema_version"], 1)
        self.assertEqual(
            metadata["documentation_repository"],
            "https://github.com/scarp-lang/scarp-docs",
        )
        self.assertIn(
            f'version = "{version}"',
            (ROOT / "Cargo.lock").read_text(encoding="utf-8"),
        )
        # The separate exact-source publisher validates any future release
        # request; the documentation test must not block an approved request.


if __name__ == "__main__":
    unittest.main()
