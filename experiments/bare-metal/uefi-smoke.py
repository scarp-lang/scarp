#!/usr/bin/env python3
"""Boot compiler-produced Scarp PE32+ images under OVMF and check their exits."""

import json
import os
from pathlib import Path
import re
import select
import shutil
import struct
import subprocess
import tempfile
import time


HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
ENTERED = b"Scarp UEFI: entered\r\n"
PASSED = b"Scarp UEFI: PASS\r\n"
ABORTED = re.compile(rb"BdsDxe: failed to start Boot[^\r\n]*: Aborted\r?\n")
KERNEL_ENTERED = b"Scarp loader: entering kernel\r\n"
KERNEL_PASSED = b"SCARP KERNEL PASS\n"


def guest_command(work, code, variables):
    flash = work / "variables.fd"
    shutil.copyfile(variables, flash)
    serial = work / "serial.log"
    command = [
        "qemu-system-x86_64", "-machine", "q35", "-accel", "tcg", "-m", "128M",
        "-nic", "none", "-display", "none", "-monitor", "none", "-no-reboot",
        "-serial", f"file:{serial}",
        "-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={code}",
        "-drive", f"if=pflash,format=raw,unit=1,file={flash}",
        "-drive", f"if=virtio,format=raw,readonly=on,file=fat:{work / 'disk'}",
    ]
    return command, serial


def stop(guest):
    if guest.poll() is None:
        guest.terminate()
        try:
            guest.wait(timeout=3)
        except subprocess.TimeoutExpired:
            guest.kill()
            guest.wait()


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
    command, serial = guest_command(work, code, variables)
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
            stop(guest)
    transcript = serial.read_bytes()[:16000] if serial.exists() else b""
    raise RuntimeError(
        f"Unexpected UEFI output for {source.name}\n"
        f"Serial: {transcript!r}\nQEMU: {(work / 'qemu.log').read_text()}"
    )


def qmp_read(guest):
    if not select.select([guest.stdout], [], [], 5)[0]:
        raise RuntimeError("QMP response timed out")
    line = guest.stdout.readline()
    if not line:
        raise RuntimeError("QMP disconnected")
    return json.loads(line)


def qmp(guest, command, **arguments):
    guest.stdin.write(json.dumps({"execute": command, "arguments": arguments, "id": 1}).encode() + b"\n")
    while True:
        response = qmp_read(guest)
        if response.get("id") == 1:
            return response["return"]


def pe_sections(image):
    pe, = struct.unpack_from("<I", image, 0x3c)
    count, = struct.unpack_from("<H", image, pe + 6)
    optional_bytes, = struct.unpack_from("<H", image, pe + 20)
    table = pe + 24 + optional_bytes
    sections = {}
    for index in range(count):
        at = table + 40 * index
        name, size, rva, _, offset = struct.unpack_from("<8s4I", image, at)
        sections[name.rstrip(b"\0")] = (size, rva, offset)
    return sections


