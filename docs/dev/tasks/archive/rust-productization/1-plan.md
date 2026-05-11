# Plan — rust-productization

## Scope

Implement the Rust product surface for wave tasks 9-11:

- Rust search/scoring in `loom-core`.
- `loom-mcp` CLI and rmcp tool server.
- Release metadata for cargo-dist, maturin, and GitHub Actions.

## Target Files

- `crates/loom-core/src/search/*`
- `crates/loom-core/src/config.rs`
- `crates/loom-core/src/indexer/pipeline.rs`
- `crates/loom-mcp/src/main.rs`
- `crates/loom-mcp/src/server.rs`
- `Cargo.toml`
- `pyproject.toml`
- `.github/workflows/release.yml`

## Verification

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo run -p loom-mcp -- status --target .`
- `cargo metadata --format-version 1 --no-deps`
- `cargo package -p loom-mcp --allow-dirty --no-verify --list`
- `UV_CACHE_DIR=/private/tmp/uv-cache uv run pytest`
- `UV_CACHE_DIR=/private/tmp/uv-cache uv run ruff check`
- `UV_CACHE_DIR=/private/tmp/uv-cache uv run mypy`
