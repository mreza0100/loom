> Author: planner

# Plan — rust-indexer

## Feature Context
Implement the Rust indexer slice for the rust-rewrite wave: Candle embeddings, staged indexing, incremental file watching, and evolutionary coupling, replacing the current Python `src/loom/indexer/` runtime while reusing the committed Rust core/store/parser foundation.

## Current State
- `crates/loom-core/src/models.rs` already defines owned `Symbol`, `ParsedEdge`, `Edge`, `FileState`, `SearchResult`, `CoupledSymbol`, `CouplingScore`, and `StoreStats`; these mirror the Python data model and are `serde` serializable.
- `crates/loom-core/src/config.rs` already loads `.loom/config.toml` and carries core knobs: `target_dir`, `db_path`, `watch_extensions`, `debounce_seconds`, `embedding_model`, `embedding_dimensions`, `max_file_size_bytes`, excluded dirs, coupling weights, and git analysis limits. Default debounce is currently `2.0`, while this task requires watcher default `500ms`.
- `crates/loom-core/src/store/mod.rs` already implements `LoomDb` with a single mutex writer, r2d2 readers, WAL pragmas, batched `insert_symbols`, `insert_edges`, `insert_embeddings`, content hashes in `index_meta`, unresolved edge queries, `resolve_edge`, `remove_file`, FTS5 search, and file-level `cochange`.
- `crates/loom-core/src/store/vector.rs` currently stores embeddings as raw BLOBs and does brute-force L2 search in Rust. It is useful as a fallback/test store, but it is not the task's intended sqlite-vec implementation and returns L2 distance rather than sqlite-vec cosine/L2 semantics.
- `crates/loom-core/src/graph.rs` already builds a `petgraph::DiGraph` from resolved DB edges and supports dependents, dependencies, shortest path, impact radius, centrality, and neighbor traversal. It is not CSR yet, but it is enough for this pipeline to trigger graph rebuilds after indexing.
- `crates/loom-core/src/parsers/` already has a `LanguageAdapter` trait, `AdapterRegistry`, `parse_file`, and adapters for JavaScript/TypeScript, Python, Go, Java, Rust, and C#. Parser output is `ParseResult { symbols, edges }`; helpers preserve full call expressions such as `this.hooks.make.callAsync`.
- `crates/loom-mcp/src/main.rs` is only a foundation shell that loads config and resolves the DB path. No Rust MCP tools, search engine, pipeline bootstrapping, or watcher startup exists yet.
- Python source of truth for behavior lives in `src/loom/indexer/embedder.py`, `src/loom/indexer/pipeline.py`, `src/loom/indexer/watcher.py`, and `src/loom/indexer/git_analyzer.py`. The Python pipeline is sequential, two-phase, content-hash aware, batches embeddings in chunks of 500, resolves imports with five strategies, and only runs git analysis on full reindex.
- Python tests under `tests/test_pipeline.py`, `tests/test_watcher.py`, and `tests/test_git_analyzer.py` define important edge behavior: unchanged files skip, deleted files remove symbols/vectors/FTS/hash metadata, parse failures do not abort the whole batch, watcher dedupes by content hash, git subprocess calls are mocked, timeouts return empty cochange results, and non-timeout subprocess errors propagate.

## Gaps & Needed Changes
- Add Rust dependencies in `crates/loom-core/Cargo.toml`:
  - `candle-core`, `candle-nn`, `candle-transformers`, `tokenizers`, `hf-hub` or equivalent download client, `safetensors`, `memmap2` where Candle APIs need it, and `indicatif` for first-run progress.
  - `sha2`, `ignore`, `rayon`, `crossbeam-channel` or `tokio::sync::mpsc`, `tokio`, `notify`, `globset`, and a small time crate such as `time` or `chrono` if recency timestamps need explicit parsing.
  - Keep external network/model download mockable behind traits; do not let tests hit HuggingFace.
- Add `crates/loom-core/src/embedder.rs`:
  - Define an `Embedder` trait with `embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>`, `embed_single`, and `build_symbol_text(name, kind, context)`.
  - Implement `CandleEmbedder` that loads `jinaai/jina-embeddings-v2-base-code` from `~/.loom/models/`, downloads missing `config.json`, `tokenizer.json`, and safetensors files on first run, mmap/zero-copy loads weights where Candle supports it, selects Metal on macOS and CPU fallback elsewhere, and validates 768-dimensional output.
  - Add `MockEmbedder` or test helper under tests only, so integration tests can exercise the pipeline without downloading a model. Mock the external downloader/model source, not `LoomDb` or parser internals.
  - Return structured `LoomError` variants for model download, tokenizer load, model load, device selection, and embedding dimension mismatch; no swallowed errors.
