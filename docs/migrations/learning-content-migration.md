# Scarp learning-content migration inventory

**Recorded:** 2026-11-09. **Status:** staged review, not a release or deletion proposal.

This inventory separates the compiler's engineering records from the official public reference and practical learning course. The initial change creates links and separately reviewed copies; historical links, immutable release tags and all compiler regression fixtures remain valid.

## Repository ownership

| Content | Authoritative public destination | Files retained in the compiler |
| --- | --- | --- |
| Language learning, installation and CLI usage, bilingual API reference | [scarp-docs](https://github.com/scarp-lang/scarp-docs) | Formal technical contracts and backward-compatible existing links |
| Numbered `.srp` lessons, exact `.stdout`, Hindi exercises, labs and example projects | [scarp-by-example](https://github.com/scarp-lang/scarp-by-example) | Compiler-owned regression examples and tests, even when copied into a lesson |
| Parser/resolver/runtime implementation, architecture, proposals, budgets, benchmark evidence and internal development plans | Compiler [scarp](https://github.com/scarp-lang/scarp) | **Remain here**; these are engineering/source-of-truth records |
| Release records, immutable candidate snapshots, publication verification and historical compatibility proof | Compiler [scarp](https://github.com/scarp-lang/scarp/tree/main/docs/releases) | **Remain here**; do not rewrite history or remove release evidence |

## Initial audited inventory (current compiler main)

- 60 top-level `.srp` examples under `examples/` are compiler regression sources. They are not automatically 60 unique beginner lessons.
- Six compiler `docs/tutorials/` Markdown files may be linked or adapted for the course; they are not automatically safe to delete.
- 37 top-level compiler `docs/*.md` files mix language/reference material, contributor and operational guidance; review them individually.
- 31 proposal files under `docs/proposals/`, 11 development records and 42 historical release records are technical/historical content, not candidates for blind migration.
- The canonical documentation branch already contains English and Hindi language/reference material. Avoid parallel rewrites of the same public API.

These are path-count observations from this audit, not a measure of implementation quality or completeness. They can change as future PRs land.

## First release-pinned migration

Compiler tag: **published `v0.0.6-alpha.4`**, immutable source commit `f04be0758cbce405ddcdb229699b0ff83c09d207`.

| Compiler regression pair (unchanged) | New numbered course location |
| --- | --- |
| `examples/temperature_softmax.srp` + `.stdout` | `examples/06-ai-foundations/12-temperature-softmax` |
| `examples/cosine_similarity.srp` + `.stdout` | `examples/06-ai-foundations/13-cosine-similarity` |
| `examples/sample_cumulative_area.srp` + `.stdout` | `examples/06-ai-foundations/14-sample-cumulative-area` |
| `examples/topk_scores.srp` + `.stdout` | `examples/06-ai-foundations/15-topk-ranking` |
| `examples/sampled_calculus.srp` + `.stdout` | `examples/04-collections/13-sampled-calculus` |

Review the source changes in [course PR #57](https://github.com/scarp-lang/scarp-by-example/pull/57), bilingual API explanations in [docs PR #91](https://github.com/scarp-lang/scarp-docs/pull/91) and compiler navigation in [PR #173](https://github.com/scarp-lang/scarp/pull/173). The course's `docs/migration-provenance.json` plus `scripts/verify_migration_provenance.py` enforce byte-for-byte identity against the pinned release in CI.

## Follow-on migration batches

1. **Audit remaining example fixtures.** Compare each published-tag fixture with existing course lessons; do not duplicate identical instruction unnecessarily. Prefer unique concepts (e.g. import inspection or editor debugging) and keep their supporting modules together.
2. **Migrate beginner tutorial ownership.** For `docs/tutorials/first-program.md`, `variables.md`, `scopes.md`, `decisions.md`, `modules.md`, and `numerical-program.md`, decide whether the by-example course already covers the topic. If so, add precise links; if not, adapt a complete tested lesson with Hindi parity.
3. **Migrate public API descriptions.** Target user-facing summaries of numerical, language and CLI behavior in scarp-docs, with version-specific signatures, errors, resource costs, English/Hindi parity and runnable course backlinks. Keep formal compiler specifications in this repository.
4. **Keep old URLs compatible.** Do not delete compiler documents while unknown inbound links or test references remain. Use concise links to the canonical page and preserve tagged references.
5. **Review links across the ecosystem.** Once the destination PRs merge, change website, playground and VS Code extension pointers to the canonical docs/lesson URLs. Never claim a draft route exists on `main` before merge.

## Mandatory quality gates

- Each course lesson must have an immutable compiler release pin, original `.srp`, corresponding byte-exact `.stdout`, a chapter entry and a matching Hindi exercise.
- The course must pass its three-OS `verify` + `exec` checks, source-provenance comparison, unit tests, Pages generation, link and sitemap inventory tests.
- Documentation must pass the local-link, bilingual code-fence parity, release-evidence and site-build checks.
- No experimental `v0.0.6-alpha.5` APIs such as `pearson_correlation` or nucleus selection may be documented as published in `v0.0.6-alpha.4`.
- Release publication, branch merges and GitHub Pages deployments remain separate gated actions. A migration PR does not authorize a new compiler prerelease.
