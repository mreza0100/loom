# Dev Environment

Manage the Loom Rust development helper: $ARGUMENTS

## Subcommands

| Input | Mode |
|-------|------|
| empty, `up`, `start` | Check the workspace |
| `kill`, `stop`, `down` | Stop tracked dev processes |
| `restart` | Stop then check |
| `status` | Show tracked processes |
| `log`, `logs` | Show recent logs |
| `clear-logs`, `cl` | Delete logs |

## Flow

1. Read `.claude/scripts/dev.sh`.
2. Keep it aligned with the Cargo workspace.
3. Run the requested subcommand.
4. Report status and any failing command.

## Expected Verification

```bash
cargo check --workspace
```
