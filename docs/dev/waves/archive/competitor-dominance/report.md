# Wave Report: competitor-dominance

**Task file:** `wave.md` | **Started:** 2026-05-12
**Total tasks:** 3 -> 0 via /jc + 1 pipeline | **Waves:** 1

## Grouping Summary

| Pipeline | Tasks included | Routing |
|---|---|---|
| `competitor-dominance-cli` | CLI parity, fact/callsite proof handles, benchmark helper parity, BM/RND gate | One pipeline; all tasks share Rust runtime and benchmark surfaces |

## Execution Plan

### Wave 1

- [x] `competitor-dominance-cli` - expose MCP runtime through CLI, add fact/callsite proof handles, update benchmark helper, verify gates.

## Log

- Refined competitor report into a narrow implementation wave because current Rust code already has exact/beyond buckets, inspect, evidence packs, behavior facts, callsites, role cards, and MCP containment descriptions.
- Added Professor recommendation into the live wave: behavior facts and callsites must be inspectable proof, not only side-channel metadata.
- Implemented CLI subcommands on `loom-mcp`: `serve`, `status`, `reindex`, `search`, `related`, `impact`, `neighborhood`, `inspect`, and `evidence-pack`.
- Added `--format text` compact terminal output while keeping JSON as the default contract.
- Added fact and callsite handle parsing to `inspect`; evidence packs now expose fact handles and inspect exact operational fact lines.
- Updated benchmark helper CLI support for search-family commands.
- Updated README, INSTALL, and runtime contract documentation.
- Verification passed: `cargo fmt --all -- --check`, `cargo build --workspace`, `cargo test --workspace`, and `cargo clippy --workspace -- -D warnings`.

## Final Summary

**Completed:** 2026-05-12 | **Pipelines:** 1 succeeded, 0 failed, 0 deferred

| Pipeline | Tasks | Status | Notes |
|---|---:|---|---|
| `competitor-dominance-cli` | 4 | DONE | CLI parity, fact/callsite proof handles, benchmark helper parity, docs, gates green |

## Professor's Wave Review

Professor review agreed that the previous proof-gate substrate was mostly present and identified the remaining benchmark gap as proof compression: facts and callsites need to become directly inspectable, query outputs need tighter containment, and Corepack metrics must measure useful symbols per token rather than MCP activity alone.

This wave incorporated the highest-impact recommendation immediately by adding inspectable fact and callsite handles and including fact handles in evidence packs. Remaining RND candidates after BM are query-intent routing, stricter source diversity caps, metric attribution by true response bucket/reason code, and lower MCP response-character budgets.
