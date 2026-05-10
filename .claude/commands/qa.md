# Loom QA — Adversarial Dogfood

Stress-test Loom's MCP tools by performing aggressive, edge-case-heavy development
tasks on the target project. Uses 360° exhaustive analysis to systematically
probe every failure mode before, during, and after each task.

**Target:** $ARGUMENTS (default: "full adversarial dogfood — all task types")

---

## Philosophy

You are not here to verify Loom works. You are here to **find where it breaks.**

Every query you send should be designed to expose a weakness: ambiguous symbol names,
cross-file relationships Loom can't see, new files it doesn't index, scores that
mislead, results that are technically correct but useless. If all your queries return
perfect results, you're not trying hard enough.

---

## Step 0 — Preflight

Call `status()`. Record baseline. If 0 symbols → stop.

Then **attack the index itself:**
- `search("")` — empty query. Does it crash or return gracefully?
- `search("xyzzy_nonexistent_symbol_42")` — pure miss. What comes back?
- `related("constructor")` — super common name. Dedup? Useful results or noise?
- `impact("log")` — appears in every file. Does it explode?
- `neighborhood("nonexistent/file.js", 1)` — bad path. Error handling?
- `reindex()` — idempotent? Does it double-count?

Log every response. Grade each: PASS / DEGRADE / CRASH.

---

## Step 1 — 360° the Loom tool surface

Before touching any code, run the **360° protocol** (domain: `test`) on the
subject: **"Loom MCP tools operating on a JavaScript codebase."**

Walk every dimension:

| Dimension | What to probe |
|-----------|---------------|
| **Inputs** | Symbols with special chars, empty strings, regex chars in queries, very long names, names that are JS keywords (`function`, `class`, `import`) |
| **State** | Empty index, mid-reindex query, index after file deletion, orphaned vectors from deleted files |
| **Boundaries** | File with 0 exports, file with 100+ functions, single-line file, file with only imports, `limit=0`, `limit=1000` |
| **Sequences** | Search before index finishes, reindex twice rapidly, edit file → query → edit again → query |
| **Timing** | Query during watcher debounce window, rapid file creation (10 files in 1s) |
| **Error paths** | Malformed JS that tree-sitter can't parse, binary file with .js extension, symlinks, circular imports |
| **Data shapes** | Symbol names with unicode, template literal heavy files, destructured imports, re-exports, `export default` anonymous |
| **Environment** | DB locked by another process, disk full simulation, permission denied on target |
| **Regressions** | Do previous dogfood report issues still exist? Did fixes introduce new problems? |

Use the 360° output to generate your attack plan for Steps 2-4.

---

## Step 2 — Pick 5 tasks (not 3)

Choose 5 tasks that are specifically designed to stress different weaknesses.
At least one from each category:

### Category A — Ambiguity attacks
Tasks where symbol names collide or relationships are indirect:
- Add a function with the same name as one in another module
- Create a module that re-exports from 3 other modules
- Add a utility used by everything (like a new validator)

### Category B — Cross-cutting changes
Tasks that touch many files and test `impact()` thoroughly:
- Change a shared type/interface that 5+ modules depend on
- Rename an exported function and update all callers
- Add a required parameter to a heavily-used function

### Category C — Edge case implementations
Tasks with tricky code structures:
- Add a class with inheritance
- Add async/await patterns
- Add destructured imports/exports
- Add code with callback chains or Promises

### Category D — Chaos monkey
Deliberately adversarial actions:
- Create a file, delete it, recreate with different content — does the index recover?
- Create a `.js` file that's actually JSON — does the parser survive?
- Create a 500-line function — does context extraction handle it?
- Create circular imports between 3 modules — does Loom detect or crash?

### Category E — Completeness probes
Tasks that test whether Loom sees the FULL picture:
- After adding a feature, use `related()` and `impact()` on every new symbol
- Compare Loom's relationship graph against manual `grep` — what did Loom miss?
- Search for a concept ("authentication flow") not a symbol name — does semantic search help?

---

## Step 3 — Execute each task

For each of the 5 tasks:

### 3a. Pre-task 360°
Run 360° (test domain) on the specific task: "What could go wrong when {task}?"
Use this to decide which Loom tools to call and what to look for in results.

### 3b. Discovery (Loom-only navigation)
- Query Loom tools ONLY — no grep, no file reads yet
- Log every call in a table:

```
| # | Tool | Args | Result summary | Grade | Notes |
|---|------|------|----------------|-------|-------|
| 1 | search | "cart" | 5 functions, all relevant | A | No noise |
| 2 | related | "getCart" | Missed addToCart | C | False negative |
```

### 3c. Implementation
- Write the actual code
- Before EVERY file edit, call `neighborhood(file, line)` on the edit location
- After EVERY file edit, call `impact()` on what you changed
- Track moments where Loom's guidance was wrong or missing

