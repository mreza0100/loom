# Task: rust-runtime-retrieval-fixes

Fix all actionable findings from the Professor review of the Rust implementation:

1. Replace the current brute-force BLOB-only vector search path with a production-oriented design that supports a real sqlite-vec backend or an explicit backend abstraction suitable for it, while keeping tests deterministic.
2. Remove silent semantic-quality degradation: Candle model initialization failure must not quietly become hashing embeddings unless explicitly configured, and degraded/fallback mode must be visible in status.
3. Wire the Rust watcher into the Rust MCP server so file changes trigger incremental indexing and graph refresh.
4. Derive default indexed extensions and excluded directories from the registered language adapters instead of drifting hard-coded config lists.
5. Make evolutionary recency participate in scoring, or remove/avoid misleading storage if it remains unused.
6. Add durable schema versioning/migration structure before further `.loom/loom.db` drift.
7. Preserve existing Rust public behavior where reasonable, update tests, and keep `cargo test --workspace`, `cargo fmt --all -- --check`, and `cargo clippy --workspace --all-targets -- -D warnings` passing.

Wave: none
