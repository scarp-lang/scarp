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


def validate_release_request_document(request, version):
    """Validate a control request's shape; validation never grants approval."""
    if not isinstance(request, dict):
        raise ValueError("release request must be a JSON object")
    allowed = {
        "version", "source_commit", "tagger_name", "tagger_email",
        "resume_commit", "resume_release_id", "not_before", "not_after",
    }
    if set(request) - allowed:
        raise ValueError("unknown release request fields")
    if request.get("version") != version:
        raise ValueError("release request version must match Cargo")
    if (not isinstance(request.get("tagger_name"), str)
            or not request["tagger_name"].strip()
            or not isinstance(request.get("tagger_email"), str)
            or not re.fullmatch(r"[^\s@]+@[^\s@]+", request["tagger_email"])):
        raise ValueError("release request requires tagger identity")
    recovery = "resume_commit" in request or "resume_release_id" in request
    if recovery:
        if ("source_commit" in request
                or not re.fullmatch(r"[0-9a-f]{40}", str(request.get("resume_commit", "")))
                or type(request.get("resume_release_id")) is not int
                or request["resume_release_id"] <= 0):
            raise ValueError("draft recovery requires its source commit and release ID")
    elif not re.fullmatch(r"[0-9a-f]{40}", str(request.get("source_commit", ""))):
        raise ValueError("request needs a full approved source SHA")
    if ("not_before" in request) != ("not_after" in request):
        raise ValueError("publication window must include both endpoints")
    for field in ("not_before", "not_after"):
        if field in request and (
            not isinstance(request[field], str)
            or not re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", request[field])
        ):
            raise ValueError("publication window must use UTC timestamps")


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
            "docs/releases/v0.0.6-alpha.2-publication.md": (
                f"Scarp v{version} — prerelease", "## New in",
                "## Compatibility and limits",
            ),
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

    def test_publication_notes_use_tag_stable_links_and_accurate_cache_scope(self):
        # Release notes are rendered on the GitHub Release page, not inside
        # docs/releases/. Relative Markdown paths there would be broken.
        notes = (ROOT / "docs/releases/v0.0.6-alpha.2-publication.md").read_text(
            encoding="utf-8"
        )
        self.assertNotIn("four new caches", notes.lower())
        self.assertIn("three cache tiers", notes)
        self.assertNotIn("](../development/", notes)
        contracts = (
            "incremental-invalidation.md",
            "semantic-cache.md",
            "token-cache.md",
            "syntax-cache.md",
            "cache-eviction.md",
            "cache-edit-planning.md",
        )
        for name in contracts:
            with self.subTest(contract=name):
                path = ROOT / "docs/development" / name
                self.assertTrue(path.is_file(), f"missing bundled contract: {name}")
                self.assertIn(f"`docs/development/{name}`", notes)
        for name in ("cache-eviction.md", "cache-edit-planning.md"):
            with self.subTest(link=name):
                self.assertIn(
                    f"https://github.com/scarp-lang/scarp/blob/v{PACKAGE_VERSION}/"
                    f"docs/development/{name}",
                    notes,
                )

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
        request_path = ROOT / ".github/release-request.json"
        if request_path.exists():
            # A separately reviewed release-control PR is allowed to pass CI.
            # This is structural validation only, not release authorization.
            validate_release_request_document(
                json.loads(request_path.read_text(encoding="utf-8")), version
            )

    def test_release_request_schema_requires_exact_version_and_source(self):
        request = {
            "version": PACKAGE_VERSION,
            "source_commit": "a" * 40,
            "tagger_name": "Release Maintainer",
            "tagger_email": "maintainer@example.invalid",
        }
        validate_release_request_document(request, PACKAGE_VERSION)
        for change in (
            {"version": PUBLISHED_VERSION},
            {"source_commit": "a" * 39},
            {"source_commit": "A" * 40},
            {"source_commit": None},
            {"tagger_name": ""},
            {"tagger_email": "invalid"},
            {"not_before": "2026-11-06T12:00:00Z"},
            {"not_after": "2026-11-06T14:00:00Z"},
            {"extra": True},
        ):
            with self.subTest(change=change):
                with self.assertRaises(ValueError):
                    validate_release_request_document({**request, **change}, PACKAGE_VERSION)

    def test_release_request_draft_recovery_schema_is_separate(self):
        recovery = {
            "version": PACKAGE_VERSION,
            "resume_commit": "b" * 40,
            "resume_release_id": 1234,
            "tagger_name": "Release Maintainer",
            "tagger_email": "maintainer@example.invalid",
        }
        validate_release_request_document(recovery, PACKAGE_VERSION)
        with self.assertRaises(ValueError):
            validate_release_request_document(
                {**recovery, "source_commit": "a" * 40}, PACKAGE_VERSION
            )


if __name__ == "__main__":
    unittest.main()