### 3d. Post-task verification
- `reindex()`
- Search for every new symbol — grade: found/not found/wrong metadata
- `related()` on each new symbol — are connections to existing code correct?
- `impact()` on each new symbol — is the blast radius accurate?
- Check `status()` — did file/symbol/edge counts change as expected?

### 3e. Comparison audit
For at least ONE task per run:
- Manually grep for all references to the symbols you touched
- Compare grep results against Loom's `related()` + `impact()` output
- Document every discrepancy: what grep found that Loom missed, and vice versa

---

## Step 4 — Watcher stress test

After all tasks, specifically test the file watcher:

1. Create a new `.js` file with 3 exported functions → wait 3s → call `status()`
2. Modify an existing file (add a function) → wait 3s → search for new function
3. Delete a file → wait 3s → search for its symbols (should be gone)
4. Rapid-fire: create 3 files within 1 second → wait 5s → verify all indexed
5. No-op save: touch a file without changing content → verify no reindex

Log: which operations triggered reindex, which didn't, any delays or failures.

---

## Step 5 — Write the dogfood report

Write `DOGFOOD-REPORT.md` in the target project root.

```markdown
# Loom Dogfood Report — {date}

**Mode:** Adversarial (360°-driven)
**Baseline:** {files} files, {symbols} symbols, {edges} edges, {vectors} vectors
**After:** {files} files, {symbols} symbols, {edges} edges, {vectors} vectors

---

## Preflight Results

| Test | Input | Expected | Actual | Verdict |
|------|-------|----------|--------|---------|
| Empty query | search("") | graceful empty/error | ... | PASS/FAIL |
| Miss | search("xyzzy...") | empty results | ... | PASS/FAIL |
| Common name | related("constructor") | deduped results | ... | PASS/FAIL |
| Ubiquitous symbol | impact("log") | manageable list | ... | PASS/FAIL |
| Bad path | neighborhood("fake.js", 1) | error message | ... | PASS/FAIL |
| Idempotent reindex | reindex() x2 | same counts | ... | PASS/FAIL |

## Tool Call Log

[Full table of every Loom call across all tasks — tool, args, grade, notes]

## Loom-vs-Grep Comparison

| Symbol | Loom found | Grep found | Loom missed | Grep missed |
|--------|-----------|-----------|-------------|-------------|
| ... | ... | ... | ... | ... |

## 360° Angles Tested

| Dimension | Angles generated | Angles tested | Issues found |
|-----------|-----------------|---------------|--------------|
| Inputs | N | N | N |
| State | N | N | N |
| ... | ... | ... | ... |

## What Loom Got Right
[Specific wins with evidence]

## What Loom Got Wrong
[Specific failures with reproduction steps]

## What's Missing
[Gaps identified through 360° that no current tool addresses]

## Watcher Test Results

| Operation | Expected | Actual | Verdict |
|-----------|----------|--------|---------|
| Create file | indexed in <5s | ... | PASS/FAIL |
| Modify file | reindexed in <5s | ... | PASS/FAIL |
| Delete file | symbols removed | ... | PASS/FAIL |
| Rapid create (3 files) | all indexed | ... | PASS/FAIL |
| No-op save | no reindex | ... | PASS/FAIL |

## Bugs

| ID | Severity | Tool | Description | Repro |
|----|----------|------|-------------|-------|
| BUG-001 | critical/high/medium/low | ... | ... | ... |

## Ratings

| Tool | Score | Trend | Notes |
|------|-------|-------|-------|
| search | X/10 | ↑↓— | ... |
| related | X/10 | ↑↓— | ... |
| impact | X/10 | ↑↓— | ... |
| neighborhood | X/10 | ↑↓— | ... |
| status | X/10 | ↑↓— | ... |
| reindex | X/10 | ↑↓— | ... |
| **watcher** | X/10 | ↑↓— | ... |

### Overall: X/10

[One paragraph: top 3 issues to fix, ordered by impact on developer experience]
```

---

## Step 6 — Regression check

If a previous `DOGFOOD-REPORT.md` exists:
- Load previous bug list — retest each one. Mark: FIXED / STILL BROKEN / REGRESSED
- Compare scores — flag any that dropped
- Check if previous "What's Missing" items were addressed

---

## Rules

- **360° is mandatory** — skip it and the report is invalid
- **5 tasks minimum** — fewer means you're not covering enough surface area
- **Every Loom call logged** — no exceptions. If you forget to log, go back and reconstruct
- **Loom-vs-grep comparison required** — at least 1 task must have a manual audit
- **Watcher stress test required** — file lifecycle must be verified
- **Be cruel** — find bugs, don't confirm features. A report with 0 bugs is a failed QA run
- **Grade honestly** — "A" means it was genuinely better than grep for that query
- **Actually implement the code** — don't just query. The implementation reveals gaps the queries don't
- **Previous bugs are retested** — regressions are worse than new bugs

---

## Done

"QA dogfood complete. {N} bugs found, {M} from previous report retested. Report: {path}. Overall: X/10."
