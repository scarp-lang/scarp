# Incremental module invalidation — phase 1 (development API)

This page specifies the **verified dependency invalidation foundation** added
after v0.0.6-alpha.1. It is not a cache, a fast compiler, a file watcher, a
language server or a published alpha.6 capability.

## Scope and invariants

`modules::invalidation::InvalidationSnapshot` is built from an already
successful, linked and type-checked `ModuleProgram`:

- It retains the compiler's **original loader-resolved identity** for every
  source, deduplicated dependencies and reverse import edges, and a shared
  reference to the original immutable `SourceMap`. It never parses new
  source, re-resolves a path, performs I/O or invents a separate dependency
  analyzer.
- Identity keys are loader-specific: `MemorySourceLoader` uses normalized
  virtual paths, while `FileSourceLoader` uses canonical paths rooted under
  the trusted directory. Display names and import string literals are not
  interchangeable with loader identities. Do not compare snapshots from
  different roots/loader configurations as a cache-validation shortcut.
- `SourceId` is local to *one* compiled graph. Recompilation can change
  discovery order. Neither `SymbolId` nor module IDs nor absolute byte spans
  are permitted to survive a rebuild.
- Default `InvalidationLimits` caps the recorded graph at 128 files, 1,024
  distinct dependency edges, 1 MiB total identity text, and 128 changed-file
  inputs. Failures are typed and no partial snapshot is exposed. This is an
  **additional** metadata budget, not a replacement for the compiler's module,
  constant, type, editor or runtime limits.
- All decisions are conservative: a false positive (more files invalidated)
  is preferable to a false negative. No execution of source statements occurs.

## Before recompiling: edit impact

Use `plan_edits(&[...])` with original **loader identities** for files
that the caller knows changed. It includes every changed file and all its
transitive importers, deduplicated and returned in old source-discovery order.

A changed `shared.srp` imported by `a.srp` and `b.srp` through a diamond
invalidates both importers and the entry file. A leaf edit does not mark
unrelated leaves. Repeated edit entries do not duplicate results.

**Any unknown identity causes full old-graph invalidation** because the new
graph may add/remove files, change canonical identity, or target previously
unreachable modules. Any nonempty edit set requires full graph discovery and
semantic validation. Even an empty edit set is **only the caller's assertion**:
this API does not read the filesystem or verify that sources remain unchanged.
Do not use it as permission to reuse a compiled program.

Deleted files, importer path edits, visibility changes, constant dependencies,
changed loader-root identity, symlink retargeting and errors must all pass
through the ordinary loader and compiler before a new result is published.
Compiler failure must be surfaced as an error, **never** replaced by stale
navigation or a previous successful `ModuleProgram`.

## After recompiling: exact transition comparison

Compile the updated graph with the *same existing compiler pipeline* first.
Then call `old_snapshot.compare(&new_snapshot)`. It compares:

1. Present/absent identities in the old and new successfully compiled graphs.
2. The **full original source bytes**, not modification timestamps or a
   lossy/hash-only approximation.
3. Each importer's deduplicated, ordered, **resolved target identities**,
   rather than the numeric source IDs (which may be renumbered).

The result identifies `removed_old`, `added_new`,
`modified_old`/`modified_new`, and the corresponding transitive
`affected_old`/`affected_new` closures. Each vector is sorted in its own
graph's discovery order. A source with identical original text but a new
loader target is correctly treated as changed. A failed recompile yields no
new snapshot, and therefore no transition.

The comparison does **not** verify compiler version, type/constant/index
budgets, external runtime settings, edited document revisions, origin-root
trust, or plugin/host configuration. It is **not** a cache-hit decision.
A future cache must include these dimensions in its key and enforce limits
before allowing any semantic result reuse.

## Embedding example

```rust
use scarp::modules::{
    self, invalidation::{InvalidationLimits, InvalidationSnapshot},
    MemorySourceLoader, ModuleLimits,
};

let mut loader = MemorySourceLoader::new();
loader.insert("main.srp", "import \"lib.srp\" as l; print(l.value);")?;
loader.insert("lib.srp", "pub bind value=1;")?;

let compiled = modules::compile_with_loader(
    "main.srp", &loader, ModuleLimits::default()
)?;
let old = InvalidationSnapshot::new(&compiled, InvalidationLimits::default())?;
let impact = old.plan_edits(&["lib.srp"])?;
assert!(impact.must_rediscover);
assert_eq!(impact.affected.len(), 2);

// Build a NEW loader with edited source text; never reuse an old compiler
// result just because the invalidation closure is smaller than the graph.
let mut edited = MemorySourceLoader::new();
edited.insert("main.srp", "import \"lib.srp\" as l; print(l.value);")?;
edited.insert("lib.srp", "pub bind value=2;")?;
let rebuilt = modules::compile_with_loader(
    "main.srp", &edited, ModuleLimits::default()
)?;
let next = InvalidationSnapshot::new(&rebuilt, InvalidationLimits::default())?;
assert_eq!(old.compare(&next).affected_new.len(), 2);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Verification and remaining work

The `tests/module_invalidation.rs` suite covers diamond reachability,
specific-leaf invalidation, exact constant edits, unchanged graphs,
added/removed imports, equal-source-content loader retargeting,
invalid visibility changes, resource caps, unknown identities and cloned
read-only snapshots. Existing module tests still guard path traversal,
trusted-root symlinks, cycles, diagnostics, constant work limits and
inspection identity. A later phase will need dedicated property-based and
cold-vs-cached differential tests if actual reuse is introduced.

Future work for [issue #117](https://github.com/scarp-lang/scarp/issues/117):

- Define a complete cache identity: compiler version/settings, loader root,
  resolved canonical identities, original bytes, module/constant/type/index
  budgets, schema version, and caller document revisions.
- Implement caller-owned bounded cache entries with explicit capacity,
  deterministic eviction, no mutable globals, no mtime-only validation.
- Build equivalent cold and incremental outputs across full and failing
  graphs, changed/deleted dependencies, transitive constants, renamed files
  and Unicode/source-coordinate cases. Preserve all diagnostics.
- Verify in actual cross-platform CI with measured workloads before
  claiming any speed or memory improvement.

This phase makes no claim of faster compilation.
