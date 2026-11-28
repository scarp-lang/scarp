# Experimental native object output

The experimental Rust API writes a completed ELF64 object at an explicit
destination. The object emitter performs compilation before the writer
touches the output directory.

```rust
let object = scarp::native_object::emit_elf64_integer_kernel("bind value = 42;")?;
scarp::native_object::write_elf64_object_new("value.o", &object)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The writer checks the Linux x86-64 relocatable header, writes and flushes
the bytes in a scratch directory beside the destination, and publishes
them with a no-overwrite hard link. The output appears after the full
object is written. Existing regular files, directories and symlinks keep
their previous contents or targets. The staging guard removes its paths
on normal return and I/O error. The destination parent must already
exist and its filesystem must support hard links. The caller controls
that directory. Directory durability after a crash and reaping staging
paths left by process termination are separate concerns.

## Experimental CLI

```sh
scarp native-object x86_64-none checked input.srp -o output.o
scarp native-object --json x86_64-none bounded 1000 loop.srp -o loop.o
scarp native-object --json x86_64-none bounded-entry evaluate 1000 512 examples/native_bounded_entry.srp -o calls.o
scarp native-object --json x86_64-none flat-entry evaluate 1000 512 examples/native_bounded_entry.srp -o calls.bin
```

The command emits a relocatable ELF64 object with one exported symbol.
`checked` and `bounded` compile a standalone pure integer function.
Checked mode accepts forward scalar control flow. Both bounded modes also
accept `while` and counted `repeat` loops.

`bounded-entry` selects a named pure integer function and includes its
reachable nonrecursive direct callees. The source file contains only
function declarations. The work allowance is shared across the call chain.
The stack budget is measured below the aligned caller stack pointer
immediately before `CALL`. It includes return addresses and outgoing
arguments inside the object. External argument slots, caller alignment
padding and interrupt frames require separate space.
Both allowances are decimal unsigned 32-bit integers.

The `flat-entry` mode writes the bounded object's position-independent
`.text` bytes alone. A loader can map the segment executable at a suitable
address and call byte zero with the checked SysV AMD64 integer ABI. Its
JSON report includes the argument, frame and failure-span metadata.

See the [function ABI](native-elf64.md) for supported source and calling
conventions. Source reads use the regular frontend file-size and UTF-8 limits.
The caller chooses a new filename with `-o`. Project inputs and linking
remain future work.

By default, success exits 0 with empty output streams. Invalid arguments or
targets exit 2. Source, compilation and file output errors exit 1 and write
text diagnostics to stderr. The compiler finishes before the first file write.
Publication uses the atomic, no-overwrite writer described above.

### JSON metadata

Pass `--json` before the target to write a single JSON record to stdout after
the new object is published. Compilation and file output errors leave stdout
empty. The object remains published if stdout fails after the write.

| Field | Meaning |
| --- | --- |
| `schema_version` | `1` for this schema |
| `target`, `mode`, `symbol` | Object target, ABI mode and exported symbol. Flat entry uses `null` for the symbol. |
| `max_work` | Native work allowance, or `null` for checked mode |
| `integer_arguments` | Number of required integer arguments |
| `call_stack_alignment` | Required caller stack alignment in bytes before `CALL` |
| `callee_frame_bytes` | Callee-allocated frame bytes |
| `callee_stack_bytes` | Callee-allocated frame plus the return address |
| `stack_argument_bytes` | Caller-owned stack argument bytes before alignment padding |
| `max_stack_bytes` | Caller-supplied stack budget in entry modes |
| `peak_stack_bytes` | Static stack bound for the selected call graph in entry modes |
| `max_active_frames` | Static depth bound including the selected entry in entry modes |
| `span_encoding` | `utf-8-bytes` |
| `failure_spans` | Half-open `[start, end]` byte ranges in the original source, indexed by the native `span_id` on failure |

Consult `failure_spans` only when the native call returns a nonzero status.
The offsets include the original UTF-8 byte lengths and line endings. Keep the
report with the object and its input source when diagnosing native failures.