def boot_kernel(work, compiler, loader, kernel, code, variables, status, marker=b""):
    boot_dir = work / "disk" / "EFI" / "BOOT"
    boot_dir.mkdir(parents=True)
    image = boot_dir / "BOOTX64.EFI"
    subprocess.run(
        [str(compiler), "native-exe", "uefi-kernel-x86_64", "boot", "kernel", "10000",
         "1024", str(loader), str(kernel), "-o", str(image)],
        cwd=ROOT,
        check=True,
    )
    data = image.read_bytes()
    sections = pe_sections(data)
    text_size, text_rva, text_offset = sections[b".text"]
    bss_size, bss_rva, _ = sections[b".bss"]
    # The x86 HLT instruction advances RIP to the following short jump.
    text = data[text_offset:text_offset + text_size]
    halt_rva = text_rva + text.index(b"\xfa\xf4\xeb\xfd") + 2
    command, serial = guest_command(work, code, variables)
    debug = work / "debug.log"
    command.extend(["-qmp", "stdio", "-debugcon", f"file:{debug}"])
    registers = ""
    with (work / "qemu.log").open("wb") as qemu_log:
        guest = subprocess.Popen(
            command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=qemu_log, bufsize=0,
        )
        try:
            qmp_read(guest)
            qmp(guest, "qmp_capabilities")
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline and guest.poll() is None:
                transcript = serial.read_bytes() if serial.exists() else b""
                if KERNEL_ENTERED not in transcript:
                    time.sleep(0.05)
                    continue
                if ABORTED.search(transcript.split(KERNEL_ENTERED, 1)[1]):
                    if status is None and debug.read_bytes() == b"":
                        return "loader aborted before kernel entry"
                    break
                registers = qmp(guest, "human-monitor-command", **{"command-line": "info registers"})
                if "HLT=1" in registers:
                    regs = {key: int(value, 16) for key, value in
                            re.findall(r"\b(R[A-Z0-9]+)\s*=([0-9a-fA-F]+)", registers)}
                    image_base = regs["R14"] - bss_rva
                    if image_base <= 0 or regs["RIP"] != image_base + halt_rva:
                        time.sleep(0.05)
                        continue
                    idt = re.search(r"IDT=\s*([0-9a-fA-F]+)\s+([0-9a-fA-F]+)", registers)
                    actual = regs["RDX"] & 0xffff_ffff
                    stack_top = regs["R14"] + bss_size
                    in_stack = stack_top - 65536 <= regs["RSP"] <= stack_top
                    if (
                        status is not None and actual == status and regs["RAX"] == 0
                        and "CPL=0" in registers and regs["RFL"] & 0x200 == 0
                        and regs["R12"] == regs["R13"] == 0 and in_stack
                        and regs["R15"] == stack_top - 65536 - 4096
                        and idt and int(idt[1], 16) == regs["R15"] - 4096 and int(idt[2], 16) == 4095
                        and debug.read_bytes() == marker
                    ):
                        return f"halted after handoff on the kernel stack with status {status}"
                    break
                time.sleep(0.05)
        finally:
            stop(guest)
    raise RuntimeError(
        f"Unexpected kernel outcome for {kernel.name}\n"
        f"Serial: {serial.read_bytes()[:3000]!r}\n"
        f"Debug port: {debug.read_bytes()[:1000]!r}\n"
        f"QEMU: {(work / 'qemu.log').read_text()}\n{registers}"
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
        loader = HERE / "uefi-loader.srp"
        kernel = HERE / "uefi-kernel.srp"
        outcome = boot_kernel(work / "kernel", compiler, loader, kernel, code, variables, 0, KERNEL_PASSED)
        print(f"atomic kernel and CPU queries: {outcome}", flush=True)
        for name, body, status in [
            ("unaligned", "return kernel_load64(1);", 9),
            ("outside-region", "return kernel_exchange64(4096, 10);", 9),
            ("outside-map", "return kernel_map_pages(kernel_map_count());", 10),
            ("invalid-debug-byte", "return qemu_debug_byte(-1);", 11),
            ("exception", "x86_breakpoint(); return 0;", 12),
            ("invalid-cpuid", "return x86_cpuid(4294967296, 0, 0);", 13),
        ]:
            entry = work / f"{name}.srp"
            entry.write_text(f"funcra kernel() -> integer {{ {body} }}\n", encoding="utf-8")
            outcome = boot_kernel(work / name, compiler, loader, entry, code, variables, status)
            print(f"{name}: {outcome}", flush=True)
        declined = work / "declined.srp"
        declined.write_text(loader.read_text().replace("return 0;", "return 1;"), encoding="utf-8")
        outcome = boot_kernel(work / "declined", compiler, declined, kernel, code, variables, None)
        print(f"declined loader: {outcome}", flush=True)


if __name__ == "__main__":
    main()
