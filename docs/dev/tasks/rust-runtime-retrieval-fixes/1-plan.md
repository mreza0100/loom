> Author: planner

# Plan — rust-runtime-retrieval-fixes

## Feature Context
Fix the actionable Professor review findings on the Rust implementation so the Rust runtime stops pretending brute-force vectors, silent embedder fallback, unwired watching, hard-coded language config, unused recency, and ad hoc schema drift are production behavior. Charming in a prototype. Less charming in a code-intelligence tool.

## Current State
- Rust workspace lives under `Cargo.toml`, `crates/loom-core`, and `crates/loom-mcp`.
- `crates/loom-core/src/store/vector.rs` defines a useful `VectorStore` trait, but only `BlobVectorStore` exists. It stores raw embeddings in `symbol_embeddings` and full-scans every vector in Rust.
- `crates/loom-core/src/store/mod.rs` hardcodes `Arc::new(BlobVectorStore)` in `LoomDb::open()`, creates schema directly in `create_schema()`, and has one narrow migration helper, `migrate_cochange_recency()`.
- `crates/loom-core/src/embedder.rs` has `DefaultEmbedder::from_config()`, which silently downgrades any Candle initialization failure into `HashingEmbedder` after a warning. Status cannot report which backend is active.
- `crates/loom-core/src/watcher.rs` has a complete `LoomWatcher`, `Debouncer`, and `ChangeHandler` abstraction. It is tested, but not started by the Rust MCP server.
- `crates/loom-mcp/src/server.rs` lazily opens config/DB/graph, exposes `search`, `related`, `impact`, `neighborhood`, `reindex`, and `status`, and refreshes graph only after explicit full reindex.
- `crates/loom-core/src/indexer/pipeline.rs` supports both `full_index()` and `incremental_index()`. Incremental indexing resolves edges afterward, but does not update git co-change data and is not called by the server watcher.
- `crates/loom-core/src/parsers/registry.rs` can already derive `get_all_extensions()` and `get_all_excluded_dirs()` from registered adapters.
- `crates/loom-core/src/config.rs` still hardcodes `watch_extensions` and `excluded_dirs` defaults instead of deriving them from `AdapterRegistry`.
- `crates/loom-core/src/git_analyzer.rs` computes both `frequency` and `recency` for co-change pairs, and `LoomDb` stores both.
- `crates/loom-core/src/search/scoring.rs` exposes `compute_evolutionary(frequency, max_frequency)`, so recency is currently not part of scoring.
- `crates/loom-core/src/search/engine.rs` calls only `get_cochange_frequency()` through `evolutionary_score()`.
- Legacy Python reference surfaces:
  - `src/loom/store/db.py` uses sqlite-vec directly via `vec_symbols`, `embedding MATCH ?`, and `k = ?`.
  - `src/loom/indexer/embedder.py` raises on model load/embed failure instead of silently substituting hashing.
  - `src/loom/config.py` derives default extensions/exclusions from the language adapter registry.
  - `src/loom/server.py` starts a watcher during initialization and routes changes into `IndexPipeline.incremental_index()`.

## Gaps & Needed Changes
- Add a production vector backend while preserving deterministic tests:
  - Extend `crates/loom-core/src/store/vector.rs` with a `VectorBackend` enum and a `SqliteVecStore`.
  - Add `sqlite-vec` and `zerocopy` dependencies to `crates/loom-core/Cargo.toml`.
  - Register sqlite-vec for every SQLite connection before creating/querying the `vec0` table. Because `LoomDb` has one writer and a reader pool, registration must apply to writer and reader connections, not just startup.
  - Implement `SqliteVecStore` with `CREATE VIRTUAL TABLE IF NOT EXISTS vec_symbols USING vec0(embedding float[N])`, insert/delete/clear/count/search parity with Python's `vec_symbols` path.
  - Keep `BlobVectorStore` as an explicit deterministic fallback/test backend, not the production default.
  - Add a small backend introspection surface, for example `VectorStore::backend_name()` and `LoomDb::vector_backend_name()`.
- Make embedder degradation explicit:
  - Add config fields in `crates/loom-core/src/config.rs`, likely `embedding_backend` or `allow_hashing_embedder_fallback`.
  - Default should be strict Candle: Candle initialization errors return `Err`, matching Python's fail-loud behavior.
  - Allow hashing only when explicitly configured for tests/offline deterministic mode.
  - Track active embedder mode in `DefaultEmbedder` and expose it via server status.
  - Preserve direct `HashingEmbedder` tests; update `DefaultEmbedder::from_config()` tests to assert strict failure versus configured fallback.
- Wire watcher into the Rust MCP server:
  - Add watcher ownership to `CoreState` in `crates/loom-mcp/src/server.rs`, probably `watcher: Mutex<Option<LoomWatcher>>`.
  - Add a startup method that creates an `IndexPipeline` callback using the shared `db`, `embedder`, `config`, and `reindex_lock`.
  - Callback must call `incremental_index(changed_paths)`, then `refresh_graph()`.
  - Start watcher from `LoomServerState::core()` after DB/graph creation, or behind config if auto-watching must be opt-out.
  - Avoid holding graph/embedder mutexes across long indexing work. The reindex lock is the coarse gate.
  - Add MCP/server tests with a fake change handler or a state-level `handle_changed_paths()` helper so tests do not rely on OS watcher timing.
