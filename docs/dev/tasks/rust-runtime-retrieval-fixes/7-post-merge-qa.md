# Post-Merge QA — rust-runtime-retrieval-fixes

**Pipeline:** rust-runtime-retrieval-fixes  
**Date:** 2026-05-11  
**Commits:** 35cea9f, 3f5e589, 6ce1d19, 893daf9  
**Status:** PASS

## Rust Gates

| Check | Result |
|---|---|
| `cargo build --workspace` | PASS |
| `cargo test --workspace` | PASS — 67 tests |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo llvm-cov --version` | NOT AVAILABLE — subcommand not installed |

## Focused Rust Checks

| Surface | Evidence | Result |
|---|---|---|
| sqlite-vec/blob backend + schema status | `vector_backend_selection_and_schema_version_are_visible` | PASS |
| old schema migration / recency column | `old_schema_without_recency_upgrades_idempotently` | PASS |
| adapter-derived config defaults | `config_defaults_and_missing_toml_fallback` | PASS |
| strict Candle failure | `default_embedder_candle_failure_is_strict_by_default` | PASS |
| explicit hashing fallback visibility | `default_embedder_explicit_fallback_reports_degraded_hashing` | PASS |
| hashing mode skips Candle | `default_embedder_hashing_mode_skips_candle_loader` | PASS |
| evolutionary recency scoring | `evolutionary_scoring_uses_recency_as_tie_breaker` | PASS |
| watcher rename batching | `debouncer_queues_create_delete_and_move_destination` | PASS |
| incremental rename cleanup | `incremental_rename_removes_old_path_and_indexes_new_path` | PASS |
| incremental delete cleanup | `incremental_delete_removes_symbols_vectors_and_hash` | PASS |
| MCP rename path | `changed_paths_helper_handles_rename_without_stale_index_rows` | PASS |
| MCP status avoids eager embedder load | `status_opens_db_without_loading_embedder` | PASS |
| vector backend switch freshness | `full_index_rebuilds_vectors_when_backend_changes` | PASS |
| active embedder fingerprint freshness | `full_index_rebuilds_vectors_when_embedding_fingerprint_changes` | PASS |
| degraded fallback fingerprint | `default_embedder_explicit_fallback_reports_degraded_hashing` | PASS |
| cochange replacement | `cochange_and_stats` | PASS |

## Python Reference Gates

| Check | Result |
|---|---|
| `UV_CACHE_DIR=/private/tmp/uv-cache uv run pytest --tb=short` | PASS — 855 passed |
| Coverage | PASS — 91.69% total |
| `UV_CACHE_DIR=/private/tmp/uv-cache uv run ruff check` | PASS |
| `UV_CACHE_DIR=/private/tmp/uv-cache uv run mypy` | PASS — 25 files |

## Result

No open actionable QA bugs. Final audit recheck returned `SPARKLING` with 0 findings. Rust line coverage was not collected because `cargo-llvm-cov` is not installed in this environment.
