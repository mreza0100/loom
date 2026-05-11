> Author: architect

# Architecture - rust-indexer

## Scope

This pipeline implements the Rust indexing runtime inside `crates/loom-core`: Candle embeddings, staged full/incremental indexing, two-phase edge resolution, file watching, and git co-change analysis.

Keep `crates/loom-mcp` mostly untouched. It can call these APIs later, but MCP tool registration belongs to the `rust-search-server` pipeline. Also keep the current `BlobVectorStore` unless a store test already forces sqlite-vec; this pipeline must generate and persist 768-dim vectors, not redesign vector search.

## Current Anchors

- `crates/loom-core/src/config.rs` already owns `LoomConfig`; change default `debounce_seconds` from `2.0` to `0.5`.
- `crates/loom-core/src/models.rs` already has owned `Symbol`, `ParsedEdge`, `Edge`, `FileState`, `StoreStats`; add indexing result structs here or in `indexer`.
- `crates/loom-core/src/store/mod.rs` already has WAL SQLite, batched inserts, FTS, `index_meta`, unresolved edge APIs, and file-level `cochange`; extend this instead of bypassing private connection state.
- `crates/loom-core/src/parsers/` already has `AdapterRegistry::with_builtin_adapters()` and `parse_file(path, Some(bytes), &registry)`.
- Python behavior to preserve lives in `src/loom/indexer/embedder.py`, `pipeline.py`, `watcher.py`, and `git_analyzer.py`.

## Crate Dependencies

Add these to `crates/loom-core/Cargo.toml`:

```toml
serde_json = { version = "1", default-features = true, features = [] }
sha2 = { version = "0.10", default-features = true, features = [] }
ignore = { version = "0.4.25", default-features = true, features = [] }
rayon = { version = "1.10", default-features = true, features = [] }
crossbeam-channel = { version = "0.5", default-features = true, features = [] }
notify = { version = "8.2", default-features = true, features = ["crossbeam-channel"] }
globset = { version = "0.4", default-features = true, features = [] }
time = { version = "0.3", default-features = false, features = ["formatting", "parsing"] }
dirs = { version = "6", default-features = true, features = [] }
tokenizers = { version = "0.23.1", default-features = true, features = [] }
hf-hub = { version = "0.5", default-features = false, features = ["ureq", "rustls-tls"] }

[target.'cfg(not(target_os = "macos"))'.dependencies]
candle-core = { version = "0.10.2", default-features = false, features = [] }
candle-nn = { version = "0.10.2", default-features = false, features = [] }
candle-transformers = { version = "0.10.2", default-features = false, features = [] }
```

For macOS Metal builds, use the same Candle crates with `metal` enabled:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
candle-core = { version = "0.10.2", default-features = false, features = ["metal"] }
candle-nn = { version = "0.10.2", default-features = false, features = ["metal"] }
candle-transformers = { version = "0.10.2", default-features = false, features = ["metal"] }
```

Do not enable CUDA/MKL/Accelerate in this pipeline. CPU must be boring and portable; Metal is an opportunistic macOS path with CPU fallback.

## File Structure

```text
crates/loom-core/src/
|-- embedder.rs
|-- indexer/
|   |-- mod.rs
|   |-- pipeline.rs
|   |-- resolver.rs
|   |-- walk.rs
|   `-- path.rs
|-- watcher.rs
|-- git_analyzer.rs
|-- store/mod.rs        # add public helper methods only
|-- config.rs           # debounce/model cache config additions
|-- error.rs            # new structured variants
`-- lib.rs              # export embedder/indexer/watcher/git_analyzer
```

Tests:

```text
crates/loom-core/tests/
|-- embedder.rs
|-- indexer_pipeline.rs
|-- resolver.rs
|-- watcher.rs
`-- git_analyzer.rs
```

## Module Responsibilities

### `embedder.rs`

Define:

```rust
pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn embed_single(&self, text: &str) -> Result<Vec<f32>>;
    fn dimensions(&self) -> usize;
}

pub fn build_symbol_text(name: &str, kind: &str, context: &str) -> String;
```

`build_symbol_text` must match Python exactly: `"{kind} {name}\n{context}"`.

Implement `CandleEmbedder`:
- model: `jinaai/jina-embeddings-v2-base-code`;
- required local files: `config.json`, `tokenizer.json`, and `model.safetensors`;
- cache root: default `~/.loom/models/`, override with `LoomConfig.model_cache_dir`;
- downloader behind `ModelSource` trait so tests mock network/model files;
- load config via `serde_json` into `candle_transformers::models::jina_bert::Config`;
- instantiate `candle_transformers::models::jina_bert::BertModel`;
- use `VarBuilder::from_mmaped_safetensors` for weights; this is the only `unsafe` block and must be isolated with a comment explaining Candle's mmap contract;
- select `Device::new_metal(0)` on macOS when available, otherwise `Device::Cpu`, with a warning on fallback;
- tokenize with `Tokenizer::from_file(tokenizer.json)`, batch encode, truncation max `8192`, padding to longest batch;
- run model forward on `input_ids`;
- mean-pool sequence output using attention mask from tokenizer padding;
- L2-normalize each embedding;
- validate every returned vector is `config.embedding_dimensions` (`768`).

