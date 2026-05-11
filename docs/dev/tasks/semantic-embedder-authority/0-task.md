# Pipeline: semantic-embedder-authority

Wave: `semantic-proof-gate`

## Task 1 - Fix Rust Candle/Jina semantic embedder path

Repair the Rust default embedding backend so `jina-embeddings-v2-base-code` loads and indexes on a fresh target without hashing fallback and without the observed tensor-name/layout failure:

- Observed bad lookup: `encoder.layer.0.mlp.gated_layers.weight`.
- Actual Jina safetensor keys include names such as `up_gated_layer`.

## Required Behaviors

- Default `cargo run -p loom-mcp -- reindex --target <fresh-corepack-clone>` succeeds with Candle/Jina.
- Hashing fallback runs only when configured.
- Status exposes backend, degraded state, model, and dimensions.
- Index fingerprints invalidate stale or mixed embedding spaces.
- Tests cover strict failure, configured fallback, stale-vector invalidation, and a network-gated live semantic smoke test.

## Boundaries

- Do not choose a new embedding model unless the current Jina model is proven incompatible and the evidence is documented.
- Rust is the production runtime; do not paper over semantic failures with degraded vectors.

## Verification

- Run focused embedder/indexer/status tests first.
- Run the workspace gates required by the pipeline when feasible:
  - `cargo build --workspace`
  - `cargo test --workspace`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all -- --check`

