> Author: architect

# Architecture - rust-runtime-retrieval-fixes

## Goal

Fix the Rust runtime retrieval foundation without changing the public MCP tool surface. The developer should leave the six existing tools intact and make the internal status honest: vector backend, embedder backend, schema version, and watcher state must be observable instead of hidden behind cheerful prototype fog. Charming for demos, fatal for code intelligence.

## Scope

In scope:

- `crates/loom-core/src/store/vector.rs`: vector backend abstraction, sqlite-vec backend, blob fallback retained for tests.
- `crates/loom-core/src/store/mod.rs`: backend selection, reader/writer extension registration, migration runner, schema/status accessors.
- `crates/loom-core/src/store/migrations.rs`: durable schema versioning and idempotent migrations.
- `crates/loom-core/src/config.rs`: vector/embedder/watch config fields and adapter-derived defaults.
- `crates/loom-core/src/embedder.rs`: strict Candle default, explicit hashing fallback, backend introspection.
- `crates/loom-core/src/search/scoring.rs` and `crates/loom-core/src/search/engine.rs`: evolutionary recency scoring.
- `crates/loom-mcp/src/server.rs`: watcher ownership, deterministic incremental handler, additive status fields.
- Focused Rust tests under `crates/loom-core/tests/` and `crates/loom-mcp/src/server.rs` tests.

Out of scope:

- New MCP tools.
- Replacing Candle.
- ANN/HNSW production backend.
- Rewriting the indexing pipeline concurrency model.
- Python implementation changes.

## File Structure

```text
crates/loom-core/
  Cargo.toml
  src/
    config.rs
    embedder.rs
    models.rs
    store/
      mod.rs
      migrations.rs       # new
      vector.rs
    search/
      engine.rs
      scoring.rs

crates/loom-mcp/
  src/
    server.rs
```

## Configuration

Add these fields to `LoomConfig` and `PartialConfig`:

```text
vector_backend: VectorBackendConfig
embedding_backend: EmbeddingBackendConfig
allow_hashing_embedder_fallback: bool
auto_watch: bool
```

Use string enums serialized from TOML:

- `vector_backend = "sqlite-vec" | "blob"`
- `embedding_backend = "candle" | "hashing"`
- `allow_hashing_embedder_fallback = false` by default
- `auto_watch = true` by default for the MCP server

Default behavior:

- Production default vector backend: `sqlite-vec`.
- Test/deterministic fallback: `blob`, selected explicitly in config or test helper.
- Production default embedder: `candle`.
- Hashing embedder is allowed only when `embedding_backend = "hashing"` or `allow_hashing_embedder_fallback = true`.
- Candle initialization errors return `Err` unless fallback is explicitly configured.

Validation:

- `vector_backend = "sqlite-vec"` requires `embedding_dimensions > 0`.
- `embedding_backend = "hashing"` is valid only for local/offline deterministic mode; no model download should be attempted.
- `allow_hashing_embedder_fallback = true` must be reflected in status if fallback actually happens.

Default indexed extensions/exclusions:

- Replace hard-coded `watch_extensions` and language-owned exclusions with:
  - `AdapterRegistry::with_builtin_adapters().get_all_extensions()`
  - `AdapterRegistry::with_builtin_adapters().get_all_excluded_dirs()`
- Union adapter exclusions with `ALWAYS_EXCLUDED = [".git", "__pycache__", ".loom"]`.
- Keep operational non-language exclusions only if the adapters do not own them. Do not keep duplicate hand-copied language defaults. Drift is not a feature, despite what config files keep trying to prove.

## Vector Store Architecture

### Types

Extend `VectorStore`:

```text
fn backend_name(&self) -> &'static str
fn create_schema(&self, conn: &Connection, dimensions: usize) -> Result<()>
fn insert_embedding(...)
fn delete_embeddings(...)
fn clear(...)
fn count(...)
fn search(...)
```

Add:

- `VectorBackend` enum in `store/vector.rs`: `SqliteVec`, `Blob`.
- `SqliteVecStore`.
- `register_sqlite_vec_once()` helper.
- `serialize_embedding_bytes()` helper using `zerocopy` for sqlite-vec binding inputs.

Keep:

- `BlobVectorStore` as deterministic fallback and migration compatibility layer.
- Existing `LoomDb::search_vectors() -> Result<Vec<(i64, f64)>>`.

