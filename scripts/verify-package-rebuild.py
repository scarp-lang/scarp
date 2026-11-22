"""Verify the extracted payloads of two independent clean Scarp package builds.

The ZIP/tar container metadata need not be byte-identical; every file payload,
including the compiler binary and BUILD-INFO.txt, must be byte-identical.
Also verify each archive's companion SHA-256 checksum file.
"""

import argparse
import hashlib
from pathlib import Path
import tarfile
import zipfile

TARGETS = {
    "x86_64-pc-windows-msvc": ".zip",
    "x86_64-unknown-linux-gnu": ".tar.gz",
    "aarch64-apple-darwin": ".tar.gz",
}


def digest(stream):
    result = hashlib.sha256()
    while chunk := stream.read(1024 * 1024):
        result.update(chunk)
    return result.hexdigest()


def validate_name(name):
    stripped = name.rstrip("/")
    if (not stripped or name.startswith("/") or "\\" in name
            or any(segment in ("", ".", "..") for segment in stripped.split("/"))):
        raise ValueError(f"unsafe archive member: {name!r}")
    return stripped


def contents(archive):
    result = {}
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as zipped:
            for member in zipped.infolist():
                name = validate_name(member.filename)
                if member.is_dir():
                    continue
                if name in result:
                    raise ValueError(f"duplicate archive member: {name}")
                mode = member.external_attr >> 16
                if mode & 0o170000 == 0o120000:
                    raise ValueError(f"symbolic link is not an ordinary file: {name}")
                with zipped.open(member) as stream:
                    result[name] = (member.file_size, digest(stream))
    elif archive.name.endswith(".tar.gz"):
        with tarfile.open(archive, "r:gz") as tarred:
            for member in tarred.getmembers():
                name = validate_name(member.name)
                if member.isdir():
                    continue
                if not member.isfile():
                    raise ValueError(f"non-regular archive member: {name}")
                if name in result:
                    raise ValueError(f"duplicate archive member: {name}")
                stream = tarred.extractfile(member)
                if stream is None:
                    raise ValueError(f"missing archive member data: {name}")
                with stream:
                    result[name] = (member.size, digest(stream))
    else:
        raise ValueError(f"unsupported archive: {archive}")
    if not result:
        raise ValueError(f"empty archive: {archive}")
    return result


def verify_checksum(archive):
    with archive.open("rb") as stream:
        actual = digest(stream)
    sidecar = archive.with_name(archive.name + ".sha256")
    expected = f"{actual}  {archive.name}\n"
    if not sidecar.is_file() or sidecar.read_text(encoding="utf-8") != expected:
        raise ValueError(f"invalid SHA-256 sidecar: {sidecar}")
    return actual


def find_archive(directory, target):
    extension = TARGETS[target]
    matches = list(directory.glob(f"scarp-*-{target}{extension}"))
    if len(matches) != 1:
        raise ValueError(f"expected one package for {target} under {directory}, found {len(matches)}")
    return matches[0]


def compare(original_dir, rebuilt_dir, target):
    original = find_archive(original_dir, target)
    rebuilt = find_archive(rebuilt_dir, target)
    if original.name != rebuilt.name:
        raise ValueError("rebuilt package name/version differs")
    original_hash = verify_checksum(original)
    rebuilt_hash = verify_checksum(rebuilt)
    first = contents(original)
    second = contents(rebuilt)
    if first != second:
        missing = sorted(first.keys() - second.keys())
        added = sorted(second.keys() - first.keys())
        changed = sorted(name for name in first.keys() & second.keys() if first[name] != second[name])
        raise ValueError(f"payload is not reproducible: missing={missing[:5]}, added={added[:5]}, changed={changed[:5]}")
    return len(first), original_hash == rebuilt_hash


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("original_dir", type=Path)
    parser.add_argument("rebuilt_dir", type=Path)
    parser.add_argument("target", choices=sorted(TARGETS))
    args = parser.parse_args()
    count, archive_identical = compare(args.original_dir, args.rebuilt_dir, args.target)
    print(f"Reproducible package payload: {count} files, target={args.target}")
    print(f"Byte-identical archives (including metadata): {archive_identical}")


if __name__ == "__main__":
    main()
