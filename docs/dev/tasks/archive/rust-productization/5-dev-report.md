# Dev Report — rust-productization

## Implemented

- Added Rust hybrid search and scoring:
  - RRF `k=60`
  - kind boosts
  - structural, semantic, evolutionary signal fusion
  - `search`, `related`, `impact`, and `neighborhood`
- Added configurable `coupling_threshold` and `top_coupled`.
- Replaced the `loom-mcp` shell with:
  - `clap` CLI
  - `status` and `reindex` subcommands
  - rmcp stdio server with six tools
  - lazy cached Candle embedder state
  - graph refresh after reindex
- Added cargo-dist metadata, maturin metadata, and release workflow.

## Tests Added

- Rust search/scoring parity tests.
- MCP tool registration and status-without-embedder tests.

## Notes

- Rust vector search currently uses the existing blob vector store, not sqlite-vec. The `VectorStore` trait remains the slot for a later sqlite-vec backend.
- `status` intentionally avoids model loading; search/reindex load the model on first use.
