# Dev Report - rust-indexer

## Implementation Summary

Implemented the Rust indexer runtime surface in `crates/loom-core`:

- Added embedder traits, `ModelSource` boundary, HuggingFace cache source, Candle device selection, mmap safetensors load, JinaBERT forward path, mean pooling, L2 normalization, symbol text builder, and dimension validation.
- Added staged full/incremental `IndexPipeline` with SHA-256 content-hash skips, real parser integration, batched SQLite writes, embedding batches, deletion cleanup, and DB-backed two-phase resolver.
- Added resolver strategies for exact file/name, import and alias resolution, `this.X` same-file lookup, suffix matching, qualified lookup, unique global lookup, and uppercase dotted class-method lookup.
- Added watcher debouncer using `notify`, content-hash modify dedupe, create/delete/move handling, configured exclusion filtering, and `.loomignore` matching.
- Added git analyzer with mockable `CommandRunner`, repo check, requested `git log --follow --name-only` command shape, parser filtering, canonical pairs, frequency, and recency scores.
- Extended config, errors, store helpers, cochange recency schema migration, and public module exports.

MCP/search/server registration stayed out of scope.

Fix loop iteration 1 addressed `BUG-RUST-INDEXER-001` through `BUG-RUST-INDEXER-004` only:

- Import alias member resolution now checks `original_name.member` before global fuzzy fallback, preserving import-map confidence `0.95`.
- Uppercase qualified class-method exact matches now keep confidence `1.0` instead of being captured by the lower-confidence dotted strategy.
- Indexing now completes embedding before writing parsed file state, so embedder failure leaves symbols, vectors, and `index_meta` untouched for that file.
- `SystemCommandRunner` now enforces the supplied timeout for production git commands and reports timed-out command output.

## Test Coverage

Added focused Rust integration tests:

- `crates/loom-core/tests/embedder.rs`: symbol text contract and mocked model-source Candle boundary.
- `crates/loom-core/tests/indexer_pipeline.rs`: full index, unchanged skip, incremental delete cleanup, fatal embedder error.
- `crates/loom-core/tests/watcher.rs`: debounce dedupe, create/delete/move destination, exclusions, unsupported extensions, `.loomignore`.
- `crates/loom-core/tests/git_analyzer.rs`: parser filtering/scoring, timeout empty result, non-timeout error propagation.
- `crates/loom-core/tests/test_qa_rust_indexer.rs`: QA regressions for import alias member confidence, uppercase qualified method confidence, and embedder failure DB atomicity.

Rust test result: `45 passed`.

Python coverage result: `855 passed, 1 warning`, total coverage `91.69%`.

## Runbook

Commands run:

- `cargo build --workspace` - passed
- `cargo test --workspace` - passed, 45 tests
- `cargo clippy --workspace -- -D warnings` - passed
- `cargo fmt --all -- --check` - passed
- `UV_CACHE_DIR=/private/tmp/uv-cache uv run pytest` - passed, 855 tests, 91.69% coverage, 1 pytest config warning for unknown `asyncio_mode`
- `UV_CACHE_DIR=/private/tmp/uv-cache uv run ruff check` - passed
- `UV_CACHE_DIR=/private/tmp/uv-cache uv run mypy` - passed
- `UV_CACHE_DIR=/private/tmp/uv-cache uv run ruff format --check` - passed

No new required environment variables.
