"""Create reproducible ZIP and tar.gz Scarp release archives from staged bytes.

Normalize path order, timestamps, owner/group and file permissions. Python's
deterministic serializers preserve exact archive hashes on clean rebuilds.
"""

import argparse
import gzip
from pathlib import Path
import stat
import tarfile
import zipfile

ZIP_DATE = (1980, 1, 1, 0, 0, 0)


def entries(root):
    if root.is_symlink():
        raise ValueError("package root must not be a symbolic link")
    root = root.resolve(strict=True)
    if not root.is_dir():
        raise ValueError("package root must be a directory")
    paths = [root, *root.rglob("*")]
    paths.sort(key=lambda path: path.relative_to(root).as_posix())
    result = []
    for path in paths:
        if path.is_symlink() or not (path.is_dir() or path.is_file()):
            raise ValueError(f"unsupported package entry: {path}")
        name = root.name if path == root else f"{root.name}/{path.relative_to(root).as_posix()}"
        if path.is_dir():
            name += "/"
        result.append((path, name))
    return result


def archive_zip(paths, output):
    with zipfile.ZipFile(output, mode="x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as result:
        for path, name in paths:
            directory = path.is_dir()
            info = zipfile.ZipInfo(name, ZIP_DATE)
            info.create_system = 3
            permissions = (stat.S_IFDIR | 0o755) if directory else (
                stat.S_IFREG | (0o755 if path.name == "scarp.exe" else 0o644)
            )
            info.external_attr = permissions << 16
            if directory:
                info.external_attr |= 0x10
                result.writestr(info, b"", compress_type=zipfile.ZIP_STORED)
            else:
                result.writestr(info, path.read_bytes(), compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)


def archive_tar(paths, output):
    with output.open("xb") as destination:
        with gzip.GzipFile(fileobj=destination, filename="", mode="wb", mtime=0, compresslevel=9) as compressed:
            with tarfile.open(fileobj=compressed, mode="w|", format=tarfile.PAX_FORMAT) as result:
                for path, name in paths:
                    directory = path.is_dir()
                    info = tarfile.TarInfo(name)
                    info.uid = 0
                    info.gid = 0
                    info.uname = ""
                    info.gname = ""
                    info.mtime = 0
                    info.mode = 0o755 if directory or path.name == "scarp" else 0o644
                    if directory:
                        info.type = tarfile.DIRTYPE
                        result.addfile(info)
                    else:
                        info.size = path.stat().st_size
                        with path.open("rb") as source:
                            result.addfile(info, source)


def build(root, archive):
    paths = entries(root)
    if archive.name.endswith(".zip"):
        archive_zip(paths, archive)
    elif archive.name.endswith(".tar.gz"):
        archive_tar(paths, archive)
    else:
        raise ValueError(f"unsupported release archive extension: {archive}")
    return len(paths)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package_directory", type=Path)
    parser.add_argument("output_archive", type=Path)
    args = parser.parse_args()
    count = build(args.package_directory, args.output_archive)
    print(f"Deterministic Scarp archive: {args.output_archive} ({count} entries)")


if __name__ == "__main__":
    main()
