# BM — Loom vs Grep Benchmark

Run a head-to-head benchmark comparing Loom's Rust MCP tools against raw grep
on a real codebase. Two agents, same tasks, side-by-side instrumentation.

**Input:** $ARGUMENTS

---

## Subcommand Routing

| Subcommand | Trigger | Action |
|------------|---------|--------|
| `setup` | starts with "setup" | Jump to § Setup |
| `compare` | starts with "compare" | Jump to § Compare |
| `clean` | starts with "clean" | Jump to § Clean |
| *(default)* | URL or project name | Full flow: Setup → Run instructions → wait → Compare |

---

## Full Flow (default)

Parse the input for a git URL and optional subdirectory.
If no URL given, ask the user what project to benchmark against.

### Step 1 — Setup
Run § Setup with the parsed URL and subdir.

### Step 2 — Launch Instructions
Print clear terminal commands for the user:

```
Ready to race! 🏁 Open two terminals:

  Terminal 1 (Loom):
    cd tmp/benchmark/bench-loom && claude
    → paste: "Read BENCHMARK-TASKS.md and execute all tasks. Write your report when done."

  Terminal 2 (Grep):
    cd tmp/benchmark/bench-grep && claude
    → paste: "Read BENCHMARK-TASKS.md and execute all tasks. Write your report when done."

Come back here when both are done. I'll crunch the numbers.
```

### Step 3 — Wait & Compare
Tell the user to come back when both agents finish, then run § Compare.

---

## Setup

**Input:** `setup <git-url> [subdir]` or just `<git-url> [subdir]`

### 1. Parse arguments
Extract the git URL and optional subdirectory from the input.

### 2. Run bench-setup.sh
```bash
bash tmp/benchmark/scripts/bench-setup.sh "<git-url>" "<subdir>"
```

### 3. Verify setup
- Confirm both directories exist: `tmp/benchmark/bench-loom/`, `tmp/benchmark/bench-grep/`
- Confirm `.mcp.json` exists in `tmp/benchmark/bench-loom/`
- Confirm `.mcp.json` launches `target/debug/loom-mcp` or `target/release/loom-mcp`
- Confirm the Loom index path is `.loom/loom.db`
- Confirm `BENCHMARK-TASKS.md` exists in both directories
- Count target files:
```bash
find tmp/benchmark/bench-loom/<subdir> -name "*.js" -not -path "*/node_modules/*" -not -path "*/.git/*" | wc -l
```

### 4. Report
```
Benchmark arena ready. 🏟️

Project:    <name>
Directory:  <subdir or root>
JS files:   <count>
Loom dir:   tmp/benchmark/bench-loom/
Grep dir:   tmp/benchmark/bench-grep/
```

---

## Compare

Read both benchmark reports and fill in the comparison table.

### 1. Find the reports
Look for reports in these locations (in order):
- `tmp/benchmark/bench-loom/BENCH-LOOM.md` and `tmp/benchmark/bench-grep/BENCH-GREP.md`
- `tmp/benchmark/bench-loom/BENCHMARK-REPORT.md` and `tmp/benchmark/bench-grep/BENCHMARK-REPORT.md`
- Any `BENCH*.md` or `BENCHMARK*.md` in either directory

If either report is missing, tell the user which agent hasn't finished yet.

### 2. Read both reports
Extract from each report:
- Per-task: time, tool calls/commands, files touched, symbols discovered, false positives
- Aggregates: total time, total calls, total files, total symbols, total false positives
- Loom-specific: grep fallbacks, false negatives, call grades
- Grep-specific: dead ends, hops

### 3. Fill the comparison table
Use `tmp/benchmark/bench-compare.md` as the template. Fill in every cell.

Calculate:
- **Δ Time** = Grep time - Loom time (positive = Loom faster)
- **Winner** per metric = whichever is better (lower time/calls/FP, higher symbols)
- **Loom unique discovery rate** = symbols found ONLY by Loom / total unique symbols
- **Loom recall vs grep** = symbols found by Loom that grep also found / total grep symbols

### 4. Answer the Key Questions
Fill in each question in the comparison template with specific data from the reports.
Don't hedge — give a direct answer backed by numbers.

### 5. Render the Verdict
Based on all data:
```
Loom advantage:     [strong / moderate / marginal / none]
Grep advantage:     [strong / moderate / marginal / none]
Net recommendation: [Loom adds value / Loom not worth it / depends on codebase size]

#1 Loom strength:   ___
#1 Loom weakness:   ___
#1 improvement:     ___
```

### 6. Write the filled comparison
Write the completed comparison to `tmp/benchmark/bench-results-<date>.md` (YYYY-MM-DD format).
Also update `tmp/benchmark/bench-compare.md` with the latest results.

### 7. Print summary
```
Benchmark complete. 📊

<verdict summary — 3 lines max>

Full comparison: tmp/benchmark/bench-results-<date>.md
```

---

## Clean

Remove benchmark artifacts:
```bash
rm -rf tmp/benchmark/bench-loom tmp/benchmark/bench-grep
```

Keep the templates: `tmp/benchmark/scripts/bench-setup.sh`, `tmp/benchmark/bench-prompt-*.md`, `tmp/benchmark/bench-compare.md`.
Keep any `tmp/benchmark/bench-results-*.md` files (historical results).

Report what was cleaned and what was preserved.

---

## Updating Benchmark Tasks

If the benchmark task prompts need updating (new tasks, better instrumentation),
edit the source files:
- `tmp/benchmark/bench-prompt-loom.md` — Loom agent tasks
- `tmp/benchmark/bench-prompt-grep.md` — Grep agent tasks
- `tmp/benchmark/bench-compare.md` — Comparison template

Then re-run setup to copy updated prompts to the benchmark directories.

---

## Rules

- **Both agents MUST use identical target symbols** — fairness is sacred
- **Never run both agents from this terminal** — they need separate Claude instances
- **Never fabricate metrics** — if a report is missing data, flag it, don't guess
- **Keep historical results** — `tmp/benchmark/bench-results-*.md` files are never deleted
- **Setup script is the source of truth** — don't manually create benchmark dirs
- **Rust runtime only** — active benchmark configs launch `loom-mcp` and use `.loom/loom.db`; Python runtime docs are historical research only.
- After finishing: "Benchmark [setup/complete/cleaned]. 📊"
