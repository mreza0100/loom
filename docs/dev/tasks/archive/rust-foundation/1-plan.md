> Author: planner

# Plan — rust-foundation

## Feature Context
Implement the Rust foundation for the `rust-rewrite` wave: a Cargo workspace, shared Loom types/config/errors, SQLite persistence primitives, and graph traversal equivalents for the current Python store/search foundation.

## Current State
- `src/loom/config.py` defines the Python runtime defaults: `target_dir`, `.loom/loom.db`, parser-driven watch extensions/excluded dirs, `debounce_seconds=2.0`, `embedding_model="jinaai/jina-embeddings-v2-base-code"`, `embedding_dimensions=768`, `max_file_size_bytes=512_000`, coupling weights `0.45/0.35/0.20`, and git analysis settings.
- `src/loom/store/models.py` defines the core data shapes that the Rust `loom-core` crate must mirror: `Symbol`, `ParsedEdge`, `Edge`, `FileState`, `CoupledSymbol`, and `SearchResult`. `CouplingScore` currently lives in `src/loom/search/scoring.py`, not the store model module.
- `src/loom/store/db.py` is the persistence contract to port first. It creates `symbols`, `edges`, `index_meta`, `symbols_fts`, `cochange`, and `vec_symbols`; enables WAL and FK enforcement; stores embeddings as 768-dim `f32` blobs; and exposes CRUD/search helpers used by the indexer and search engine.
- `src/loom/store/graph.py` wraps a NetworkX `DiGraph` over resolved edges only. It rebuilds from DB rows where `target_id IS NOT NULL`, collapses duplicate `(source_id, target_id)` edges by highest confidence, and provides dependents, dependencies, shortest path, `impact_radius`, centrality, and bidirectional neighbor traversal.
- `src/loom/search/scoring.py` defines the score math that Rust types and graph tests should preserve where relevant: relationship weights, depth decay, semantic distance conversion, evolutionary frequency normalization, and weighted signal fusion.
- `src/loom/server.py` exposes the current MCP surface: `search`, `related`, `impact`, `neighborhood`, `reindex`, and `status`. This pipeline should not port MCP behavior, but its output model names and status fields should remain compatible.
- `pyproject.toml` shows current Python dependencies: FastMCP, tree-sitter grammars, sqlite-vec, watchdog, fastembed, and NetworkX. Rust dependencies should be introduced in Cargo manifests, not by changing Python packaging in this foundation pass.
- No Rust workspace currently exists in the repo; there are no `Cargo.toml` or `.rs` files at the expected project roots.

## Gaps & Needed Changes
- Add root `Cargo.toml` as a Cargo workspace with members:
  - `crates/loom-core`
  - `crates/loom-mcp`
- Add `crates/loom-core/Cargo.toml` with foundational dependencies:
  - `serde` with `derive`
  - `thiserror`
  - `toml`
  - `time` or `chrono` for index metadata timestamps
  - `rusqlite` with `bundled`, `functions`, and FTS-capable SQLite build support
  - `r2d2` and `r2d2_sqlite` for reader pooling
  - `parking_lot` or standard `Mutex` for the serialized writer
  - `petgraph`
  - `tracing`
  - `sqlite-vec` if it integrates cleanly in this pass; otherwise hide vector operations behind an adapter trait with a brute-force/blob fallback.
- Add `crates/loom-mcp/Cargo.toml` as a compiling binary shell with `anyhow`, `tracing-subscriber`, and a dependency on `loom-core`. Do not implement rmcp tools in this pipeline.
- Add `crates/loom-core/src/lib.rs` exporting modules:
  - `config`
  - `error`
  - `models`
  - `store`
  - `graph`
  - optional `scoring` if `CouplingScore` and score helpers live in core for later search reuse.
- Add `crates/loom-core/src/models.rs`:
  - `Symbol { id: Option<i64>, name, kind, file, line, end_line, language, context }`
  - `ParsedEdge { source_name, target_name, relationship, target_file }`
  - `Edge { id, source_id, target_id, target_name, target_file, relationship, confidence, original_name }`
  - `FileState { path, content_hash, last_indexed }`
  - `CouplingScore { structural, semantic, evolutionary, combined }`
  - `CoupledSymbol { symbol, score, reason }`
  - `SearchResult { symbol, score, coupled }`
  Use `serde::{Serialize, Deserialize}` derives and integer IDs as `i64` to match SQLite row IDs.
