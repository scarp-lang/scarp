"""Independent UTF-8/line-ending oracle and terminal nonblocking regression.

Usage: python3 scripts/test_program_input.py target/debug/scarp
No external Python packages are required. The PTY test runs on Unix only.
"""
import json
import os
from pathlib import Path
import random
import subprocess
import sys
import tempfile


def main():
    binary = str(Path(sys.argv[1]).resolve())
    count = 0
    with tempfile.TemporaryDirectory(prefix="scarp-input-oracle-") as directory:
        source = Path(directory) / "input.srp"
        source.write_text('print("before"); print(read_line());', encoding="utf-8")

        def invoke(data, *options, command="exec"):
            return subprocess.run(
                [binary, command, *options, str(source)], input=data,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10, check=False,
            )

        rng = random.Random(20261110)
        alphabet = ["a", "Z", "0", " ", "\t", "\0", "é", "🌍", "क", "\u0301", "\r"]
        for index in range(128):
            text = "".join(rng.choice(alphabet) for _ in range(rng.randrange(65)))
            ending = "\r\n" if index % 2 else "\n"
            data = (text + ending).encode("utf-8")
            # Independent raw-byte line oracle, including a CR immediately
            # before an LF regardless of its origin in the text/terminator.
            expected = data[:-1]
            if expected.endswith(b"\r"):
                expected = expected[:-1]
            result = invoke(data)
            assert result.returncode == 0 and not result.stderr, result
            assert result.stdout == b"before\n" + expected + b"\n", result
            count += 1

        for data, options, code in [
            (b"", [], "E0055"),
            (b"\xff\n", [], "E0056"),
            (b"a\n", ["--max-input-reads", "0"], "E0058"),
            (b"long\n", ["--max-input-line-bytes", "2"], "E0058"),
            (b"long\n", ["--max-input-bytes", "2"], "E0058"),
        ]:
            result = invoke(data, "--diagnostic-format=json", *options)
            assert result.returncode == 1 and not result.stdout, result
            json.loads(result.stderr)
            assert code.encode() in result.stderr, result
            count += 1
        result = invoke(b"never consumed\n", command="check")
        assert result.returncode == 1 and not result.stdout and b"E0054" in result.stderr, result
        result = invoke(b"", command="verify")
        assert result.returncode == 0 and not result.stdout and not result.stderr, result
        count += 2

        if os.name == "posix":
            import pty
            import select
            master, slave = pty.openpty()
            try:
                # No terminal input or EOF is supplied. A hidden blocking read
                # would timeout, and buffered program output must remain empty.
                result = subprocess.run(
                    [binary, "exec", str(source)], stdin=slave,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10, check=False,
                )
                assert result.returncode == 1 and not result.stdout and b"E0054" in result.stderr, result
                source.write_text('if false { print(read_line()); } print("ok");', encoding="utf-8")
                result = subprocess.run(
                    [binary, "exec", str(source)], stdin=slave,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10, check=False,
                )
                assert result.returncode == 0 and result.stdout == b"ok\n" and not result.stderr, result
                count += 2

                source.write_text('print(ask_or("Name> ", "Guest"));', encoding="utf-8")
                # Wait for the prompt while the terminal has no input queued.
                # Reading line-buffered input first would block the handshake.
                with subprocess.Popen(
                    [binary, "exec", "--interactive", str(source)], stdin=slave,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0,
                ) as child:
                    try:
                        expected_prompt = b"Name> "
                        prompt = bytearray()
                        while len(prompt) < len(expected_prompt):
                            ready, _, _ = select.select([child.stdout], [], [], 5)
                            assert ready, "interactive prompt was not flushed before terminal input"
                            chunk = os.read(child.stdout.fileno(), len(expected_prompt) - len(prompt))
                            assert chunk, "Scarp exited before flushing its prompt"
                            prompt.extend(chunk)
                        assert prompt == expected_prompt, prompt
                        os.write(master, "नमस्ते 🌍\n".encode("utf-8"))
                        stdout, stderr = child.communicate(timeout=5)
                        assert child.returncode == 0 and not stderr, (child.returncode, stderr)
                        assert stdout == "नमस्ते 🌍\n".encode("utf-8"), stdout
                        count += 1
                    finally:
                        if child.poll() is None:
                            child.kill()
            finally:
                os.close(slave)
                os.close(master)
    print(f"program-input oracle: {count} checks passed")


if __name__ == "__main__":
    main()
