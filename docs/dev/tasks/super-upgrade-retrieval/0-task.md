# Super Upgrade Retrieval

## Scope

Implement the `wave.md` pipeline in one coordinated pass:

1. Exact symbol enumeration tool.
2. Compact expansion and inspection caps.
3. Benchmark metric gate that checks whether Loom beats grep on more than 60% of comparable metrics.

## Functional Requirements

- Add a read-only `symbols` MCP tool and CLI subcommand.
- `symbols` must support `query`, optional `file_prefix`, optional `kind`, bounded `limit`, and suffix method enumeration such as `execute`.
- `symbols` must return compact handles/anchors/reason codes/budget metadata and truthful truncation.
- Cap `related`, `impact`, and `neighborhood` result lists with truthful budget metadata.
- Keep `inspect` paginated but reduce default returned payload.
- Update README, INSTALL, and runtime contract docs.
- Add focused tests for symbol enumeration, caps, and fact/callsite proof behavior.
- Add `tmp/benchmark/scripts/compare-metric-wins.py` to compute comparable metric win rate from latest BM artifacts.

## Acceptance Gates

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Then run `$bm` on fresh Corepack clones and evaluate the metric-win rate.
