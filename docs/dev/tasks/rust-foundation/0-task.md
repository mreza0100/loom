> Author: wave

# Task: rust-foundation

Wave: rust-rewrite

Source task file: `wave.md`

## Scope

Implement the Rust foundation for Loom: Cargo workspace, shared core types/config/errors, SQLite persistence, and graph traversal.

## Included Wave Tasks

### 1. Cargo workspace + core types + config

Initialize a Rust Cargo workspace with:
- `loom-core`: shared types, config, errors.
- `loom-mcp`: server binary crate shell.

Define Rust structs/enums mirroring Python `src/loom/store/models.py` and search result shapes:
- `Symbol`
- `Edge`
- `ParsedEdge`
- `CoupledSymbol`
- `SearchResult`
- `CouplingScore`

Use `serde` derives. Implement `LoomConfig` loaded from `.loom/config.toml`, preserving defaults from `src/loom/config.py`: embedding model, coupling weights, thresholds, target/index paths, file watching and git analysis settings. Missing config must fall back to defaults. Use `thiserror` for library errors and `anyhow` at binary boundaries.

Boundaries: no parser, embedding, database logic beyond config path helpers, or MCP behavior.

### 2. SQLite store with rusqlite

Port `src/loom/store/db.py` to Rust using `rusqlite` with bundled SQLite. Implement:
- Tables: `symbols`, `edges`, `index_meta`, `cochange`.
- FTS5 virtual table for keyword search.
- Vector storage/search primitive for 768-dim `f32` embeddings, with sqlite-vec if practical in this pipeline; otherwise isolate the adapter so sqlite-vec can be enabled without rewriting callers.
- WAL mode.
- One serialized writer plus pooled reader connections.
- CRUD/search methods equivalent to Python `LoomDB`: `insert_symbol`, `insert_edge`, `insert_embedding`, `search_fts`, `search_vectors`, `remove_file`, `get_edges_from`, `get_edges_to`, `get_unresolved_edges`, `resolve_edge`, index metadata, co-change methods, and fuzzy symbol lookup.

Key behavior: schema auto-creates under `.loom/`, FK cascades remove file-owned rows, bulk writes use transactions, concurrent readers do not block each other.

Boundaries: no parser, graph algorithms, search fusion, or embedding generation.

### 3. Graph engine with petgraph

Port `src/loom/store/graph.py` to Rust using `petgraph`, preferring CSR where practical. Implement:
- graph rebuild from SQLite edges
- symbol-id to node mapping
- BFS/DFS traversal with max depth
- `impact_radius`
- `neighborhood`
- degree centrality or hub scoring
- graceful handling of unresolved/terminal edges

Key behavior: depth-limited traversal returns depth-decayed confidence scores and is safe on empty graphs or missing symbols.

Boundaries: no vector similarity, no final coupling scorer/search engine.

## Required Verification

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`

