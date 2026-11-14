"""Exercise Scarp against real Windows console input, with redirected output."""

import ctypes as c
from ctypes import wintypes as w
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading


class KeyEvent(c.Structure):
    _fields_ = [
        ("down", w.BOOL),
        ("repeat", w.WORD),
        ("virtual_key", w.WORD),
        ("scan", w.WORD),
        ("character", w.WCHAR),
        ("modifiers", w.DWORD),
    ]


class InputRecord(c.Structure):
    # KEY_EVENT_RECORD has the largest size and alignment of the event union.
    _fields_ = [("kind", w.WORD), ("key", KeyEvent)]


def console_api():
    api = c.WinDLL("kernel32", use_last_error=True)
    api.AllocConsole.argtypes = []
    api.AllocConsole.restype = w.BOOL
    api.FreeConsole.argtypes = []
    api.FreeConsole.restype = w.BOOL
    api.GetConsoleMode.argtypes = [w.HANDLE, c.POINTER(w.DWORD)]
    api.GetConsoleMode.restype = w.BOOL
    api.WriteConsoleInputW.argtypes = [
        w.HANDLE, c.POINTER(InputRecord), w.DWORD, c.POINTER(w.DWORD),
    ]
    api.WriteConsoleInputW.restype = w.BOOL
    return api


def write_line(api, handle, text):
    records = []
    for character in text + "\r":
        kind = 0x0001  # KEY_EVENT
        virtual_key = 0x0D if character == "\r" else 0  # VK_RETURN
        for down in (True, False):
            records.append(InputRecord(kind, KeyEvent(down, 1, virtual_key, 0, character, 0)))
    events = (InputRecord * len(records))(*records)
    written = w.DWORD()
    if not api.WriteConsoleInputW(handle, events, len(events), c.byref(written)):
        raise c.WinError(c.get_last_error())
    assert written.value == len(events), written.value


def read_prefix(pipe, expected):
    outcome = queue.Queue()
    reader = threading.Thread(target=lambda: outcome.put(pipe.read(len(expected))), daemon=True)
    reader.start()
    try:
        prefix = outcome.get(timeout=10)
    except queue.Empty as exc:
        raise AssertionError("prompt was not flushed before the console read") from exc
    reader.join()
    assert prefix == expected, prefix


def main():
    import msvcrt

    binary = Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="scarp-console-") as directory:
        source = Path(directory) / "greeting.srp"
        source.write_text(
            'bind name = input("नाम> "); '
            'bind age = input_integer("Age> "); print(name, age);',
            encoding="utf-8",
        )
        api = console_api()
        # The test gets a fresh console even if CI launched Python from a
        # console. Child stdout/stderr still use byte-preserving pipes.
        if not api.FreeConsole() or not api.AllocConsole():
            raise c.WinError(c.get_last_error())
        try:
            descriptor = os.open("CONIN$", os.O_RDWR | os.O_BINARY)
            with os.fdopen(descriptor, "r+b", buffering=0) as console:
                handle = msvcrt.get_osfhandle(console.fileno())
                mode = w.DWORD()
                assert api.GetConsoleMode(handle, c.byref(mode)), c.get_last_error()
                assert mode.value & 0x0002, "expected line input mode"
                rejected = subprocess.run(
                    [binary, "exec", source], stdin=console,
                    capture_output=True, timeout=10,
                )
                assert rejected.returncode == 1 and rejected.stdout == b"", rejected
                assert b"E0054" in rejected.stderr, rejected.stderr

                with subprocess.Popen(
                    [binary, "exec", "--interactive", source], stdin=console,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0,
                ) as child:
                    try:
                        read_prefix(child.stdout, "नाम> ".encode("utf-8"))
                        write_line(api, handle, "नमस्ते")
                        read_prefix(child.stdout, b"Age> ")
                        write_line(api, handle, "-23")
                        out, err = child.communicate(timeout=10)
                        assert child.returncode == 0 and not err, (child.returncode, err)
                        assert out == "नमस्ते -23\n".encode("utf-8"), out
                    finally:
                        if child.poll() is None:
                            child.kill()
        finally:
            api.FreeConsole()
    print("Windows console input passed: opt-in, prompt flush, UTF-8 and CRLF")


if __name__ == "__main__":
    main()
