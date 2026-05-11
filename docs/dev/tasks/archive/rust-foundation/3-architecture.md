> Author: architect

# Architecture — rust-foundation

## Scope

Build the Rust foundation for the `rust-rewrite` wave while the Python implementation remains untouched. This pipeline introduces a Cargo workspace, shared core models/config/errors, SQLite persistence primitives, and graph traversal equivalents for the current Python store/search foundation.

This is a foundation slice, not the whole glorious rewrite parade. No parsers, no embedding generation, no search fusion engine, no watcher, no git analyzer execution, and no real MCP tool behavior.

## Workspace Structure

Create a root Cargo workspace:

| Path | Purpose |
|---|---|
| `Cargo.toml` | workspace manifest only; no root package |
| `crates/loom-core/Cargo.toml` | library crate for types, config, errors, store, graph |
| `crates/loom-core/src/lib.rs` | public module exports |
| `crates/loom-core/src/models.rs` | Python-compatible data shapes |
| `crates/loom-core/src/config.rs` | `.loom/config.toml` loading and defaults |
| `crates/loom-core/src/error.rs` | structured library errors |
| `crates/loom-core/src/store/mod.rs` | `LoomDb`, schema, CRUD/search contracts |
| `crates/loom-core/src/store/vector.rs` | vector adapter trait and implementations |
| `crates/loom-core/src/graph.rs` | `SymbolGraph` traversal API |
| `crates/loom-mcp/Cargo.toml` | compiling binary shell |
| `crates/loom-mcp/src/main.rs` | binary boundary using `anyhow`, no MCP behavior yet |

Workspace resolver must be `resolver = "2"`. Keep Rust artifacts alongside Python without changing `pyproject.toml`.

## Dependency Decisions

Use these crate versions/features unless implementation reveals an actual compile blocker:

| Crate | Version | Features | Crate |
|---|---:|---|---|
| `serde` | `1` | `derive` | `loom-core` |
| `thiserror` | `2` | default | `loom-core` |
| `toml` | `1` | default | `loom-core` |
| `time` | `0.3` | `formatting`, `parsing`, `serde` | `loom-core` |
| `rusqlite` | `0.39` | `bundled`, `functions`, `load_extension` | `loom-core` |
| `r2d2` | `0.8` | default | `loom-core` |
| `r2d2_sqlite` | `0.34` | `bundled` | `loom-core` |
| `parking_lot` | `0.12` | default | `loom-core` |
| `petgraph` | `0.8` | `std`, `stable_graph`, `matrix_graph`; no `rayon` yet | `loom-core` |
| `sqlite-vec` | `0.1.9` | none | `loom-core`, optional via adapter boundary if linking blocks |
| `tracing` | `0.1` | default | both |
| `anyhow` | `1` | default | `loom-mcp` only |
| `tracing-subscriber` | `0.3` | `fmt`, `env-filter` | `loom-mcp` |

`loom-core` must not depend on `anyhow`. Keep matchable errors there; save the “whatever exploded, wrap it nicely” ergonomics for binaries.

## Core Models

Mirror Python model names and field semantics so later `rmcp` schemas can serialize the same concepts:

| Rust Type | Fields |
|---|---|
| `Symbol` | `id: Option<i64>`, `name: String`, `kind: String`, `file: String`, `line: i64`, `end_line: i64`, `language: String`, `context: String` |
| `ParsedEdge` | `source_name: String`, `target_name: String`, `relationship: String`, `target_file: Option<String>` |
| `Edge` | `id: Option<i64>`, `source_id: i64`, `target_id: Option<i64>`, `target_name: String`, `target_file: Option<String>`, `relationship: String`, `confidence: f64`, `original_name: Option<String>` |
| `FileState` | `path: String`, `content_hash: String`, `last_indexed: String` |
| `CouplingScore` | `structural: f64`, `semantic: f64`, `evolutionary: f64`, `combined: f64` |
| `CoupledSymbol` | `symbol: Symbol`, `score: f64`, `reason: String` |
| `SearchResult` | `symbol: Symbol`, `score: f64`, `coupled: Vec<CoupledSymbol>` |

