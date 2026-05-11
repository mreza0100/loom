> Author: planner

# Plan - semantic-embedder-authority

## Feature Context

The default Rust embedding path must use `jinaai/jina-embeddings-v2-base-code` through Candle without silently dropping into hashing. The observed failure is a model weight lookup mismatch: upstream Candle asks for `encoder.layer.0.mlp.gated_layers.weight`, while the downloaded Jina checkpoint carries `encoder.layer.0.mlp.up_gated_layer.weight` plus related `down_layer` and layer norm tensors.

## Current State

- `LoomConfig` already defaults to Candle, the Jina code model, 768 dimensions, and `allow_hashing_embedder_fallback = false`.
- `DefaultEmbedder` already fails strictly by default and only degrades to hashing when the config explicitly permits it.
- `IndexPipeline` already fingerprints files with the actual embedder fingerprint and `LoomDb::file_index_is_fresh` rejects mismatched fingerprints.
- MCP status exposes vector backend, embedder backend, and degraded state, but not model or dimensions.
- CLI `status` currently prints store stats only, so it does not expose the embedding backend contract.

## Gaps & Needed Changes

- Replace the incompatible upstream `candle_transformers::models::jina_bert::BertModel` construction path with a Loom-local Jina v2 model loader matching current safetensor keys:
  - `mlp.up_gated_layer.weight`
  - `mlp.down_layer.{weight,bias}`
  - `attention.self.layer_norm_{q,k}`
  - `layer_norm_{1,2}`
- Build ALiBi bias for the actual sequence length during inference rather than preallocating the full 8192-token square at model load.
- Pass an attention mask into the Jina model so padded tokens do not participate in attention.
- Add model and dimension fields to status responses and make CLI `status` use the same status surface.
- Add a network-gated live smoke test for the real Jina model path.

## Integration Surface

- `crates/loom-core/src/embedder.rs`
- `crates/loom-core/src/jina_bert.rs`
- `crates/loom-core/src/lib.rs`
- `crates/loom-core/tests/embedder.rs`
- `crates/loom-mcp/src/main.rs`
- `crates/loom-mcp/src/server.rs`

## Risks & Dependencies

- Live Jina smoke testing is large and network/model-cache dependent, so it must be gated by an explicit environment variable.
- Candle Metal may be unavailable locally; existing behavior should fall back to CPU with a warning.
- The local Jina model implementation must stay private to Loom to avoid creating a public API promise around model internals.

## Research Needed

No model replacement is planned. The current Jina checkpoint is compatible once Loom follows the current Jina v2 key layout and forward pass.

Analysis complete.
