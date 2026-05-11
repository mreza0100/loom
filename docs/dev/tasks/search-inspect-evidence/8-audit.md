# Audit - search-inspect-evidence

Scope: changed Rust search/MCP code for compact handles, inspect, evidence pack, and containment policy.

## Findings

No open findings after remediation.

## Remediated During Audit

FINDING-001: Unbounded inspect line offset
Severity: medium
Where: `crates/loom-core/src/search/engine.rs`, `crates/loom-mcp/src/server.rs`
Risk: an extreme `line_offset` could pass through MCP validation and lose information during integer conversion before snippet paging.
Fix: added `MAX_INSPECT_LINE_OFFSET` validation and checked/saturating conversion in core.

## Residual Risk

- Exact lexical hits remain symbol-FTS based, not whole-file grep equivalence. Runtime docs now state this explicitly.
- Evidence pack uses the current search/graph signals. Later role-card and state-flow waves should improve coverage, but missing concepts are surfaced rather than hidden.

## Verification

```bash
cargo test -p loom-core --test search
cargo test -p loom-mcp
cargo clippy --workspace -- -D warnings
```

Result: PASS.