All public models derive `Debug`, `Clone`, `PartialEq`, `Serialize`, and `Deserialize`. Use `i64` for SQLite row IDs because SQLite does, and arguing with SQLite is a hobby for people who enjoy losing.

Foundation models should be owned `String`s. Later parser pipelines can introduce borrowed `Cow<'src, str>` extraction types and convert at the persistence boundary; this slice starts at the persisted/API boundary.

## Config

Implement `LoomConfig` with defaults matching `src/loom/config.py`:

| Field | Default |
|---|---|
| `target_dir` | constructor argument |
| `db_path` | `.loom/loom.db` |
| `watch_extensions` | current adapter extensions: `.py`, `.js`, `.jsx`, `.ts`, `.tsx`, `.go`, `.java`, `.rs`, `.cs` |
| `debounce_seconds` | `2.0` |
| `embedding_model` | `jinaai/jina-embeddings-v2-base-code` |
| `embedding_dimensions` | `768` |
| `max_file_size_bytes` | `512000` |
| `excluded_dirs` | adapter exclusions plus always `.git`, `__pycache__`, `.loom` |
| `structural_weight` | `0.45` |
| `semantic_weight` | `0.35` |
| `evolutionary_weight` | `0.20` |
| `enable_git_analysis` | `true` |
| `git_max_commits` | `500` |
| `git_max_files_per_commit` | `20` |

Config loading:

1. `LoomConfig::default_for_target(target_dir)` returns Python-equivalent defaults.
2. `LoomConfig::load(target_dir)` reads `target_dir/.loom/config.toml` if present.
3. Missing file means defaults, not an error.
4. Partial TOML overrides only provided fields.
5. Invalid TOML is a typed `LoomError::ConfigParse`.
6. `resolve_db_path()` creates `.loom/` and returns `target_dir.join(db_path)`.
7. Reject negative weights and reject active total weight `<= 0.0`.
8. Always union user exclusions with `.git`, `__pycache__`, and `.loom`.

Do not implement parser registry discovery in this pipeline. Hard-code the current extension/exclusion defaults in Rust and document that parser tasks will own dynamic registry behavior.

## Error Handling

Define `LoomError` with `thiserror`:

| Variant | Used For |
|---|---|
| `ConfigIo` | reading `.loom/config.toml`, creating `.loom/` |
| `ConfigParse` | TOML parse/decode failures |
| `InvalidConfig` | invalid weights, dimensions, paths |
| `Database` | `rusqlite::Error` |
| `Pool` | reader pool failures |
| `VectorDimension` | embedding length != configured dimensions |
| `VectorStore` | sqlite-vec/blob adapter errors |
| `MissingConnection` | methods called before open, if builder pattern needs it |
| `GraphLookup` | impossible graph lookup states; ordinary missing symbols return empty |
| `InvalidInput` | empty names, impossible limits, negative depths |

Expose `pub type Result<T> = std::result::Result<T, LoomError>`.

Rule: do not swallow exceptions/errors. Every binary boundary that logs an error must use structured `tracing` with the error chain; every library method returns a typed error.

## SQLite Store

### Connection Model

`LoomDb` owns:

| Field | Purpose |
|---|---|
| `config: LoomConfig` | dimensions and paths |
| `db_path: PathBuf` | resolved DB path |
| `writer: parking_lot::Mutex<rusqlite::Connection>` | one serialized writer |
| `readers: r2d2::Pool<SqliteConnectionManager>` | pooled read connections |
| `vector_store: Box<dyn VectorStore + Send + Sync>` or enum | vector backend boundary |

Use synchronous APIs in `loom-core`. `rusqlite` is blocking; later async code can call it through `spawn_blocking` or a dedicated writer worker.

On every connection, apply:

- `PRAGMA journal_mode=WAL`
- `PRAGMA synchronous=NORMAL`
- `PRAGMA foreign_keys=ON`

Foreign keys are per connection. Yes, SQLite made this weird, because apparently one footgun per table was too generous.

Reader pool size: `max(2, available_parallelism())`, capped at `16`. Writer is always one connection.

### Schema

Create these tables/indexes on open:

