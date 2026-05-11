# Architecture — rust-productization

## Search

`loom-core::search` stays synchronous and has no MCP dependencies. `scoring.rs` owns pure signal math, while `engine.rs` orchestrates store, embedder, graph, and config.

## MCP/CLI

`loom-mcp` is the product boundary. It owns `clap` CLI parsing, rmcp tool registration, and lazy runtime state:

- `status` opens config/DB/graph only.
- `search`, `related`, `impact`, `neighborhood`, and `reindex` lazily load and cache `CandleEmbedder`.
- `reindex` is guarded by a single-flight mutex and refreshes graph state after indexing.

## Distribution

Cargo package metadata and cargo-dist targets live at workspace root. Maturin metadata is present without replacing the current Hatch Python build backend yet, so existing Python package workflows remain intact while the Rust binary packaging path is documented/configured.
