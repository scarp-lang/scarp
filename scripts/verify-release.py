"""Verify native release assets against an exact version and source commit."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tarfile
import zipfile

EXTERNAL_DOCS_VERSIONS = {"0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3"}
DOCUMENTATION_REPOSITORY = "https://github.com/scarp-lang/scarp-docs"
METADATA_SOURCE = ".github/release-metadata.json"
METADATA_MEMBER = "RELEASE-METADATA.json"


def validate_release_metadata(data, version):
    metadata = json.loads(data)
    if (not isinstance(metadata, dict)
            or set(metadata) != {"schema_version", "version", "documentation_repository"}
            or type(metadata["schema_version"]) is not int or metadata["schema_version"] != 1
            or metadata["version"] != version
            or metadata["documentation_repository"] != DOCUMENTATION_REPOSITORY):
        raise ValueError("invalid release metadata schema, version or documentation repository")
    return metadata


def release_notes_at(source_directory, commit, version):
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("expected the full release commit")
    path = METADATA_SOURCE if version in EXTERNAL_DOCS_VERSIONS else f"docs/releases/v{version}.md"
    data = subprocess.check_output(["git", "-C", str(source_directory), "show", f"{commit}:{path}"], text=True, encoding="utf-8")
    if version not in EXTERNAL_DOCS_VERSIONS:
        return data
    metadata = validate_release_metadata(data, version)
    if version in ("0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3"):
        # Read the reviewed notes from the exact approved source commit. Unlike
        # earlier metadata-only releases, alpha.4 carries a real feature/limit
        # inventory and must not render notes from a moving checkout.
        # Candidate guides deliberately remain unpublished. The publisher
        # reads separately reviewed public-facing notes from the SAME
        # immutable source commit, never from a moving checkout.
        notes_path = (
            f"docs/releases/v{version}-publication.md"
            if version in ("0.0.6-alpha.2", "0.0.6-alpha.3")
            else f"docs/releases/v{version}.md"
        )
        notes = subprocess.check_output(
            ["git", "-C", str(source_directory), "show", f"{commit}:{notes_path}"],
            text=True, encoding="utf-8",
        ).rstrip()
        if version in ("0.0.6-alpha.2", "0.0.6-alpha.3"):
            if (
                not notes.startswith(f"# Scarp v{version} — prerelease\n")
                or any(phrase in notes.lower() for phrase in (
                    "not a published github release",
                    "source candidate under validation",
                    "do not authorize a tag",
                    "status: preparation only",
                ))
            ):
                raise ValueError("publication notes contain candidate-only status")
        return (f"{notes}\n\nSource commit: `{commit}`\n\n"
                f"[Scarp documentation]({metadata['documentation_repository']})\n")
    return (f"Scarp v{version}\n\nSource commit: `{commit}`\n\n"
            f"[Scarp documentation]({metadata['documentation_repository']})\n")


def verify_source_members(read_member, root, source_files):
    for name, expected in source_files.items():
        data = read_member(root + "/" + name)
        digest = hashlib.sha1(f"blob {len(data)}\0".encode() + data).hexdigest()
        if digest != expected:
            raise ValueError(f"package source differs from release commit: {name}")


def source_files_at(source_directory, commit):
    result = subprocess.run(
        ["git", "-C", str(source_directory), "ls-tree", "-rz", commit, "--",
         "LICENSE", "README.md", "CHANGELOG.md", "CONTRIBUTING.md", "docs", "examples", METADATA_SOURCE],
        check=True, stdout=subprocess.PIPE,
    )
    files = {}
    for record in result.stdout.split(b"\0"):
        if record:
            metadata, name = record.decode("utf-8").split("\t", 1)
            mode, kind, digest = metadata.split()
            if kind != "blob" or mode not in ("100644", "100755"):
                raise ValueError(f"unsupported packaged source entry: {name}")
            files[METADATA_MEMBER if name == METADATA_SOURCE else name] = digest
    if "README.md" not in files or "LICENSE" not in files:
        raise ValueError("release source tree is missing root documentation")
    return files


def verify(directory, version, commit, source_directory=None):
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
        raise ValueError("expected a three-part package version")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("expected the full release commit")
    source_files = source_files_at(source_directory, commit) if source_directory is not None else {}
    checksums = []
    for target, extension in (
        ("x86_64-pc-windows-msvc", ".zip"),
        ("x86_64-unknown-linux-gnu", ".tar.gz"),
        ("aarch64-apple-darwin", ".tar.gz"),
    ):
        root = f"scarp-{version}-{target}"
        name = root + extension
        matches = list(directory.rglob(name))
        if len(matches) != 1:
            raise ValueError(f"expected exactly one {name}")
        archive = matches[0]
        with archive.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        checksum = f"{digest}  {name}\n"
        if archive.with_name(name + ".sha256").read_text().strip() != checksum.strip():
            raise ValueError(f"checksum mismatch: {name}")
        metadata = None
        if extension == ".zip":
            with zipfile.ZipFile(archive) as package:
                paths = set(package.namelist())
                record = package.read(root + "/BUILD-INFO.txt")
                verify_source_members(package.read, root, source_files)
                if root + "/" + METADATA_MEMBER in paths:
                    metadata = package.read(root + "/" + METADATA_MEMBER)
        else:
            with tarfile.open(archive, "r:gz") as package:
                paths = set(package.getnames())
                stream = package.extractfile(root + "/BUILD-INFO.txt")
                if stream is None:
                    raise ValueError(f"missing build record: {name}")
                record = stream.read()
                if not package.getmember(root + "/scarp").mode & 0o111:
                    raise ValueError(f"missing executable permission: {name}")
                def read_member(name):
                    member = package.getmember(name)
                    if not member.isfile():
                        raise ValueError(f"packaged source is not a regular file: {name}")
                    return package.extractfile(member).read()
                verify_source_members(read_member, root, source_files)
                if root + "/" + METADATA_MEMBER in paths:
                    metadata = read_member(root + "/" + METADATA_MEMBER)
        if version in EXTERNAL_DOCS_VERSIONS:
            if metadata is None:
                raise ValueError(f"{name}: missing {METADATA_MEMBER}")
            validate_release_metadata(metadata, version)
        fields = dict(line.split("=", 1) for line in record.decode("utf-8").splitlines())
        for key, expected in (
            ("version", version), ("target", target),
            ("commit", commit), ("working_tree", "clean"),
        ):
            if fields.get(key) != expected:
                raise ValueError(f"{name}: unexpected {key}: {fields.get(key)}")
        if not fields.get("compiler", "").startswith("rustc "):
            raise ValueError(f"missing compiler record: {name}")
        executable = "scarp.exe" if extension == ".zip" else "scarp"
        iteration_examples = (
            "examples/loops.srp", "examples/loops.stdout",
            "examples/repeat.srp", "examples/repeat.stdout",
            "examples/control_escapes.srp", "examples/control_escapes.stdout",
        ) if (version.startswith("0.0.3-") or version in ("0.0.4-dev", "0.0.4-alpha.1", "0.0.4-alpha.2", "0.0.4-alpha.3", "0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3")) else ()
        syntax_migration_files = (
            "examples/output.srp", "examples/output.stdout",
            "docs/output.md", "docs/syntax-migration.md",
        ) if version in ("0.0.3-beta.2", "0.0.3-beta.3", "0.0.4-dev", "0.0.4-alpha.1", "0.0.4-alpha.2", "0.0.4-alpha.3", "0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3") else ()
        function_files = (
            "docs/functions.md", "examples/functions.srp", "examples/functions.stdout",
            "examples/function_contracts.srp", "examples/function_contracts.stdout",
            "examples/function_recursion.srp", "examples/function_recursion.stdout",
            "docs/budgets.md", "docs/reference/cli.md", "docs/embedding.md",
        ) if version in ("0.0.3-beta.3", "0.0.4-dev", "0.0.4-alpha.1", "0.0.4-alpha.2", "0.0.4-alpha.3", "0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3") else ()
        float_files = (
            "docs/floats.md", "examples/float_refinement.srp", "examples/float_refinement.stdout",
            "examples/float_smoothing.srp", "examples/float_smoothing.stdout",
        ) if version in ("0.0.4-alpha.1", "0.0.4-alpha.2", "0.0.4-alpha.3", "0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3") else ()
        vector_files = (
            "docs/vectors.md", "examples/vectors.srp", "examples/vectors.stdout",
            "examples/vector_probabilities.srp", "examples/vector_probabilities.stdout",
            "examples/coefficient_fit.srp", "examples/coefficient_fit.stdout",
        ) if version in ("0.0.4-alpha.2", "0.0.4-alpha.3", "0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3") else ()
        alpha3_files = (
            "examples/scaled_norm.srp", "examples/scaled_norm.stdout",
            "examples/terminal_recursion.srp", "examples/terminal_recursion.stdout",
            "examples/vector_allocation.srp", "examples/vector_allocation.stdout",
        ) if version in ("0.0.4-alpha.3", "0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3") else ()
        alpha4_files = (
            "docs/tiles.md", "docs/memory-lifetimes.md",
            "docs/architecture.md", "docs/testing.md",
            *(f"examples/{name}.{extension}" for name in (
                "matrix_inference", "linear_transform", "matrix_classification",
                "matrix_probabilities", "matrix_targets", "matrix_forward", "tile_windows",
                "vector_windows", "classification_training", "logit_loss", "log_space", "score_selection",
            ) for extension in ("srp", "stdout")),
        ) if version in ("0.0.4-alpha.4", "0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3") else ()
        module_files = (
            "docs/modules.md", "docs/tutorials/modules.md", "docs/proposals/0029-source-modules.md",
            "examples/module_demo.srp", "examples/module_demo.stdout",
            "examples/modules/linear.srp", "examples/modules/settings.srp",
        ) if version in ("0.0.5-alpha.1", "0.0.5-alpha.2", "0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3") else ()
        alpha2_files = (
            *(f"examples/{example}.{extension}" for example in (
                "module_inspection", "constant_expressions", "typed_declarations",
                "scalar_conversions", "unicode_text", "inspection_json",
                "integer_collections", "bit_fields", "numerical_kernels", "collection_module",
            ) for extension in ("srp", "stdout")),
            "examples/inspection_json.modules.json", "examples/modules/integer_series.srp",
        ) if version in EXTERNAL_DOCS_VERSIONS else ()
        type_tooling_files = (
            "examples/interned_types.rs", "examples/text_construction.srp",
            "examples/text_construction.stdout", "examples/diagnostics/type_error.srp",
            "examples/diagnostics/type_error.diagnostic.json",
        ) if version in ("0.0.5-alpha.3", "0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3") else ()
        project_files = ("docs/project-workflow.md", "docs/releases/v0.0.5-alpha.4.md") if version in ("0.0.5-alpha.4", "0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3") else ()
        alpha5_files = (
            "examples/native_scalar_repeat.srp", "examples/native_scalar_repeat.stdout",
            "docs/proposals/0035-native-scalar-c11.md",
            "docs/releases/v0.0.5-alpha.5.md",
        ) if version in ("0.0.5-alpha.5", "0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3") else ()
        alpha6_files = (
            "docs/releases/v0.0.6-alpha.1.md", "docs/symbol-index.md",
            "docs/root-mean-square.md",
            "examples/symbol_index.rs", "examples/symbol_navigation.srp",
            "examples/symbol_navigation.stdout", "examples/modules/navigation_values.srp",
            "examples/symbol_navigation.symbols.json", "examples/symbol_navigation.references.json",
            "examples/root_mean_square.srp", "examples/root_mean_square.stdout",
        ) if version in ("0.0.6-alpha.1", "0.0.6-alpha.2", "0.0.6-alpha.3") else ()
        alpha6_2_files = (
            "docs/releases/v0.0.6-alpha.2.md",
            "docs/releases/v0.0.6-alpha.2-publication.md",
            "docs/proposals/0037-compiler-owned-native-ir.md",
            "docs/development/incremental-invalidation.md",
            "docs/development/semantic-cache.md",
            "docs/development/token-cache.md",
            "docs/development/syntax-cache.md",
            "docs/development/cache-eviction.md",
            "docs/development/cache-edit-planning.md",
        ) if version in ("0.0.6-alpha.2", "0.0.6-alpha.3") else ()
        alpha6_3_files = (
            "docs/releases/v0.0.6-alpha.3.md",
            "docs/releases/v0.0.6-alpha.3-publication.md",
            "docs/releases/v0.0.6-alpha.3-preparation.md",
            "docs/topk-selection.md",
            "docs/temperature-softmax.md",
            "examples/topk_scores.srp",
            "examples/topk_scores.stdout",
            "examples/temperature_softmax.srp",
            "examples/temperature_softmax.stdout",
        ) if version == "0.0.6-alpha.3" else ()
        release_files = (METADATA_MEMBER,) if version in EXTERNAL_DOCS_VERSIONS else (f"docs/releases/v{version}.md",)
        for member in (
            executable, "LICENSE", "README.md", "CHANGELOG.md", "CONTRIBUTING.md",
            *release_files, "examples/bindings.srp",
            "examples/mutable.srp", "examples/scopes.srp", "examples/decisions.srp",
            *iteration_examples, *syntax_migration_files, *function_files, *float_files, *vector_files,
            *alpha3_files, *alpha4_files, *module_files, *alpha2_files, *type_tooling_files,
            *project_files, *alpha5_files, *alpha6_files, *alpha6_2_files, *alpha6_3_files,
        ):
            if root + "/" + member not in paths:
                raise ValueError(f"{name}: missing {member}")
        checksums.append(checksum)
        print(f"Verified {name}: {digest}")
    return "".join(sorted(checksums))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--write-manifest", action="store_true")
    parser.add_argument("--source-directory", type=Path, help="compare packaged docs/examples with exact Git blobs")
    args = parser.parse_args()
    try:
        manifest = verify(args.directory, args.version, args.commit, args.source_directory)
        path = args.directory / "SHA256SUMS"
        if args.write_manifest:
            with path.open("x", encoding="utf-8", newline="\n") as output:
                output.write(manifest)
        elif path.read_text(encoding="utf-8") != manifest:
            raise ValueError("SHA256SUMS differs from the verified archives")
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError, tarfile.TarError, zipfile.BadZipFile) as error:
        parser.exit(1, f"release verification failed: {error}\n")


if __name__ == "__main__":
    main()