- Add `crates/loom-core/src/indexer/mod.rs`, `pipeline.rs`, and `resolver.rs`:
  - Implement `IndexPipeline<E: Embedder>` with `full_index()`, `incremental_index(changed_paths)`, and internal staged indexing.
  - File discovery must use `ignore::WalkBuilder` / `WalkParallel`, respect `.gitignore`, `.loomignore`, `config.excluded_dirs`, `config.watch_extensions`, `.loom/`, `.git/`, `node_modules/`, and `max_file_size_bytes`.
  - Content hash must be SHA-256 over file bytes. Compare against `LoomDb::get_file_hash`; unchanged files skip before parse/embed work.
  - Use bounded channels between stages: discovery -> parse -> embed -> write. Keep every queue bounded to avoid recreating the original Python/ONNX memory incident in a better font.
  - Parser stage should run on a dedicated rayon pool, not tokio worker threads. The existing `parse_file(path, Some(bytes), &registry)` currently creates a parser per call; acceptable for v1, but do not call `par_iter()` from async tool handlers.
  - Writer stage should use batched DB transactions via existing `insert_symbols`, `insert_edges`, `insert_embeddings`, and `set_file_hash`. Add a `set_file_hashes` batch helper if single-row writes become the bottleneck.
  - Preserve Python import-edge handling: for `relationship == "imports"`, use file anchor symbol as source, store local binding in `target_name`, normalized module path in `target_file`, and exported name in `original_name` when aliased.
  - Implement Phase 2 in `resolver.rs`: build known-files and import map from the DB, resolve all unresolved edges using the Python five-strategy order:
    1. exact file + name, confidence `1.0`;
    2. import-resolved, including alias/original name and dotted member expressions, confidence `0.95`;
    3. `this.X` same-file/class method resolution, confidence `0.95` or fallback `0.9`;
    4. file suffix match, confidence `0.9`;
    5. qualified/fuzzy/global unique name matches, confidence `0.8` / `0.6`, plus uppercase dotted class-method exact match at `1.0`.
  - Add store helpers needed by resolver without reaching through private connection state: list distinct files, list import edges with source file, same-file `LIKE` lookup, fuzzy qualified lookup, and batch `resolve_edges`.
  - Full reindex should support a true clean rebuild path. Existing `remove_file` supports incremental replacement, but full reindex currently needs either table-clearing helpers or a clear-and-walk method. Implement explicit `clear_index()` on `LoomDb` rather than manually deleting tables from the pipeline.
- Add `crates/loom-core/src/watcher.rs`:
  - Implement a `notify` watcher wrapper plus a debouncer that collects create/modify/delete/move destinations into a `BTreeSet<PathBuf>`.
  - Default debounce for this task should be 500ms. Either change `LoomConfig::default_for_target().debounce_seconds` to `0.5` or add a watcher-specific default while documenting the compatibility decision.
  - Ignore `.loom/`, `.git/`, `node_modules/`, configured excluded dirs, unsupported extensions, oversized files, and `.loomignore` patterns.
  - Maintain per-path SHA-256 hashes to avoid reindexing unchanged modify events, but always enqueue creates/moves/deletes the way Python does.
  - Flush callback should trigger `IndexPipeline::incremental_index`; git analysis must not run from watcher flushes.
  - Tests should cover debounce coalescing with fake time where possible, hash dedupe, create/delete/move behavior, excluded dirs, unsupported extensions, and callback error propagation/logging.
- Add `crates/loom-core/src/git_analyzer.rs`:
  - Implement `GitAnalyzer { target_dir, watch_extensions, max_commits, max_files_per_commit }` that shells out to `git` only at runtime. Tests must mock the command runner.
  - Use `git rev-parse --is-inside-work-tree` for repo detection.
  - Use `git log --follow --name-only --max-count=N --pretty=format:---COMMIT---` or, if `--follow` cannot be combined robustly for all-file history, document and test the chosen equivalent. The task explicitly calls for `--follow --name-only`.
  - Parse commits into file groups, filter by configured extensions, skip commits with fewer than two files or more than `git_max_files_per_commit`, emit canonical pairs, and compute both frequency and recency.
  - Store co-change in `cochange` with frequency and recency scoring. Current schema only has `file_a`, `file_b`, and `frequency`; add `recency REAL NOT NULL DEFAULT 0.0` or `last_seen`/`recency_score` columns plus store methods like `upsert_cochange(file_a, file_b, frequency, recency)`, `get_cochange(file_a, file_b)`, and updated `get_top_cochanges`.
  - Keep timeout behavior equivalent to Python: git log timeout returns an empty result with a warning; unexpected command-runner errors propagate with full context.