- Derive default extensions/exclusions from adapters:
  - Update `LoomConfig::default_for_target()` to call `AdapterRegistry::with_builtin_adapters().get_all_extensions()` and `.get_all_excluded_dirs()`.
  - Union parser exclusions with the always-excluded set and existing operational exclusions that are not language-owned, if still needed.
  - Update config tests in `crates/loom-core/tests/foundation.rs` so defaults are validated against registry behavior, not copied literals.
- Make recency real in scoring:
  - Replace `get_cochange_frequency()` usage in `SearchEngine::evolutionary_score()` with `get_cochange()` so both `frequency` and `recency` are available.
  - Change `compute_evolutionary()` to accept recency, for example `compute_evolutionary(frequency, recency, max_frequency)`, and combine normalized frequency with clamped recency.
  - Keep output range `[0.0, 1.0]` and keep `fuse_signals()` behavior stable.
  - Update `crates/loom-core/tests/search.rs` and `crates/loom-core/tests/git_analyzer.rs` to prove newer co-change pairs score higher when frequency is equal.
- Add durable schema versioning/migrations:
  - Introduce `crates/loom-core/src/store/migrations.rs`.
  - Use either `PRAGMA user_version` or a small `schema_migrations` table. Prefer `PRAGMA user_version` for one local SQLite DB, plus migration functions for readability.
  - Move schema creation and upgrades behind `run_migrations(conn, vector_store, dimensions)`.
  - Include migrations for current base tables, `cochange.recency`, and vector backend table setup.
  - Make migrations idempotent and safe for existing `.loom/loom.db` files created by the current Rust implementation.
  - Add tests that create an older schema without `recency` and without schema version, then open through `LoomDb::open()` and verify upgrade.
- Status surface:
  - Extend `crates/loom-core/src/models.rs::StoreStats` or `crates/loom-mcp/src/server.rs::StatusResponse` with:
    - active vector backend
    - active embedder backend/degraded mode
    - schema version
    - watcher active flag
  - Keep existing JSON fields stable for current clients.
- Tests to add/update:
  - `crates/loom-core/tests/foundation.rs`: vector backend selection, schema version migration, status stats fields if core-owned.
  - `crates/loom-core/tests/embedder.rs`: strict Candle failure path and explicit hashing fallback config.
  - `crates/loom-core/tests/search.rs`: recency participates in evolutionary scoring.
  - `crates/loom-core/tests/watcher.rs`: existing debouncer tests remain; add server integration only if it can be deterministic.
  - `crates/loom-mcp/src/server.rs` tests: watcher status, explicit reindex still works, and incremental helper refreshes graph.

## Integration Surface
- Public Rust MCP tools remain unchanged:
  - `search({ query, limit, kind })`
  - `related({ symbol, file, kind })`
  - `impact({ symbol, file, kind })`
  - `neighborhood({ file, line })`
  - `reindex()`
  - `status()`
- `status()` response gains additive fields only. Do not rename/remove `stats`, `graph_nodes`, or `graph_edges`.
- `LoomDb::open(config)` should continue working for existing callers, but internally select vector backend from config.
- `LoomDb::search_vectors()` return shape stays `Vec<(i64, f64)>`, preserving search engine callers.
- `IndexPipeline::new(config, db, embedder)` and `full_index()`/`incremental_index()` stay stable.
- Config additions must be TOML-loadable through `PartialConfig`:
  - explicit hashing fallback flag or embedder mode
  - vector backend mode if selection is user-configurable
  - optional auto-watch flag if product decides watcher startup should be configurable
- sqlite-vec integration must account for reader pool setup. A successful writer registration alone is not enough because vector search uses reader connections.

## Risks & Dependencies
- `sqlite-vec` is pre-v1 and the current docs label it unstable. Pin the version and keep `BlobVectorStore` as a deterministic fallback for tests and emergency local use.
- `sqlite-vec` registration through `sqlite3_auto_extension()` is process-global and unsafe; wrap it in a `Once` helper with a clear safety comment.
- `sqlite-vec`'s documented Rust binding currently exposes the C entrypoint and recommends `zerocopy::AsBytes`; this must be reconciled with `rusqlite 0.39` already in the workspace.
- Existing tests rely on `LoomDb::open()` with no model/network access. Do not make DB open depend on Candle or watcher startup.
- Starting the watcher in the MCP server must not keep tests alive forever or leak threads in a way that hangs `cargo test`.
- Status must be honest. If hashing fallback is enabled, say so. If vector backend is blob fallback, say so. The whole point is to stop hiding degraded retrieval under a lab coat.
- Migration work touches the store foundation. Keep it small, transactional, and heavily tested before changing retrieval behavior.

## Research Needed
- sqlite-vec Rust API details:
  - Official docs say the Rust crate statically links sqlite-vec and exposes `sqlite3_vec_init` for registration with SQLite's `sqlite3_auto_extension()`.
  - Official docs show KNN as `WHERE embedding MATCH ? AND k = ? ORDER BY distance` on a `vec0` virtual table.
  - Confirm exact crate version compatibility with the workspace's `rusqlite = 0.39` before implementation.
- No new ML library research is needed. The pipeline is about fail-loud Candle initialization and explicit hashing fallback, not replacing Candle.
