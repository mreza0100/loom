# Loom Build Pipeline

Run the full Loom pipeline for: $ARGUMENTS

**All feature requests MUST go through this pipeline.** No cowboy coding.
**Autonomous execution: once started, `/build` runs to completion. The only stop points are: pre-flight failure and Fix Loop Escalation → BLOCKED-DEFERRED.**

---

## Pre-flight — validate before starting

Check `$ARGUMENTS` for coherence. If too vague → stop with diagnostic. Do NOT proceed.

---

## Step 0 — Name the pipeline, clean up stale dirs

### 0a. Stale pipeline cleanup

```bash
for dir in docs/dev/tasks/*/; do
  name=$(basename "$dir")
  [ "$name" = "archive" ] && continue
  if [ ! -d ".worktrees/$name" ]; then
    echo "STALE: $dir (no active worktree)"
  fi
done
```

For each stale dir:
- Has `BLOCKED.md` → SKIP (preserved for `/jc`)
- Has `7-post-merge-qa.md` → archive it
- Otherwise → archive with `ABANDONED.md` marker

### 0b. Name the pipeline

**If `$ARGUMENTS` contains `[Pipeline: {name}]`:** Extract and use that name — the wave runner pre-assigned it, pre-placed the task manifest at `docs/dev/tasks/{name}/0-task.md`, and already ran uniqueness checks. Skip name generation and uniqueness check below; proceed directly to path variable resolution.

**Otherwise (standalone invocation):** Choose a short, descriptive kebab-case name based on the feature (e.g., `search-improvements`, `indexer-caching`).

**Name uniqueness check (standalone only — skip when `[Pipeline: ...]` is present):** Before proceeding, verify the chosen name does NOT already exist in:
- `docs/dev/tasks/archive/` — archived pipelines
- `docs/dev/tasks/` — active pipelines
- `.worktrees/` — active worktrees

```bash
ls docs/dev/tasks/archive/ docs/dev/tasks/ .worktrees/ 2>/dev/null | grep -x "{name}"
```

If the name exists, append a version suffix (e.g., `search-improvements-v2`) or choose a more specific name. **NEVER reuse an archived pipeline name** — it causes doc conflicts and breaks traceability.

Resolve path variables:
- **`$PIPELINE`** = `{name}` — the pipeline name (kebab-case, unique across active + archived). Extracted from `[Pipeline: {name}]` in `$ARGUMENTS` when present (wave-invoked), otherwise chosen by build (standalone).
- **`$WAVE`** = wave name extracted from `[Wave: {wave-name}]` in `$ARGUMENTS`, otherwise `none`. This value is forwarded to gitter so merge + docs commits carry a `Wave:` trailer for git-history traceability back to `docs/dev/waves/archive/{wave}/`.
- **`$DOCS`** = `docs/dev/tasks/{name}` — pipeline docs from repo root
- **`$DOCS_REL`** = `../../docs/dev/tasks/{name}` — pipeline docs from worktree
- **`$WORKTREE`** = `.worktrees/{name}` — pipeline worktree directory
- **`$ARCHIVE`** = `docs/dev/tasks/archive` — archive parent directory

```bash
mkdir -p docs/dev/tasks/{name}
```

**Write the task manifest** — idempotent, wave runner pre-places this when invoked from `/wave`:
```bash
[ -f docs/dev/tasks/{name}/0-task.md ] && echo "manifest exists — wave pre-placed it" || echo "manifest missing — standalone build"
```
- **Exists** → read it as-is, do NOT overwrite. Wave wrote the pipeline-specific task spec here.
- **Missing** (standalone build only) → write it now:
  ```markdown
  # Task: {name}

  {verbatim $ARGUMENTS — stripped of [Wave: ...] and [Pipeline: ...] tokens}

  Wave: {$WAVE or none}
  ```

**Pass `$PIPELINE`, `$DOCS`, `$DOCS_REL`, and `$WORKTREE` to every agent invocation.** Agents should never hardcode doc or worktree paths — they use what you give them.

---

## Step 1 — Codebase Analysis (planner)

Spawn the planner agent. **Model: sonnet.**

```
Agent(general-purpose, model: "sonnet"): "You are the Loom planner. Read and follow .claude/agents/planner.md.
  Mode: ANALYSIS. Pipeline: {name}. Feature: {feature request}.
  Analyze the src/loom/ codebase and write $DOCS/1-plan.md."
```

Wait for completion. Read the plan.

---

## Step 2 — Git Setup (worktree + ports)

