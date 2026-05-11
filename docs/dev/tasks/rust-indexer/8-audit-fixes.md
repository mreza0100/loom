# Audit Fixes — rust-indexer

## Result

PASS. All audit findings were addressed and regression-tested where practical.

## Fixes

- Replaced bounded parser fanout with Rayon parallel collection to remove the >64 file deadlock risk.
- Made per-file index replacement transactional across old-row removal, symbols, edges, embeddings, and content hash updates.
- Canonicalized index paths and rejected relative path escapes before incremental indexing.
- Removed stale indexed files during full reindex when files disappear from disk.
- Configured Candle tokenization truncation at 8192 tokens before model inference.
- Reworked watcher debounce to use one worker thread instead of one sleeping thread per event.
- Removed unused `rusqlite/load_extension`, `time`, dead error variants, and stale indexer structs/helpers.

## Regression Coverage

- `full_index_handles_more_files_than_old_parser_channel_bound`
- `full_index_removes_stale_deleted_files`
- `incremental_index_rejects_relative_path_escape`

## Validation

```text
cargo test --workspace -> PASS, 48 tests
cargo clippy --workspace --all-targets -- -D warnings -> PASS
UV_CACHE_DIR=/private/tmp/uv-cache uv run pytest -> PASS, 855 passed, coverage 91.69%
UV_CACHE_DIR=/private/tmp/uv-cache uv run ruff check -> PASS
UV_CACHE_DIR=/private/tmp/uv-cache uv run mypy -> PASS
```

No push performed.
