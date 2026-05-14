# Loom Super Upgrade Wave

Refined from `tmp/competitors/loom-competitors-aggregate-2026-05-11.md`, the latest Professor gap analysis, and the RND3 Corepack benchmark.

## Goal

Upgrade Loom's product surface so a benchmark agent can beat grep on more than 60% of comparable BM metrics, not only on useful symbols per token.

Comparable metrics exclude tool-type artifacts such as "MCP calls = 0" for grep and focus on result quality, token cost, runtime, shell containment, payload size, and resource use.

## Pipeline: `super-upgrade-retrieval`

### Task 1 - Exact Symbol Enumeration Tool

Add a first-class read-only `symbols` surface for exact symbol enumeration.

Requirements:

- MCP tool `symbols` with `query`, optional `file_prefix`, optional `kind`, and bounded `limit`.
- CLI subcommand `loom-mcp symbols`.
- Response contract must return compact handles, anchors, summaries, reason codes, budget metadata, and truncation.
- It must solve same-name/suffix enumeration such as command `execute` methods without broad shell grep.
- It must avoid embedding/model loading when practical.
- Tests must cover suffix method enumeration and file-prefix filtering.

### Task 2 - Compact Expansion And Inspection

Make expansion tools harder to bloat.

Requirements:

- `related`, `impact`, and `neighborhood` must cap results with truthful truncation and omitted counts.
- `inspect` defaults/caps must prefer smaller snippets while preserving pagination.
- MCP descriptions must explicitly route agents to `symbols` for enumeration and small `inspect` calls for citations.
- Runtime contract docs must describe the new caps and `symbols` tool.

### Task 3 - Benchmark Metric Gate

Make BM/RND evaluate the user's actual bar.

Requirements:

- Add a benchmark comparison helper under `tmp/benchmark/scripts/` that reads latest BM artifacts and computes metric winners.
- It must report the percentage of comparable metrics where Loom beats grep.
- Write/update readable benchmark reports under `tmp/benchmark/`.
- After implementation gates pass, run `$bm` with fresh Corepack clones.
- If Loom wins more than 60% of comparable metrics, stop. Otherwise continue RND iterations against the measured largest gap.

## Acceptance Gates

- `cargo fmt --all -- --check`
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- BM comparison with metric-win percentage under `tmp/benchmark/`

## Non-Goals

- No cloud defaults.
- No mutating search tools.
- No full Zoekt/trigram engine in this wave.
- No LSP adapter in this wave.
- No commits or pushes unless explicitly requested.
