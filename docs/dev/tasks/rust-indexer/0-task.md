> Author: wave

# Task: rust-indexer

Wave: rust-rewrite

Source task file: `wave.md`

## Scope

Implement Rust embedding, indexing, file watching, and evolutionary coupling pipeline.

## Included Wave Tasks

### 6. Candle embedding engine

Implement an embedding subsystem using `candle-core`, `candle-nn`, `candle-transformers`, and `tokenizers`.

Requirements:
- load `jina-embeddings-v2-base-code` from safetensors
- mmap/zero-copy weights where supported
- Metal backend on macOS, CPU fallback elsewhere
- first-run model download into `~/.loom/models/`
- batch API: `Vec<String>` -> `Vec<Vec<f32>>`
- 768-dim vectors
- mock external downloads in tests

Boundaries: no runtime model switching and no quantization in v1.

### 7. Indexer pipeline + two-phase resolver

Implement the full indexing pipeline:

`ignore::WalkParallel` -> bounded channel -> rayon parser stage -> bounded channel -> embedder batch stage -> bounded channel -> SQLite writer.

Requirements:
- gitignore and `.loomignore` aware walking
- content SHA-256 dedup via `index_meta`
- full and incremental reindex
- batched DB transactions
- Phase 1 inserts symbols and unresolved edges
- Phase 2 resolves unresolved edges using fuzzy lookup
- no unbounded channels
- no `par_iter()` on tokio worker threads

### 8. File watcher + git analyzer

Implement:
- `notify` watcher with configurable debounce, default 500ms
- ignore `.loom/`, `node_modules/`, `.git/`, `.loomignore` patterns
- incremental reindex on changed files while MCP server is running
- git analyzer using `git log --follow --name-only`
- co-change frequency and recency scoring stored in `cochange`

Git analysis runs on explicit reindex, not every file change.

## Dependencies

Depends on `rust-foundation` and `rust-parsers`.

## Required Verification

- `cargo build --workspace`
- `cargo test --workspace`
- integration tests with real SQLite and mocked external model/network
- watcher debounce tests
- git analyzer parser tests
- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`