### sqlite-vec Schema

Create the virtual table:

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS vec_symbols USING vec0(
  symbol_id INTEGER PRIMARY KEY,
  embedding float[DIMENSIONS]
);
```

Search shape:

```sql
SELECT symbol_id, distance
FROM vec_symbols
WHERE embedding MATCH ? AND k = ?
ORDER BY distance;
```

Use `k = ?` rather than relying on `LIMIT`, because sqlite-vec documents `k` as the portable path and notes `LIMIT` depends on newer SQLite behavior.

Insert path:

- Validate dimensions before SQL.
- Use `INSERT OR REPLACE INTO vec_symbols(symbol_id, embedding) VALUES (?, ?)`.
- Bind embedding as little-endian `f32` bytes.

Delete/clear/count:

- Mirror current `BlobVectorStore` behavior, using `vec_symbols`.
- Deleting a symbol must delete the corresponding vector row before symbol deletion.

### sqlite-vec Registration

Registration must happen before any connection creates or queries `vec0`.

Implement:

- `register_sqlite_vec_once() -> Result<()>` in `store/vector.rs` or `store/mod.rs`.
- It calls `sqlite3_auto_extension(sqlite3_vec_init)` once per process.
- Put the unsafe call behind a `OnceLock<Result<(), String>>` or equivalent so registration is process-global and idempotent.
- Include a tight safety comment: the function pointer is the sqlite-vec extension entrypoint, statically linked by the crate, and SQLite owns auto-extension invocation for future connections.

Important connection behavior:

- Call registration before opening the writer connection when `vector_backend = "sqlite-vec"`.
- Reader pool connections are opened lazily; auto-extension registration covers future connections, but `reader()` should still perform a cheap smoke check once per pooled connection class if practical, or rely on `vec_version()` tests to prove registration.
- Keep `apply_pragmas()` for every reader as today.

### Blob Compatibility

Do not silently choose blob when sqlite-vec initialization fails. Return an error unless config says `vector_backend = "blob"`.

Blob backend remains useful for:

- deterministic unit tests,
- sqlite-vec unavailable build isolation,
- migration tests that intentionally inspect old `symbol_embeddings`.

Status must say `"blob"` when blob is active. No trench coat.

## Schema Versioning and Migrations

Add `crates/loom-core/src/store/migrations.rs`.

Use `PRAGMA user_version` as the durable schema version. This is one local SQLite DB, so a migration table would be bureaucratic theater.

Constants:

```text
CURRENT_SCHEMA_VERSION = 3
```

Recommended migration sequence:

- Version 0 -> 1: create base relational schema:
  - `symbols`
  - `symbols_fts`
  - `edges`
  - `index_meta`
  - `cochange` with `recency`
  - indexes
- Version 1 -> 2: ensure `cochange.recency` exists for DBs created by current Rust code before the migration structure.
- Version 2 -> 3: create active vector backend schema:
  - `vec_symbols` for sqlite-vec, or
  - `symbol_embeddings` for blob.

Open behavior:

```text
LoomDb::open(config)
  -> select VectorStore from config
  -> register sqlite-vec if needed
  -> open writer
  -> apply pragmas
  -> build reader pool
  -> run_migrations(writer, vector_store, embedding_dimensions)
  -> return LoomDb
```

Migration requirements:

- Idempotent: rerunning `LoomDb::open()` must not change data or fail.
- Transactional where SQLite allows it. `CREATE VIRTUAL TABLE` can participate in SQLite transactions, but keep vector table creation in its own clearly named migration step for easy debugging.
- Existing DBs with no `user_version` but with current tables must upgrade cleanly.
- Preserve existing symbol/vector data when possible:
  - If `vector_backend = "blob"`, keep `symbol_embeddings`.
  - If `vector_backend = "sqlite-vec"` and an old `symbol_embeddings` table exists, do not try to migrate BLOB rows into `vec_symbols` unless all dimensions validate. Prefer creating the new table and allowing the next reindex to populate it. Document this in a migration comment/test expectation.
- Add `LoomDb::schema_version() -> Result<i64>`.

## Embedder Architecture

### Backend Identity

Add:

```text
pub enum EmbedderBackend {
  Candle,
  Hashing,
}

