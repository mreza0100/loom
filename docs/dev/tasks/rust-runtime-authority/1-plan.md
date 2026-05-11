> Author: planner

# Plan - rust-runtime-authority

## Feature Context

The Rust `loom-mcp` binary is the only active runtime. This pipeline removes stale Python assumptions from benchmark launch paths, active manuals, and runtime docs before later semantic proof gates depend on benchmark evidence.

## Current State

- `crates/loom-core/src/config.rs` already defaults to `.loom/loom.db`.
- `crates/loom-mcp/src/main.rs` exposes Rust CLI subcommands for `status` and `reindex`.
- `crates/loom-mcp/src/server.rs` exposes `search`, `related`, `impact`, `neighborhood`, `reindex`, and `status`.
- The worktree already contains deleted Python package files and Rust embedder/status changes from the previous pipeline.
- Active benchmark helpers under `tmp/benchmark/scripts/` still contain Python runtime launch paths and the old flat `.loom.db` path.

## Gaps & Needed Changes

- Convert active benchmark MCP configs to invoke `target/debug/loom-mcp` or `LOOM_MCP_BIN`.
- Convert active benchmark scripts to check and report `.loom/loom.db`.
- Replace Python indexing helpers with Rust CLI wrappers.
- Add a guard test that fails on active benchmark references to `python -m loom`.
- Add an authoritative Rust runtime contract documenting MCP JSON, status fields, storage path, schema version, scoring semantics, and benchmark metrics.
- Label Python/Rust comparison artifacts as historical research.

## Integration Surface

- Runtime docs: `README.md`, `INSTALL.md`, `docs/dev/runtime-contract.md`.
- Benchmark manuals/configs: `.claude/commands/bm.md`, `tmp/benchmark/README.md`, `tmp/benchmark/scripts/*`.
- Guard coverage: `crates/loom-core/tests/runtime_authority.rs`.
- Pipeline docs: `docs/dev/tasks/rust-runtime-authority/`.

## Risks & Dependencies

- `tmp/` is ignored, but the current wave uses it for active local benchmark helpers. The guard treats those files as optional so clean checkouts are not forced to carry ignored artifacts.
- Later benchmark pipelines may replace the temporary scripts with tracked harness code; the runtime contract should remain the source of truth.
- Existing dirty worktree changes are preserved and not reverted.

## Research Needed

No external research needed. The required contract is derived from the current Rust source and wave manifest.

Analysis complete.
