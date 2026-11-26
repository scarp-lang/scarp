#!/usr/bin/env python3
"""Run one opt-in streamed BigInt workload and verify every output byte."""

import argparse
import hashlib
import json
import os
import platform
import random
import shutil
import subprocess
import tempfile
import time
from pathlib import Path


REPO = Path(__file__).resolve().parents[2]
SEED = 0x5CA4_2026
TILE_BYTES = 1024 * 1024
FACTOR_DIGITS = b"-999999999"
DIVISION_REMAINDER = 42_424_242


def tiles(pattern, count, final_increment=0):
    while count > len(pattern):
        yield pattern
        count -= len(pattern)
    if count:
        final = pattern[:count]
        if final_increment:
            final = final[:-1] + bytes([final[-1] + final_increment])
        yield final


def write_operand(path, prefix, pattern, count):
    with path.open("wb") as stream:
        stream.write(prefix)
        for block in tiles(pattern, count):
            stream.write(block)


def product_stream(sample, digits, factor, addend):
    full_chunks, leading = divmod(digits, len(sample))
    head = sample[:leading]
    copies = TILE_BYTES // len(sample)

    # Compute the product with Python integers in 1,024-digit chunks.
    # For this repeated fixture, verify that incoming carry cannot change
    # outgoing carry. Each interior chunk then has the same expected bytes.
    radix = 10 ** len(sample)
    carry, tail = divmod(int(sample) * factor, radix)
    if not 0 <= addend < factor or tail + factor - 1 >= radix:
        raise ValueError("fixture has an ambiguous carry")
    top = int(head or b"0") * factor + (carry if full_chunks else addend)

    def blocks():
        if full_chunks:
            middle = f"{tail + carry:0{len(sample)}d}".encode()
            yield from tiles(middle * copies, (full_chunks - 1) * len(sample))
            yield f"{tail + addend:0{len(sample)}d}".encode()

    return str(top).encode(), blocks()


