# Dev Report - search-inspect-evidence

## Implementation Summary

- Extended compact search-family result contracts with ranks, file handles, anchors, summaries, reason codes, and `budget` metadata.
- Kept exact/beyond buckets from `search-contract-foundation` and made serialized hits handle-first by skipping internal full `Symbol` payloads in JSON.
- Added reversible file handles in the format `file:{index_revision}:{hex_repo_relative_path}` alongside existing stable symbol handles.
- Added `SearchEngine::inspect` for bounded read-only source snippets with stale-handle responses, path containment under `target_dir`, line/character budgets, and pagination metadata.
- Added `SearchEngine::evidence_pack` to orchestrate compact search buckets, graph/semantic findings, inspected snippets, coverage checklist, omitted/truncated metadata, and missing concepts.
- Registered read-only MCP `inspect` and `evidence_pack` tools with action-oriented descriptions and schema validation.
- Updated `docs/dev/runtime-contract.md` to document compact default responses, inspect workflow, evidence packs, and the remaining symbol-FTS limitation.

## Test Coverage

- Added core tests for compact hit metadata, symbol handle inspection, file handle inspection, stale handle guidance, budgeted snippets, and evidence pack output.
- Updated JSON compatibility coverage to assert serialized hits expose handles/anchors and do not include internal `symbol` payloads.
- Updated MCP tests to assert `inspect` and `evidence_pack` registration, read-only annotations, containment descriptions, and schema descriptions.

## Runbook

Focused checks:

```bash
cargo test -p loom-core --test search
cargo test -p loom-core --test foundation
cargo test -p loom-mcp
```

Workspace gates:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

Git phases:

- MERGE skipped: active run explicitly requested no commits.
- DOCS-COMMIT skipped: active run explicitly requested no commits.
- PUSH skipped: active run explicitly requested no push.
