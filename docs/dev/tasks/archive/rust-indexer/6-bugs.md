# QA Bug Report - rust-indexer

Result: PASS - fix-loop QA iteration 1 verified.

## Test Files Reviewed

- `crates/loom-core/tests/test_qa_rust_indexer.rs`
- `crates/loom-core/tests/git_analyzer.rs`
- `crates/loom-core/tests/indexer_pipeline.rs`
- `crates/loom-core/tests/embedder.rs`

## Regression Verdict

### BUG-RUST-INDEXER-001 - Import alias member calls resolve through fuzzy fallback instead of alias/original-name strategy

Status: FIXED

Evidence:
- Targeted test: `qa_import_alias_member_resolution_uses_original_export_confidence` - PASS.
- Implementation now checks `original_name.method` during import-map resolution in `crates/loom-core/src/indexer/resolver.rs`.
- Verified confidence remains `0.95` for `AliasService.fetch` imported from `OriginalService.fetch`.

### BUG-RUST-INDEXER-002 - Uppercase qualified class-method exact match is ordered after lower-confidence fuzzy match

Status: FIXED

Evidence:
- Targeted test: `qa_uppercase_qualified_class_method_resolution_keeps_exact_confidence` - PASS.
- Resolver now returns `1.0` for uppercase dotted exact matches before the lower-confidence dotted fallback.
- Verified `Parser.parse` resolves to the exact method with confidence `1.0`.

### BUG-RUST-INDEXER-003 - Embedder failure leaves partial DB state and marks file hash as indexed

Status: FIXED

Evidence:
- Targeted test: `qa_embedder_failure_does_not_mark_file_indexed_without_vectors` - PASS.
- Pipeline now embeds parsed symbols before writing parsed file state to SQLite.
- Verified forced embedder failure leaves `0` symbols, `0` vectors, and no `index_meta` hash for the failed file.

### BUG-RUST-INDEXER-004 - Production git command runner accepts a timeout parameter but never enforces it

Status: FIXED

Evidence:
- Targeted test: `system_command_runner_enforces_timeout` - PASS.
- `SystemCommandRunner::run` now spawns the child process, polls `try_wait`, kills on deadline, and returns `timed_out: true`.
- Verified `sh -c "sleep 2"` with a `50ms` timeout exits in under one second.

## Checks

- `cargo test -p loom-core --test test_qa_rust_indexer -- --nocapture` - PASS: 3 passed.
- `cargo test -p loom-core --test git_analyzer system_command_runner_enforces_timeout -- --nocapture` - PASS: 1 passed.
- `cargo build --workspace` - PASS.
- `cargo test --workspace` - PASS: 45 passed.
- `cargo fmt --all -- --check` - PASS.
- `cargo clippy --workspace -- -D warnings` - PASS.
- `UV_CACHE_DIR=/private/tmp/uv-cache uv run pytest --tb=short` - PASS: 855 passed, coverage 91.69%, 1 warning for unknown `asyncio_mode`.
- `UV_CACHE_DIR=/private/tmp/uv-cache uv run ruff check` - PASS.
- `UV_CACHE_DIR=/private/tmp/uv-cache uv run mypy` - PASS.
- `UV_CACHE_DIR=/private/tmp/uv-cache uv run ruff format --check` - PASS.
- Raw print scan in `src/`, `crates/loom-core/src`, `crates/loom-mcp/src` - PASS.

## Compliance

- No git commands run.
- No network/model downloads occur in the verified tests:
  - Rust embedder tests use a local `ModelSource` test double and do not instantiate `CandleEmbedder::from_config` or `HfHubModelSource`.
  - Indexer tests use test `Embedder` implementations and do not load or download model files.
  - Git analyzer tests mock the command-runner boundary except for the timeout test, which runs local `sh -c "sleep 2"` only.
- No internal DB/parser mocks used:
  - QA indexer tests use real `LoomDb`.
  - Pipeline tests exercise real SQLite, real parser dispatch, real store helpers, and test-only embedders at the external inference boundary.
  - Parser tests call real `parse_file` with real `AdapterRegistry`.
- `BUG-MOCK-VIOLATION`: not found.
- `BUG-RAW-PRINT`: not found.