pub struct EmbedderStatus {
  backend: &'static str,
  degraded: bool,
  dimensions: usize,
  model: Option<String>,
}
```

Either expose `DefaultEmbedder::backend_name()` / `is_degraded()` directly or through an `EmbedderIntrospection` trait. Keep it small.

### DefaultEmbedder::from_config()

Behavior:

- `embedding_backend = "candle"` and Candle loads:
  - return `DefaultEmbedder::Candle`
  - status: `backend = "candle"`, `degraded = false`
- `embedding_backend = "candle"` and Candle fails:
  - if `allow_hashing_embedder_fallback = false`, return the Candle error
  - if `allow_hashing_embedder_fallback = true`, log full error and return hashing
  - status: `backend = "hashing"`, `degraded = true`
- `embedding_backend = "hashing"`:
  - return hashing immediately
  - no model download
  - status: `backend = "hashing"`, `degraded = false`

Logging:

- Use `tracing::warn!(error = ?source, ...)` or equivalent so full error context is not erased.
- Do not swallow Candle initialization errors without explicit config. The Python reference already fails loud; Rust should stop cosplaying as a helpful intern hiding the smoke alarm.

## Watcher Integration

`crates/loom-core/src/watcher.rs` already owns OS events and debouncing. Wire it into `loom-mcp` without making tests depend on OS watcher timing.

### CoreState Ownership

Add to `CoreState`:

```text
watcher: Mutex<Option<LoomWatcher>>
watcher_started: AtomicBool or watcher_status inside Mutex
```

`CoreState` responsibilities:

- Lazy embedder initialization remains.
- `reindex_lock` gates full and incremental indexing.
- Graph refresh happens after successful full or incremental indexing.
- Watcher lifetime is tied to `CoreState`.

### Startup Flow

In `LoomServerState::core()` after DB and graph initialization:

1. Build `CoreState`.
2. If `config.auto_watch`, call `core.start_watcher_once()`.
3. Store `Arc<CoreState>`.

`start_watcher_once()`:

- Builds a `FnChangeHandler`.
- The handler captures `Weak<CoreState>` to avoid accidental reference cycles.
- On changes:
  - upgrade weak pointer,
  - call `core.handle_changed_paths(paths)`,
  - log full error if the callback fails.

### Deterministic Helper

Add:

```text
CoreState::handle_changed_paths(paths: Vec<PathBuf>) -> Result<IndexResult>
```

Flow:

```text
handle_changed_paths
  -> lock reindex_lock
  -> get/create embedder outside graph mutex
  -> IndexPipeline::new(config.clone(), db.clone(), embedder)
  -> incremental_index(paths)
  -> refresh_graph()
  -> return result
```

Do not hold `graph` mutex while indexing. Do not hold `embedder` mutex across embedding work after the `Arc` is cloned.

### Deletions

`IndexPipeline::incremental_index()` already removes deleted files when given a missing path. Watcher callback should pass paths exactly as received from the debouncer; do not pre-filter deleted paths in server code.

### Watcher Status

Add additive fields to `StatusResponse`:

```text
vector_backend: String
embedder_backend: Option<String>
embedder_degraded: bool
schema_version: i64
watcher_active: bool
auto_watch: bool
```

`embedder_backend` can be `None` before the first search/reindex if status has not loaded the model. That preserves the current useful behavior: `status()` opens the DB without forcing model download. If the team wants an always-filled configured value too, add `configured_embedder_backend: String`.

Existing fields stay:

- `stats`
- `graph_nodes`
- `graph_edges`

## Evolutionary Recency Scoring

Current data stores recency but search ignores it. Fix by using `get_cochange()` in `SearchEngine::evolutionary_score()`.

Change scoring:

```text
compute_evolutionary(frequency, recency, max_frequency)
```

Recommended formula:

```text
frequency_score = frequency / max_frequency, clamped 0..1
recency_score = recency clamped 0..1
score = 0.75 * frequency_score + 0.25 * recency_score
```

Rationale:

- Frequency remains the main signal.
- Recency breaks ties and boosts fresh co-change evidence.
- Existing rankings stay broadly stable.
- Score remains `[0.0, 1.0]`.

If `get_cochange()` returns `None`, score is `0.0`.

Do not remove the `recency` column. It is already produced by `GitAnalyzer` and ordered in `get_top_cochanges()`. Make it useful instead of pretending storage is a scrapbook.

## Concurrency Model

This pipeline is a fix pass, not a pipeline rewrite. Keep the current concurrency:

```text
MCP tokio runtime
  -> tool handler enters synchronous core APIs
  -> reindex_lock serializes full/incremental indexing
  -> IndexPipeline::index_paths
       rayon global pool parses files in parallel
       single embedder batches texts in chunks of 128
       single SQLite writer transaction per file
  -> refresh in-memory SymbolGraph
