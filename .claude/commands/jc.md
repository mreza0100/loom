# JC - Live Debug, Diagnose, And Fix

Debug, diagnose, trace, and fix Loom live on `main`: $ARGUMENTS

JC is the fast hotfix lane. Use it for targeted runtime bugs, config issues, diagnostics, logs, and narrow repairs. Bigger feature work belongs in `/build`.

## Classify

| Mode | Examples |
|------|----------|
| Diagnostic | trace indexer flow, locate scoring code, explain empty search |
| Fix | parser crash, stale index rows, bad config default, broken MCP response |

Start diagnostic when ambiguous. Switch to fix only when evidence shows a code change is needed.

## Investigate

- Read `CLAUDE.md`.
- Trace through `crates/loom-core/` and `crates/loom-mcp/`.
- Use concrete file and line references.
- For hangs, add a timeout before re-running and inspect process state.

## Fix Rules

- Keep changes narrow.
- Use `tracing` logs, never private source content.
- Mock external dependencies only.
- Add a regression test when practical.
- No new dependency without explaining why `/build` is not needed.

## Verify

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

## Git

Use gitter for `JC-COMMIT` when a commit is required. Never push unless explicitly requested.

## Report

Lead with root cause, files changed, tests run, and remaining risk.
