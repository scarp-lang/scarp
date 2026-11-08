"""Exercise the release verifier with complete and damaged package fixtures."""

import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile
import hashlib
import json
import subprocess

spec = importlib.util.spec_from_file_location(
    "verify_release", Path(__file__).with_name("verify-release.py")
)
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)
COMMIT = "a" * 40


class ReleasedExamples(unittest.TestCase):
    def test_v002_examples_retain_their_published_bytes(self):
        root = Path(__file__).resolve().parents[1]
        manifest = root / "tests/fixtures/v0.0.2-examples.sha256"
        entries = manifest.read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(entries), 22)
        for entry in entries:
            expected, name = entry.split("  ", 1)
            self.assertEqual(hashlib.sha256((root / "tests/fixtures/releases/v0.0.2" / name).read_bytes()).hexdigest(), expected, name)
            if name.endswith(".stdout"):
                self.assertEqual(hashlib.sha256((root / name).read_bytes()).hexdigest(), expected, name)


class ReleaseAssets(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.directory = Path(self.scratch.name)
        self.build()

    def build(self, *, omit=None, executable=True, version="0.0.2", working_tree="clean", record_version=None, commit=COMMIT, extra_files=None):
        for target, suffix in (
            ("x86_64-pc-windows-msvc", ".zip"),
            ("x86_64-unknown-linux-gnu", ".tar.gz"),
            ("aarch64-apple-darwin", ".tar.gz"),
        ):
            root = f"scarp-{version}-{target}"
            archive = self.directory / (root + suffix)
            files = {name: b"fixture" for name in (
                "scarp.exe" if suffix == ".zip" else "scarp", "LICENSE",
                "README.md", "CHANGELOG.md", "CONTRIBUTING.md",
                f"docs/releases/v{version}.md", "examples/bindings.srp",
                "examples/mutable.srp", "examples/scopes.srp", "examples/decisions.srp",
            ) if name != omit and not (version in ("0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4") and name.startswith("docs/releases/"))}
            if (version.startswith("0.0.3-") or version in ("0.0.4-dev", "0.0.4-alpha.1", "0.0.4-alpha.2", "0.0.4-alpha.3", "0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4")):
                for name in ("loops", "repeat", "control_escapes"):
                    for extension in ("srp", "stdout"):
                        member = f"examples/{name}.{extension}"
                        if member != omit:
                            files[member] = b"fixture"
            if version in ("0.0.3-beta.2", "0.0.3-beta.3", "0.0.4-dev", "0.0.4-alpha.1", "0.0.4-alpha.2", "0.0.4-alpha.3", "0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4"):
                for member in (
                    "examples/output.srp", "examples/output.stdout",
                    "docs/output.md", "docs/syntax-migration.md",
                ):
                    if member != omit:
                        files[member] = b"fixture"
            if version in ("0.0.3-beta.3", "0.0.4-dev", "0.0.4-alpha.1", "0.0.4-alpha.2", "0.0.4-alpha.3", "0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4"):
                for member in (
                    "docs/functions.md", "examples/functions.srp", "examples/functions.stdout",
                    "examples/function_contracts.srp", "examples/function_contracts.stdout",
                    "examples/function_recursion.srp", "examples/function_recursion.stdout",
                    "docs/budgets.md", "docs/reference/cli.md", "docs/embedding.md",
                ):
                    if member != omit:
                        files[member] = b"fixture"
            if version in ("0.0.4-alpha.1", "0.0.4-alpha.2", "0.0.4-alpha.3", "0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4"):
                for member in (
                    "docs/floats.md", "examples/float_refinement.srp", "examples/float_refinement.stdout",
                    "examples/float_smoothing.srp", "examples/float_smoothing.stdout",
                ):
                    if member != omit:
                        files[member] = b"fixture"
            if version in ("0.0.4-alpha.2", "0.0.4-alpha.3", "0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4"):
                for member in (
                    "docs/vectors.md", "examples/vectors.srp", "examples/vectors.stdout",
                    "examples/vector_probabilities.srp", "examples/vector_probabilities.stdout",
                    "examples/coefficient_fit.srp", "examples/coefficient_fit.stdout",
                ):
                    if member != omit:
                        files[member] = b"fixture"
            if version in ("0.0.4-alpha.3", "0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4"):
                for name in ("scaled_norm", "terminal_recursion", "vector_allocation"):
                    for extension in ("srp", "stdout"):
                        member = f"examples/{name}.{extension}"
                        if member != omit:
                            files[member] = b"fixture"
            if version in ("0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4"):
                for member in ("docs/tiles.md", "docs/memory-lifetimes.md", "docs/architecture.md", "docs/testing.md"):
                    if member != omit:
                        files[member] = b"fixture"
                for name in ("matrix_inference", "linear_transform", "matrix_classification", "matrix_probabilities",
                             "matrix_targets", "matrix_forward", "tile_windows", "vector_windows",
                             "classification_training", "logit_loss", "log_space", "score_selection"):
                    for extension in ("srp", "stdout"):
                        member = f"examples/{name}.{extension}"
                        if member != omit:
                            files[member] = b"fixture"
            if version in ("0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4"):
                for member in (
                    "docs/modules.md", "docs/tutorials/modules.md", "docs/proposals/0029-source-modules.md",
                    "examples/module_demo.srp", "examples/module_demo.stdout",
                    "examples/modules/linear.srp", "examples/modules/settings.srp",
                ):
                    if member != omit:
                        files[member] = b"fixture"
            if version in ("0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4"):
                for name in ("module_inspection", "constant_expressions", "typed_declarations", "scalar_conversions", "unicode_text", "inspection_json", "integer_collections", "bit_fields", "numerical_kernels", "collection_module"):
                    for extension in ("srp", "stdout"):
                        member = f"examples/{name}.{extension}"
                        if member != omit:
                            files[member] = b"fixture"
                for member in ("examples/inspection_json.modules.json", "examples/modules/integer_series.srp"):
                    if member != omit:
                        files[member] = b"fixture"
                if omit != "RELEASE-METADATA.json":
                    files["RELEASE-METADATA.json"] = json.dumps(dict(schema_version=1, version=version, documentation_repository="https://github.com/scarp-lang/scarp-docs")).encode()
            if version in ("0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4"):
                for member in ("examples/interned_types.rs", "examples/text_construction.srp", "examples/text_construction.stdout", "examples/diagnostics/type_error.srp", "examples/diagnostics/type_error.diagnostic.json"):
                    if member != omit:
                        files[member] = b"fixture"
            if version in ("0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4"):
                # Manifest-based projects and authored release notes are
                # required for the new candidate across all three archives.
                for member in ("docs/project-workflow.md", "docs/releases/v0.0.5-alpha.4.md"):
                    if member != omit:
                        files[member] = b"fixture"
            if version in ("0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4"):
                for member in (
                    "docs/releases/v0.0.5-alpha.5.md",
                    "docs/proposals/0035-native-scalar-c11.md",
                    "examples/native_scalar_repeat.srp",
                    "examples/native_scalar_repeat.stdout",
                ):
                    if member != omit:
                        files[member] = b"fixture"
            if version in ("0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4"):
                for member in (
                    "docs/releases/v0.0.6-alpha.1.md", "docs/symbol-index.md",
                    "docs/root-mean-square.md", "examples/symbol_index.rs",
                    "examples/symbol_navigation.srp", "examples/symbol_navigation.stdout",
                    "examples/modules/navigation_values.srp",
                    "examples/symbol_navigation.symbols.json", "examples/symbol_navigation.references.json",
                    "examples/root_mean_square.srp", "examples/root_mean_square.stdout",
                ):
                    if member != omit:
                        files[member] = b"fixture"
            if version in ("0.0.6-alpha.2", "0.0.6-alpha.3", "0.0.6-alpha.4"):
                for member in (
                    "docs/releases/v0.0.6-alpha.2.md",
                    "docs/releases/v0.0.6-alpha.2-publication.md",
                    "docs/proposals/0037-compiler-owned-native-ir.md",
                    "docs/development/incremental-invalidation.md",
                    "docs/development/semantic-cache.md",
                    "docs/development/token-cache.md",
                    "docs/development/syntax-cache.md",
                    "docs/development/cache-eviction.md",
                    "docs/development/cache-edit-planning.md",
                ):
                    if member != omit:
                        files[member] = b"fixture"
            if version in ("0.0.6-alpha.3", "0.0.6-alpha.4"):
                for member in (
                    "docs/releases/v0.0.6-alpha.3.md",
                    "docs/releases/v0.0.6-alpha.3-publication.md",
                    "docs/releases/v0.0.6-alpha.3-preparation.md",
                    "docs/topk-selection.md",
                    "docs/temperature-softmax.md",
                    "examples/topk_scores.srp",
                    "examples/topk_scores.stdout",
                    "examples/temperature_softmax.srp",
                    "examples/temperature_softmax.stdout",
                ):
                    if member != omit:
                        files[member] = b"fixture"
            if version == "0.0.6-alpha.4":
                for member in (
                    "docs/releases/v0.0.6-alpha.4.md",
                    "docs/releases/v0.0.6-alpha.4-publication.md",
                    "docs/releases/v0.0.6-alpha.4-preparation.md",
                    "docs/native-elf64.md", "docs/cosine-similarity.md",
                    "docs/sampled-calculus.md",
                    "examples/sampled_calculus.srp", "examples/sampled_calculus.stdout",
                    "examples/sample_cumulative_area.srp", "examples/sample_cumulative_area.stdout",
                    "examples/cosine_similarity.srp", "examples/cosine_similarity.stdout",
                ):
                    if member != omit:
                        files[member] = b"fixture"
            files.update(extra_files or {})
            files["BUILD-INFO.txt"] = (
                f"version={record_version or version}\ntarget={target}\ncommit={commit}\n"
                f"working_tree={working_tree}\ncompiler=rustc fixture\n"
            ).encode()
            if suffix == ".zip":
                with zipfile.ZipFile(archive, "w") as package:
                    for name, data in files.items():
                        package.writestr(root + "/" + name, data)
            else:
                with tarfile.open(archive, "w:gz") as package:
                    for name, data in files.items():
                        member = tarfile.TarInfo(root + "/" + name)
                        member.size = len(data)
                        member.mode = 0o755 if name == "scarp" and executable else 0o644
                        package.addfile(member, io.BytesIO(data))
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            archive.with_name(archive.name + ".sha256").write_text(
                f"{digest}  {archive.name}\n", encoding="utf-8"
            )

    def test_alpha6_4_requires_scientific_and_native_release_assets(self):
        version = "0.0.6-alpha.4"
        self.build(version=version)
        self.assertEqual(len(verifier.verify(self.directory, version, COMMIT).splitlines()), 3)
        for member in (
            "RELEASE-METADATA.json",
            "docs/releases/v0.0.6-alpha.4.md",
            "docs/releases/v0.0.6-alpha.4-publication.md",
            "docs/releases/v0.0.6-alpha.4-preparation.md",
            "docs/native-elf64.md", "docs/cosine-similarity.md",
            "docs/sampled-calculus.md",
            "examples/sampled_calculus.srp", "examples/sampled_calculus.stdout",
            "examples/sample_cumulative_area.srp", "examples/sample_cumulative_area.stdout",
            "examples/cosine_similarity.srp", "examples/cosine_similarity.stdout",
            "docs/releases/v0.0.6-alpha.3-publication.md",
            "examples/topk_scores.srp",
        ):
            with self.subTest(member=member):
                self.build(version=version, omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, version, COMMIT)

    def test_alpha6_3_requires_new_ai_guides_and_all_inherited_candidate_files(self):
        version = "0.0.6-alpha.3"
        self.build(version=version)
        self.assertEqual(len(verifier.verify(self.directory, version, COMMIT).splitlines()), 3)
        for member in (
            "RELEASE-METADATA.json",
            "docs/releases/v0.0.6-alpha.3.md",
            "docs/releases/v0.0.6-alpha.3-publication.md",
            "docs/releases/v0.0.6-alpha.3-preparation.md",
            "docs/topk-selection.md",
            "docs/temperature-softmax.md",
            "examples/topk_scores.srp",
            "examples/topk_scores.stdout",
            "examples/temperature_softmax.srp",
            "examples/temperature_softmax.stdout",
            "docs/releases/v0.0.6-alpha.2-publication.md",
            "docs/development/cache-edit-planning.md",
            "examples/root_mean_square.srp",
            "examples/native_scalar_repeat.srp",
            "docs/project-workflow.md",
        ):
            with self.subTest(member=member):
                self.build(version=version, omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, version, COMMIT)

    def test_alpha6_3_rejects_stale_metadata_with_valid_checksums(self):
        version = "0.0.6-alpha.3"
        stale = json.dumps(dict(
            schema_version=1, version="0.0.6-alpha.2",
            documentation_repository=verifier.DOCUMENTATION_REPOSITORY,
        )).encode()
        self.build(version=version, extra_files={"RELEASE-METADATA.json": stale})
        with self.assertRaisesRegex(ValueError, "invalid release metadata"):
            verifier.verify(self.directory, version, COMMIT)

    def test_alpha6_3_immutable_public_notes_and_source_files(self):
        version = "0.0.6-alpha.3"
        source, _, files = self.alpha2_source_fixture(version=version)
        candidate = f"docs/releases/v{version}.md"
        publication = f"docs/releases/v{version}-publication.md"
        ranking = "docs/topk-selection.md"
        example = "examples/topk_scores.srp"
        for name, data in (
            (candidate, b"# Internal source candidate\n"),
            (publication, (
                "# Scarp v0.0.6-alpha.3 — prerelease\n\n"
                "Approved public notes with Top-K and temperature softmax.\n"
            ).encode("utf-8")),
            (ranking, b"# Exact Top-K numerical contract\n"),
            (example, b"print(topk([1.0, 2.0], 1));\n"),
        ):
            path = source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            files[name] = data
        subprocess.run(
            ["git", "-C", str(source), "add", "--", candidate, publication, ranking, example],
            check=True,
        )
        subprocess.run([
            "git", "-C", str(source), "-c", "user.name=Release Fixture",
            "-c", "user.email=fixture@example.invalid", "commit", "--quiet",
            "-m", "approved alpha3 public notes",
        ], check=True)
        commit = subprocess.check_output(
            ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
        ).strip()
        self.build(version=version, commit=commit, extra_files=files)
        self.assertEqual(len(verifier.verify(self.directory, version, commit, source).splitlines()), 3)
        expected = verifier.release_notes_at(source, commit, version)
        self.assertIn("Approved public notes", expected)
        self.assertNotIn("Internal source candidate", expected)
        self.assertIn(commit, expected)

        (source / publication).write_text("# Moving checkout\n", encoding="utf-8")
        self.assertEqual(verifier.release_notes_at(source, commit, version), expected)
        files[publication] += b"tampered\n"
        self.build(version=version, commit=commit, extra_files=files)
        with self.assertRaisesRegex(ValueError, "source differs.*publication.md"):
            verifier.verify(self.directory, version, commit, source)
        files[publication] = (
            "# Scarp v0.0.6-alpha.3 — prerelease\n\n"
            "Approved public notes with Top-K and temperature softmax.\n"
        ).encode("utf-8")
        files[ranking] += b"tampered\n"
        self.build(version=version, commit=commit, extra_files=files)
        with self.assertRaisesRegex(ValueError, "source differs.*topk-selection.md"):
            verifier.verify(self.directory, version, commit, source)
        files[ranking] = b"# Exact Top-K numerical contract\n"
        files[example] += b"tampered\n"
        self.build(version=version, commit=commit, extra_files=files)
        with self.assertRaisesRegex(ValueError, "source differs.*topk_scores.srp"):
            verifier.verify(self.directory, version, commit, source)

    def test_alpha6_3_publisher_rejects_candidate_only_public_notes(self):
        source, _, _ = self.alpha2_source_fixture(version="0.0.6-alpha.3")
        publication = source / "docs/releases/v0.0.6-alpha.3-publication.md"
        publication.parent.mkdir(parents=True, exist_ok=True)
        for invalid in (
            "# Scarp v0.0.6-alpha.3 — release candidate\n",
            "# Scarp v0.0.6-alpha.3 — prerelease\n\n"
            "Status: source candidate under validation.\n",
        ):
            publication.write_text(invalid, encoding="utf-8")
            subprocess.run(["git", "-C", str(source), "add", "--", str(publication)], check=True)
            subprocess.run([
                "git", "-C", str(source), "-c", "user.name=Release Fixture",
                "-c", "user.email=fixture@example.invalid",
                "commit", "--quiet", "-m", "invalid candidate-only alpha3 notes",
            ], check=True)
            commit = subprocess.check_output(
                ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
            ).strip()
            with self.assertRaisesRegex(ValueError, "candidate-only status"):
                verifier.release_notes_at(source, commit, "0.0.6-alpha.3")

    def test_alpha6_2_requires_cache_docs_and_inherited_approved_features(self):
        version = "0.0.6-alpha.2"
        self.build(version=version)
        self.assertEqual(len(verifier.verify(self.directory, version, COMMIT).splitlines()), 3)
        for member in (
            "RELEASE-METADATA.json",
            "docs/releases/v0.0.6-alpha.2.md",
            "docs/releases/v0.0.6-alpha.2-publication.md",
            "docs/proposals/0037-compiler-owned-native-ir.md",
            "docs/development/incremental-invalidation.md",
            "docs/development/semantic-cache.md",
            "docs/development/token-cache.md",
            "docs/development/syntax-cache.md",
            "docs/development/cache-eviction.md",
            "docs/development/cache-edit-planning.md",
            "docs/releases/v0.0.6-alpha.1.md",
            "docs/symbol-index.md", "docs/root-mean-square.md",
            "examples/symbol_navigation.references.json",
            "examples/root_mean_square.srp",
            "examples/native_scalar_repeat.srp",
            "docs/project-workflow.md",
        ):
            with self.subTest(member=member):
                self.build(version=version, omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, version, COMMIT)

    def test_alpha6_2_rejects_old_metadata_despite_valid_archive_checksums(self):
        version = "0.0.6-alpha.2"
        stale = json.dumps(dict(
            schema_version=1, version="0.0.6-alpha.1",
            documentation_repository=verifier.DOCUMENTATION_REPOSITORY,
        )).encode()
        self.build(version=version, extra_files={"RELEASE-METADATA.json": stale})
        with self.assertRaisesRegex(ValueError, "invalid release metadata"):
            verifier.verify(self.directory, version, COMMIT)

    def test_alpha6_2_publication_notes_and_cache_docs_use_exact_source(self):
        version = "0.0.6-alpha.2"
        source, _, files = self.alpha2_source_fixture(version=version)
        candidate = f"docs/releases/v{version}.md"
        publication = f"docs/releases/v{version}-publication.md"
        syntax = "docs/development/syntax-cache.md"
        retention = "docs/development/cache-eviction.md"
        planning = "docs/development/cache-edit-planning.md"
        for name, data in (
            (candidate, b"# Internal release candidate, not a published GitHub release\n"),
            (publication, (
                "# Scarp v0.0.6-alpha.2 — prerelease\n\n"
                "Approved public release notes and cache features.\n"
            ).encode("utf-8")),
            (syntax, b"# Verified source-relative syntax spans\n"),
            (retention, b"# Approved scoped cache eviction contract\n"),
            (planning, b"# Approved transactional edit plan contract\n"),
        ):
            path = source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            files[name] = data
        subprocess.run(
            ["git", "-C", str(source), "add", "--", candidate, publication, syntax, retention, planning],
            check=True,
        )
        subprocess.run([
            "git", "-C", str(source), "-c", "user.name=Release Fixture",
            "-c", "user.email=fixture@example.invalid",
            "commit", "--quiet", "-m", "approved alpha2 public notes",
        ], check=True)
        commit = subprocess.check_output(
            ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
        ).strip()
        self.build(version=version, commit=commit, extra_files=files)
        self.assertEqual(len(verifier.verify(self.directory, version, commit, source).splitlines()), 3)
        expected = verifier.release_notes_at(source, commit, version)
        self.assertIn("Approved public release notes", expected)
        self.assertNotIn("Internal release candidate", expected)
        self.assertIn(commit, expected)

        # A moving checkout must not change the already-approved Git source.
        (source / publication).write_text("# Modified checkout notes\n", encoding="utf-8")
        self.assertEqual(verifier.release_notes_at(source, commit, version), expected)
        files[publication] += b"tampered\n"
        self.build(version=version, commit=commit, extra_files=files)
        with self.assertRaisesRegex(ValueError, "source differs.*publication.md"):
            verifier.verify(self.directory, version, commit, source)
        files[publication] = (
            "# Scarp v0.0.6-alpha.2 — prerelease\n\n"
            "Approved public release notes and cache features.\n"
        ).encode("utf-8")
        files[syntax] += b"tampered\n"
        self.build(version=version, commit=commit, extra_files=files)
        with self.assertRaisesRegex(ValueError, "source differs.*syntax-cache.md"):
            verifier.verify(self.directory, version, commit, source)
        files[syntax] = b"# Verified source-relative syntax spans\n"
        files[retention] += b"tampered\n"
        self.build(version=version, commit=commit, extra_files=files)
        with self.assertRaisesRegex(ValueError, "source differs.*cache-eviction.md"):
            verifier.verify(self.directory, version, commit, source)
        files[retention] = b"# Approved scoped cache eviction contract\n"
        files[planning] += b"tampered\n"
        self.build(version=version, commit=commit, extra_files=files)
        with self.assertRaisesRegex(ValueError, "source differs.*cache-edit-planning.md"):
            verifier.verify(self.directory, version, commit, source)

    def test_alpha6_2_publisher_rejects_candidate_only_public_notes(self):
        source, _, _ = self.alpha2_source_fixture(version="0.0.6-alpha.2")
        publication = source / "docs/releases/v0.0.6-alpha.2-publication.md"
        publication.parent.mkdir(parents=True, exist_ok=True)
        for text in (
            "# Scarp v0.0.6-alpha.2 — release candidate\n",
            "# Scarp v0.0.6-alpha.2 — prerelease\n\n"
            "Status: source candidate under validation.\n",
        ):
            publication.write_text(text, encoding="utf-8")
            subprocess.run(["git", "-C", str(source), "add", "--", str(publication)], check=True)
            subprocess.run([
                "git", "-C", str(source), "-c", "user.name=Release Fixture",
                "-c", "user.email=fixture@example.invalid", "commit", "--quiet",
                "-m", "invalid candidate-only publication notes",
            ], check=True)
            commit = subprocess.check_output(
                ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
            ).strip()
            with self.assertRaisesRegex(ValueError, "candidate-only status"):
                verifier.release_notes_at(source, commit, "0.0.6-alpha.2")

    def test_alpha6_requires_navigation_rms_and_all_prior_feature_contents(self):
        version = "0.0.6-alpha.1"
        self.build(version=version)
        self.assertEqual(len(verifier.verify(self.directory, version, COMMIT).splitlines()), 3)
        for member in (
            "RELEASE-METADATA.json", "docs/releases/v0.0.6-alpha.1.md",
            "docs/symbol-index.md", "docs/root-mean-square.md",
            "examples/symbol_index.rs", "examples/modules/navigation_values.srp",
            "examples/symbol_navigation.srp", "examples/symbol_navigation.stdout",
            "examples/symbol_navigation.symbols.json", "examples/symbol_navigation.references.json",
            "examples/root_mean_square.srp", "examples/root_mean_square.stdout",
            "examples/native_scalar_repeat.srp", "docs/project-workflow.md",
            "examples/integer_collections.srp", "examples/repeat.srp",
        ):
            with self.subTest(member=member):
                self.build(version=version, omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, version, COMMIT)

    def test_alpha6_rejects_prior_version_metadata_with_valid_checksums(self):
        stale = json.dumps(dict(schema_version=1, version="0.0.5-alpha.5",
                                documentation_repository=verifier.DOCUMENTATION_REPOSITORY)).encode()
        self.build(version="0.0.6-alpha.1", extra_files={"RELEASE-METADATA.json": stale})
        with self.assertRaisesRegex(ValueError, "invalid release metadata"):
            verifier.verify(self.directory, "0.0.6-alpha.1", COMMIT)

    def test_alpha6_notes_and_navigation_match_the_approved_source(self):
        version = "0.0.6-alpha.1"
        source, _, files = self.alpha2_source_fixture(version=version)
        notes = f"docs/releases/v{version}.md"
        navigation = "examples/symbol_navigation.references.json"
        for name, data in [(notes, b"# Approved v0.0.6-alpha.1 navigation and RMS\n"),
                           (navigation, b'{"schema_version":1,"kind":"scarp.references"}\n')]:
            path = source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            files[name] = data
        subprocess.run(["git", "-C", str(source), "add", "--", notes, navigation], check=True)
        subprocess.run(["git", "-C", str(source), "-c", "user.name=Release Fixture",
                        "-c", "user.email=fixture@example.invalid", "commit", "--quiet", "-m", "approved alpha6"], check=True)
        commit = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
        self.build(version=version, commit=commit, extra_files=files)
        self.assertEqual(len(verifier.verify(self.directory, version, commit, source).splitlines()), 3)
        expected = verifier.release_notes_at(source, commit, version)
        self.assertIn("navigation and RMS", expected)
        self.assertIn(commit, expected)
        (source / notes).write_text("# Unapproved checkout notes\n", encoding="utf-8")
        self.assertEqual(verifier.release_notes_at(source, commit, version), expected)
        files[navigation] += b"\n"
        self.build(version=version, commit=commit, extra_files=files)
        with self.assertRaisesRegex(ValueError, "source differs.*symbol_navigation.references.json"):
            verifier.verify(self.directory, version, commit, source)

    def test_alpha5_requires_native_scalar_example_and_prior_release_files(self):
        version = "0.0.5-alpha.5"
        self.build(version=version)
        self.assertEqual(len(verifier.verify(self.directory, version, COMMIT).splitlines()), 3)
        for member in (
            "RELEASE-METADATA.json",
            "docs/project-workflow.md",
            "docs/releases/v0.0.5-alpha.4.md",
            "docs/releases/v0.0.5-alpha.5.md",
            "docs/proposals/0035-native-scalar-c11.md",
            "examples/native_scalar_repeat.srp",
            "examples/native_scalar_repeat.stdout",
            "examples/repeat.srp",
            "examples/integer_collections.srp",
        ):
            with self.subTest(member=member):
                self.build(version=version, omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, version, COMMIT)

    def test_complete_packages_produce_sorted_manifest(self):
        manifest = verifier.verify(self.directory, "0.0.2", COMMIT)
        self.assertEqual(len(manifest.splitlines()), 3)
        self.assertEqual(manifest.splitlines(), sorted(manifest.splitlines()))

    def test_alpha2_uses_external_metadata_and_retains_inherited_content(self):
        version = "0.0.5-alpha.2"
        self.build(version=version)
        with zipfile.ZipFile(self.directory / f"scarp-{version}-x86_64-pc-windows-msvc.zip") as archive:
            self.assertFalse(any(name.endswith(f"docs/releases/v{version}.md") for name in archive.namelist()))
        self.assertEqual(len(verifier.verify(self.directory, version, COMMIT).splitlines()), 3)
        for member in ("RELEASE-METADATA.json", "examples/integer_collections.srp", "examples/bit_fields.stdout", "examples/numerical_kernels.srp", "examples/collection_module.stdout", "examples/modules/integer_series.srp", "examples/inspection_json.modules.json", "docs/reference/cli.md", "examples/repeat.srp"):
            with self.subTest(member=member):
                self.build(version=version, omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, version, COMMIT)

    def test_alpha4_requires_project_workflow_release_notes_and_inherited_files(self):
        version = "0.0.5-alpha.4"
        self.build(version=version)
        self.assertEqual(len(verifier.verify(self.directory, version, COMMIT).splitlines()), 3)
        for member in (
            "RELEASE-METADATA.json",
            "docs/project-workflow.md",
            "docs/releases/v0.0.5-alpha.4.md",
            "examples/interned_types.rs",
            "examples/text_construction.srp",
            "examples/integer_collections.srp",
            "examples/bit_fields.stdout",
            "examples/inspection_json.modules.json",
            "examples/repeat.srp",
        ):
            with self.subTest(member=member):
                self.build(version=version, omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, version, COMMIT)

    def test_alpha4_rejects_stale_release_metadata(self):
        incorrect = json.dumps(dict(
            schema_version=1, version="0.0.5-alpha.3",
            documentation_repository="https://github.com/scarp-lang/scarp-docs",
        )).encode()
        self.build(version="0.0.5-alpha.4", extra_files={"RELEASE-METADATA.json": incorrect})
        with self.assertRaisesRegex(ValueError, "invalid release metadata"):
            verifier.verify(self.directory, "0.0.5-alpha.4", COMMIT)

    def test_alpha3_requires_new_examples_metadata_and_inherited_content(self):
        version = "0.0.5-alpha.3"
        self.build(version=version)
        self.assertEqual(len(verifier.verify(self.directory, version, COMMIT).splitlines()), 3)
        for member in ("RELEASE-METADATA.json", "examples/interned_types.rs", "examples/text_construction.srp", "examples/text_construction.stdout", "examples/diagnostics/type_error.srp", "examples/diagnostics/type_error.diagnostic.json", "examples/integer_collections.srp", "examples/inspection_json.modules.json", "examples/repeat.srp", "docs/reference/cli.md"):
            with self.subTest(member=member):
                self.build(version=version, omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, version, COMMIT)

    def test_alpha3_rejects_alpha2_metadata_even_with_valid_archive_hashes(self):
        metadata = json.dumps(dict(schema_version=1, version="0.0.5-alpha.2", documentation_repository="https://github.com/scarp-lang/scarp-docs")).encode()
        self.build(version="0.0.5-alpha.3", extra_files={"RELEASE-METADATA.json": metadata})
        with self.assertRaisesRegex(ValueError, "invalid release metadata"):
            verifier.verify(self.directory, "0.0.5-alpha.3", COMMIT)

    def test_alpha2_damaged_metadata_is_rejected_even_with_valid_archive_hashes(self):
        for data in (b"{}", b"[]", b"invalid-json", b'{"schema_version":1,"version":"0.0.5-alpha.1","documentation_repository":"https://github.com/scarp-lang/scarp-docs"}'):
            with self.subTest(data=data):
                self.build(version="0.0.5-alpha.2", extra_files={"RELEASE-METADATA.json": data})
                with self.assertRaises(ValueError):
                    verifier.verify(self.directory, "0.0.5-alpha.2", COMMIT)

    def alpha2_source_fixture(self, version="0.0.5-alpha.2"):
        source, _, files = self.source_fixture()
        data = json.dumps(dict(schema_version=1, version=version, documentation_repository="https://github.com/scarp-lang/scarp-docs")).encode()
        path = source / ".github/release-metadata.json"
        path.parent.mkdir()
        path.write_bytes(data)
        subprocess.run(["git", "-C", str(source), "add", ".github/release-metadata.json"], check=True)
        subprocess.run(["git", "-C", str(source), "-c", "user.name=Release Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--quiet", "-m", "metadata"], check=True)
        commit = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
        files["RELEASE-METADATA.json"] = data
        return source, commit, files

    def test_alpha2_metadata_is_compared_to_the_approved_git_blob(self):
        source, commit, files = self.alpha2_source_fixture()
        self.build(version="0.0.5-alpha.2", commit=commit, extra_files=files)
        self.assertEqual(len(verifier.verify(self.directory, "0.0.5-alpha.2", commit, source).splitlines()), 3)
        files["RELEASE-METADATA.json"] += b"\n"
        self.build(version="0.0.5-alpha.2", commit=commit, extra_files=files)
        with self.assertRaisesRegex(ValueError, "source differs.*RELEASE-METADATA.json"):
            verifier.verify(self.directory, "0.0.5-alpha.2", commit, source)

    def test_alpha2_generated_body_reads_the_approved_commit_not_the_checkout(self):
        source, commit, _ = self.alpha2_source_fixture()
        expected = verifier.release_notes_at(source, commit, "0.0.5-alpha.2")
        (source / ".github/release-metadata.json").write_text("invalid checkout data", encoding="utf-8")
        self.assertEqual(verifier.release_notes_at(source, commit, "0.0.5-alpha.2"), expected)
        self.assertIn(commit, expected)
        self.assertIn("https://github.com/scarp-lang/scarp-docs", expected)

    def test_alpha4_release_notes_read_exact_source_commit(self):
        source, _, _ = self.alpha2_source_fixture(version="0.0.5-alpha.4")
        notes_path = source / "docs/releases/v0.0.5-alpha.4.md"
        notes_path.parent.mkdir(parents=True, exist_ok=True)
        notes_path.write_text("# Scarp v0.0.5-alpha.4\n\nApproved project workflow.\n", encoding="utf-8")
        subprocess.run(
            ["git", "-C", str(source), "add", "docs/releases/v0.0.5-alpha.4.md"],
            check=True,
        )
        subprocess.run(
            ["git", "-C", str(source), "-c", "user.name=Release Fixture",
             "-c", "user.email=fixture@example.invalid", "commit",
             "--quiet", "-m", "approved alpha4 notes"],
            check=True,
        )
        commit = subprocess.check_output(
            ["git", "-C", str(source), "rev-parse", "HEAD"], text=True,
        ).strip()
        expected = verifier.release_notes_at(source, commit, "0.0.5-alpha.4")
        self.assertIn("Approved project workflow.", expected)
        self.assertIn(commit, expected)
        self.assertIn("https://github.com/scarp-lang/scarp-docs", expected)
        # Changing the current checkout must never modify approved notes.
        notes_path.write_text("# malicious newer checkout\n", encoding="utf-8")
        self.assertEqual(
            verifier.release_notes_at(source, commit, "0.0.5-alpha.4"), expected
        )

    def test_alpha3_metadata_and_examples_match_the_approved_source(self):
        source, commit, files = self.alpha2_source_fixture(version="0.0.5-alpha.3")
        example = "examples/text_construction.srp"
        files[example] = b'print("approved source");\n'
        (source / example).parent.mkdir(parents=True, exist_ok=True)
        (source / example).write_bytes(files[example])
        subprocess.run(["git", "-C", str(source), "add", example], check=True)
        subprocess.run(["git", "-C", str(source), "-c", "user.name=Release Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--quiet", "-m", "example"], check=True)
        commit = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
        self.build(version="0.0.5-alpha.3", commit=commit, extra_files=files)
        self.assertEqual(len(verifier.verify(self.directory, "0.0.5-alpha.3", commit, source).splitlines()), 3)
        expected = verifier.release_notes_at(source, commit, "0.0.5-alpha.3")
        (source / ".github/release-metadata.json").write_text("invalid checkout data", encoding="utf-8")
        self.assertEqual(verifier.release_notes_at(source, commit, "0.0.5-alpha.3"), expected)
        for member in ("RELEASE-METADATA.json", example):
            with self.subTest(member=member):
                changed = dict(files)
                changed[member] += b"\n"
                self.build(version="0.0.5-alpha.3", commit=commit, extra_files=changed)
                with self.assertRaisesRegex(ValueError, "source differs.*" + member):
                    verifier.verify(self.directory, "0.0.5-alpha.3", commit, source)

    def test_alpha4_requires_new_and_inherited_package_content(self):
        self.build(version="0.0.4-alpha.4")
        self.assertEqual(len(verifier.verify(self.directory, "0.0.4-alpha.4", COMMIT).splitlines()), 3)
        for member in (
            "docs/tiles.md", "docs/memory-lifetimes.md", "docs/architecture.md", "docs/testing.md",
            "docs/vectors.md", "docs/floats.md", "docs/functions.md", "docs/budgets.md",
            "docs/releases/v0.0.4-alpha.4.md", "examples/repeat.srp", "examples/vector_allocation.stdout",
            "examples/matrix_inference.srp", "examples/matrix_inference.stdout",
            "examples/linear_transform.srp", "examples/linear_transform.stdout",
            "examples/matrix_classification.srp", "examples/matrix_classification.stdout",
            "examples/matrix_probabilities.srp", "examples/matrix_probabilities.stdout",
            "examples/matrix_targets.srp", "examples/matrix_targets.stdout",
            "examples/matrix_forward.srp", "examples/matrix_forward.stdout",
            "examples/tile_windows.srp", "examples/tile_windows.stdout",
            "examples/vector_windows.stdout", "examples/classification_training.srp",
        ):
            with self.subTest(member=member):
                self.build(version="0.0.4-alpha.4", omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, "0.0.4-alpha.4", COMMIT)

    def test_wrong_source_commit_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "unexpected commit"):
            verifier.verify(self.directory, "0.0.2", "b" * 40)

    def test_modules_candidate_requires_new_and_inherited_package_content(self):
        version = "0.0.5-alpha.1"
        self.build(version=version)
        self.assertEqual(len(verifier.verify(self.directory, version, COMMIT).splitlines()), 3)
        for member in (
            "docs/modules.md", "docs/tutorials/modules.md", "docs/proposals/0029-source-modules.md",
            "examples/module_demo.srp", "examples/module_demo.stdout",
            "examples/modules/linear.srp", "examples/modules/settings.srp",
            "docs/tiles.md", "docs/memory-lifetimes.md", "docs/architecture.md", "docs/testing.md",
            "docs/vectors.md", "docs/floats.md", "docs/functions.md", "docs/budgets.md",
            "docs/output.md", "docs/syntax-migration.md", "docs/reference/cli.md", "docs/embedding.md",
            "examples/repeat.srp", "examples/control_escapes.stdout", "examples/output.srp",
            "examples/function_contracts.srp", "examples/float_smoothing.stdout", "examples/coefficient_fit.srp",
            "examples/scaled_norm.srp", "examples/terminal_recursion.stdout", "examples/vector_allocation.stdout",
            "examples/matrix_inference.srp", "examples/matrix_forward.stdout", "examples/tile_windows.srp",
            "examples/vector_windows.stdout", "examples/logit_loss.srp", "examples/score_selection.stdout",
            "docs/releases/v0.0.5-alpha.1.md",
        ):
            with self.subTest(member=member):
                self.build(version=version, omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, version, COMMIT)

    def test_corrupt_archive_is_rejected_before_opening(self):
        next(self.directory.glob("*.zip")).write_bytes(b"damaged")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            verifier.verify(self.directory, "0.0.2", COMMIT)

    def test_missing_release_document_is_rejected(self):
        self.build(omit="docs/releases/v0.0.2.md")
        with self.assertRaisesRegex(ValueError, "missing docs/releases"):
            verifier.verify(self.directory, "0.0.2", COMMIT)

    def test_unix_executable_permissions_are_required(self):
        self.build(executable=False)
        with self.assertRaisesRegex(ValueError, "executable permission"):
            verifier.verify(self.directory, "0.0.2", COMMIT)

    def test_beta_packages_include_iteration_and_control_byte_fixtures(self):
        self.build(version="0.0.3-beta.1")
        self.assertEqual(len(verifier.verify(self.directory, "0.0.3-beta.1", COMMIT).splitlines()), 3)

    def test_beta_packages_cannot_omit_the_repeat_example(self):
        self.build(version="0.0.3-beta.1", omit="examples/repeat.srp")
        with self.assertRaisesRegex(ValueError, "missing examples/repeat"):
            verifier.verify(self.directory, "0.0.3-beta.1", COMMIT)

    def test_beta_packages_reject_old_version_records(self):
        self.build(version="0.0.3-beta.1", record_version="0.0.2")
        with self.assertRaisesRegex(ValueError, "unexpected version"):
            verifier.verify(self.directory, "0.0.3-beta.1", COMMIT)

    def test_modified_source_records_are_not_release_assets(self):
        self.build(working_tree="modified")
        with self.assertRaisesRegex(ValueError, "unexpected working_tree"):
            verifier.verify(self.directory, "0.0.2", COMMIT)

    def test_syntax_release_requires_migration_guides(self):
        for member in ("docs/output.md", "docs/syntax-migration.md"):
            with self.subTest(member=member):
                self.build(version="0.0.3-beta.2", omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, "0.0.3-beta.2", COMMIT)

    def test_syntax_release_requires_raw_output_example_and_snapshot(self):
        for member in ("examples/output.srp", "examples/output.stdout"):
            with self.subTest(member=member):
                self.build(version="0.0.3-beta.2", omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, "0.0.3-beta.2", COMMIT)

    def test_complete_syntax_release_keeps_sorted_checksums(self):
        self.build(version="0.0.3-beta.2")
        entries = verifier.verify(self.directory, "0.0.3-beta.2", COMMIT).splitlines()
        self.assertEqual(len(entries), 3)
        self.assertEqual(entries, sorted(entries))

    def test_function_candidate_requires_its_guide_and_all_example_pairs(self):
        for member in (
            "docs/functions.md", "examples/functions.srp", "examples/functions.stdout",
            "examples/function_contracts.srp", "examples/function_contracts.stdout",
            "examples/function_recursion.srp", "examples/function_recursion.stdout",
            "examples/repeat.srp", "docs/syntax-migration.md",
            "docs/budgets.md", "docs/reference/cli.md", "docs/embedding.md",
        ):
            with self.subTest(member=member):
                for version in ("0.0.3-beta.3", "0.0.4-dev"):
                    self.build(version=version, omit=member)
                    with self.assertRaisesRegex(ValueError, "missing " + member):
                        verifier.verify(self.directory, version, COMMIT)

    def test_complete_function_candidate_has_three_sorted_checksums(self):
        for version in ("0.0.3-beta.3", "0.0.4-dev"):
            self.build(version=version)
            entries = verifier.verify(self.directory, version, COMMIT).splitlines()
            self.assertEqual(len(entries), 3)
            self.assertEqual(entries, sorted(entries))

    def source_fixture(self):
        source = self.directory / "source"
        source.mkdir()
        files = {
            "README.md": b"fixture", "LICENSE": b"fixture",
            "docs/reference/cli.md": "exact tagged guide\nनमस्ते\n".encode(),
            "docs/extra.md": b"also required from the exact tree\n",
        }
        for name, data in files.items():
            path = source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        subprocess.run(["git", "init", "--quiet", str(source)], check=True)
        subprocess.run(["git", "-C", str(source), "add", "."], check=True)
        subprocess.run([
            "git", "-C", str(source), "-c", "user.name=Release Fixture",
            "-c", "user.email=fixture@example.invalid", "commit", "--quiet", "-m", "fixture",
        ], check=True)
        commit = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
        return source, commit, files

    def test_vector_candidate_requires_new_and_inherited_guides_and_fixtures(self):
        for member in (
            "docs/vectors.md", "examples/vectors.srp", "examples/vectors.stdout",
            "examples/vector_probabilities.srp", "examples/vector_probabilities.stdout",
            "examples/coefficient_fit.srp", "examples/coefficient_fit.stdout",
            "docs/floats.md", "examples/float_refinement.srp", "docs/functions.md",
            "docs/budgets.md", "docs/embedding.md", "docs/syntax-migration.md",
            "examples/repeat.srp", "examples/output.stdout", "docs/releases/v0.0.4-alpha.2.md",
        ):
            with self.subTest(member=member):
                self.build(version="0.0.4-alpha.2", omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, "0.0.4-alpha.2", COMMIT)

    def test_complete_vector_candidate_and_stale_version_record(self):
        self.build(version="0.0.4-alpha.2")
        manifest = verifier.verify(self.directory, "0.0.4-alpha.2", COMMIT)
        self.assertEqual(len(manifest.splitlines()), 3)
        self.assertEqual(manifest.splitlines(), sorted(manifest.splitlines()))
        self.build(version="0.0.4-alpha.2", record_version="0.0.4-alpha.1")
        with self.assertRaisesRegex(ValueError, "unexpected version"):
            verifier.verify(self.directory, "0.0.4-alpha.2", COMMIT)

    def test_float_alpha_requires_numeric_and_inherited_function_contents(self):
        for member in (
            "docs/floats.md", "examples/float_refinement.srp", "examples/float_refinement.stdout",
            "examples/float_smoothing.srp", "examples/float_smoothing.stdout",
            "docs/functions.md", "docs/budgets.md", "docs/reference/cli.md", "docs/embedding.md",
            "docs/syntax-migration.md", "examples/repeat.srp", "examples/output.stdout",
            "examples/function_contracts.srp", "docs/releases/v0.0.4-alpha.1.md",
        ):
            with self.subTest(member=member):
                self.build(version="0.0.4-alpha.1", omit=member)
                with self.assertRaisesRegex(ValueError, "missing " + member):
                    verifier.verify(self.directory, "0.0.4-alpha.1", COMMIT)

    def test_complete_float_alpha_matches_exact_source_and_sorted_checksums(self):
        source, commit, files = self.source_fixture()
        files["docs/floats.md"] = b"finite binary64 guide\n"
        path = source / "docs/floats.md"
        path.write_bytes(files["docs/floats.md"])
        subprocess.run(["git", "-C", str(source), "add", "docs/floats.md"], check=True)
        subprocess.run([
            "git", "-C", str(source), "-c", "user.name=Release Fixture",
            "-c", "user.email=fixture@example.invalid", "commit", "--quiet", "-m", "float guide",
        ], check=True)
        commit = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
        self.build(version="0.0.4-alpha.1", commit=commit, extra_files=files)
        entries = verifier.verify(self.directory, "0.0.4-alpha.1", commit, source).splitlines()
        self.assertEqual(len(entries), 3)
        self.assertEqual(entries, sorted(entries))
        files["docs/floats.md"] = b"stale float guide with a valid archive checksum"
        self.build(version="0.0.4-alpha.1", commit=commit, extra_files=files)
        with self.assertRaisesRegex(ValueError, "source differs.*docs/floats.md"):
            verifier.verify(self.directory, "0.0.4-alpha.1", commit, source)

    def test_packages_match_exact_git_source_in_all_three_archives(self):
        source, commit, files = self.source_fixture()
        self.build(version="0.0.3-beta.3", commit=commit, extra_files=files)
        # A modified checkout must not replace the immutable tagged content.
        (source / "README.md").write_text("uncommitted change")
        manifest = verifier.verify(self.directory, "0.0.3-beta.3", commit, source)
        self.assertEqual(len(manifest.splitlines()), 3)

    def test_valid_hashes_do_not_hide_stale_packaged_documentation(self):
        source, commit, files = self.source_fixture()
        files["docs/reference/cli.md"] = b"stale guide with valid archive checksum"
        self.build(version="0.0.3-beta.3", commit=commit, extra_files=files)
        with self.assertRaisesRegex(ValueError, "source differs.*docs/reference/cli.md"):
            verifier.verify(self.directory, "0.0.3-beta.3", commit, source)

    def test_every_tagged_guide_is_required_even_outside_the_minimum_file_list(self):
        source, commit, files = self.source_fixture()
        del files["docs/extra.md"]
        self.build(version="0.0.3-beta.3", commit=commit, extra_files=files)
        with self.assertRaises(KeyError):
            verifier.verify(self.directory, "0.0.3-beta.3", commit, source)


if __name__ == "__main__":
    unittest.main()