| Table | Notes |
|---|---|
| `symbols` | same columns as Python: `id`, `name`, `kind`, `file`, `line`, `end_line`, `language`, `context` |
| `edges` | `source_id` FK `ON DELETE CASCADE`, `target_id` FK `ON DELETE SET NULL`, plus target/name/relationship/confidence/original_name |
| `index_meta` | `file_path`, `content_hash`, `last_indexed` |
| `symbols_fts` | FTS5 virtual table over `name`, `kind`, `file`, `context`, external content `symbols` |
| `cochange` | `file_a`, `file_b`, `frequency`, unique pair |
| vector backend table | `vec_symbols` if sqlite-vec works, otherwise BLOB fallback table |

Keep logical compatibility with Python, not byte-identical SQL. Future migration/import tooling needs same data concepts, constraints, and indexes.

### Store API

Implement these methods with Rust naming conventions while preserving Python behavior:

| Method | Behavior |
|---|---|
| `insert_symbol` | inserts `symbols` row and matching `symbols_fts` row, returns `i64` |
| `insert_edge` | inserts edge, returns row id |
| `insert_embedding` | validates dimension then delegates to vector store |
| `search_fts` | sanitizes FTS special tokens and returns ranked `Vec<Symbol>` |
| `search_vectors` | returns `Vec<(i64, f64)>` sorted by ascending distance |
| `get_symbol_by_id` | optional symbol |
| `get_symbol_by_name` | exact name, optional file |
| `get_symbol_by_name_fuzzy` | same five strategies as Python |
| `get_colocated_symbols` | file symbols ordered by line |
| `remove_file` | nullifies incoming targets, deletes vectors/FTS rows, deletes symbols, deletes `index_meta` |
| `get_edges_from` / `get_edges_to` | resolved/unresolved edge rows by source/target ID |
| `get_edges_to_by_name` | resolved and unresolved edges matching target name |
| `get_unresolved_edges` | `target_id IS NULL` |
| `resolve_edge` | update `target_id` and `confidence` |
| `remove_edges_for_source` | delete source-owned edges |
| `get_file_hash` / `set_file_hash` | content hash metadata |
| `upsert_cochange` | canonical `(min, max)` file ordering, replace frequency |
| `get_cochange_frequency` | canonical lookup, missing = `0` |
| `get_top_cochanges` | partner files sorted by frequency desc |
| `get_stats` | `symbols`, `edges`, `files`, `vectors`, `last_indexed`, `stale_files`, `cochange_pairs` |

Bulk helpers should be included for symbols/edges/embeddings and must use transactions. Even if parser work lands later, the store should not force row-at-a-time indexing. That way lies sadness and progress bars that age like milk.

### FTS Sanitization

Port `_sanitize_fts_query` exactly:

- Trim whitespace.
- Empty query returns empty results.
- Split on whitespace.
- Quote tokens whose uppercase value is `AND`, `OR`, `NOT`, or `NEAR`.
- Quote tokens containing any of `- * " ^ : .`.

Use parameter binding for the `MATCH` query. No string interpolation except fixed placeholder lists generated from integer counts.

### Fuzzy Symbol Lookup

Preserve Python order:

1. Exact `(name, file)` if file given, otherwise exact name.
2. If file given, exact name anywhere, filtered by `ends_with(file)` or `ends_with("/" + file)`.
3. If name has no `.`, query `LIKE "%.name"` with optional file/suffix restriction.
4. Toggle leading underscore and retry exact/suffix/method suffix.
5. Return empty.

This matters because edge resolution behavior depends on stable ambiguity handling.

## Vector Store Boundary

Define a small adapter surface:

| Method | Purpose |
|---|---|
| `create_schema(conn, dimensions)` | create backend table(s) |
| `insert_embedding(conn, symbol_id, embedding)` | dimension-checked insert |
| `delete_embeddings(conn, symbol_ids)` | used by `remove_file` |
| `count(conn)` | stats |
| `search(conn, embedding, limit)` | top-K `(symbol_id, distance)` |

Primary adapter: `SqliteVecVectorStore`.

Fallback adapter: `BlobVectorStore`.

### Sqlite-Vec Path

Use `sqlite-vec` only through the adapter. Current docs expose `sqlite3_vec_init`; their example registers it through `sqlite3_auto_extension`, which requires `unsafe`. Keep that `unsafe` isolated in one function with a comment explaining the FFI boundary and test it by querying `vec_version()`.

