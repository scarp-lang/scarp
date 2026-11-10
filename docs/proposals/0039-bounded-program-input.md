# Proposal 0039: Bounded program input before richer I/O

**Status:** Gate A implemented in the alpha.6 integration candidate; Gate B deferred  
**Reviewed:** 2026-11-10  
**Intended starting milestone:** post-v0.0.6-alpha.5 core-language work  
**Scope:** deterministic UTF-8 stdin lines, explicit host capability, and a separately gated interactive mode

## Motivation and baseline

Scarp already has `print`, `println`, `write`, `eprint`, and `ewrite`; those are **output statements**. Typed `funcra`, bindings, branches, loops, scalar types, numerical collections, and module verification are also present. This proposal records the preimplementation design. Gate A is now implemented and specified by [the input contract](../program-input.md); `ask` remains unsupported. Published alpha.5 has no `read_line`.

The existing interpreter evaluates the selected execution path and captures stdout/stderr events before replaying them. This provides no program output for source/evaluation errors. A prompt must be visible *before* waiting for interactive stdin, so a naïve input API would silently violate an existing observable guarantee.

## Design principles

1. Preserve `print`, its supported `println` alias, `write`, `eprint`, and `ewrite` exactly as they behave today. Do not rename or remove them to create artificial language uniqueness.
2. Make external input an **impure capability**. No hidden process-global stdin access from the Rust library, static verification, or pure `funcra`.
3. Keep ordinary no-input programs' output ordering, buffering, budgets, diagnostics and return codes unchanged.
4. Expose input using the existing `string` type before inventing a general optional/exception/composite type.
5. Bound per-line size, total consumed input, and read count independently of execution work; account for captured text's lifetime. Do not claim these logical limits measure total process RAM.
6. Specify EOF, invalid UTF-8, I/O failure, and line endings rather than returning an ambiguous empty string.
7. Split deterministic input and user-visible interactive prompts into **separate implementation gates**. No prerelease should claim terminal interactivity if it only handles redirected stdin.

## Gate A — bounded line input (first implementation PR)

**Gate A spelling:** `read_line() -> string`, a zero-argument impure expression callable from an impure function or file-level initializer.

**Illustrative syntax, NOT executable on published alpha.5:**

```scarp
bind name: string = read_line();
print("Hello", name);
```

With a single input line `Ada\n`, expected stdout is `Hello Ada\n`. This milestone supports **piped or explicitly host-supplied input**, not an interactive prompt. A second read consumes a second line. The function does not itself emit output.

- Consume one UTF-8 line at the selected call site. Remove a terminating LF; remove a preceding CR **only** when the line ended with CRLF. Preserve a bare CR inside a line.
- Preserve valid Unicode scalar content without trimming, normalization, locale transformations, or implicit number conversion. An empty line is a valid empty string.
- On EOF before a new line, report a distinct bounded runtime diagnostic, **not** `""`; the final unterminated nonempty line is a valid last line. On invalid UTF-8, reader failure, or exceeded input limit, report specific bounded diagnostics at the call span. Allocate diagnostic codes only after checking the live registry.
- No ambient stdin in embedding APIs: the Rust host supplies `&mut impl BufRead` or equivalent explicit reader plus independent input limits. Existing writer-only APIs remain backward-compatible and must fail clearly if a selected input call has no reader; they must not hang.
- CLI stdin can be enabled for redirected/piped use after explicit command-path validation and test coverage. Ensure terminal-connected stdin behavior is documented: either reject with a clear interactive-mode guidance diagnostic or supply Gate B; **never silently hang behind a buffered prompt**.
- `pure funcra`, constant expressions, parameter defaults, `needs`, `ensures`, and other pure contexts cannot read input. A typed resolver checks forbidden effects even on branches that will not execute.
- `scarp verify FILE` and project-directory verification must type-check but never consume input. `scarp inspect` likewise has no runtime reads. Historical evaluating `check` must not silently consume stdin; encountering a selected input operation should fail with an explicit diagnostic.
- Retain all-or-nothing *program output* for Gate A: validate and evaluate selected work, reading the explicit input source, then replay captured output on success. Host input consumption is itself non-rollbackable; document that distinction.

**Resource limits (design defaults to review, not shipped commitments):** maximum 64 KiB per line, 1 MiB of total source bytes consumed per execution, and 4,096 line reads. Enforce limits incrementally before allocating unbounded buffers. Preserve existing runtime steps, slot/snapshot quotas, and vector/text allocation accounting. Decide how limits are represented in `ExecutionOptions` without breaking public Rust structs.

## Gate B — terminal prompts (independent PR after Gate A)

Consider `ask(prompt: string) -> string` or an explicit `--interactive` execution mode only after reviewing the effect model and terminal output contract. A prompt must be written and flushed *before* blocking for input. Because prompt bytes cannot be rolled back if a later statement fails, this mode requires an **explicitly documented non-atomic stream policy**.

Do not implement `ask` merely by composing buffered `write` with `read_line`; this would hide the prompt. Do not retrospectively change the atomic-output behavior of existing `scarp exec` programs. Decide on prompt destination, absence of a terminal, broken pipes, input echoes, stdin EOF, stderr diagnostics, and flush errors before merging Gate B. No echo-suppression/password input functionality is proposed.

**Illustrative future syntax, not yet supported:**

```scarp
bind name = ask("Your name: ");
print("Hello", name);
```

## Relationship to other core features

- Scalar parsing and formatting, e.g. `text_to_integer`, is being developed separately in PR #179. Integrate that code only after review and green tests; stdin must not duplicate its conversions.
- Keep `integer` checked i64 separate from the draft `bigint` work in PR #172.
- Existing `if`/`elif`/`else`, `while`/`repeat`, typed `funcra`, `bind`/`var`, and primitive operators are **implemented**, not TODO items.
- After input, prioritize a small typed composite (`tuple` or fixed array), a coherent optional/result error model, and stable text APIs. Do not attempt general maps, a package manager, native executable linking, or GPUs in the same core-input PR.
- Keep the compiler as the authoritative implementation. Publish public tutorials in `scarp-docs` and runnable teaching examples in `scarp-by-example` only after binary-compatible behavior lands.

## Required validation matrix

1. Empty line versus immediate EOF, last unterminated line, CRLF and bare CR.
2. ASCII, Devanagari, emoji, combining marks, invalid UTF-8, and Unicode error spans.
3. One read, repeated reads, unused functions, input inside a selected conditional and a loop, and input not reached due to an earlier branch.
4. Purity and contract/default/constant rejections; no hidden input in `verify`, `inspect`, `check` or diagnostics.
5. Exact boundaries for per-line bytes, total bytes and number of lines; no unlimited allocation on extremely long lines.
6. Deterministic injected-reader Rust tests, no process-global stdin, concurrent independent executions, reader I/O errors, output replay, and no output after runtime failure.
7. Backwards compatibility for the old dual-writer and single-writer Rust APIs and all five output statements.
8. Actual Windows, Linux, macOS and MSRV CI checks; rustfmt, Clippy, doc tests and exact stdout/stderr fixtures.
9. When Gate B is designed: prompt flush order, buffering and partial-output disclosure on failure, redirected stdin behavior, and command-line opt-in.
10. Release smoke tests against **the exact final source SHA**. No tag, version bump, release request, or publication without a separate reviewed authorization.

## Non-goals for this proposal

No unbounded host filesystem/network/process I/O, password/hidden-input features, new native build command, grammar redesign, breaking output aliases, or claim of stable language compatibility. This proposal preserves the design history; [the input contract](../program-input.md) defines the implemented Gate A behavior. It is not release authorization.
