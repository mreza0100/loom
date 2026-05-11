# Dev Report — rust-runtime-retrieval-fixes

## Implementation Summary

- Added explicit runtime config for `vector_backend`, `embedding_backend`, `allow_hashing_embedder_fallback`, and `auto_watch`.
- Replaced drift-prone default extension/exclusion literals with `AdapterRegistry`-derived defaults plus the always-excluded directories.
- Added durable SQLite migrations via `PRAGMA user_version`, with `CURRENT_SCHEMA_VERSION = 4` and idempotent upgrade support for older cochange/index metadata schemas missing `recency` or `embedding_fingerprint`.
- Added a `VectorStore` backend identity surface, `SqliteVecStore`, explicit `BlobVectorStore` fallback, process-wide sqlite-vec registration, and `LoomDb` backend/schema status accessors.
- Pinned `sqlite-vec` to `=0.1.9` instead of the architecture doc's `0.1.10-alpha.3` because the alpha crate fails to compile locally with a missing `sqlite-vec-diskann.c` include.
- Made `DefaultEmbedder` strict by default: Candle errors now return `Err`; hashing is used only when configured directly or when explicit fallback is enabled, and status exposes degraded mode.
- Wired `LoomWatcher` into the Rust MCP server with `CoreState::handle_changed_paths()` for deterministic incremental indexing and graph refresh.
- Added additive `status()` fields: vector backend, embedder backend/degraded state, schema version, watcher active flag, and auto-watch config.
- Updated evolutionary scoring to use stored recency alongside frequency.
- Added active embedder fingerprints to index freshness so runtime fallback from Candle to hashing invalidates stale vectors instead of mixing embedding spaces.
- Replaced cochange rows atomically on full git analysis so stale evolutionary relationships do not survive changed git-analysis windows.
- Added a pinned dependency-audit GitHub workflow for Rust and Python lockfiles.

## Test Coverage

- Added/updated config, migration, vector backend, embedder strictness/fallback, recency scoring, status, and deterministic incremental-index watcher tests.
- External model loading remains mocked/avoided in tests; internal SQLite/indexing/search paths use real implementations.
- Numeric coverage was not collected because `cargo llvm-cov` is not installed in this environment.

## Fix Loop 1 — BUG-001

- Fixed watcher rename handling so `RenameMode::Both` queues the source path as a deletion candidate and the destination path as an index candidate.
- Added conservative handling for other rename/name modes: existing paths are queued for create/modify indexing and missing paths are queued for deletion.
- Added focused regressions for the debouncer, `IndexPipeline::incremental_index()`, and the MCP server `CoreState::handle_changed_paths()` path so renames leave no stale `index_meta`, symbol, or vector rows.
- BUG-RAW-PRINT is intentionally left unchanged: the reported `println!` calls are CLI stdout JSON for `status` and `reindex`, not diagnostic logging. The CLI boundary is allowed to write machine-readable command output to stdout; operational logs remain on tracing/stderr.

## Audit Fixes

- Fixed vector backend freshness after switching between `blob` and `sqlite-vec` by checking active-backend vector row coverage before skipping unchanged files.
- Added embedding fingerprints to `index_meta`, then tightened the fingerprint to use the live embedder identity, including degraded fallback state.
- Added regressions for backend switches, embedding fingerprint changes, and legacy schema upgrades.
- Added transactional cochange replacement during full index.
- Hardened the security workflow with SHA-pinned actions, pinned audit tools, and locked `uv export`.

## Runbook

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

All commands passed after final audit fixes. `cargo llvm-cov --version` was attempted and reported that the subcommand is not installed.