Use schema equivalent to Python:

- virtual table `vec_symbols`
- `embedding float[768]` by default
- rowid must match `symbols.id`
- query shape returns `rowid, distance` ordered by distance

If sqlite-vec cannot compile cleanly with `rusqlite 0.39` in this slice, ship `BlobVectorStore` and leave `SqliteVecVectorStore` behind a feature flag or unconstructed module. The public `VectorStore` API must not change.

### Blob Fallback

Store embeddings as raw little-endian `f32` bytes:

- table: `symbol_embeddings(symbol_id INTEGER PRIMARY KEY REFERENCES symbols(id) ON DELETE CASCADE, embedding BLOB NOT NULL)`
- expected length: `dimensions * 4`
- search: scan rows, decode to `f32`, compute L2 distance or cosine distance consistently with tests

This is slower, yes. It also compiles, which is a charming property in a foundation layer.

## Graph Engine

Implement `SymbolGraph` in `crates/loom-core/src/graph.rs`.

### Representation

Preferred implementation for this pipeline:

- Build an internal deduplicated edge list from SQLite.
- Use `petgraph::Graph<(), EdgeMeta, Directed>` first if CSR metadata ergonomics slow development.
- Hide the concrete representation behind `SymbolGraph` so CSR can replace it locally.

CSR is still the architectural target for the rewrite, but correctness and API stability win in the foundation slice. If the developer can implement `petgraph::csr::Csr<(), EdgeMeta>` cleanly, use it; otherwise `Graph` is acceptable behind the same API.

Maintain:

| Map | Purpose |
|---|---|
| `symbol_id_to_node: HashMap<i64, NodeIndex>` | O(1) lookup |
| `node_to_symbol_id: Vec<i64>` or `HashMap<NodeIndex, i64>` | result conversion |

### Build Semantics

`build_from_db` loads only `edges WHERE target_id IS NOT NULL`.

Duplicate `(source_id, target_id)` pairs collapse to the highest-confidence edge. Preserve the winning edge's `relationship` and `confidence`.

Unresolved edges are terminal because they have no node target. They remain visible through DB methods, not graph traversal.

### Traversal API

| Method | Behavior |
|---|---|
| `build_from_db(&LoomDb)` | full rebuild from resolved edges |
| `dependents(symbol_id, max_depth)` | reverse traversal; nodes that depend on symbol |
| `dependencies(symbol_id, max_depth)` | forward traversal; nodes the symbol depends on |
| `shortest_path(source_id, target_id)` | directed shortest path, optional vector of symbol IDs |
| `impact_radius(symbol_id, max_depth)` | dependents with depth-decayed confidence |
| `centrality(top_n)` | in-degree centrality excluding self-loops |
| `neighbors_with_metadata(symbol_id, max_depth)` | merge dependents/dependencies, keep highest confidence; dependents win ties |

Missing symbol or empty graph returns empty results, not an error. Negative depth is impossible in the Rust type; depth `0` returns empty.

Depth-decayed score:

`confidence * (1.0 / 2.0_f64.powi(depth - 1))`

Sort impact results by descending score.

## Concurrency Model

This pipeline mostly builds synchronous foundation APIs, but the architecture must not paint future async work into a corner.

Current slice:

```
loom-mcp binary shell
    └── calls synchronous loom-core APIs directly

LoomDb
    ├── writer: 1 serialized rusqlite connection behind Mutex
    └── readers: bounded r2d2 pool, max(2, available_parallelism) capped at 16

SymbolGraph
    └── rebuilt synchronously from DB snapshots
```

Future indexer/server thread budgets to preserve in API design:

```
ignore::WalkParallel (available_parallelism threads)
    --bounded mpsc(32)-->
rayon parser pool (available_parallelism threads)
    --bounded mpsc(batch_size * 2, default 64)-->
candle embedder task (1 model owner, batch-oriented)
    --bounded mpsc(20)-->
SQLite writer worker (1 blocking writer)
```

Do not make `LoomDb` require a Tokio runtime. Later async handlers should call store operations via `spawn_blocking` or a writer worker. Never run CPU-bound `rayon` work on Tokio worker threads.

## Module Responsibilities

