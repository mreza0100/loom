# Competitor Dominance CLI

## Scope

Implement the refined `wave.md` pipeline:

1. Add CLI parity for the active Rust MCP runtime.
2. Update benchmark helper CLI parity.
3. Add fact/callsite proof handles.
4. Verify with workspace gates, then run BM/RND against Corepack.

## Functional Requirements

- `loom-mcp` keeps no-subcommand MCP server behavior.
- `loom-mcp serve` explicitly starts the MCP server.
- `loom-mcp status` and `loom-mcp reindex` keep JSON output compatibility.
- `loom-mcp search`, `related`, `impact`, `neighborhood`, `inspect`, and `evidence-pack` call the same `LoomServerState` and `SearchEngine` paths as MCP.
- JSON output is default; `--format text` is compact, handle-first, and terminal-friendly.
- Benchmark helper `tmp/benchmark/scripts/bench-cli.py` can call all CLI search-family subcommands.
- Docs cover CLI usage in README, INSTALL, and the runtime contract.
- Evidence packs expose inspectable behavior fact handles with anchors.
- `inspect` resolves fact and callsite handles into bounded source snippets.
- Operational fact tests prove env/config evidence can be cited without shelling out.

## Acceptance Gates

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

Then run `$bm` and RND iterations until Loom beats grep or the blocker is concrete.