Use the `gitter` agent in **SETUP** phase. **Model: sonnet.**
- "Pipeline: {name}. Phase: SETUP."
- Creates worktree at `.worktrees/{name}`

---

## Step 3 — Architecture

Spawn the architect agent. **Model: sonnet.**

```
Agent(general-purpose, model: "sonnet"): "You are the Loom architect. Read and follow .claude/agents/architect.md.
  Pipeline: {name}.
  All pipeline docs: $DOCS/.
  Write your architecture doc to $DOCS/3-architecture.md.
  Architecture doc ONLY — no code. Developer derives work queue from your doc.
  NEVER run git commands."
```

---

## Step 4 — Development (on worktree)

Read ports from `$DOCS/ports.md`, then launch developer. **Model: sonnet.**

```
Agent(general-purpose, model: "sonnet"): "You are the Loom developer. Read and follow .claude/agents/developer.md.

  Pipeline: {name}.
  Worktree: $WORKTREE. Branch: pipeline/{name}.
  ALL pipeline docs from worktree: $DOCS_REL/.
  Write dev report to $DOCS_REL/5-dev-report.md.
  NEVER run git commands."
```

---

## Step 5 — QA (BEFORE merge)

**CRITICAL: QA runs against the worktree, NOT main.**

```
Agent(general-purpose, model: "sonnet"): "You are the Loom QA engineer. Read and follow .claude/agents/qa.md.

  Mode: PRE-MERGE. Pipeline: {name}.
  Worktree: $WORKTREE.
  ALL pipeline docs from worktree: $DOCS_REL/.
  Write bug report to $DOCS_REL/6-bugs.md."
```

---

## Fix Loop (capped at 3 iterations)

If `$DOCS/6-bugs.md` has `Status: OPEN`:

1. **Developer fixes** — spawn developer on worktree, reads `6-bugs.md`
2. **Re-run QA**
3. Repeat until `Status: NONE` OR 3 iterations OR escalation trigger

### Fix Loop Escalation — BLOCKED-DEFERRED

When: iteration cap reached, hung test, same bug returns, sub-agent orphan.

1. Write `$DOCS/BLOCKED.md` with root cause, state preserved info, resume protocol
2. Do NOT delete worktree or release ports
3. Return BLOCKED-DEFERRED to user

---

## Step 6 — Git Merge

Use `gitter` in **MERGE** phase. **Model: sonnet.**
- "Pipeline: {name}. Wave: {$WAVE or 'none'}. Phase: MERGE."

---

## Step 7 — Post-Merge QA (on main)

```
Agent(general-purpose, model: "sonnet"): "You are the Loom QA engineer. Read and follow .claude/agents/qa.md.

  Mode: POST-MERGE. Pipeline: {name}. Run against src/loom/ on main.
  Pipeline docs: $DOCS/.
  Return results inline."
```

Write `$DOCS/7-post-merge-qa.md`.

If post-merge QA fails, spawn a fix pipeline `{name}-postmerge-fix`.

---

## Step 8 — Pipeline Audit

```
Agent(general-purpose, model: "sonnet"): "You are the code auditor. Read and follow .claude/commands/ca.md.
  Pipeline audit — scope to Loom code changed by pipeline {name}.
  Read $DOCS/7-post-merge-qa.md for context.
  Return findings inline."
```

Write `$DOCS/8-audit.md`. If BLOCKING findings, spawn fix pipeline.

---

## Step 9 — Commit Docs

Use `gitter` in **DOCS-COMMIT** phase. **Model: sonnet.**
- "Pipeline: {name}. Wave: {$WAVE or 'none'}. Phase: DOCS-COMMIT."

---

## Pipeline Reference

| # | Step | Who | Produces |
|---|------|-----|----------|
| 1 | Analysis | planner | `$DOCS/1-plan.md` |
| 2 | Git setup | gitter (SETUP) | Worktree, ports, `$DOCS/ports.md` |
| 3 | Architecture | architect | `$DOCS/3-architecture.md` |
| 4 | Develop | developer | Code in worktree + `$DOCS/5-dev-report.md` |
| 5 | QA | qa | Tests + `$DOCS/6-bugs.md` |
| - | Fix loop | developer → qa | Repeat until NONE |
| 6 | Merge | gitter (MERGE) | Commits + merges to main |
| 7 | Post-merge QA | qa (POST-MERGE) | `$DOCS/7-post-merge-qa.md` |
| 8 | Audit | code auditor | `$DOCS/8-audit.md` |
| 9 | Commit docs | gitter (DOCS-COMMIT) | Commits doc changes |

---

## Done

"Build complete ({name}). All tests pass on main. Audit clean. Docs committed."