Do not add runtime model switching or quantization. Do not hit HuggingFace in tests.

### `indexer/path.rs`

Centralize path rules:
- DB paths are slash-separated paths relative to `config.target_dir`;
- reject paths outside target dir;
- normalize platform separators;
- resolve relative import module paths with POSIX semantics matching Python `_resolve_import_path`;
- expose `should_index(path, metadata, config)` for watcher and walker.

### `indexer/walk.rs`

Use `ignore::WalkBuilder`:
- `add_custom_ignore_filename(".loomignore")`;
- `git_ignore(true)`, `git_global(true)`, `git_exclude(true)`, `parents(true)`;
- `hidden(false)` so dotfiles can be indexed if extension matches, except explicit excluded dirs;
- `max_filesize(Some(config.max_file_size_bytes as u64))`;
- `threads(rayon_threads)`;
- `filter_entry` to skip `.loom/`, `.git/`, `node_modules/`, and `config.excluded_dirs`.

The walker emits absolute paths plus relative DB paths into the discovery channel after content-hash skip checks.

### `indexer/pipeline.rs`

Define:

```rust
pub struct IndexPipeline<E: Embedder> { ... }
pub struct IndexResult {
    pub indexed: usize,
    pub skipped: usize,
    pub deleted: usize,
    pub symbols: usize,
    pub edges: usize,
    pub embeddings: usize,
    pub resolved: usize,
    pub cochange_pairs: usize,
    pub errors: usize,
}
```

Public API:

```rust
impl<E: Embedder> IndexPipeline<E> {
    pub fn full_index(&self) -> Result<IndexResult>;
    pub fn incremental_index<I>(&self, changed_paths: I) -> Result<IndexResult>
    where
        I: IntoIterator<Item = PathBuf>;
}
```

Full index:
1. discover files with `ignore::WalkParallel`;
2. skip unchanged files by SHA-256 against `LoomDb::get_file_hash`;
3. parse and insert changed files;
4. embed symbols in batches;
5. resolve all unresolved edges;
6. run git analysis only if `config.enable_git_analysis`;
7. rebuild graph if a graph handle is supplied.

Full index should not blindly clear all tables by default, because Python full index is content-hash idempotent. Add `clear_index()` for future clean rebuilds, but `full_index()` should remove and replace each changed file and skip unchanged files.

Incremental index:
- deleted paths call `db.remove_file(relative_path)`;
- new/modified paths follow the same parse/embed/write path;
- unsupported paths are ignored, but deleted indexed paths still remove stale DB rows;
- re-run Phase 2 for all unresolved edges because newly added symbols can resolve old edges;
- never run git analysis from watcher-driven incremental updates.

Parse errors are per-file nonfatal: log the full error stack/context, increment `errors`, and continue. Embedder and DB failures are batch-fatal and return `Err`, because partial vectors/transactions corrupt result quality.

### Pipeline Data Flow

```text
ignore::WalkParallel
  threads = rayon_threads
  `-- bounded crossbeam(256) FileJob
       |
rayon parser pool
  threads = rayon_threads
  fs::read + parse_file + edge normalization
  `-- bounded crossbeam(64) ParsedFile
       |
embedder batch stage
  one worker, batch_size = 128 CPU / 256 Metal, max_wait = 50ms
  `-- bounded crossbeam(16) EmbeddedBatch
       |
SQLite writer
  one writer, transaction_size = 500 files or 1000 symbols
