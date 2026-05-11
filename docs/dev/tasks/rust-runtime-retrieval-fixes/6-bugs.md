# QA Bug Report — rust-runtime-retrieval-fixes

**Pipeline:** rust-runtime-retrieval-fixes  
**Date:** 2026-05-11  
**Fix loop:** 1 — BUG-001 watcher rename fix recheck  
**Status:** NONE  
**Result:** PASS

## Scope

QA re-ran the Rust runtime retrieval fix surfaces after the BUG-001 watcher rename fix:

- watcher rename/deletion stale-index behavior
- incremental indexing cleanup for stale `index_meta`, symbols, and vectors
- MCP server `CoreState::handle_changed_paths()` rename path
- sqlite-vec/blob vector backend coverage
- strict/embedder fallback status coverage
- schema migration/versioning coverage
- adapter-derived config defaults
- evolutionary recency scoring
- workspace validation commands

No git commands were run. Temporary adversarial QA tests were created, executed, and removed after verification.

## 360° Test Sweep

Angles covered before rerun:

| Dimension | Targeted probes |
|---|---|
| Inputs | `RenameMode::Both`, missing old path, existing new path, deletion path, query for renamed symbol |
| State | full index before rename, stale old file in DB, post-incremental graph refresh |
| Boundaries | one-file rename, one-file delete, zero stale rows expected |
| Sequences | full index -> rename event -> debouncer flush -> incremental index -> search/status |
| Timing | watcher debounce batching via deterministic debouncer flush, not OS timing |
| Error paths | missing path removal, vector deletion, hash cleanup |
| Data shapes | Python symbols with same name before/after rename |
| Environment | local cargo validation, missing `cargo llvm-cov` subcommand |
| Auth/Authz | N/A: local runtime/MCP test surface has no auth layer |
| Regressions | previous BUG-001 repro, existing watcher/indexer/server regression tests |

## Verification Run

| Command | Result |
|---|---|
| `cargo test -p loom-core --test test_qa_fix_loop_rename -- --nocapture` | PASS — 2 temporary QA tests passed |
| `cargo test --workspace` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo build --workspace` | PASS |
| `cargo llvm-cov --version` | NOT RUNNABLE — subcommand not installed |

Focused existing regression checks:

| Command | Result |
|---|---|
| `cargo test -p loom-core --test watcher debouncer_queues_create_delete_and_move_destination -- --nocapture` | PASS |
| `cargo test -p loom-core --test indexer_pipeline incremental_rename_removes_old_path_and_indexes_new_path -- --nocapture` | PASS |
| `cargo test -p loom-mcp server::tests::changed_paths_helper_handles_rename_without_stale_index_rows -- --nocapture` | PASS |

## Findings

| ID | Severity | Area | Status | Summary |
|---|---|---|---|---|
| BUG-001 | High | watcher / incremental indexing | FIXED | Rename events now queue old path deletion and new path indexing; incremental indexing removes stale file hash, symbols, and vectors. |

## BUG-001 Recheck — Rename Events No Longer Leave Stale Indexed Files

**Status:** FIXED  
**Evidence:**

- `crates/loom-core/src/watcher.rs` handles `RenameMode::Both` by calling `enqueue_deleted()` for the first path and `force_enqueue()` for the last path.
- `crates/loom-core/tests/watcher.rs` verifies rename batching includes both old and new paths.
- `crates/loom-core/tests/indexer_pipeline.rs` verifies `incremental_index([old, new])` leaves one indexed file, one symbol, one vector, no old hash, and the symbol attached to `new.py`.
- `crates/loom-mcp/src/server.rs` verifies `CoreState::handle_changed_paths([old, new])` has the same no-stale-row behavior through the MCP server path.
- Temporary QA repro additionally verified search returns the renamed symbol only from `new.py`.

## Compliance Notes

- The previous low-severity `BUG-RAW-PRINT` note is not counted as an open product bug in this fix loop. The remaining `println!` calls are intentional CLI stdout JSON for `status` and `reindex`, not diagnostic logging. Operational logs remain on tracing/stderr.
- Coverage could not be collected because `cargo llvm-cov` is not installed in this environment.

## Final Rating

| Surface | Score | Notes |
|---|---:|---|
| sqlite-vec backend | 8/10 | Existing vector backend tests pass. |
| embedder fallback | 9/10 | Strict/default and explicit degraded fallback tests pass. |
| schema migrations | 8/10 | Versioned migration tests pass. |
| adapter config | 9/10 | Registry-derived config tests pass. |
| recency scoring | 8/10 | Recency tie-breaker test passes. |
| MCP watcher | 9/10 | Rename/delete stale-index behavior passes across debouncer, pipeline, and MCP helper. |

**Overall:** 9/10. BUG-001 is fixed. No open actionable QA bugs remain for this pipeline.
