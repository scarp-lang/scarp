# Proposal 0034 - Machine inspection, explanations and compiler benchmarks

Status: unreleased development; no version, tag or publication change.

Add `scarp modules --json [--] <path>` and additive
ModuleProgram::write_inspection_json. Preserve ordinary text output. Compile the
same bounded source graph before emitting output; selected runtime calls, defaults
and contracts do not execute. Invalid constants still fail compilation. Reject
duplicate/unsupported flags before I/O, preserving native paths and `--` behavior.

JSON schema version 1 contains entry source ID zero, discovery-ordered modules,
loader path labels, source-ordered import records, unique dependencies/dependents,
typed public/private symbols and borrowed function parameter/default-presence
metadata, plus constant reference edges. All spans are two-element half-open
file-local UTF-8 byte arrays. Import/symbol spans belong to their module; constant
reference records identify both owning and target sources. Source IDs are local
to the compilation, and path display labels may be lossy for native non-Unicode
names. Do not expose source contents, constant values or default expressions.

Use a dependency-free streaming JSON writer with standard escaping, deterministic
field/order/newline policy and preserved writer errors. Compute local offsets from
known source starts rather than scanning source prefixes for line/columns. The
writer is linear in emitted metadata/string length, not number of declarations
times file length. Consumers needing positions can use the existing source-map API.

Add `scarp explain E####` and per-command help. Canonical codes only; unknown or
malformed input is usage status 2 without opening source files. One diagnostic
catalogue generates existing enum variants/codes plus explanations/lookup/all-kinds
metadata, avoiding separate drifting code registries. Existing assignments and
rendered diagnostics remain unchanged. Explanations cover every current code.

Add a dependency-free compiler benchmark harness for annotated source compilation,
module constant/dependency compilation and JSON writing. Validate workload results
before timing. Provide quick CI smoke mode, CSV measurements and methodology;
measure compilation/formatting rather than claim a new native execution backend.

Test exact JSON snapshots, control/Unicode escaping, original names and signatures,
local span ownership, constant edges, no runtime execution/literal leakage,
writer failures, all-code catalogue invariants and CLI statuses/native paths.
Independently parse emitted JSON with Python and native PowerShell package checks.
No JSON library for Scarp values, LSP, caching, formatter, native build or package
commands are implied by these tools.
