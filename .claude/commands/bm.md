# BM — Headless Loom vs Grep Benchmark

Run a head-to-head benchmark comparing Loom's Rust MCP tools against raw grep
on a real codebase. The default benchmark target is Corepack, and the default
flow is fully automated: two fresh clones, two headless Codex agents, same
tasks, sequential execution, raw event capture, resource timing, and a final
metrics report.

**Input:** $ARGUMENTS

---

## Subcommand Routing

| Subcommand | Trigger | Action |
|------------|---------|--------|
| `setup` | starts with "setup" | Jump to § Setup |
| `compare` | starts with "compare" | Jump to § Compare |
| `clean` | starts with "clean" | Jump to § Clean |
| *(default)* | empty, URL, or project name | Full flow: Setup → Prompt Adaptation → Headless Runs → Compare |

---

## Defaults

If no URL is provided, benchmark Corepack:

```text
https://github.com/nodejs/corepack.git
```

If a URL is provided, use that URL instead. Optional trailing text may specify a
subdirectory or benchmark focus, but both agents must still receive identical
target symbols/tasks.

Use `tmp/benchmark/` for all clones, logs, reports, and generated artifacts.
Do not write generated benchmark artifacts outside `tmp/benchmark/` or the two
disposable benchmark clones.

---

## Full Flow (default)

### Step 1 — Parse Target

Extract a git URL and optional subdirectory/focus from `$ARGUMENTS`.

- Empty input → `https://github.com/nodejs/corepack.git`
- URL input → that URL
- Project name without URL → infer only if unambiguous; otherwise use Corepack
  and note the fallback

### Step 2 — Setup

Run § Setup for the parsed URL/subdir. This must create two fresh clones:

```text
tmp/benchmark/bench-loom/
tmp/benchmark/bench-grep/
```

### Step 3 — Adapt Benchmark Prompts

Before launching agents, inspect the target repository just enough to make the
task prompts repo-native. The stock prompts may be stale or webpack-specific;
do not send agents irrelevant symbols.

Write clone-local task files only:

```text
tmp/benchmark/bench-loom/BENCHMARK-TASKS.md
tmp/benchmark/bench-grep/BENCHMARK-TASKS.md
```

Prompt adaptation rules:

- Both agents must get the same target symbols and task concepts.
- Prefer real source files over tests for primary targets.
- Include TypeScript/JavaScript/Go/Rust/etc. according to the target repo, not
  only `*.js`.
- Include one mutation task that edits only the disposable clone, then verifies
  the new symbol through each agent's allowed method.
- Preserve the same five benchmark categories:
  1. call-chain trace
  2. blast-radius analysis
  3. semantic/cross-term discovery
  4. modify and verify
  5. ambiguity resolution
- Require `BENCH-LOOM.md` / `BENCH-GREP.md` and `BENCH-METRICS.json`.
- Require per-task timing, calls/commands, files touched, symbols discovered,
  false positives, false negatives/dead ends, and limitations.

For Corepack specifically, use these default targets unless current repo
contents prove they no longer exist:

| Task | Target |
|------|--------|
| call-chain trace | `Engine.findProjectSpec` |
| blast radius | `resolveDescriptor` |
| semantic discovery | `signature and integrity verification during package manager install` |
| modify/verify | add `benchmarkProbe()` near `shouldSkipIntegrityCheck` in `sources/corepackUtils.ts` |
| ambiguity | `execute` command methods |

If a default target is missing, choose the closest repo-native replacement and
state the substitution in the final report.

### Step 4 — Headless Runs

Run headless Codex agents sequentially, never in parallel:

1. Loom-enabled agent
2. Grep-only control agent

Use one timestamped results directory:

