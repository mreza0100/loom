# Architecture - semantic-embedder-authority

## Goals

- Make Candle/Jina the working default semantic embedding backend.
- Keep hashing as an explicit configuration choice or explicit degraded fallback only.
- Expose enough runtime status to prove which embedding space is active.
- Preserve index freshness by comparing actual embedder fingerprints before skipping files.

## File Responsibilities

- `crates/loom-core/src/jina_bert.rs`: private Candle implementation for the Jina v2 code model shape used by `jinaai/jina-embeddings-v2-base-code`.
- `crates/loom-core/src/embedder.rs`: model download/cache boundary, tokenizer batching, attention masks, pooling, normalization, and `DefaultEmbedder` selection.
- `crates/loom-mcp/src/server.rs`: structured status response including backend, degraded state, model, and dimensions.
- `crates/loom-mcp/src/main.rs`: CLI status should emit the same status contract as the MCP status tool.
- `crates/loom-core/tests/embedder.rs` and `crates/loom-mcp/src/server.rs` tests: strict/fallback/status coverage plus live smoke gate.

## Data Model / API Changes

- Add `embedder_model: Option<String>` and `embedder_dimensions: Option<usize>` to `StatusResponse`.
- Preserve the existing `index_meta.embedding_fingerprint` schema and actual embedder fingerprint format.
- Keep the local Jina model module private to `loom-core`.

## Algorithms

- Load safetensors through `VarBuilder::from_mmaped_safetensors`.
- Instantiate a Jina v2 encoder using current checkpoint names:
  - q/k post projection layer norms under `attention.self.layer_norm_q` and `attention.self.layer_norm_k`.
  - GLU projection under `mlp.up_gated_layer`.
  - MLP output under `mlp.down_layer`.
  - residual norms under `layer_norm_1` and `layer_norm_2`.
- Build ALiBi dynamically for the current batch sequence length and broadcast it into attention scores.
- Convert tokenizer masks into additive attention masks, keeping valid tokens at `0.0` and padded tokens at a large negative score.
- Mean-pool only valid token vectors and L2-normalize final embeddings.

## Test Plan

- Focused embedder tests:
  - hashing mode skips Candle.
  - Candle failure remains strict by default.
  - configured fallback reports degraded hashing and a distinct fingerprint.
  - live Jina smoke test runs only with `LOOM_LIVE_JINA_SMOKE=1`.
- Focused indexer tests:
  - stale vectors rebuild when the embedder fingerprint changes.
- Status tests:
  - status exposes backend, degraded state, model, and dimensions before and after embedder initialization.
- Workspace gates:
  - `cargo build --workspace`
  - `cargo test --workspace`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all -- --check`

## Risks

- The live smoke test is intentionally opt-in because first-run model download is large and network-sensitive.
- Future upstream Jina checkpoint layout changes should fail loudly in Candle load instead of falling back silently.
