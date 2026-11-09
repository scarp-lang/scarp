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

## Second phase — historical compiler tutorial navigation

The six older compiler tutorials now retain their original paths and sample explanations, but prominently direct readers to the maintained English/Hindi documentation and compiler-backed course. Their initial "development branch" / "latest prerelease" labels have been disambiguated with full version numbers; historical tags themselves are unchanged.

| Compiler-side tutorial retained | Canonical public guide | Runnable learning |
| --- | --- | --- |
| `docs/tutorials/first-program.md` | [Current first program, v0.0.6-alpha.4](https://scarp-lang.github.io/scarp-docs/guide/first-program-alpha4) | [01-basics](https://github.com/scarp-lang/scarp-by-example/tree/main/examples/01-basics) |
| `docs/tutorials/variables.md` | [Variables and scopes](https://scarp-lang.github.io/scarp-docs/language/variables) | Basics `bind`, `var`, scopes |
| `docs/tutorials/scopes.md` | [Lexical scopes](https://scarp-lang.github.io/scarp-docs/language/variables) | Basics, scope and shadowing |
| `docs/tutorials/decisions.md` | [Control flow](https://scarp-lang.github.io/scarp-docs/language/control-flow) | [02-control-flow](https://github.com/scarp-lang/scarp-by-example/tree/main/examples/02-control-flow) |
| `docs/tutorials/modules.md` | [Modules and visibility](https://scarp-lang.github.io/scarp-docs/language/modules-and-visibility) | [05-modules](https://github.com/scarp-lang/scarp-by-example/tree/main/examples/05-modules) |
| `docs/tutorials/numerical-program.md` | [Vectors and tiles](https://scarp-lang.github.io/scarp-docs/language/numerics), [published alpha.4 numerical guide](https://scarp-lang.github.io/scarp-docs/language/scientific-kernels-alpha4) | [04-collections](https://github.com/scarp-lang/scarp-by-example/tree/main/examples/04-collections) |

The new [English](https://github.com/scarp-lang/scarp-docs/blob/docs-migrate-numerical-guides-20261109/docs/guide/first-program-alpha4.md) and [Hindi](https://github.com/scarp-lang/scarp-docs/blob/docs-migrate-numerical-guides-20261109/docs/hi/guide/first-program-alpha4.md) first-program walkthroughs use unchanged `.srp` and stdout from the existing course (hello and bindings). Their `scarp verify` and `scarp exec` examples are release-tagged and pass the site snippet policy; the older historical `scarp check` / `scarp run` guidance stays available.

**Merge dependency:** this compiler PR references an as-yet-unpublished new documentation route. Merge documentation PR #91 and verify the deployed site first, then merge this navigation PR; do not send users to the new route on production before the docs deploy.

## New published alpha.5 release

[Scarp v0.0.6-alpha.5](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.5) was published on 9 November 2026, from exact source `205e92f166f186c3905e5880d41c31717b697b8e`. Future alpha.5 course examples must preserve this distinct provenance rather than rewriting earlier alpha.4 fixtures. Compiler regression sources remain in this repository.

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
- The now-published `v0.0.6-alpha.5` APIs (`pearson_correlation` and deterministic nucleus selection) must not be advertised as available in alpha.4. Keep all migration copies byte-pinned to the specific release from which they originated.
- Release publication, branch merges and GitHub Pages deployments remain separate gated actions. A migration PR does not authorize a new compiler prerelease.