### `loom-core::models`

Owns stable data contracts only. No DB row conversion logic here beyond optional constructors if they are trivial.

### `loom-core::config`

Owns defaults, TOML decoding, validation, path resolution, and `.loom/` creation.

### `loom-core::error`

Owns typed error enum and `Result<T>`.

### `loom-core::store`

Owns SQLite schema, connection lifecycle, CRUD/search primitives, FTS sanitization, vector adapter dispatch, and stats.

It must not own graph traversal, parser logic, embedding model loading, search fusion, git history analysis, or MCP formatting.

### `loom-core::graph`

Owns in-memory graph rebuild and traversal over resolved edges.

It must not query vector similarity, compute final three-signal coupling scores, or format MCP responses.

### `loom-mcp`

Compiles as a binary shell:

- initializes tracing to stderr
- returns `anyhow::Result<()>`
- may load config and print a minimal startup line
- does not register MCP tools in this pipeline

## Testing Requirements

Add Rust-native tests. Python tests cannot prove Rust behavior, which is unfair but tragically true.

| Area | Required Tests |
|---|---|
| config | defaults, missing TOML fallback, partial override, invalid TOML, invalid weights, db path creation |
| models | serde round trips for all public model types |
| schema | DB creates under temp `.loom/`, WAL/FK pragmas active, FTS5 table creation succeeds |
| symbols/edges | insert/read symbol, insert/read edge, confidence round trip, unresolved edge resolution |
| remove_file | nullifies incoming targets, cascades outgoing edges, removes FTS/vector rows, removes index metadata |
| fuzzy lookup | exact, file suffix, method suffix, underscore toggle, no-match empty |
| FTS | special token quoting for `AND`, dotted names, punctuation-heavy names, empty query |
| vectors | dimension mismatch typed error, insert/search top-K, delete embeddings during `remove_file` |
| cochange | canonical ordering, upsert replacement, missing frequency `0`, top partners |
| graph | rebuild resolved only, duplicate edge chooses highest confidence, dependencies/dependents, shortest path, impact decay, centrality without self-loops, missing node empty |
| binary | `loom-mcp` builds and exits cleanly in smoke test if a test harness is added |

