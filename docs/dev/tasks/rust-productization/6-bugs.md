# QA / Audit Bugs — rust-productization

## Fixed

- Sanitized MCP error responses so private paths, SQL internals, git stderr, model cache paths, and parser errors are logged server-side but not returned to clients.
- Added MCP request bounds for query, symbol, file, kind, and limit.
- Added an embedding model allowlist; custom model repos now require `allow_custom_embedding_model = true`.
- Added unresolved-by-name impact fallback for partially resolved indexes.
- Enforced `related()` coupling threshold for structural results.
- Over-fetch kind-filtered search candidates to reduce post-truncation false empties.
- Made DB/graph server state lazy on first tool/CLI use rather than stdio server construction.
- Switched PyPI packaging to a real maturin binary build while preserving Python source inclusion for tests.
- Added wheel artifact upload to the release workflow.
- Aligned Cargo metadata and README repository URLs.

## Deferred Follow-Ups

- Rust vector search still uses the current blob-vector full scan backend; sqlite-vec/ANN wiring should be a dedicated store/search pipeline.
- Server startup does not auto-index or start a watcher. Users must call `reindex`; watcher lifecycle should be a dedicated product behavior decision.