```

Thread budgets:

- Tokio: keep `loom-mcp` on `rt-multi-thread`; no custom worker override in this task. Runtime worker count remains Tokio default unless `main.rs` already overrides it.
- Rayon: global rayon pool default, effectively `available_parallelism()`.
- SQLite:
  - one writer guarded by `parking_lot::Mutex<Connection>`,
  - reader pool size remains `available_parallelism().clamp(2, 16)`.
- Watcher:
  - one notify watcher backend thread as managed by `notify`,
  - one debounce flush thread from `LoomWatcher`.

Channel diagram:

```text
notify OS events
  -> LoomWatcher callback
  -> Debouncer pending set
  -> std::sync::mpsc flush signal (unbounded std channel, one message per notify event)
  -> ChangeHandler
  -> CoreState::handle_changed_paths
  -> IndexPipeline::incremental_index
  -> rayon parse fanout
  -> batch embedder chunks of 128
  -> SQLite writer mutex
  -> SymbolGraph rebuild
```

Buffer sizes:

- No new tokio/rayon channels in this fix.
- Existing watcher uses `std::sync::mpsc`; do not introduce an unbounded indexing queue. The `reindex_lock` is the backpressure point.
- If a future rewrite adds channels, use bounded `mpsc(32)` between discovery and parse, `mpsc(16)` between parse and embed, and `mpsc(8)` between embed and DB writes as documented in `docs/dev/research/rust-techniques-2026-05-11.md`.

## Dependency Changes

Add to `crates/loom-core/Cargo.toml`:

```toml
sqlite-vec = { version = "0.1.10-alpha.3", default-features = false }
zerocopy = { version = "0.8.33", default-features = false, features = ["std"] }
```

Keep existing:

```toml
rusqlite = { version = "0.39", features = ["bundled", "functions"] }
```

Do not enable `rusqlite/loadable_extension` unless implementation proves the safe `rusqlite::auto_extension` wrapper is better than direct `rusqlite::ffi::sqlite3_auto_extension`. The sqlite-vec official Rust docs use the FFI path with `bundled`.

No new dependency for migrations. `PRAGMA user_version` plus plain functions is enough.

## Research Notes

### Vector Backend

| Criteria | sqlite-vec | hnsw_rs | Current BlobVectorStore |
|----------|------------|---------|-------------------------|
| crates.io version | `0.1.10-alpha.3` | `0.3.4` | internal |
| downloads | lib.rs reports ~50,975/month | lower visibility; current crate is active but niche | not applicable |
| Last updated | docs show `v0.1.10-alpha.3` | docs/source show 0.3.4 with recent updates | current code |
| MSRV | unknown from `cargo info`; workspace uses Rust 1.82 | unknown from `cargo info`; latest crate uses edition 2024 metadata | workspace MSRV |
| unsafe blocks | required one-time SQLite auto-extension registration | internal ANN implementation details, not needed now | none in vector store |
| Used by / backing | Mozilla Builders, Fly.io, Turso, SQLite Cloud sponsorship listed in docs | pure Rust HNSW ecosystem | only Loom tests/runtime |
| License | MIT/Apache-2.0 | MIT/Apache-2.0 | Loom MIT |
| Binary size | sqlite-vec C source package about 1MB; linked extension, no separate service | larger ANN dependency surface | none |
| Operational fit | single SQLite file, matches Python reference, supports `vec0` KNN | requires separate ANN persistence/rebuild design | deterministic but brute-force full scan |

**Decision:** `sqlite-vec` now, behind `VectorStore`; keep blob only as explicit fallback/test backend. `hnsw_rs` is a future ANN candidate, not needed for this bug-fix pipeline.

Sources:

- sqlite-vec Rust docs: official crate embeds C source, exposes `sqlite3_vec_init`, and recommends registration through SQLite auto-extension.
- sqlite-vec KNN docs: `vec0` virtual tables support `embedding MATCH ? AND k = ?`; `LIMIT` depends on SQLite version.
- rusqlite docs: the safe `auto_extension` module exists only behind the `loadable_extension` feature.

### Vector Byte Binding

| Criteria | zerocopy 0.8.33 | Manual `f32::to_le_bytes` |
|----------|------------------|---------------------------|
| crates.io version | `0.8.33` stable; `0.9.0-alpha.0` exists but skip alpha | internal |
| MSRV | 1.56 from `cargo info`, below workspace 1.82 | workspace |
| License | BSD-2-Clause OR Apache-2.0 OR MIT | Loom MIT |
| Type ergonomics | `IntoBytes`/byte conversion helpers, docs recommend it for sqlite-vec | more boilerplate |
| Risk | mature, widely used, but API name changed from old docs' `AsBytes` to newer stable traits | no dependency, but easy to duplicate badly |

**Decision:** use `zerocopy = "0.8.33"` with `features = ["std"]`. Do not use the alpha `0.9` line. If the developer finds API naming friction, a tiny local byte conversion helper is acceptable for blob, but sqlite-vec binding should follow current zerocopy stable API.

## Testing Plan

Core tests:

- `foundation.rs`
  - `LoomConfig::default_for_target()` extensions equal registry-derived extensions.
  - exclusions include adapter exclusions plus always-excluded directories.
  - explicit `vector_backend = "blob"` selects blob.
  - default selects sqlite-vec.
  - `schema_version()` equals `CURRENT_SCHEMA_VERSION`.
  - opening an older schema without `cochange.recency` upgrades it.
- New or existing vector tests:
  - sqlite-vec insert/search returns nearest symbol IDs.
  - blob insert/search remains deterministic.
  - sqlite-vec dimension mismatch fails before SQL.
  - `LoomDb::vector_backend_name()` reports active backend.
- `embedder.rs` tests:
  - `embedding_backend = "hashing"` does not call model source.
  - Candle failure returns `Err` when fallback is disabled.
  - Candle failure returns hashing with degraded status when fallback is enabled.
  - existing `HashingEmbedder` deterministic tests stay.
- `search.rs`
  - equal frequency, higher recency produces higher evolutionary score.
  - no cochange returns `0.0`.
  - fused score remains clamped.
- `server.rs` tests:
  - `status()` opens DB without loading embedder.
  - status includes vector backend, schema version, watcher flags.
  - deterministic `handle_changed_paths()` indexes a changed file and refreshes graph.
  - watcher can be disabled with `auto_watch = false` for tests.

Do not write a test that sleeps and hopes the OS watcher fires. That is how CI becomes a haunted house with invoices.

Validation commands for developer:

```bash
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Implementation Order