```bash
RESULTS_DIR="tmp/benchmark/results/corepack-headless-$(date +%Y-%m-%d-%H%M%S)"
mkdir -p "$RESULTS_DIR"
printf 'tmp/benchmark/bench-loom\n' > "$RESULTS_DIR/loom-workdir.txt"
printf 'tmp/benchmark/bench-grep\n' > "$RESULTS_DIR/grep-workdir.txt"
printf '%s\n' "$RESULTS_DIR" > tmp/benchmark/latest-results-dir.txt
LOCK_DIR="tmp/benchmark/.bm.lock"
if ! mkdir "$LOCK_DIR" 2>/dev/null; then
  echo "Another BM run owns $LOCK_DIR; stop instead of contaminating shared clones." >&2
  exit 1
fi
trap 'rmdir "$LOCK_DIR"' EXIT
```

If the target is not Corepack, replace `corepack-headless` with a short slug
derived from the repo name.

#### Loom Agent Command Shape

Build the Rust `loom-mcp` binary immediately before the run, then pass the MCP
server explicitly to Codex. This avoids stale binaries and ambient user config.

```bash
cargo build -p loom-mcp
```

```bash
RESULTS_DIR="<results-dir>"
START=$(date +%s)
/usr/bin/time -lp codex exec \
  --json \
  --output-last-message "$RESULTS_DIR/loom-final.txt" \
  --model gpt-5.5 \
  --sandbox workspace-write \
  --ignore-user-config \
  --ignore-rules \
  --ephemeral \
  -C tmp/benchmark/bench-loom \
  -c 'mcp_servers.loom.command="/absolute/path/to/target/debug/loom-mcp"' \
  -c 'mcp_servers.loom.args=["--target", "/absolute/path/to/tmp/benchmark/bench-loom"]' \
  -c 'mcp_servers.loom.cwd="/absolute/path/to/loom"' \
  -c 'mcp_servers.loom.enabled=true' \
  -c 'mcp_servers.loom.required=true' \
  -c 'mcp_servers.loom.startup_timeout_sec=60' \
  -c 'mcp_servers.loom.tool_timeout_sec=120' \
  'Read BENCHMARK-TASKS.md and execute all tasks exactly. Use Loom MCP as primary navigation. Call status first; reindex if needed. Use symbols for exact/same-name enumeration such as execute methods, search for conceptual discovery, evidence_pack for broad proof, and inspect only selected handles. Update BENCH-LOOM.md and BENCH-METRICS.json after each task, then finalize them before your final response.' \
  > "$RESULTS_DIR/loom-events.jsonl" \
  2> "$RESULTS_DIR/loom-stderr-time.log"
STATUS=$?
END=$(date +%s)
printf 'status=%s\nstart=%s\nend=%s\nelapsed=%s\n' "$STATUS" "$START" "$END" "$((END-START))" > "$RESULTS_DIR/loom-run-meta.txt"
```

Use the actual absolute paths for the current checkout.

Do not use `--ask-for-approval` with `codex exec`; this CLI rejects that flag in
the benchmark environment.

#### Grep Agent Command Shape

Run the control after the Loom process exits:

```bash
RESULTS_DIR="<results-dir>"
START=$(date +%s)
/usr/bin/time -lp codex exec \
  --json \
  --output-last-message "$RESULTS_DIR/grep-final.txt" \
  --model gpt-5.5 \
  --sandbox workspace-write \
  --ignore-user-config \
  --ignore-rules \
  --ephemeral \
  -C tmp/benchmark/bench-grep \
  'Read BENCHMARK-TASKS.md and execute all tasks exactly. Use only grep/find/sed/cat/head/tail/wc/date/ls/pwd and direct file edits; no MCP tools, no semantic search, no AST tools. Update BENCH-GREP.md and BENCH-METRICS.json after each task, then finalize them before your final response.' \
  > "$RESULTS_DIR/grep-events.jsonl" \
  2> "$RESULTS_DIR/grep-stderr-time.log"
STATUS=$?
END=$(date +%s)
printf 'status=%s\nstart=%s\nend=%s\nelapsed=%s\n' "$STATUS" "$START" "$END" "$((END-START))" > "$RESULTS_DIR/grep-run-meta.txt"
```

