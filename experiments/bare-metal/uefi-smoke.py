#!/usr/bin/env python3
"""Boot compiler-produced Scarp PE32+ images under OVMF and check their exits."""

import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time


HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
ENTERED = b"Scarp UEFI: entered\r\n"
PASSED = b"Scarp UEFI: PASS\r\n"
ABORTED = re.compile(rb"BdsDxe: failed to start Boot[^\r\n]*: Aborted\r?\n")


def boot(work, compiler, source, budget, code, variables, expect_abort):
    disk = work / "disk"
    boot_dir = disk / "EFI" / "BOOT"
    boot_dir.mkdir(parents=True)
    image = boot_dir / "BOOTX64.EFI"
    subprocess.run(
        [str(compiler), "native-exe", "uefi-x86_64", "boot", str(budget),
         "1024", str(source), "-o", str(image)],
        check=True,
        cwd=ROOT,
    )
    if not expect_abort:
        assembly = subprocess.check_output(["objdump", "-d", str(image)], text=True)
        if not re.search(r"\bf3 90\s+pause\b", assembly):
            raise RuntimeError("Expected PAUSE in the Scarp-generated machine code")
    flash = work / "variables.fd"
    shutil.copyfile(variables, flash)
    serial = work / "serial.log"
    command = [
        "qemu-system-x86_64", "-machine", "q35", "-accel", "tcg", "-m", "128M",
        "-nic", "none", "-display", "none", "-monitor", "none", "-no-reboot",
        "-serial", f"file:{serial}",
        "-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={code}",
        "-drive", f"if=pflash,format=raw,unit=1,file={flash}",
        "-drive", f"if=virtio,format=raw,readonly=on,file=fat:{disk}",
    ]
    with (work / "qemu.log").open("wb") as qemu_log:
        guest = subprocess.Popen(command, stdin=subprocess.DEVNULL, stderr=qemu_log)
        try:
            deadline = time.monotonic() + 35
            while time.monotonic() < deadline:
                data = serial.read_bytes() if serial.exists() else b""
                if ENTERED in data:
                    after_entry = data.split(ENTERED, 1)[1]
                    if ABORTED.search(after_entry):
                        if expect_abort and PASSED not in after_entry:
                            return "entered, returned EFI_ABORTED"
                        break
                    if PASSED in after_entry:
                        after_pass = after_entry.split(PASSED, 1)[1]
                        if not expect_abort and b"BdsDxe: loading Boot" in after_pass:
                            return "entered, printed PASS, returned to firmware"
                        if expect_abort:
                            break
                if guest.poll() is not None:
                    break
                time.sleep(0.05)
        finally:
            if guest.poll() is None:
                guest.terminate()
                try:
                    guest.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    guest.kill()
                    guest.wait()
    transcript = serial.read_bytes()[:16000] if serial.exists() else b""
    raise RuntimeError(
        f"Unexpected UEFI output for {source.name}\n"
        f"Serial: {transcript!r}\nQEMU: {(work / 'qemu.log').read_text()}"
    )


def main():
    code = Path(os.environ.get("SCARP_OVMF_CODE", "/usr/share/OVMF/OVMF_CODE_4M.fd")).resolve()
    variables = Path(os.environ.get("SCARP_OVMF_VARS", "/usr/share/OVMF/OVMF_VARS_4M.fd")).resolve()
    if not code.is_file() or not variables.is_file():
        raise SystemExit("Install matching OVMF files or set SCARP_OVMF_CODE and SCARP_OVMF_VARS.")
    compiler = os.environ.get("SCARP_UEFI_COMPILER")
    if compiler is None:
        subprocess.run(["cargo", "build", "--locked", "--offline", "--bin", "scarp"],
                       cwd=ROOT, check=True)
        compiler = ROOT / "target" / "debug" / "scarp"
    compiler = Path(compiler).resolve()
    with tempfile.TemporaryDirectory(prefix="scarp-uefi-") as directory:
        work = Path(directory)
        source = HERE / "uefi.srp"
        divide = work / "divide.srp"
        divide.write_text(
            'funcra boot() -> integer { print("Scarp UEFI: entered"); return 1 / 0; }\n',
            encoding="utf-8",
        )
        nonzero = work / "nonzero.srp"
        nonzero.write_text(
            'funcra boot() -> integer { print("Scarp UEFI: entered"); return 5; }\n',
            encoding="utf-8",
        )
        for name, entry, budget, expect_abort in [
            ("aggregate and PAUSE", source, 1000, False),
            ("exhausted work budget", source, 1, True),
            ("checked division", divide, 1000, True),
            ("nonzero source result", nonzero, 1000, True),
        ]:
            outcome = boot(work / entry.stem / str(budget), compiler, entry, budget,
                           code, variables, expect_abort)
            print(f"{name}: {outcome}", flush=True)


if __name__ == "__main__":
    main()