```

This is a synchronous core pipeline using `crossbeam-channel`, not `tokio::mpsc`, because `loom-core` currently has no async runtime dependency and `loom-mcp` is not in scope. Future async MCP handlers must call `full_index`/`incremental_index` via `spawn_blocking`; never call `rayon` parallel iterators on tokio worker threads.

Thread budget:
- `rayon_threads = available_parallelism().saturating_sub(1).clamp(1, 8)` by default;
- embedder worker = 1 because batching beats concurrent model calls;
- SQLite writer = 1 because `LoomDb` already serializes writes with a mutex;
- if a future `loom-mcp` tokio runtime calls this pipeline, tokio workers should be `max(2, available_parallelism / 2)`.

### `indexer/resolver.rs`

Implement `EdgeResolver` as a DB-backed service. It must not access `LoomDb` private connections.

Store helpers needed:
- `clear_index()`;
- `set_file_hashes(&[(String, String)])`;
- `list_symbol_files() -> Vec<String>`;
- `get_import_edges_with_source_file() -> Vec<ImportEdgeRow>`;
- `find_symbols_like_name(pattern, file: Option<&str>, limit)`;
- `resolve_edges_batch(&[(edge_id, target_id, confidence)])`;
- `get_source_symbol(edge.source_id)` can reuse `get_symbol_by_id`;
- co-change helpers with recency, below.

Resolution order must match Python:
1. exact `target_file + target_name`, confidence `1.0`;
2. import-resolved lookup using `(source_file, first_target_segment)` import map, confidence `0.95`;
3. `this.X` same-file class method lookup, confidence `0.95`, same-file suffix fallback `0.9`;
4. target file suffix match, confidence `0.9`;
5. qualified/full dotted unique match, confidence `0.8`;
6. global unique simple-name match, confidence `0.6`;
7. uppercase dotted `Class.method` exact unique match, confidence `1.0`.

Import edge preservation:
- for `relationship == "imports"`, source is the first symbol in the importing file;
- `target_name` stores the local binding;
- `target_file` stores normalized module path;
- `original_name` stores exported/original name if alias differs.

### `watcher.rs`

Use `notify::recommended_watcher` and implement Loom-owned debouncing. Do not use `notify-debouncer-*`; Python behavior needs content-hash dedupe and delete semantics.

Types:

```rust
pub struct LoomWatcher { ... }
pub struct WatcherHandle { ... }
pub trait ChangeHandler: Send + Sync {
    fn handle_changes(&self, paths: Vec<PathBuf>) -> Result<()>;
}
```

Behavior:
- default debounce = `Duration::from_millis(500)`;
- collect create/modify/delete/move destination paths into `BTreeSet<PathBuf>`;
- maintain per-path SHA-256 map for modify dedupe;
- create/move/delete always enqueue if extension/exclusion allows;
- ignore `.loom/`, `.git/`, `node_modules/`, configured excluded dirs, unsupported extensions, oversized existing files, and `.loomignore` patterns;
- on flush, call `IndexPipeline::incremental_index`;
- log callback errors with full context and keep watcher alive;
- expose `stop()`/drop handle so the future MCP server can shut down cleanly.

### `git_analyzer.rs`

Use a trait boundary:

```rust
pub trait CommandRunner: Send + Sync {
    fn run(&self, cmd: &mut std::process::Command, timeout: Duration) -> Result<CommandOutput>;
}
```

Production runner shells out; tests use fake output. This is the external boundary.

Commands:
- repo check: `git rev-parse --is-inside-work-tree`;
- analysis: `git log --follow --name-only --max-count=N --pretty=format:---COMMIT---`.

Note: `--follow` is path-oriented in git, so repository-wide rename tracking is imperfect. Keep the requested command shape for this pipeline, document that file-level co-change is best-effort, and preserve deterministic parsing.

Parse rules:
- split commits by sentinel;
- filter to `config.watch_extensions`;
- normalize to target-relative slash paths when possible;
- skip commits with fewer than 2 files or more than `git_max_files_per_commit`;
- count canonical lexicographic pairs;
- recency score: for commit index `i` starting at 0, add `1.0 / (1.0 + i as f64)` to the pair's recency accumulator; final recency is clamped to `0.0..=1.0`.

Store schema change:

```sql
ALTER TABLE cochange ADD COLUMN recency REAL NOT NULL DEFAULT 0.0;
```

Since schema creation is fresh in Rust, include `recency` in `CREATE TABLE`. Add migration-safe code that checks `PRAGMA table_info(cochange)` and adds the column if missing.

Store methods:
- `upsert_cochange(file_a, file_b, frequency, recency)`;
- `get_cochange(file_a, file_b) -> Option<CochangeRow>`;
- keep `get_cochange_frequency` for existing tests;
- update `get_top_cochanges` ordering to `frequency DESC, recency DESC`.

Timeout behavior:
- git log timeout returns empty result with warning;
- non-timeout runner errors return `Err`;
- nonzero git log exit returns `Err` with stderr, except repo check returns `false`.

## Error Handling

Extend `LoomError` with structured variants:
- `EmbedderDownload`;
- `EmbedderTokenizer`;
- `EmbedderModel`;
- `EmbedderDevice`;
- `EmbeddingDimension`;
- `IndexerIo`;
- `IndexerPath`;
- `IndexerChannel`;
- `Watcher`;
- `GitCommand`;
- `GitParse`.

Every `except` equivalent must log context. Rust functions should return `Result`; only per-file parser failures are downgraded into `IndexResult.errors`.

## Mock Boundaries

Mock external dependencies only:
- HuggingFace/model download via `ModelSource`;
- actual Candle inference in pipeline integration tests via `MockEmbedder`;
- git subprocess via `CommandRunner`;
- notify OS event stream by directly feeding watcher event handler/debouncer in tests.

Do not mock:
- `LoomDb`;
- parser registry/adapters within one hop;
- resolver DB lookups;
- file hashing/path normalization.

Use real temp directories and real SQLite for integration tests.

## Research Notes

### Embedding Engine

| Criteria | Candle | ort / fastembed-rs | Burn |
|----------|--------|--------------------|------|
| Fit | Native Rust tensors + transformers | ONNX Runtime wrapper | Training-oriented Rust ML |
| Jina support | `candle_transformers::models::jina_bert` exists | fastembed supports Jina through ONNX | no clear JinaBERT path |
| Memory | safetensors mmap via Candle | can reproduce ONNX arena issue unless carefully tuned | less proven for this model |
| Device | CPU, Metal feature on macOS | CPU/CUDA depending ORT packaging | backend-dependent |
| Distribution | Rust crates, no ORT shared lib | ORT shared library problem | larger implementation risk |
| Source | [Candle docs](https://huggingface.co/docs/transformers/en/community_integrations/candle), [JinaBERT module](https://docs.rs/candle-transformers/latest/candle_transformers/models/jina_bert/index.html) | prior RR notes | prior RR notes |

**Decision:** Candle. It has the exact JinaBERT implementation and mmap safetensors path this pipeline needs.

### Model Download / Cache

| Criteria | `hf-hub` | manual `reqwest` |
|----------|----------|------------------|
| Cache | HF-compatible cache builder, custom cache dir | must implement locking/layout |
| API | `ApiBuilder::with_cache_dir`, progress option | bespoke |
| Tests | wrap behind `ModelSource` | wrap behind `ModelSource` |
| TLS | choose `rustls-tls` | choose `rustls-tls` |
| Source | [hf-hub ApiBuilder docs](https://rustdocs.webschool.au/hf_hub/api/sync/struct.ApiBuilder.html) | n/a |

**Decision:** `hf-hub` with `default-features = false, features = ["ureq", "rustls-tls"]`, wrapped by `ModelSource`.

### Tokenization

| Criteria | `tokenizers` | custom tokenizer |
|----------|--------------|------------------|
| Compatibility | loads HF `tokenizer.json` | high regression risk |
| Batch API | native encode batch, padding/truncation utilities | must rebuild |
| Maintenance | HuggingFace crate | Loom-owned liability |
| Source | [tokenizers Rust docs](https://docs.rs/tokenizers/latest/tokenizers/tokenizer/index.html) | n/a |

**Decision:** `tokenizers`; configure truncation/padding explicitly in code.

### File Walking

| Criteria | `ignore` | `walkdir` |
|----------|----------|-----------|
| Gitignore | built in | manual |
| Parallel walk | `WalkParallel` | no |
| Custom ignore | `add_custom_ignore_filename(".loomignore")` | manual |
| Used by | ripgrep ecosystem | broad but lower-level |
| Source | [WalkBuilder docs](https://docs.rs/ignore/latest/ignore/struct.WalkBuilder.html) | n/a |

**Decision:** `ignore::WalkParallel`.

### File Watching

| Criteria | `notify` | polling loop |
|----------|----------|--------------|
| Platforms | native backends for macOS/Linux/Windows | portable but inefficient |
| API | `recommended_watcher` | Loom-owned scanner |
| Debounce | external crates available, but custom needed | custom |
| Source | [notify docs](https://docs.rs/notify/latest/notify/) | n/a |

**Decision:** `notify` with Loom-owned debounce/hash logic.

### Channels

| Criteria | `crossbeam-channel` | `tokio::sync::mpsc` |
|----------|---------------------|---------------------|
| Runtime requirement | none | tokio runtime |
| Pipeline fit | synchronous `loom-core` stages | better for MCP/server layer |
| Backpressure | bounded channels | bounded channels |
| Risk | simple | easy to accidentally mix rayon on tokio workers |

**Decision:** `crossbeam-channel` inside `loom-core`; future MCP calls wrap the pipeline in `spawn_blocking`.

## Verification

Developer must run:

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
```

Required focused tests:
- embedder text construction and dimension validation;
- model-source downloader mocked, no network;
- pipeline full index indexes changed files and skips unchanged files;
- incremental delete removes symbols/vectors/hash and nullifies inbound target edges;
- parser failure increments errors without aborting the batch;
- resolver covers exact, import, alias, `this.X`, suffix, qualified, unique, ambiguous-unresolved paths;
- watcher debounce coalesces events and dedupes same-content modify;
- watcher queues create/delete/move destination;
- watcher ignores `.loom/`, `.git/`, `node_modules/`, configured dirs, unsupported extensions, `.loomignore`;
- git parser filters extensions, noisy commits, single-file commits, canonical pairs, frequency, recency;
- git timeout returns empty result; non-timeout runner errors propagate;
- DB cochange migration and `get_top_cochanges` order.
