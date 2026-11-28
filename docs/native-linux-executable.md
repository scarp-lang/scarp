# Standalone Linux x86-64 executables

`scarp native-exe` generates an executable Linux ELF64 image for a source
file containing supported pure functions and a zero-argument
`pure funcra main() -> integer`. The compiler uses the verified bounded
native call graph and x86-64 emitter. The output has its own process entry
and uses the Linux exit syscall. It has no linker, libc, dynamic loader
or interpreter dependency.

```sh
scarp native-exe linux-x86_64 1000 4096 examples/native_linux_exit.srp -o scarp-exit
./scarp-exit
echo $?
# 45
```

The first limit sets the shared native work allowance. The second sets the
maximum native private call-chain stack size in bytes, including return
addresses. The process startup stack is outside that allowance. The writer
requires a new destination path and publishes it atomically. Unix hosts
set its executable permissions. A Linux x86-64 environment runs the image.

| Process status | Meaning |
| --- | --- |
| 0–125 | The exact integer returned by `main` |
| 126 | `main` returned an integer outside the process result range |
| 129–135 | Native runtime status 1–7, offset by 128 |

The compiler accepts the established nonrecursive integer/boolean subset
with checked operations, bounded loops and supported inline aggregate
locals. Unsupported valid source gives E0049, including tensor execution,
heap-backed values, standard I/O, and recursive calls. Runtime process
statuses omit source spans. The relocatable object API retains its checked
status-and-span result for hosts that need that information.