Required verification:

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`

## Trade-Off Decisions

| Decision | Rationale |
|---|---|
| Owned model strings in foundation | Public persisted models need simple serde and DB boundaries; parser borrowing comes later |
| Sync `loom-core` store API | `rusqlite` is blocking; forcing async now would just hide blocking behind prettier syntax |
| One writer plus pooled readers | Matches SQLite WAL strengths and avoids write races |
| Adapter boundary for vectors | sqlite-vec Rust integration has an `unsafe` static extension path; fallback keeps pipeline shippable |
| Start graph behind abstraction | CSR is target, but `Graph` may ship faster while preserving API |
| `time` over `chrono` | Smaller feature surface and enough for UTC/index timestamps; no local timezone semantics needed |
| Hard-code Rust config defaults for extensions | Parser registry does not exist yet in Rust; foundation should not invent it |

## Research Notes

### SQLite Binding

| Criteria | `rusqlite` | `sqlx` |
|---|---|---|
| crates.io downloads | 60.9M total, 14.2M recent | 95.5M total, 25.1M recent |
| Latest stable | `0.39.0`, published 2026-03-15 | `0.8.6`, latest stable; `0.9.0-alpha.1` also exists |
| SQLite distribution | `bundled` via `libsqlite3-sys` | SQLite support, but async abstraction heavier |
| Extension/functions support | documented `functions`, `load_extension`, `auto_extension` modules | less direct for sqlite-vec/static extension path |
| Fit | direct blocking SQLite API, matches Python store port | async toolkit, more machinery than foundation needs |
| License | MIT | MIT OR Apache-2.0 |

**Decision:** `rusqlite` with `bundled`, `functions`, and `load_extension`. Loom needs tight SQLite control, FTS, PRAGMAs, and sqlite-vec extension registration more than async query ergonomics.

Sources: crates.io API for `rusqlite` and `sqlx`; docs.rs `rusqlite` module docs.

### SQLite Reader Pool

| Criteria | `r2d2_sqlite` | `deadpool-sqlite` |
|---|---|---|
| crates.io downloads | 5.3M total, 770K recent | 469K total, 66K recent |
| Latest | `0.34.0`, published 2026-05-10 | `0.13.0`, published 2026-02-17 |
| Runtime requirement | none | async pool, Tokio-oriented |
| rusqlite compatibility | direct `SqliteConnectionManager` for `rusqlite` | wraps rusqlite for async callers |
| Fit | simple pooled blocking readers | better if the store itself were async |
| License | MIT | MIT/Apache-style project family, verify exact crate if used |

**Decision:** `r2d2_sqlite` plus `r2d2`. Keep `loom-core` synchronous; async belongs at later MCP/indexer boundaries.

Sources: crates.io API for `r2d2_sqlite` and `deadpool-sqlite`; docs.rs `r2d2_sqlite`.

### Vector Store

| Criteria | `sqlite-vec` | `hnsw_rs` | BLOB fallback |
|---|---|---|---|
| crates.io downloads | 1.5M total, 1.05M recent | 435K total, 169K recent | no new crate |
| Latest stable | `0.1.9`; alpha `0.1.10` exists | `0.3.4`, published 2026-02-28 | internal |
| Search type | SQLite virtual table, brute force | in-memory ANN/HNSW | brute-force scan |
| Persistence | same SQLite file | separate in-memory/rebuild story | same SQLite file |
| Unsafe/FFI | yes, static extension registration | no SQLite FFI, ANN internals | no FFI |
| Fit | best parity with Python and unified DB | better later for large ANN | safest fallback |
| License | MIT/Apache-2.0 | MIT/Apache-2.0 | project license |

**Decision:** primary `sqlite-vec` behind `VectorStore`; mandatory BLOB fallback. Do not expose sqlite-vec details to callers.

Sources: crates.io API for `sqlite-vec` and `hnsw_rs`; docs.rs `sqlite-vec` source showing `sqlite3_vec_init`; project research doc notes sqlite-vec is brute-force today.

### Graph Library

| Criteria | `petgraph` | `graph` |
|---|---|---|
| crates.io downloads | 361M total, 75.9M recent | 91K total, 7K recent |
| Latest | `0.8.3`, published 2025-09-30 | `0.3.1`, published 2023-11-20 |
| CSR support | documented `petgraph::csr::Csr` | high-performance graph algorithms, smaller ecosystem |
| Maintainers/ecosystem | release team, broad Rust usage | Neo4j Labs repo, narrower |
| License | MIT OR Apache-2.0 | MIT |

**Decision:** `petgraph`. Prefer CSR if clean, otherwise use `Graph` behind `SymbolGraph`.

Sources: crates.io API for `petgraph` and `graph`; docs.rs `petgraph::csr`.

### Timestamp Crate

| Criteria | `time` | `chrono` |
|---|---|---|
| crates.io downloads | 657.9M total, 116.2M recent | 570.4M total, 108.1M recent |
| Latest | `0.3.47`, published 2026-02-05 | `0.4.44`, published 2026-02-23 |
| Feature fit | `formatting`, `parsing`, `serde`; no timezone baggage needed | excellent but broader timezone/local-time surface |
| License | MIT OR Apache-2.0 | MIT OR Apache-2.0 |

**Decision:** `time` with `formatting`, `parsing`, and `serde`. Store timestamps as strings compatible with SQLite/Python `datetime('now')` style where practical.

Sources: crates.io API for `time` and `chrono`; docs.rs `time`.

## Developer Work Queue

1. Add workspace manifests and minimal compiling crates.
2. Implement `models`, `error`, and config loading/validation.
3. Add store schema creation and connection model with PRAGMA tests.
4. Implement symbol/edge/index metadata CRUD and FTS sanitization/search.
5. Implement vector adapter trait with sqlite-vec attempt and BLOB fallback.
6. Implement cochange methods and stats.
7. Implement graph rebuild/traversal APIs.
8. Add focused Rust tests for every behavior above.
9. Run the required Cargo verification suite.

## Non-Goals

- Do not port language adapters.
- Do not port embedder/model loading.
- Do not port search engine fusion.
- Do not port FastMCP/rmcp tools.
- Do not add file watcher behavior.
- Do not run git commands from this pipeline.
