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
PUBLISHED_VERSION = "0.0.6-alpha.7"
ALPHA6_SOURCE = "cdf6a3924ec2a36c7beca5a9aec4f01a0bce515e"
HISTORICAL_ALPHA3 = "0.0.6-alpha.3"
HISTORICAL_ALPHA2 = "0.0.6-alpha.2"
HISTORICAL_ALPHA1 = "0.0.6-alpha.1"
HISTORICAL_ALPHA4 = "0.0.5-alpha.4"
HISTORICAL_ALPHA5 = "0.0.5-alpha.5"
PACKAGE_VERSION = "0.0.6-alpha.8"


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
    def assert_no_reused_alpha4_authorization(self):
        """A reviewed future request must match its new version and source."""
        request_path = ROOT / ".github/release-request.json"
        if not request_path.exists():
            return
        request = json.loads(request_path.read_text(encoding="utf-8"))
        validate_release_request_document(request, PACKAGE_VERSION)
        self.assertNotEqual(
            request.get("source_commit"),
            "f04be0758cbce405ddcdb229699b0ff83c09d207",
            "alpha.4 source approval cannot authorize alpha.5",
        )

    def test_current_release_anchors(self):
        version = package_version()
        self.assertEqual(version, PACKAGE_VERSION)
        needles = {
            "README.md": (f"**{PUBLISHED_VERSION}**", f"latest published prerelease is\n[{PUBLISHED_VERSION}]"),
            "docs/README.md": (f"**{PUBLISHED_VERSION} prerelease**", f"latest published prerelease is\n[{PUBLISHED_VERSION}]"),
            "docs/compatibility.md": (f"`{PUBLISHED_VERSION}` is **a published GitHub prerelease**", f"latest published prerelease is\n[{PUBLISHED_VERSION}]"),
            "docs/reference/cli.md": (f"v{PUBLISHED_VERSION}",),
            "docs/development/feature-matrix.md": (f"Cargo declares `{version}`",),
            "CHANGELOG.md": (
                f"## {PUBLISHED_VERSION} — published GitHub prerelease, 2026-11-11",
                "## 0.0.6-alpha.6 — published GitHub prerelease, 2026-11-10",
            ),
            f"docs/releases/v{version}-publication.md": (
                f"# Scarp v{version} — prerelease",
                "## Compatibility and limits",
            ),
            f"docs/releases/v{PUBLISHED_VERSION}-validation.md": (
                "Published GitHub experimental prerelease", "draft=false", "prerelease=true",
                "b1c1fd232b418267d95520c69ee563ac99ecae30", "4712acbd01914b86ea2fdfcb904b24edf44e5f44", "441415412", "42987993233",
                "fc584bd9eec7d2ce10a31892d418a1319543c669b5c36ef390761f885d11d319",
            ),
            "docs/releases/v0.0.6-alpha.5.md": ("Scarp v0.0.6-alpha.5", "source candidate", "Publication gates"),
            "docs/releases/v0.0.6-alpha.5-publication.md": (
                "Scarp v0.0.6-alpha.5 — prerelease", "## New in",
                "## Compatibility and limits",
            ),
            "docs/releases/v0.0.6-alpha.2.md": (f"Scarp v{HISTORICAL_ALPHA2}", "not a published GitHub release", "release engineering"),
            "docs/releases/v0.0.6-alpha.1.md": (f"Scarp v{HISTORICAL_ALPHA1}", "published GitHub prerelease", "post-publication verification record"),
            "docs/releases/v0.0.6-alpha.1-validation.md": (
                f"Scarp v{HISTORICAL_ALPHA1}", "aa329a22cf2bf92d2c9814a26ff9c8b67a72e6e8",
                "42020731912", "5 November 2026",
            ),
            "docs/releases/v0.0.6-alpha.3-validation.md": (
                f"Scarp v{HISTORICAL_ALPHA3}",
                "ceff07d9014f466974e0c9e02fc5d23573f55777",
                "42372237527", "437560402", "7 November 2026",
                "ca60692c2d93be0173ec5955bcad65e512a73846bcdc65ecdc3e13746e07e1ad",
            ),
            "docs/releases/v0.0.6-alpha.4-validation.md": (
                "Scarp v0.0.6-alpha.4",
                "f04be0758cbce405ddcdb229699b0ff83c09d207",
                "438100037", "42459313202",
                "958f8a62aec536dd6919ce4bfbb65d9d1afef3c40cdfe3c7427a59db35e69132",
            ),
            "docs/releases/v0.0.6-alpha.5-validation.md": (
                "Scarp v0.0.6-alpha.5",
                "205e92f166f186c3905e5880d41c31717b697b8e",
                "439059496", "42610738759",
                "b671dc5307c4b11a2b782870f20313afa1f082e3670c90b35957c53d009867ef",
            ),
            "docs/releases/v0.0.6-alpha.2-validation.md": (
                f"Scarp v{HISTORICAL_ALPHA2}",
                "398b8ff705154a02423fe458ce3a427401e81e31",
                "42207322756", "436398698", "6 November 2026",
                "57d2a62eb0263ac1de5cd9c926273f71bbd4e70ea853fb65ba25f0dc3614055a",
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

    def test_alpha5_published_notes_and_historical_candidate_snapshot(self):
        candidate = (ROOT / "docs/releases/v0.0.6-alpha.5.md").read_text(encoding="utf-8")
        public = (ROOT / "docs/releases/v0.0.6-alpha.5-publication.md").read_text(encoding="utf-8")
        self.assertIn("Versioned development source candidate", candidate)
        self.assertIn("# Scarp v0.0.6-alpha.5 — prerelease", public)
        self.assertNotIn("source candidate", public.lower())
        self.assert_no_reused_alpha4_authorization()
        for filename in (
            "docs/native-elf64.md", "docs/cosine-similarity.md",
            "docs/sampled-calculus.md",
            "examples/sampled_calculus.srp", "examples/sampled_calculus.stdout",
            "examples/sample_cumulative_area.srp", "examples/sample_cumulative_area.stdout",
            "examples/cosine_similarity.srp", "examples/cosine_similarity.stdout",
            "docs/nucleus-selection.md", "docs/pearson-correlation.md",
            "examples/nucleus_selection.srp", "examples/nucleus_selection.stdout",
            "examples/pearson_correlation.srp", "examples/pearson_correlation.stdout",
        ):
            with self.subTest(file=filename):
                self.assertTrue((ROOT / filename).is_file())

    def test_alpha3_immutable_candidate_snapshot_and_published_notes(self):
        candidate = (ROOT / "docs/releases/v0.0.6-alpha.3.md").read_text(encoding="utf-8")
        public = (ROOT / "docs/releases/v0.0.6-alpha.3-publication.md").read_text(encoding="utf-8")
        checklist = (ROOT / "docs/releases/v0.0.6-alpha.3-preparation.md").read_text(encoding="utf-8")
        self.assertIn("Not a published GitHub prerelease", candidate)
        self.assertIn(".github/release-request.json", checklist)
        self.assertIn("# Scarp v0.0.6-alpha.3 — prerelease", public)
        self.assertIn("## Compatibility and limits", public)
        for phrase in (
            "not a published github release", "source candidate under validation",
            "do not authorize a tag", "status: preparation only",
        ):
            self.assertNotIn(phrase, public.lower())
        for member in (
            "docs/topk-selection.md", "docs/temperature-softmax.md",
            "examples/topk_scores.srp", "examples/topk_scores.stdout",
            "examples/temperature_softmax.srp", "examples/temperature_softmax.stdout",
        ):
            with self.subTest(member=member):
                self.assertTrue((ROOT / member).is_file())
                self.assertIn(member, public + checklist)

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
                    f"https://github.com/scarp-lang/scarp/blob/v{HISTORICAL_ALPHA2}/"
                    f"docs/development/{name}",
                    notes,
                )

    def test_latest_published_anchor_and_immutable_candidate_snapshot(self):
        version = PUBLISHED_VERSION
        self.assertEqual(PACKAGE_VERSION, package_version())
        self.assertNotEqual(version, PACKAGE_VERSION)
        self.assertFalse((ROOT / ".github/release-request.json").exists())
        for filename in ("README.md", "docs/README.md", "docs/compatibility.md"):
            with self.subTest(file=filename):
                text = (ROOT / filename).read_text(encoding="utf-8")
                matches = re.findall(r"latest published prerelease is\s*\[([^]]+)\]", text)
                self.assertEqual(matches, [version])
                self.assertIn(f"v{version}", text)
        # The tagged candidate file intentionally remains a pre-release
        # snapshot; current status is in the separate publication record.
        candidate = (ROOT / "docs/releases/v0.0.6-alpha.3.md").read_text(encoding="utf-8")
        self.assertIn("Not a published GitHub prerelease", candidate)
        published = (ROOT / "docs/releases/v0.0.6-alpha.3-validation.md").read_text(encoding="utf-8")
        self.assertIn("draft=false", published)
        self.assertIn("prerelease=true", published)
        self.assertIn("3f668c85aef3c82776277aec16fc124fbbb860ae", published)
        self.assertIn("c116498900ecf3bd227fc5eeb6ffc641822182f6", published)
        previous_candidate = (ROOT / "docs/releases/v0.0.6-alpha.2.md").read_text(encoding="utf-8")
        self.assertIn("not a published GitHub release", previous_candidate)
        previous_record = (ROOT / "docs/releases/v0.0.6-alpha.2-validation.md").read_text(encoding="utf-8")
        self.assertIn("34e782105624d4d134c499a13f84ae472d74d803", previous_record)


    def test_alpha6_publication_record_and_retired_source_authorization(self):
        current = (ROOT / "docs/releases/v0.0.6-alpha.6-validation.md").read_text(encoding="utf-8")
        self.assertIn("Published GitHub experimental prerelease", current)
        self.assertIn("Seven uploaded GitHub Release assets", current)
        self.assertIn("bounded input, text and tuples", current)
        self.assertIn(ALPHA6_SOURCE, current)
        self.assertIn("4bb6900f79d5a9b19af393a5dbe75a75ba99ddaf", current)
        self.assertIn("440327277", current)
        self.assertIn("42815043683", current)
        self.assertIn("e2a57e714719f07e43b177c9e5ef59637698cbb6219404cfa89d5aa20a2739ec", current)
        request_path = ROOT / ".github/release-request.json"
        if request_path.exists():
            request = json.loads(request_path.read_text(encoding="utf-8"))
            self.assertNotEqual(request.get("version"), "0.0.6-alpha.6")
            self.assertNotIn(ALPHA6_SOURCE, (request.get("source_commit"), request.get("resume_commit")))
        self.assert_no_reused_alpha4_authorization()
        for filename in (
            "README.md", "docs/README.md", "docs/compatibility.md",
            "docs/reference/cli.md", "docs/development/feature-matrix.md",
            "CHANGELOG.md", "docs/topk-selection.md",
            "docs/temperature-softmax.md",
        ):
            with self.subTest(file=filename):
                text = (ROOT / filename).read_text(encoding="utf-8")
                self.assertIn("0.0.6-alpha.3", text)
                self.assertNotIn("0.0.6-alpha.3` candidate source is **not published**", text)
        self.assertIn(
            "v0.0.6-alpha.3", (ROOT / "docs/releasing.md").read_text(encoding="utf-8")
        )

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
        # Match explicit alpha labels, including the real alpha.6 candidate.
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
            {"version": HISTORICAL_ALPHA1},
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
