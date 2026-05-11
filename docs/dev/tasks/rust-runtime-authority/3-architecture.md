# Architecture - rust-runtime-authority

## Goals

- Make Rust `loom-mcp` the only active runtime in docs and benchmark configs.
- Keep historical Python comparison artifacts allowed only when clearly labeled historical.
- Add a lightweight guard that catches stale active `python -m loom` benchmark references.
- Document runtime contracts precisely enough for later benchmark and proof gates.

## File Responsibilities

- `docs/dev/runtime-contract.md`: source-of-truth Rust runtime contract for MCP JSON, tool inputs, status fields, scoring, storage, schema version, and benchmark metrics.
- `README.md` and `INSTALL.md`: point users to the Rust contract and keep quick-start examples on `loom-mcp`.
- `.claude/commands/bm.md`: active benchmark command manual, updated to describe Rust MCP configs and `.loom/loom.db`.
- `tmp/benchmark/scripts/*`: local active benchmark helpers, updated to invoke the Rust binary and use `.loom/loom.db`.
- `crates/loom-core/tests/runtime_authority.rs`: guard test for active runtime references.

## Data Model / API Changes

No runtime data model change. The contract documents the existing store path and schema version:

- database path: `<target>/.loom/loom.db`
- target config: `<target>/.loom/config.toml`
- schema version: `CURRENT_SCHEMA_VERSION = 4`

## Algorithms

No search algorithm change. The documentation records the existing coupling semantics:

- structural relationship score with confidence and depth decay;
- semantic score as clamped `1.0 - vector_distance`;
- evolutionary score as frequency plus recency;
- weighted fusion with structural/semantic renormalization when evolutionary evidence is unavailable.

## Test Plan

- `cargo test -p loom-core runtime_authority`
- `cargo test -p loom-mcp status_opens_db_without_loading_embedder`
- Workspace gates as feasible:
  - `cargo build --workspace`
  - `cargo test --workspace`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all -- --check`

## Risks

- The ignored `tmp/benchmark` helpers are local active artifacts, so the guard scans them only when present.
- Some historical docs still contain Python terms by design; the guard scope intentionally excludes archived and historical paths.
