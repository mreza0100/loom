# Dev Environment Setup

Manage the Loom development environment: $ARGUMENTS

---

## How this works

The `/dev` command has two jobs:
1. **Maintain** `.claude/scripts/dev.sh` — keep it in sync with the actual project state
2. **Run** the script and present a pretty report

---

## Subcommands

| Input | Mode |
|-------|------|
| (empty), `up`, `start` | **UP** — start the MCP server |
| `kill`, `stop`, `down` | **KILL** — stop all running processes |
| `restart` | **RESTART** — kill then start fresh |
| `status` | **STATUS** — show what's running |
| `log`, `logs` | **LOG** — show recent log output |
| `clear-logs`, `cl` | **CLEAR-LOGS** — delete all logs |

---

## Step 0 — Script Maintenance (runs BEFORE every UP or RESTART)

### 0a. Read current project state
1. `pyproject.toml` — dependencies, scripts
2. `src/loom/__main__.py` — entry point, port

### 0b. Read `.claude/scripts/dev.sh`

### 0c. Compare and detect drift

### 0d. Update if drift detected — surgical edits only

---

## Mode: UP (default)

1. Run Script Maintenance (Step 0)
2. Run: `./.claude/scripts/dev.sh up 2>&1` (timeout: 60s)
3. Parse structured output
4. Report:

```
Dev environment is up!

| Service | Status | URL |
|---------|--------|-----|
| Loom MCP Server | [GREEN] running | stdio (MCP protocol) |

Commands:
  /dev           Start dev environment
  /dev kill      Stop all processes
  /dev restart   Kill + start fresh
  /dev status    Show what's running
  /dev log       Show recent logs
  /dev cl        Clear all logs
```

---

## Mode: KILL

```bash
./.claude/scripts/dev.sh kill 2>&1
```

---

## Mode: STATUS

```bash
./.claude/scripts/dev.sh status 2>&1
```

---

## Mode: LOG

```bash
./.claude/scripts/dev.sh log [service] [N] 2>&1
```

---

## Mode: CLEAR-LOGS

```bash
./.claude/scripts/dev.sh clear-logs 2>&1
```
