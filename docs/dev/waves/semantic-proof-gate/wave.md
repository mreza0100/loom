# Wave: semantic-proof-gate

Source task file: `wave.md` at repo root.

This wave implements the 2026-05-11 professor-refined plan in dependency order:

1. Fix the live Rust Candle/Jina semantic embedder path.
2. Remove stale active Python runtime assumptions.
3. Establish search contract fixtures, FTS evidence, and versioned response handles.
4. Add exact/beyond buckets, compact inspect workflow, MCP containment text, and evidence packs.
5. Add behavior facts, callsite records, and role cards.
6. Add lightweight data/state-flow neighborhoods.
7. Add payload budgets, staged ranking, trust coverage, and containment regressions.
8. Add deterministic Corepack benchmark harness, shell-escape metrics, and acceptance gate.
9. Run the final sequential Corepack grep/no-MCP versus new-Loom head-to-head gate.

## Pipeline Grouping

| Pipeline | Tasks | Dependency note |
|---|---:|---|
| `semantic-embedder-authority` | 1 | Must pass before product-claim work. |
| `rust-runtime-authority` | 2 | Must pass before benchmark machinery relies on runtime paths. |
| `search-contract-foundation` | 3, 4, 5 | Locks fixtures, lexical evidence, and response contracts together. |
| `search-inspect-evidence` | 6, 7, 8, 9 | Builds the visible exact/beyond workflow and proof tools. |
| `relatable-index-signals` | 10, 11, 12 | Adds behavior facts, callsites, and role cards on stable contracts. |
| `state-flow-neighborhoods` | 13 | Depends on stable fact/callsite ids. |
| `ranking-trust-regressions` | 14, 15, 16, 17 | Depends on handles, evidence packs, and new signals. |
| `corepack-benchmark-gate` | 18, 19, 20 | Builds the repeatable benchmark harness and gate. |
| `corepack-head-to-head-run` | final run | Executes the final sequential benchmark under `tmp/benchmark/corepack-gate/`. |

## Pre-flight

- Existing anchors found in `crates/loom-core/src/embedder.rs`, `crates/loom-core/src/config.rs`, `crates/loom-core/src/store/`, `crates/loom-core/src/search/`, and `crates/loom-mcp/src/server.rs`.
- Existing docs and task archives already contain Rust runtime and benchmark references.
- New names such as `exact_hits`, `beyond_grep`, `inspect`, and `evidence_pack` are intended additions, not missing existing entities.
- Worktree is already dirty before wave start; preserve existing changes and do not revert unrelated user state.
- No JC pre-flight tasks: every task has code logic, tests, new files, dependencies, or broad subsystem impact.

