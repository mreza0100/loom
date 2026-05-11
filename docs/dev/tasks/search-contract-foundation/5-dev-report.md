# Dev Report - search-contract-foundation

## Implementation Summary

- Added versioned response contract structs for search, related, impact, neighborhood, and reserved inspect/evidence-pack responses in `crates/loom-core/src/models.rs`.
- Added stable symbol handles in the format `symbol:{index_revision}:{symbol_id}` and an index revision hash derived from indexed store facts.
- Extended FTS retrieval with `search_fts_with_evidence`, returning bounded snippets, matched text, rank, field, reason, match kind, and sanitized query.
- Hardened FTS query sanitization for quoted phrases, punctuation-heavy identifiers, reserved FTS operators, and punctuation-only empty queries.
- Changed `SearchEngine::search` to return `SearchResponse` with `exact_hits` and `beyond_grep` buckets, deterministic de-duplication, reason codes, graph-only candidates, and compact coupled handles.
- Wrapped `related`, `impact`, and `neighborhood` in named versioned response structs.
- Updated MCP serialization to return the new named response objects, while leaving `inspect` and `evidence_pack` unregistered for later pipelines.
- Documented the intentional response-shape break in `docs/dev/runtime-contract.md`.

## Test Coverage

- Added/updated store tests for quoted strings, punctuation-heavy identifiers, field evidence, empty sanitized queries, rank ordering, and bounded lexical evidence.
- Added a golden search fixture covering exact hits, semantic beyond-grep hits, graph beyond-grep hits, duplicate candidate removal, reason codes, file/line anchors, ordering stability, and empty lexical results with semantic fallback.
- Added JSON shape coverage for the versioned search response contract.
- Updated MCP tool registration tests to assert `inspect` and `evidence_pack` are not exposed in this pipeline.

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
