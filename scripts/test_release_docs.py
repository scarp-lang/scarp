"""Offline consistency check for published and candidate Scarp release documentation.

Run: python3 -m unittest scripts/test_release_docs.py
This keeps published-version references consistent while preserving
immutable historical prerelease records.
"""

from pathlib import Path
import re
import json
import unittest

ROOT = Path(__file__).resolve().parents[1]
PUBLISHED_VERSION = "0.0.5-alpha.5"
HISTORICAL_ALPHA4 = "0.0.5-alpha.4"
PACKAGE_VERSION = "0.0.6-alpha.1"


def package_version():
    cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version = "([^"]+)"$', cargo, re.MULTILINE)
    if match is None:
        raise AssertionError("Cargo.toml package version not found")
    return match.group(1)


class ReleaseDocConsistencyTests(unittest.TestCase):
    def test_current_release_anchors(self):
        version = package_version()
        self.assertEqual(version, PACKAGE_VERSION)
        needles = {
            "README.md": (f"**{version}**", f"latest published prerelease is\n[{PUBLISHED_VERSION}]"),
            "docs/README.md": (f"**{version} candidate**", f"latest published prerelease is\n[{PUBLISHED_VERSION}]"),
            "docs/compatibility.md": (f"`{version}` is an unpublished candidate", f"latest published prerelease is\n[{PUBLISHED_VERSION}]"),
            "docs/reference/cli.md": (f"v{version}",),
            "docs/development/feature-matrix.md": (f"Cargo declares {version}",),
            "CHANGELOG.md": (f"## {version} — candidate, unpublished",),
            "docs/releases/v0.0.6-alpha.1.md": (f"Scarp v{version}", "unpublished prerelease candidate", "not evidence of publication"),
            "docs/releases/v0.0.5-alpha.4.md": (f"Scarp v{HISTORICAL_ALPHA4}",),
            "docs/releases/v0.0.5-alpha.5.md": (f"Scarp v{PUBLISHED_VERSION}", "Publication integrity", "not evidence of publication"),
            "docs/releases/v0.0.5-alpha.5-validation.md": (
                f"Scarp v{PUBLISHED_VERSION}", "2eeed0aa52ccb59523307841ad8ffbd83f8911a8",
                "41825396712", "4 November 2026",
            ),
            "docs/releases/v0.0.5-alpha.4-validation.md": (
                f"Scarp v{HISTORICAL_ALPHA4}",
                "79141329176f5dce51457e90a6652902f6432135",
                "41642854489",
            ),
        }
        for filename, expected in needles.items():
            with self.subTest(file=filename):
                text = (ROOT / filename).read_text(encoding="utf-8")
                for needle in expected:
                    self.assertIn(needle, text, f"update {filename} when changing the package release anchor")

    def test_latest_published_prerelease_remains_separate_from_candidate(self):
        version = PUBLISHED_VERSION
        self.assertEqual(PACKAGE_VERSION, package_version())
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