If either run fails before the agent starts, preserve the failed launch logs,
fix the launch command, and rerun that side. Exclude pre-agent launch failures
from the benchmark comparison, but mention them.

If an agent starts and then fails, do not hide it. Compare the failure as a real
benchmark outcome.

### Step 5 — Compare

Run § Compare using:

- agent reports
- `BENCH-METRICS.json`
- Codex JSONL event streams
- `/usr/bin/time -lp` logs
- run meta files

### Step 6 — Report

End with a concise verdict and links to:

- full comparison report
- Loom agent report
- Grep agent report
- results directory

After finishing: `Benchmark complete. 📊`

---

## Setup

**Input:** `setup <git-url> [subdir]`, `<git-url> [subdir]`, or empty for Corepack.

### 1. Parse arguments

Extract the git URL and optional subdirectory. Use Corepack if empty.

### 2. Run bench-setup.sh

```bash
bash tmp/benchmark/scripts/bench-setup.sh "<git-url>" "<subdir>"
```

### 3. Verify setup

- Confirm both directories exist: `tmp/benchmark/bench-loom/`, `tmp/benchmark/bench-grep/`
- Confirm `.mcp.json` exists in `tmp/benchmark/bench-loom/`
- Confirm `.mcp.json` launches `target/debug/loom-mcp` or `target/release/loom-mcp`
- Confirm `BENCHMARK-TASKS.md` exists in both directories
- Count relevant source files for the target language(s), excluding `.git`,
  dependency directories, generated build output, and vendor directories

For TypeScript/JavaScript targets:

```bash
find tmp/benchmark/bench-loom \( -name "*.ts" -o -name "*.tsx" -o -name "*.js" -o -name "*.mjs" \) \
  -not -path "*/node_modules/*" \
  -not -path "*/.git/*" \
  -not -path "*/dist/*" \
  | wc -l
```

### 4. Report

```text
Benchmark arena ready.

Project:    <name>
Directory:  <subdir or root>
Sources:    <count and language mix>
Loom dir:   tmp/benchmark/bench-loom/
Grep dir:   tmp/benchmark/bench-grep/
```

---

## Compare

Read both benchmark reports and fill in a comparison.

### 1. Find inputs

Look for reports in these locations:

- `tmp/benchmark/bench-loom/BENCH-LOOM.md`
- `tmp/benchmark/bench-grep/BENCH-GREP.md`
- `tmp/benchmark/bench-loom/BENCH-METRICS.json`
- `tmp/benchmark/bench-grep/BENCH-METRICS.json`
- `$RESULTS_DIR/{loom,grep}-events.jsonl`
- `$RESULTS_DIR/{loom,grep}-stderr-time.log`
- `$RESULTS_DIR/{loom,grep}-run-meta.txt`

If either agent report is missing, say which side has not completed.

### 2. Extract metrics

From agent reports and JSON metrics:

- per-task time
- total time
- calls/commands
- files touched/opened
- files written
- symbols discovered
- false positives
- false negatives
- dead ends / grep fallbacks / hops
- Loom call grades
- semantic wins/noise
- limitations

From Codex JSONL event streams:

- total events
- `item.started` / `item.completed`
- command executions
- MCP tool calls by tool
- file changes
- token usage from `turn.completed`
- noncached input tokens = input tokens - cached input tokens
- output tokens
- reasoning output tokens

From `/usr/bin/time -lp`:

- real/user/sys time
- max resident set size
- page faults
- voluntary/involuntary context switches
- instructions retired
- cycles elapsed
- peak memory footprint

### 3. Calculate

- **Runner wall-time delta** = Loom real time - Grep real time
- **Agent wall-time delta** = Loom reported total - Grep reported total
- **Token ratio** = Loom noncached input+output / Grep noncached input+output
- **Winner** per metric = lower time/calls/tokens/noise, higher useful symbols/recall
- **Loom unique discovery rate** only if both symbol lists are comparable
- **Loom recall vs grep** only if both symbol lists use comparable granularity

