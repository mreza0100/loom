# Dev Report - rust-runtime-authority

## Implementation Summary

- Added `docs/dev/runtime-contract.md` as the active Rust runtime contract for MCP JSON, tool inputs, status fields, scoring semantics, `.loom/loom.db`, schema version, and benchmark metrics.
- Linked the runtime contract from `README.md` and `INSTALL.md`.
- Updated the active benchmark manual in `.claude/commands/bm.md` to use `tmp/benchmark/...`, Rust `loom-mcp`, and `.loom/loom.db`.
- Converted local active benchmark helpers under `tmp/benchmark/scripts/` away from retired Python runtime imports and `python -m loom` launch configs.
- Labeled the Python/Rust comparison artifact under `tmp/benchmark/` as historical research.
- Added `crates/loom-core/tests/runtime_authority.rs` to fail on active retired runtime references and to verify the Rust runtime contract covers required semantics.

## Test Coverage

- `crates/loom-core/tests/runtime_authority.rs`
  - Scans active manuals/configs/docs plus optional local benchmark scripts for retired `python -m loom` and flat `.loom.db` references.
  - Verifies the Rust runtime contract documents `loom-mcp`, `.loom/loom.db`, MCP tools, status fields, scoring weights, useful-symbol metric language, and `CURRENT_SCHEMA_VERSION`.

## Runbook

- Build Rust MCP binary:
  - `cargo build --workspace`
- Verify runtime authority guard:
  - `cargo test -p loom-core --test runtime_authority`
- Run active benchmark index:
  - `cargo run -p loom-mcp -- --target tmp/benchmark/codebases/cockroach-loom reindex`
  - or `tmp/benchmark/scripts/index-cockroach.py tmp/benchmark/codebases/cockroach-loom`
- Active benchmark configs should launch:
  - `target/debug/loom-mcp --target <target>`

## Git Phases

MERGE, DOCS-COMMIT, and push were skipped because this run explicitly requested no commits and no push.
