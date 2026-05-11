# Wave Report: semantic-proof-gate

**Task file:** wave.md | **Started:** 2026-05-11
**Total tasks:** 20 plus final gate -> 0 via /jc + 9 pipelines | **Waves:** 9 sequential dependency groups

## JC Pre-flight

No tasks routed to `/jc`. All tasks touch code logic, tests, new files, dependency-sensitive behavior, or broad subsystem contracts.

## Grouping Summary

| Pipeline | Tasks included | Routing |
|---|---|---|
| `semantic-embedder-authority` | 1 | embedder, config, store, MCP status |
| `rust-runtime-authority` | 2 | docs, configs, benchmark scripts, runtime guard |
| `search-contract-foundation` | 3, 4, 5 | store, search, response contracts, tests |
| `search-inspect-evidence` | 6, 7, 8, 9 | search engine, MCP server, inspect/evidence tools |
| `relatable-index-signals` | 10, 11, 12 | parser/indexer/store/search signals |
| `state-flow-neighborhoods` | 13 | parser/indexer/store/search graph signals |
| `ranking-trust-regressions` | 14, 15, 16, 17 | ranking, budgets, trust metadata, regressions |
| `corepack-benchmark-gate` | 18, 19, 20 | benchmark harness and acceptance gate |
| `corepack-head-to-head-run` | final required run | sequential Corepack grep/no-MCP vs new Loom measurement |

## Execution Plan

### Wave 1
- [x] `semantic-embedder-authority` - Fix Candle/Jina semantic backend and index freshness contracts.

### Wave 2
- [x] `rust-runtime-authority` - Remove active Python runtime assumptions and guard benchmark configs.

### Wave 3
- [x] `search-contract-foundation` - Add golden fixtures, FTS evidence, and versioned response contracts.

### Wave 4
- [x] `search-inspect-evidence` - Add exact/beyond split, compact handles, inspect, containment schemas, and evidence pack.

### Wave 5
- [ ] `relatable-index-signals` - Add behavior facts, callsites, and deterministic role cards.

### Wave 6
- [ ] `state-flow-neighborhoods` - Add lightweight data/state flow neighborhoods.

### Wave 7
- [ ] `ranking-trust-regressions` - Add budgets, staged ranking, trust coverage, and containment regression suite.

### Wave 8
- [ ] `corepack-benchmark-gate` - Add deterministic benchmark harness, metrics, and Corepack gate.

### Wave 9 - Last
- [ ] `corepack-head-to-head-run` - Run the final sequential Corepack gate and write artifacts under `tmp/benchmark/corepack-gate/`.

## Pipeline Updates

### semantic-embedder-authority - 2026-05-11

- Implemented a Loom-local Candle loader for the current Jina v2 code checkpoint key layout.
- Verified default fresh-target `reindex` with Candle/Jina: 1 indexed file, 2 symbols, 2 embeddings, 0 errors.
- Verified status fields: backend `candle`, degraded `false`, model `jinaai/jina-embeddings-v2-base-code`, dimensions `768`.
- Workspace gates passed:
  - `cargo build --workspace`
  - `cargo test --workspace`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all -- --check`
- Git phases were skipped for this pipeline because the active run explicitly requested no commits and no push.

### rust-runtime-authority - 2026-05-11

- Declared Rust `loom-mcp` as the active runtime in `docs/dev/runtime-contract.md`, with MCP JSON, tool inputs, status fields, scoring semantics, `.loom/loom.db`, schema version, and benchmark metric contracts.
- Updated active benchmark manuals and local benchmark helpers away from `python -m loom` and flat `.loom.db` assumptions.
- Added a runtime authority guard in `crates/loom-core/tests/runtime_authority.rs`.
- Labeled the Python/Rust comparison artifact as historical research.
- Verification passed:
  - `cargo test -p loom-core --test runtime_authority`
  - `cargo test -p loom-mcp status_opens_db_without_loading_embedder`
  - benchmark helper shell/Python syntax checks
  - retired runtime reference scan over active docs/configs/scripts
  - `cargo build --workspace`
  - `cargo test --workspace`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all -- --check`
- Git phases were skipped for this pipeline because the active run explicitly requested no commits and no push.

### search-contract-foundation - 2026-05-12

- Added versioned response contracts for search, related, impact, neighborhood, and reserved inspect/evidence-pack shapes.
- Added stable `symbol:{index_revision}:{symbol_id}` handles and an index revision hash from indexed store facts.
- Extended FTS retrieval with bounded lexical evidence: snippet, matched text, rank, field, reason, match kind, and sanitized query.
- Split search output into `exact_hits` and `beyond_grep`, with deterministic de-duplication and reason codes across lexical, semantic, and graph candidates.
- Added golden fixtures for exact hits, semantic/graph beyond-grep hits, duplicate removal, empty lexical fallback, ordering stability, anchors, and JSON shape.
- Documented the intentional response-shape break in `docs/dev/runtime-contract.md`.
- Verification passed:
  - `cargo test -p loom-core --test search`
  - `cargo test -p loom-core --test foundation`
  - `cargo test -p loom-mcp`
  - `cargo build --workspace`
  - `cargo test --workspace`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all -- --check`
- Git phases were skipped for this pipeline because the active run explicitly requested no commits and no push.

### search-inspect-evidence - 2026-05-12

- Extended search-family responses with compact handles, file handles, ranks, anchors, one-line summaries, reason codes, and budget metadata.
- Added read-only `inspect` workflow for symbol/file handles with stale-handle guidance, path containment, bounded snippets, citable file/line anchors, and pagination metadata.
- Added read-only `evidence_pack(query, budget_tokens)` workflow with exact hits, beyond-grep findings, graph/semantic evidence, inspected snippets, coverage checklist, omissions, truncation, and missing concepts.
- Rewrote MCP surface descriptions and schemas around the intended sequence: search first, inspect selected handles, evidence pack before final answer, shell last resort.
- Updated runtime contract docs to state the compact response shape and avoid claiming whole-file grep equivalence while exact matching remains symbol-FTS based.
- Verification passed:
  - `cargo test -p loom-core --test search`
  - `cargo test -p loom-core --test foundation`
  - `cargo test -p loom-mcp`
  - `cargo build --workspace`
  - `cargo test --workspace`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all -- --check`
- Git phases were skipped for this pipeline because the active run explicitly requested no commits and no push.

## Pre-flight Notes

- Root `wave.md` is professor-refined and already ordered.
- Existing anchors verified across `crates/loom-core`, `crates/loom-mcp`, docs, and command manuals.
- The worktree was dirty at wave start; the wave must preserve unrelated existing modifications.
- No push is permitted. Local commits are not assumed unless explicitly requested by the active pipeline contract and repo rules allow it.