If a metric cannot be safely computed, write `N/A` and explain why.

Also run the structured side-by-side calculator against the explicit run
directory:

```bash
python3 tmp/benchmark/scripts/compare-metric-wins.py --results-dir "$RESULTS_DIR"
```

Treat `runner_clean=false` as a benchmark failure until the contaminated side is
rerun.

### 4. Answer Key Questions

Answer directly with numbers:

- Did Loom discover more useful symbols per token?
- Did Loom reduce search operations?
- Did Loom improve recall?
- Did Loom reduce false positives?
- Where did Loom clearly help?
- Where did grep clearly help?
- Is Loom worth it for this target size?

### 5. Render Verdict

```text
Loom advantage:     strong / moderate / marginal / none
Grep advantage:     strong / moderate / marginal / none
Net recommendation: Loom adds value / Loom not worth it / depends on codebase size

#1 Loom strength:   ___
#1 Loom weakness:   ___
#1 improvement:     ___
```

### 6. Write comparison

Write:

```text
tmp/benchmark/bench-results-<YYYY-MM-DD>-<slug>-headless.md
tmp/benchmark/bench-compare.md
```

Do not overwrite historical `bench-results-*` reports. If the exact filename
already exists, add a timestamp suffix.

### 7. Print summary

```text
Benchmark complete. 📊

<verdict summary — 3 lines max>

Full comparison: tmp/benchmark/bench-results-<...>.md
```

---

## Clean

Remove only disposable benchmark clones and per-run transient clone artifacts:

```bash
rm -rf tmp/benchmark/bench-loom tmp/benchmark/bench-grep
```

Preserve:

- `tmp/benchmark/scripts/`
- `tmp/benchmark/previous-benchmarks/`
- `tmp/benchmark/bench-prompt-*.md`
- `tmp/benchmark/bench-compare.md`
- `tmp/benchmark/bench-results-*.md`
- `tmp/benchmark/results/`

Report what was cleaned and what was preserved.

---

## Updating Benchmark Tasks

For durable task-prompt changes, edit source prompt files if they exist:

- `tmp/benchmark/bench-prompt-loom.md`
- `tmp/benchmark/bench-prompt-grep.md`
- `tmp/benchmark/bench-compare.md`

If those files do not exist, the run may write clone-local
`BENCHMARK-TASKS.md` files after setup. Do not create durable prompt templates
unless the user asks.

---

## Rules

- **Run headless Codex agents sequentially by default** — Loom first, grep second.
- **Never run both agents in parallel** — shared machine load pollutes metrics.
- **Both agents MUST use identical target symbols** — fairness is sacred.
- **Use two fresh clones under `tmp/benchmark/` every full run.**
- **Default to Corepack when no target is provided.**
- **Measure everything available** — reports, JSON metrics, Codex events, token usage, resource timing, command/tool counts, files, symbols, false positives, failures, and limitations.
- **Never fabricate metrics** — if a report is missing data, flag it.
- **Compare the explicit `$RESULTS_DIR` for the run** — never silently fall back to an older latest directory.
- **Reject contaminated runs** — grep side must have zero MCP calls; Loom side may use only the `loom` MCP server.
- **Keep historical results** — `tmp/benchmark/bench-results-*.md` and `tmp/benchmark/results/` are never deleted by default.
- **Setup script is the source of truth for clone creation** — don't manually create benchmark dirs.
- **Rust runtime only** — active benchmark configs launch `loom-mcp`; Python runtime docs are historical research only.
- **Benchmark clone mutation is allowed** only inside `tmp/benchmark/bench-loom` and `tmp/benchmark/bench-grep` for benchmark tasks like modify-and-verify.
- **Do not commit or push.**
- After finishing: `Benchmark [setup/complete/cleaned]. 📊`
