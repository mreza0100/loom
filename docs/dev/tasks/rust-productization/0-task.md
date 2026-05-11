> Author: wave

# Task: rust-productization

Wave: rust-rewrite

Source task file: `wave.md`

## Scope

Implement Rust search, MCP server/CLI, and distribution plumbing.

## Included Wave Tasks

### 9. Search engine + coupling scorer

Port `src/loom/search/engine.py` and `src/loom/search/scoring.py` to Rust.

Requirements:
- FTS5 keyword search + vector search
- Reciprocal Rank Fusion, `k=60`
- coupling formula: `w1 * structural + w2 * semantic + w3 * evolutionary`
- configurable weights and threshold
- `search(query)` with top-K and top coupled expansions
- `related(symbol)` above coupling threshold
- `impact(symbol)` using graph traversal and evolutionary co-changers
- `neighborhood(file, line)` for symbol-at-location
- score breakdowns and human-readable reasons

### 10. MCP server with rmcp + CLI

Implement MCP server using `rmcp` with stdio transport. Register tools matching Python:
- `search`
- `related`
- `impact`
- `neighborhood`
- `reindex`
- `status`

CLI via `clap`:
- `loom-mcp` starts stdio server
- `loom-mcp reindex`
- `loom-mcp status`

Server should lazily initialize DB/graph/model on first use and handle concurrent tool calls.

### 11. Cross-platform distribution

Set up:
- `cargo-dist` for GitHub Releases
- macOS aarch64/x86_64
- Linux x86_64/aarch64
- Windows x86_64
- `maturin` PyPI wheel wrapping the Rust binary, `pip install loom-mcp`
- Homebrew tap metadata via cargo-dist
- CI workflow for build/test/release matrix

Boundaries: no musl static linking, no Windows ARM, no auto-update.

## Dependencies

Depends on `rust-foundation`, `rust-parsers`, and `rust-indexer`.

## Required Verification

- `cargo build --workspace`
- `cargo test --workspace`
- CLI smoke tests
- MCP tool schema/handler tests
- distribution config validation where possible without publishing
- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`