- Update `crates/loom-core/src/lib.rs` to export the new modules and core traits/types used by future `loom-mcp` search/server work.
- Keep `crates/loom-mcp/src/main.rs` mostly untouched for this pipeline unless a minimal manual `reindex` hook is needed for integration tests. Full MCP tool registration belongs to the later `rust-search-server` task.

## Integration Surface
- `LoomConfig`:
  - `embedding_model`, `embedding_dimensions`, `target_dir`, `db_path`, `watch_extensions`, `excluded_dirs`, `max_file_size_bytes`, `debounce_seconds`, `enable_git_analysis`, `git_max_commits`, and `git_max_files_per_commit` are direct pipeline inputs.
  - Add model cache path only if needed; default should be `~/.loom/models/` and should be overridable in tests.
- `Embedder` trait:
  - `embed(&[String]) -> Result<Vec<Vec<f32>>>` must preserve input order and return one vector per text.
  - `build_symbol_text` should match Python: `"{kind} {name}\n{context}"`.
- `IndexPipeline` API:
  - `full_index() -> Result<IndexResult>` returns counts for indexed files, symbols, edges, resolved edges, deleted files if relevant, embeddings, and cochange pairs.
  - `incremental_index(paths: impl IntoIterator<Item = PathBuf>) -> Result<IndexResult>` handles new/changed/deleted paths and re-runs Phase 2 for all unresolved edges.
  - Git analysis runs from explicit full reindex/reindex, not from watcher-driven incremental flushes.
- `LoomDb` additions:
  - `clear_index`, `list_indexed_files` or `list_symbol_files`, `get_import_edges_with_source_file`, same-file `LIKE` lookup, global qualified lookup, batch hash writes, batch edge resolution, and cochange recency support.
  - Existing `remove_file`, `insert_symbols`, `insert_edges`, `insert_embeddings`, `get_unresolved_edges`, `resolve_edge`, and fuzzy lookup should be reused rather than duplicated.
- Parser integration:
  - Keep using `AdapterRegistry::with_builtin_adapters()` and `parse_file`.
  - Parser output paths should be rewritten to paths relative to `config.target_dir` before DB insert, matching Python behavior.
- Watcher integration:
  - Watcher callback must pass absolute changed paths into `incremental_index`; the pipeline converts to relative DB paths internally.
  - A stop/shutdown handle is required so the MCP server can exit cleanly later.

## Risks & Dependencies
- Candle/JinaBERT loading is the highest unknown. Jina uses ALiBi positional encoding; verify Candle's exact model struct and safetensors naming before wiring the downloader too deeply.
- Metal support may be build-target sensitive. Gate it with `cfg(target_os = "macos")` and feature flags so Linux/CI CPU builds stay boring, because boring CI is a gift humanity rarely deserves.
- `sqlite-vec` is not actually wired in the Rust store yet despite the Python implementation using it. Decide whether this pipeline must replace `BlobVectorStore` now or leave sqlite-vec for the search task; if left for later, document that vector search is functionally correct but not final.
- The task asks for cochange by `symbol_a_id`/`symbol_b_id`, while current Python/Rust store tracks file-level pairs. The plan should implement file-level cochange now to match existing scoring/search APIs unless the search-server pipeline simultaneously changes scoring to symbol-level. If symbol-level is required, it needs a mapping step from cochanged files to symbols and a schema migration.
- Existing Rust graph uses `DiGraph`, not CSR. This is acceptable for this pipeline's graph rebuild hooks, but the graph task's CSR intent remains technical debt unless already handled elsewhere.
- `ignore::WalkParallel` and `notify` may emit platform-specific paths. Normalize all stored DB paths to slash-separated relative paths for deterministic resolver and cochange behavior.
- Full reindex table clearing must preserve schema and FTS/vector consistency. Use DB helpers and tests rather than pipeline-owned SQL strings.
- Do not run git commands during planning or tests except through mocked command-runner tests. The implementation may shell out at runtime by design.

## Research Needed
- Candle implementation details for `jinaai/jina-embeddings-v2-base-code`: exact Candle module type, pooling strategy, normalization expectation, tokenizer truncation/padding settings, and safetensors file names.
- `hf-hub` offline/cache controls for deterministic tests and configurable `~/.loom/models/` location.
- `notify` v7/v8 API shape and whether to use built-in debouncer helpers or a Loom-owned debounce layer. Prefer Loom-owned logic because Python behavior has specific hash/dedupe semantics.
- `ignore` crate `.loomignore` support: confirm whether adding a custom ignore filename is enough or whether `OverrideBuilder`/custom `WalkBuilder` plumbing is required.
- `sqlite-vec` Rust static-loading API if this pipeline is expected to replace `BlobVectorStore` immediately.
- Practical semantics of `git log --follow --name-only` for repository-wide cochange. If `--follow` is path-oriented, use the closest robust command and document the tradeoff before implementation.
