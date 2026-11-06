"""Offline consistency check for published Scarp release documentation.

Run: python3 -m unittest scripts/test_release_docs.py
This keeps published-version references consistent while preserving
immutable historical prerelease records.
"""

from pathlib import Path
import re
import json
import unittest

ROOT = Path(__file__).resolve().parents[1]
PUBLISHED_VERSION = "0.0.6-alpha.1"
HISTORICAL_ALPHA4 = "0.0.5-alpha.4"
HISTORICAL_ALPHA5 = "0.0.5-alpha.5"
PACKAGE_VERSION = "0.0.6-alpha.2"


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
            "docs/README.md": (f"**{version} prerelease**", f"latest published prerelease is\n[{PUBLISHED_VERSION}]"),
            "docs/compatibility.md": (f"`{version}` is a candidate", f"latest published prerelease is\n[{PUBLISHED_VERSION}]"),
            "docs/reference/cli.md": (f"v{version}",),
            "docs/development/feature-matrix.md": (f"Cargo declares {version}",),
            "CHANGELOG.md": (f"## {version} — prerelease candidate, 2026-11-06",),
            "docs/releases/v0.0.6-alpha.2.md": (f"Scarp v{version}", "not a published GitHub release", "release engineering"),
            "docs/releases/v0.0.6-alpha.1.md": (f"Scarp v{PUBLISHED_VERSION}", "published GitHub prerelease", "post-publication verification record"),
            "docs/releases/v0.0.6-alpha.1-validation.md": (
                f"Scarp v{PUBLISHED_VERSION}", "aa329a22cf2bf92d2c9814a26ff9c8b67a72e6e8",
                "42020731912", "5 November 2026",
            ),
            "docs/releases/v0.0.5-alpha.4.md": (f"Scarp v{HISTORICAL_ALPHA4}",),
            "docs/releases/v0.0.5-alpha.5.md": (f"Scarp v{HISTORICAL_ALPHA5}", "Publication integrity", "not evidence of publication"),
            "docs/releases/v0.0.5-alpha.5-validation.md": (
                f"Scarp v{HISTORICAL_ALPHA5}", "2eeed0aa52ccb59523307841ad8ffbd83f8911a8",
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

    def test_latest_published_anchor_remains_separate_from_candidate(self):
        version = PUBLISHED_VERSION
        self.assertEqual(PACKAGE_VERSION, package_version())
        self.assertNotEqual(version, PACKAGE_VERSION)
        for filename in ("README.md", "docs/README.md", "docs/compatibility.md"):
            with self.subTest(file=filename):
                text = (ROOT / filename).read_text(encoding="utf-8")
                matches = re.findall(r"latest published prerelease is\s*\[([^]]+)\]", text)
                self.assertEqual(matches, [version])


    def test_no_stale_candidate_status_for_current_release(self):
        for filename in (
            "README.md",
            "docs/README.md",
            "docs/compatibility.md",
            "docs/reference/cli.md",
            "docs/releases/v0.0.6-alpha.1.md",
            "docs/modules.md",
            "docs/testing.md",
            "docs/architecture.md",
            "docs/performance.md",
            "docs/vectors.md",
            "docs/releasing.md",
            "CHANGELOG.md",
        ):
            with self.subTest(file=filename):
                content = (ROOT / filename).read_text(encoding="utf-8")
                self.assertNotIn("unpublished candidate", content.lower())
                self.assertNotIn("latest published prerelease remains v0.0.5-alpha.5", content.lower())
                self.assertNotIn("unreleased v0.0.6", content.lower())
                self.assertNotIn("unpublished v0.0.6", content.lower())

    def test_release_link_labels_match_target_versions(self):
        # The version family 0.0.6 starts at alpha.1, not "alpha.6".
        # Match labels to their actual release-note targets so future edits
        # cannot accidentally substitute the series number for the alpha.
        documents = (
            "README.md",
            "CHANGELOG.md",
            "docs/README.md",
            "docs/compatibility.md",
            "docs/testing.md",
            "docs/reference/cli.md",
            "docs/modules.md",
            "docs/symbol-index.md",
            "docs/development/feature-matrix.md",
            "docs/development/incremental-invalidation.md",
            "docs/releases/v0.0.6-alpha.1-validation.md",
        )
        links = re.compile(r"\[([^\]\r\n]+)\]\(([^)\r\n]+)\)")
        release_target = re.compile(
            r"v\d+\.\d+\.\d+-alpha\.(\d+)(?:-validation)?\.md(?:#[\w-]+)?$"
        )
        alpha_label = re.compile(r"\balpha\.(\d+)\b", re.IGNORECASE)
        for filename in documents:
            with self.subTest(file=filename):
                content = (ROOT / filename).read_text(encoding="utf-8")
                self.assertNotRegex(
                    content, r"\balpha\.6\b",
                    "use the full v0.0.6-alpha.1 label, not ambiguous alpha.6",
                )
                for link_label, href in links.findall(content):
                    target = release_target.search(href)
                    label = alpha_label.search(link_label)
                    if target and label:
                        with self.subTest(file=filename, link=href):
                            self.assertEqual(
                                label.group(1), target.group(1),
                                f"{link_label!r} does not match {href!r}",
                            )

    def test_candidate_metadata_lockfile_and_no_implicit_release_request(self):
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
        self.assertFalse((ROOT / ".github/release-request.json").exists())
        # Candidate preparation cannot authorize a future release. Publication
        # still needs a separate explicitly reviewed exact-source request.


if __name__ == "__main__":
    unittest.main()