1. Config enums and defaults:
   - Add vector/embedder/watch config fields.
   - Derive default extensions/exclusions from `AdapterRegistry`.
   - Update config tests.
2. Migrations:
   - Add `store/migrations.rs`.
   - Move schema creation behind `run_migrations()`.
   - Add schema version access and old-schema tests.
3. Vector backend:
   - Add sqlite-vec and zerocopy dependencies.
   - Add backend enum and `SqliteVecStore`.
   - Select backend in `LoomDb::open()`.
   - Add backend status/accessors and vector tests.
4. Embedder strictness:
   - Add backend/degraded introspection.
   - Make Candle fail loud by default.
   - Preserve explicit hashing mode.
5. Recency scoring:
   - Update scoring function and search engine lookup.
   - Add tie-break tests.
6. MCP watcher:
   - Add `CoreState::handle_changed_paths()`.
   - Start `LoomWatcher` once when `auto_watch`.
   - Add additive status fields and deterministic server tests.
7. Full validation:
   - Run workspace tests, fmt check, clippy with warnings denied.

## Acceptance Criteria

- Default Rust DB opens with sqlite-vec backend and reports it.
- Blob vector search is never silently selected after sqlite-vec failure.
- Candle initialization failure is an error unless hashing fallback is explicitly configured.
- Status exposes active/degraded retrieval state without forcing model load on plain status.
- Rust MCP server starts watcher when configured and routes changes through incremental indexing plus graph refresh.
- Default watch extensions/exclusions come from registered adapters.
- Recency affects evolutionary scoring.
- `PRAGMA user_version` is set and old Rust schemas migrate idempotently.
- Existing MCP tool names and request shapes remain unchanged.
- `cargo test --workspace`, `cargo fmt --all -- --check`, and `cargo clippy --workspace --all-targets -- -D warnings` pass.
