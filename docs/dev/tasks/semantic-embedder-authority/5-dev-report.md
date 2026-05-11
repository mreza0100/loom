# Dev Report - semantic-embedder-authority

## Implementation Summary

- Added a Loom-local Candle Jina v2 model implementation in `crates/loom-core/src/jina_bert.rs`.
- Switched `CandleEmbedder` from upstream `candle_transformers::models::jina_bert::BertModel` to the local loader so current `jinaai/jina-embeddings-v2-base-code` safetensor keys load directly:
  - `encoder.layer.*.mlp.up_gated_layer.weight`
  - `encoder.layer.*.mlp.down_layer.{weight,bias}`
  - `encoder.layer.*.attention.self.layer_norm_{q,k}`
  - `encoder.layer.*.layer_norm_{1,2}`
- Added additive attention masking for tokenizer padding.
- Built ALiBi bias per actual sequence length during inference instead of preallocating the configured 8192-token square at model load.
- Extended status output with `embedder_model` and `embedder_dimensions`.
- Changed CLI `status` to return the same structured status surface as the MCP status tool.
- Added a gated live Jina smoke test behind `LOOM_LIVE_JINA_SMOKE=1`.

## Test Coverage

- Strict Candle failure remains fatal by default.
- Explicit hashing fallback remains degraded and fingerprint-distinct.
- Hashing mode still avoids Candle initialization.
- Indexer stale-vector invalidation still rebuilds when embedder fingerprint changes.
- MCP status tests cover backend/degraded/model/dimensions.
- Live Jina smoke test was run with `LOOM_LIVE_JINA_SMOKE=1` and passed.
- Default fresh-target `reindex` was run against `tmp/semantic-embedder-authority-fresh` and produced 2 Candle/Jina embeddings without fallback.

## Runbook

Focused checks:

```bash
cargo test -p loom-core --test embedder
cargo test -p loom-core --test indexer_pipeline full_index_rebuilds_vectors_when_embedding_fingerprint_changes
cargo test -p loom-mcp status_opens_db_without_loading_embedder
cargo test -p loom-mcp changed_paths_helper_indexes_incrementally_and_refreshes_graph
LOOM_LIVE_JINA_SMOKE=1 cargo test -p loom-core --test embedder live_jina_candle_smoke_when_enabled -- --nocapture
cargo run -p loom-mcp -- reindex --target tmp/semantic-embedder-authority-fresh
cargo run -p loom-mcp -- status --target tmp/semantic-embedder-authority-fresh
```

Workspace gates:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

Git phases:

- MERGE skipped: user explicitly requested no commits for this run.
- DOCS-COMMIT skipped: user explicitly requested no commits for this run.
- Push skipped: user explicitly requested no push for this run.
