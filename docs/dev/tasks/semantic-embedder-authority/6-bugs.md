# Bugs - semantic-embedder-authority

## Test Files

- `crates/loom-core/tests/embedder.rs`
- `crates/loom-core/tests/indexer_pipeline.rs`
- `crates/loom-mcp/src/server.rs`

## Findings

No open bugs found.

## Verification

- `cargo test -p loom-core --test embedder` - PASS
- `cargo test -p loom-core --test indexer_pipeline full_index_rebuilds_vectors_when_embedding_fingerprint_changes` - PASS
- `cargo test -p loom-mcp status_opens_db_without_loading_embedder` - PASS
- `cargo test -p loom-mcp changed_paths_helper_indexes_incrementally_and_refreshes_graph` - PASS
- `LOOM_LIVE_JINA_SMOKE=1 cargo test -p loom-core --test embedder live_jina_candle_smoke_when_enabled -- --nocapture` - PASS
- `cargo run -p loom-mcp -- reindex --target tmp/semantic-embedder-authority-fresh` - PASS
- `cargo run -p loom-mcp -- status --target tmp/semantic-embedder-authority-fresh` - PASS

QA complete. Result: PASS