- Add `crates/loom-core/src/config.rs`:
  - `LoomConfig` with Python-equivalent defaults.
  - `LoomConfig::load(target_dir)` that reads `.loom/config.toml` if present and falls back field-by-field to defaults.
  - `resolve_db_path()` that creates `.loom/` and returns `target_dir.join(db_path)`.
  - Validate coupling weights are non-negative and warn or error if the active total is invalid; the Python code assumes sane values, Rust should make bad TOML obvious.
  - Preserve `.git`, `__pycache__`, and `.loom` as always-excluded directories. Until Rust parser registry exists, hard-code the current primary extension/default exclusion set or keep it configurable.
- Add `crates/loom-core/src/error.rs`:
  - `LoomError` via `thiserror` for config IO/TOML, DB, vector dimension mismatch, missing connection/resource, graph lookup, and invalid input.
  - Type alias `pub type Result<T> = std::result::Result<T, LoomError>`.
  - Keep `anyhow` out of `loom-core`; use it only in `loom-mcp`.
- Add `crates/loom-core/src/store.rs` or `store/mod.rs`:
  - `LoomDb` with one serialized writer connection and pooled read connections.
  - `connect/open` creates parent `.loom/`, enables `PRAGMA journal_mode=WAL`, `PRAGMA synchronous=NORMAL`, and `PRAGMA foreign_keys=ON` on every connection.
  - Create schema equivalent to Python, including `symbols_fts` and `cochange`.
  - Prefer FK behavior over manual cleanup: `edges.source_id REFERENCES symbols(id) ON DELETE CASCADE`; `edges.target_id REFERENCES symbols(id) ON DELETE SET NULL`.
  - Implement `insert_symbol`, `insert_edge`, `insert_embedding`, `search_fts`, `search_vectors`, `get_symbol_by_id`, `get_symbol_by_name`, `get_symbol_by_name_fuzzy`, `get_colocated_symbols`, `remove_file`, `get_edges_from`, `get_edges_to`, `get_edges_to_by_name`, `get_unresolved_edges`, `resolve_edge`, `remove_edges_for_source`, `get_file_hash`, `set_file_hash`, `upsert_cochange`, `get_cochange_frequency`, `get_top_cochanges`, and `get_stats`.
  - Preserve Python FTS sanitization behavior for special tokens such as `AND`, `OR`, `NOT`, `NEAR`, and punctuation-heavy names.
  - Vector storage must enforce `embedding_dimensions == 768` by default and return a typed error on mismatch. Store vectors as raw little-endian `f32` blobs if sqlite-vec cannot be made reliable in this first foundation slice.
  - Bulk insert methods should wrap symbols, edges, FTS rows, and embeddings in transactions for future indexer use.
- Add vector adapter boundary:
  - `VectorStore` trait with `create_schema`, `insert_embedding`, and `search`.
  - `SqliteVecVectorStore` implementation if `sqlite-vec` is practical with statically loaded extension support.
  - `BlobVectorStore` fallback using SQL row scan plus Rust cosine/L2 helper for tests and future sqlite-vec swap-in. Yes, brute force is ugly; ugly that compiles is still a foundation.
- Add `crates/loom-core/src/graph.rs`:
  - Build a petgraph graph from all resolved DB edges.
  - Maintain `symbol_id -> NodeIndex` and `NodeIndex -> symbol_id` mappings.
  - Deduplicate duplicate source/target pairs by highest confidence and preserve relationship metadata from the winning edge.
  - Implement `dependents(symbol_id, max_depth)`, `dependencies(symbol_id, max_depth)`, `shortest_path(source_id, target_id)`, `impact_radius(symbol_id, max_depth)`, `centrality(top_n)`, and `neighbors_with_metadata(symbol_id, max_depth)`.
  - Use depth-decayed confidence exactly like Python: `confidence * (1 / 2^(depth - 1))`.
  - Return empty results for missing symbols or empty graphs; unresolved edges are terminal because they have no `target_id`.
  - Start with `petgraph::Graph` if mutable rebuild ergonomics are simpler, but isolate construction so switching to CSR is a local implementation change. The wave intent prefers CSR where practical; correctness and API stability beat premature cleverness here.