def prepare_product_or_division(case, left, right, digits, rng):
    sample = rng.randbytes(1024).translate(bytes(b"12345678"[i % 8] for i in range(256)))
    factor = abs(int(FACTOR_DIGITS))
    pattern = sample * (TILE_BYTES // len(sample))
    if case == "mul":
        full, leading = divmod(digits, len(sample))
        write_operand(left, sample[:leading], pattern, full * len(sample))
        right.write_bytes(FACTOR_DIGITS)
        prefix, blocks = product_stream(sample, digits, factor, 0)
        return left, right, "mul", b"-" + prefix, blocks

    # Construct -(quotient * divisor + remainder) independently, with a
    # known dense quotient that never has to be stored in the oracle's RAM.
    quotient_digits = digits - (len(FACTOR_DIGITS) - 1)
    prefix, blocks = product_stream(sample, quotient_digits, factor, DIVISION_REMAINDER)
    with left.open("wb") as stream:
        stream.write(b"-" + prefix)
        for block in blocks:
            stream.write(block)
    right.write_bytes(FACTOR_DIGITS[1:])
    full, leading = divmod(quotient_digits, len(sample))
    return left, right, "div", b"-" + sample[:leading], tiles(pattern, full * len(sample))


def prepare(case, directory, digits):
    rng = random.Random(SEED)
    left = directory / "left"
    right = directory / "right"
    if case in {"mul", "div"}:
        return prepare_product_or_division(case, left, right, digits, rng)
    alphabet = b"1234" if case == "double" else b"12345678"
    sample = rng.randbytes(TILE_BYTES).translate(
        bytes(alphabet[index % len(alphabet)] for index in range(256))
    )
    complement = bytes.maketrans(b"12345678", b"87654321")
    if case == "double":
        write_operand(left, b"", sample, digits)
        doubled = sample.translate(bytes.maketrans(b"1234", b"2468"))
        return left, left, "add", b"", tiles(doubled, digits)
    if case == "add":
        write_operand(left, b"", sample, digits)
        write_operand(right, b"", sample.translate(complement), digits)
        return left, right, "add", b"", tiles(b"9" * TILE_BYTES, digits)
    # 9*10^(d-1) - (7*10^(d-1) + X) = 10^(d-1) + (10^(d-1)-1-X) + 1.
    # X contains only digits 1..8, so the final +1 changes one oracle digit.
    write_operand(left, b"9", b"0" * TILE_BYTES, digits - 1)
    write_operand(right, b"7", sample, digits - 1)
    return left, right, "sub", b"1", tiles(sample.translate(complement), digits - 1, 1)


def run_measured(command):
    with tempfile.TemporaryFile() as stats:
        start = time.perf_counter_ns()
        process = subprocess.Popen(command, stdout=stats)
        peak_rss_bytes = None
        if platform.system() in {"Linux", "Darwin"}:
            _, status, usage = os.wait4(process.pid, 0)
            process.returncode = os.waitstatus_to_exitcode(status)
            peak_rss_bytes = usage.ru_maxrss * (1024 if platform.system() == "Linux" else 1)
        else:
            process.wait()
        elapsed = time.perf_counter_ns() - start
        if process.returncode:
            raise subprocess.CalledProcessError(process.returncode, command)
        stats.seek(0)
        return json.load(stats), elapsed, peak_rss_bytes


def verify_output(path, prefix, blocks):
    digest = hashlib.sha256()
    start = time.perf_counter_ns()
    with path.open("rb") as result:
        if result.read(len(prefix)) != prefix:
            raise ValueError("incorrect result prefix")
        digest.update(prefix)
        offset = len(prefix)
        for expected in blocks:
            actual = result.read(len(expected))
            if actual != expected:
                raise ValueError(f"incorrect result in block beginning at byte {offset}")
            digest.update(actual)
            offset += len(expected)
        if result.read(1):
            raise ValueError("unexpected bytes after result")
    return digest.hexdigest(), offset, time.perf_counter_ns() - start


def command_text(*command, cwd=None):
    try:
        return subprocess.check_output(command, cwd=cwd, text=True, stderr=subprocess.DEVNULL).strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def linux_host_field(file, key):
    path = Path("/proc") / file
    if path.is_file():
        with path.open() as stream:
            for line in stream:
                name, sep, value = line.partition(":")
                if sep and name.strip() == key:
                    return value.strip()
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scarp", type=Path, help="path to a release-mode scarp executable")
    parser.add_argument("--digits", type=int, default=1_000_000, help="decimal digits in the main operand")
    parser.add_argument("--case", choices=("add", "sub", "double", "mul", "div"), default="add")
    parser.add_argument("--buffer-bytes", type=int, default=256 * 1024)
    parser.add_argument("--work-dir", type=Path, default=Path(tempfile.gettempdir()))
    args = parser.parse_args()
    division = args.case == "div"
    factor_digits = len(FACTOR_DIGITS) - 1
    min_digits = factor_digits + 2 if division else 2
    min_buffer_bytes = 9 if division else 30
    if not min_digits <= args.digits <= (2**64 - 1) // 2 or not min_buffer_bytes <= args.buffer_bytes <= 2**32 - 1:
        parser.error(f"provide at least {min_digits} digits and a buffer budget of at least {min_buffer_bytes} bytes within the supported range")

    digits = args.digits
    left_bytes = digits + int(division)
    right_digits = factor_digits if args.case in {"mul", "div"} else digits
    right_bytes = right_digits + int(args.case == "mul")
    if division:
        output_bytes = digits - factor_digits + 1
        scratch_bytes = 0
    else:
        output_bytes = digits + (len(FACTOR_DIGITS) if args.case == "mul" else 0)
        limb_count = (digits + 8) // 9
        max_input_limbs = (max(left_bytes, right_bytes) + 8) // 9
        chunk_limbs = min(max_input_limbs, (min(args.buffer_bytes, 1024 * 1024) - 8) // 22)
        scratch_limbs = limb_count + (args.case != "sub")
        scratch_bytes = 24 + 4 * scratch_limbs + 8 * ((scratch_limbs + chunk_limbs - 1) // chunk_limbs)
    temp_bytes = scratch_bytes + output_bytes
    operand_storage = left_bytes if args.case == "double" else left_bytes + right_bytes
    free_before = shutil.disk_usage(args.work_dir).free
    if operand_storage + temp_bytes > free_before:
        parser.error(f"need {operand_storage + temp_bytes} logical bytes in the workspace, found {free_before} free")

    with tempfile.TemporaryDirectory(prefix="scarp-bigint-bench-", dir=args.work_dir) as tmp:
        work = Path(tmp)
        left, right, operation, prefix, oracle = prepare(args.case, work, digits)
        output = work / "output"
        command = [
            str(args.scarp.resolve()), "bigint", operation, "--stats",
            "--max-input-bytes", str(left_bytes + right_bytes), "--max-output-bytes", str(output_bytes),
            "--max-temp-bytes", str(temp_bytes), "--max-buffer-bytes", str(args.buffer_bytes),
            str(left), str(right), str(output),
        ]
        available = linux_host_field("meminfo", "MemAvailable")
        stats, elapsed, peak_rss = run_measured(command)
        digest, verified_bytes, verify_ns = verify_output(output, prefix, oracle)
        if (verified_bytes != output_bytes or stats["peak_temp_bytes"] != temp_bytes
                or stats["scratch_bytes"] != scratch_bytes):
            raise ValueError("observed sizes differ from the benchmark plan")
        if division and stats["remainder"] != -DIVISION_REMAINDER:
            raise ValueError("incorrect signed remainder")
        record = {
            "case": args.case,
            "operation": operation,
            "operand_digits": [digits, right_digits],
            "distinct_operand_files": 1 if left == right else 2,
            "persistent_input_bytes": operand_storage,
            "workers": 1,
            "backend": "forward decimal staging" if division
            else "seekable decimal input and little-endian limb scratch",
            "algorithm": {
                "add": "base-10^9 carry",
                "sub": "base-10^9 borrow",
                "mul": "base-10^9 single-limb multiplication",
                "div": "base-10^9 long division with a partial decimal tail",
            }[operation],
            "fixture_seed": SEED,
            "elapsed_ns": elapsed,
            "launcher_observed_peak_rss_bytes": peak_rss,
            "verified_output_bytes": verified_bytes,
            "verification_ns": verify_ns,
            "output_sha256": digest,
            "filesystem": command_text("stat", "-f", "-c", "%T", str(args.work_dir))
            if platform.system() == "Linux" else None,
            "disk_free_before_bytes": free_before,
            "system": platform.platform(),
            "cpu": linux_host_field("cpuinfo", "model name") or platform.processor(),
            "available_ram_before_bytes": int(available.split()[0]) * 1024 if available else None,
            "rustc": command_text("rustc", "--version"),
            "scarp": command_text(str(args.scarp.resolve()), "--version"),
            "checkout_sha": command_text("git", "rev-parse", "HEAD", cwd=REPO),
            **stats,
        }
        if digits == right_digits:
            record["operand_digits_each"] = digits
        print(json.dumps(record, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