- Add focused Rust tests under `crates/loom-core/tests/` or module-local `#[cfg(test)]`:
  - config defaults and missing `.loom/config.toml` fallback
  - partial TOML override
  - model serialization/deserialization
  - DB schema creation and FK enforcement
  - symbol/edge CRUD and confidence round trips
  - unresolved edge resolution
  - `remove_file` cascades outgoing edges, nullifies incoming targets, removes FTS/vector rows, and clears index metadata
  - fuzzy lookup strategies: exact, file suffix, method suffix, underscore toggle
  - FTS special-character query sanitization
  - vector dimension mismatch and top-K search
  - cochange canonical ordering and frequency lookup
  - graph rebuild, duplicate edge confidence choice, depth-limited traversal, impact decay, centrality, self-loop/missing-node handling.

## Integration Surface
- Rust public model names should match Python/MCP concepts so later `rmcp` schemas can serialize the same shape:
  - `Symbol`
  - `Edge`
  - `ParsedEdge`
  - `CoupledSymbol`
  - `SearchResult`
  - `CouplingScore`
- Store API should be synchronous in `loom-core` because `rusqlite` is blocking. Later async MCP/indexer code can call it through `spawn_blocking` or a dedicated writer worker.
- Use relative project paths in config fields and DB rows, matching current Python behavior. `target_dir` remains the absolute anchor for resolving `.loom/loom.db`.
- `LoomConfig` must preserve these defaults for downstream parity:
  - `db_path = ".loom/loom.db"`
  - `debounce_seconds = 2.0`
  - `embedding_model = "jinaai/jina-embeddings-v2-base-code"`
  - `embedding_dimensions = 768`
  - `max_file_size_bytes = 512000`
  - `structural_weight = 0.45`
  - `semantic_weight = 0.35`
  - `evolutionary_weight = 0.20`
  - `enable_git_analysis = true`
  - `git_max_commits = 500`
  - `git_max_files_per_commit = 20`
- DB schema compatibility matters more than byte-identical SQL. Preserve logical tables, columns, constraints, and indexes so future migration/import tooling can map Python `.loom/loom.db` data if needed.
- `status`-ready stats should expose at least Python's current fields: `symbols`, `edges`, `files`, `vectors`, `last_indexed`, `stale_files`, and `cochange_pairs`.
- Do not implement parser adapters, embedding generation, search fusion, watcher behavior, git analyzer execution, or MCP tools in this pipeline. Only prepare APIs they will call.

## Risks & Dependencies
- `sqlite-vec` Rust integration may be the riskiest dependency in this slice. Keep the adapter boundary small so the developer can ship a correct blob fallback if extension loading/static registration fights back. Very noble of SQLite extensions to remind us humility exists.
- `rusqlite` connection pooling requires PRAGMAs per connection. Foreign keys being enabled only on the writer would silently break reader-side tests and future invariants.
- FTS5 support must be present in the bundled SQLite build. Verify with a schema creation test, not vibes.
- Petgraph CSR is memory-efficient but less ergonomic for incremental mutation. Since this foundation only needs rebuild-from-DB, prefer a rebuild boundary and hide the concrete graph type behind `SymbolGraph`.
- Python currently has no `.loom/config.toml` loader, only dataclass defaults. Rust is introducing config-file loading per wave spec; tests must define precedence clearly.
- Existing Python tests cannot verify Rust code. Add Rust-native tests and keep Python untouched.
- Root workspace changes can coexist with Python packaging, but avoid changing `pyproject.toml` in this pipeline unless cargo tooling forces documentation of the new Rust workspace elsewhere.
- Do not run or require git commands. The feature does not need repository history; it needs schema/API parity.

## Research Needed
- Confirm exact `sqlite-vec` crate initialization path for statically linked use with `rusqlite` and bundled SQLite.
- Confirm whether `rusqlite` bundled SQLite includes FTS5 by default in this dependency/version combination; if not, identify the feature/build flag.
- Pick `time` vs `chrono` for timestamp serialization before implementation; either is fine, but the DB surface should return strings compatible with Python's `datetime('now')` values.
- Decide whether `petgraph::csr::Csr` can preserve edge metadata cleanly enough for traversal results. If not, use `Graph` behind an API that can later swap to CSR.
- Confirm clippy settings needed for a new workspace so `cargo clippy --workspace -- -D warnings` is not blocked by default-generated binary boilerplate.
