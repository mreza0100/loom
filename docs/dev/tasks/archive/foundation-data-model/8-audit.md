# 🔍 Code Auditor Report

**Scope:** `foundation-data-model` pipeline — `store/models.py`, `store/db.py`, `indexer/parser.py`, `indexer/pipeline.py`, `search/engine.py`, `server.py`
**Date:** 2026-05-11
**Verdict:** NEEDS A SWEEP

---

## Summary

| Category | Findings | Critical | Actionable |
|----------|----------|----------|------------|
| Dead Code | 3 | 0 | 3 |
| Stale Dependencies | 1 | 0 | 1 |
| Architectural Smells | 3 | 0 | 3 |
| Type Safety Gaps | 2 | 0 | 2 |
| Naming Inconsistencies | 1 | 0 | 1 |
| Code Quality | 3 | 0 | 3 |
| 7A Info Leakage | 1 | 0 | 1 |
| 7B Injection | 1 | 0 | 0 |
| 7C LLM & Embedding | 0 | 0 | 0 |
| 7D Crypto & Secrets | 1 | 0 | 1 |
| 7E Supply Chain | 1 | 0 | 1 |
| **Total** | **17** | **0** | **16** |

---

## Findings

### 💀 Dead Code

```
DEAD: _resolve_single_edge last branch (lines 334-337) in src/loom/indexer/pipeline.py:334
  Type: unreachable branch
  Last meaningful use: never reached in production
  Safe to remove: yes

  Analysis: The branch fires when `len(parts) >= 2 and parts[0][0].isupper()`.
  That is exactly the same condition that already triggered Strategy 4 (line 311:
  `if simple_name != target_name`). Strategy 4 calls `get_symbol_by_name(target_name)`.
  The "last branch" calls the identical `get_symbol_by_name(target_name)` again.
  If Strategy 4 returned ≤0 or ≥2 results, the last branch will see the same results —
  it is a duplicate lookup with no new information. The only difference is the return
  confidence (1.0 vs 0.8), but the branch is never reached because:
    - Non-dotted expressions (parts length 1) fail the len(parts) >= 2 guard.
    - Dotted expressions already exhausted target_name lookup in Strategy 4.

  The QA test `TestStrategyUppercaseDottedFallback` passes because Strategy 4 resolves
  the edge at 0.8 — the test accepts "either 0.8 or 1.0", confirming it never exercises
  the last branch directly.
```

```
DEAD: remove_edges_for_source in src/loom/store/db.py:260
  Type: unused export — defined in LoomDB but never called from src/
  Last meaningful use: only exercised by tests/test_db.py:515
  Safe to remove: yes but check first — it may be intended for a future re-index strategy
    that avoids full remove_file(). If re-indexing will eventually skip full file removal,
    keep it. Otherwise, dead API surface.
```

```
DEAD: FileState dataclass in src/loom/store/models.py:45
  Type: unused export — no import found in any file in src/loom/
  Last meaningful use: never
  Safe to remove: yes
```

---

### 📦 Stale Dependencies

```
STALE-DEP: [dependency-groups].dev in pyproject.toml
  Listed in: pyproject.toml [dependency-groups] section (lines at bottom)
  Imports found: N/A — this is a duplicate declaration
  Verdict: investigate

  The project declares dev dependencies in BOTH [project.optional-dependencies].dev
  (pytest, pytest-asyncio, pytest-cov, ruff, mypy) AND [dependency-groups].dev
  (mypy, pytest-cov, ruff — with different version pins, e.g. mypy>=2.0.0 vs mypy>=1.13).
  These two groups disagree on minimum versions and are not in sync. The uv toolchain
  will resolve them independently, which can produce surprising installs. Pick one
  mechanism and delete the other.
```

---

### 🏚️ Architectural Smells

```
SMELL: SQL in Indexer Layer (not Store layer)
  Where: src/loom/indexer/pipeline.py:224 (_build_import_map) and :318 (Strategy 4b)
  What: IndexPipeline directly executes SQL via self._db.conn.execute() instead of
    calling a LoomDB method. Two places:
    1. _build_import_map: a JOIN query to fetch all import edges
    2. Strategy 4b: a LIKE pattern query on symbols
  Impact: The store/db.py boundary is supposed to be the only place that owns SQL.
    The pipeline bypassing it makes LoomDB's API surface a lie — callers need to know
    about internal schema to bypass it. Future schema migrations must update two layers.
  Fix: Add get_all_import_edges() and get_symbols_by_name_pattern(pattern: str) to
    LoomDB. Pipeline calls those methods. Remove direct conn.execute() from pipeline.
```

```
SMELL: Private method access across module boundary
  Where: src/loom/indexer/pipeline.py:323
  What: `self._db._row_to_symbol(r)` — calling a private helper of LoomDB from pipeline.
    The noqa: SLF001 suppresses the ruff warning but doesn't fix the coupling.
  Impact: _row_to_symbol is an implementation detail of LoomDB. If the row schema
    changes (e.g., column order), pipeline breaks silently. The fix for the SQL smell
    above also fixes this: if pipeline calls get_symbols_by_name_pattern(), it gets
    Symbol objects back and never touches _row_to_symbol.
  Fix: addressed by same LoomDB method extraction above.
```

```
SMELL: Two raw f-string SQL queries suppressed with noqa: S608 in remove_file
  Where: src/loom/store/db.py:150 and :154
  What: The IN (?,?,?...) placeholders pattern is constructed via string formatting
    because SQLite doesn't support parameterized IN lists. The values are integer IDs
    fetched directly from the DB (not user-controlled), so there is no injection risk.
    However S608 is suppressed without an explanatory comment, which means future
    readers can't distinguish "safe dynamic SQL" from "someone lazily silenced a warning."
  Impact: Low today, but creates a false-positive pattern that can mask real S608 issues.
  Fix: Add an inline comment explaining WHY it's safe:
    `# noqa: S608 — placeholders are int IDs from DB, not user input`
```

---

### 🕳️ Type Safety Gaps

```
TYPE-GAP: `tuple` without type args in _row_to_symbol and _row_to_edge
  Where: src/loom/store/db.py:400 and :413
  Code:
    def _row_to_symbol(row: tuple) -> Symbol:  # type: ignore[type-arg]
    def _row_to_edge(row: tuple) -> Edge:  # type: ignore[type-arg]
  Risk: The type: ignore suppresses mypy's complaint but leaves the tuple contents
    completely untyped. A column reordering in SCHEMA silently breaks row[0] through
    row[7] access with no type-level feedback.
  Fix: Use tuple[int, str, str, str, int, int, str, str] for _row_to_symbol
    and tuple[int, int, int | None, str, str | None, str, float, str | None]
    for _row_to_edge. Remove the type: ignore comments.
    Alternatively, use sqlite3.Row (row_factory = sqlite3.Row) for named column access.
```

```
TYPE-GAP: dict[str, Any] return type on get_stats()
  Where: src/loom/store/db.py:375
  Code: def get_stats(self) -> dict[str, int | str | None]:
  Risk: The actual return is a mix of int, str, and None values. The current annotation
    is partially typed (better than Any) but doesn't enforce which keys have which types.
    Callers in server.py do `**stats` spread into a dict[str, Any] — silent type loss.
  Fix: Define a StatsResult TypedDict with named fields and precise types:
    symbols: int, edges: int, files: int, vectors: int,
    last_indexed: str | None, stale_files: int
```

---

### 🏷️ Naming Inconsistencies

```
NAMING: Inconsistent confidence semantics in _resolve_single_edge docstring vs code
  Places:
    - src/loom/indexer/pipeline.py:241 (docstring: "Strategies 1-5")
    - src/loom/indexer/pipeline.py:334 (code: unnamed 6th strategy after "Strategy 5")
  Convention: Docstrings should list all resolution paths.
  Fix: Remove the last branch entirely (it's dead code, see Dead Code section).
    If kept, add it to the docstring as "Strategy 6: Uppercase dotted fallback (1.0)".
    The docstring claiming 5 strategies when 6 branches exist is misleading.
```

---

### 🧹 Code Quality & Clean Design

```
QUALITY: Magic numbers — vec search limits and similarity threshold
  Where:
    - src/loom/search/engine.py:238 — `limit=10` (impact vec search)
    - src/loom/search/engine.py:246 — `sim > 0.3` (impact sim threshold)
    - src/loom/search/engine.py:346 — `limit=10` (find_coupled vec search)
    - src/loom/search/engine.py:354 — `sim > 0.3` (find_coupled sim threshold)
  What: The limit=10 and similarity threshold 0.3 appear twice each, hardcoded.
    MAX_STRUCTURAL_RESULTS = 30 was correctly extracted — these weren't.
  Impact: Maintainability — changing the semantic search depth or quality threshold
    requires hunting for the two occurrences.
  Fix: Add two constants alongside the existing module-level ones:
    MAX_SEMANTIC_RESULTS = 10
    MIN_SEMANTIC_SIMILARITY = 0.3
```

```
QUALITY: score literals across CoupledSymbol construction
  Where:
    - src/loom/search/engine.py:204, 230 — score=0.8 (structural incoming)
    - src/loom/search/engine.py:279 — score=0.5 (no-anchor colocated)
    - src/loom/search/engine.py:285 — score=0.4 (colocated fallback)
    - src/loom/search/engine.py:314 — score=0.7 (structural outgoing)
    - src/loom/search/engine.py:335 — score=0.6 (structural incoming)
  What: Six distinct coupling score values with no named constants. The semantic
    of each (outgoing > incoming > colocated > colocated-fallback) is implicit in
    the ordering, not in a named constant that documents the ranking scheme.
  Impact: If the coupling score model changes, all six literals must be updated
    with no compile-time safety. The ranking logic is not self-documenting.
  Fix: Define named constants: SCORE_STRUCTURAL_OUTGOING = 0.7, SCORE_STRUCTURAL_INCOMING = 0.6,
    SCORE_IMPACT_STRUCTURAL = 0.8, SCORE_COLOCATED = 0.5, SCORE_COLOCATED_FALLBACK = 0.4.
    This also makes the coupling score hierarchy readable at a glance.
```

```
QUALITY: Default value duplication for debounce_sec
  Where:
    - src/loom/config.py:12 — `debounce_seconds: float = 2.0`
    - src/loom/indexer/watcher.py:38 — `debounce_sec: float = 2.0`
    - src/loom/indexer/watcher.py:135 — `debounce_sec: float = 2.0`
  What: The default debounce value is 2.0 in three places. LoomConfig is the single
    source of truth for configuration — the watcher defaults should not duplicate it.
  Impact: Changing the debounce default in config.py would have no effect on the
    watcher unless the caller passes config.debounce_seconds explicitly (which server.py
    does correctly). But a future caller forgetting to pass the value gets the watcher's
    stale default.
  Fix: The watcher's default is fine as a standalone API, but add a comment noting
    that server.py must pass config.debounce_seconds explicitly to override it.
    Optionally: remove the watcher default and make it required.
```

---

### 🔐 Security Deep Scan

#### 7A — Info Leakage & Error Exposure

```
SECURITY: 7A/path-echo-in-error
  Where: src/loom/server.py:157
  What: `return {"error": f"No symbols found in '{file}'. File may not exist or is not indexed."}`
    The `file` parameter is MCP caller input, echoed verbatim back in the error response.
    For a local developer tool this is LOW severity — but if the MCP server is ever run
    in a shared context, this echoes arbitrary caller input into the response.
  Severity: LOW
  Risk: Minimal in local tool context. Would be MEDIUM if Loom goes multi-tenant.
  Fix: Replace with a static message: `"No symbols found. File may not be indexed."`.
    Callers already know what file they passed.
```

#### 7B — Injection Attacks

```
SECURITY: 7B/dynamic-sql-in-list (suppressed, NOT a real vulnerability)
  Where: src/loom/store/db.py:150, :154
  What: Dynamic SQL via f-string for IN-list construction. All values are integer
    IDs fetched from the database itself — not user-controlled input. The S608
    suppressions are correct. Not a vulnerability.
  Severity: LOW (informational only — the suppression needs a comment explaining this)
  Risk: None in current form. Risk would arise if someone copies this pattern for
    user-controlled data.
  Fix: Add explanatory comment to each noqa: S608 as noted in Architectural Smells.
```

All other injection vectors checked: no eval/exec/compile, no subprocess calls,
no template injection, no f-string SQL with user-controlled data anywhere in the pipeline.

#### 7C — LLM & Embedding Security

No injection vectors found. The embedder consumes pre-indexed symbol text (code content).
Each DB is scoped to a single `target_dir` — no cross-project vector leakage possible
with the current single-file DB model. 🧠

#### 7D — Cryptographic Failures & Secrets

```
SECURITY: 7D/lock-file-untracked
  Where: uv.lock (untracked at conversation start — now tracked)
  What: uv.lock showed as `??` (untracked) in git status at audit start.
    At time of audit the file is 2277 lines and exists on disk but its
    tracked/untracked state should be confirmed by the team.
  Severity: LOW
  Risk: Non-deterministic installs if lock file is not consistently committed.
    A supply-chain attacker could substitute different package versions.
  Fix: Ensure `uv.lock` is committed and CI runs `uv sync --locked`.
```

No hardcoded secrets, API keys, or credentials found in any scoped file.
No `random.` module usage in security contexts. `.gitignore` correctly excludes
`.env`, `.env.local`, `.env.test`.

#### 7E — Supply Chain & Dependencies

```
SECURITY: 7E/duplicate-dep-version-conflict
  Where: pyproject.toml — [project.optional-dependencies].dev vs [dependency-groups].dev
  What: Dev dependencies declared twice with conflicting minimum versions:
    - mypy: >=1.13 (optional-deps) vs >=2.0.0 (dependency-groups)
    - pytest-cov: >=7.0 (optional-deps) vs >=7.1.0 (dependency-groups)
    - ruff: >=0.8 (optional-deps) vs >=0.15.12 (dependency-groups)
    - pytest, pytest-asyncio: only in optional-deps; missing from dependency-groups
  Impact: Ambiguous install targets — `uv sync --extra dev` vs `uv sync --group dev`
    may produce different environments. CI may use one, local dev another.
  Fix: Remove [dependency-groups].dev entirely; keep [project.optional-dependencies].dev.
    Or migrate all to [dependency-groups] and remove optional-deps. Pick one.
```

No known-vulnerable packages identified. `uv.lock` pins all transitive dependencies.

---

## Quick Wins (fix in < 5 minutes each)

1. **Remove `FileState` from `store/models.py:45`** — dead dataclass, zero imports, safe delete.
2. **Add explanatory comments to the two `noqa: S608` suppressions in `db.py:150,154`** — one-liner each: `# noqa: S608 — placeholders are int IDs from DB, not user input`.
3. **Add `MAX_SEMANTIC_RESULTS = 10` and `MIN_SEMANTIC_SIMILARITY = 0.3` constants to `engine.py`** and replace the four bare literals.
4. **Fix the `neighborhood` error message in `server.py:157`** — drop the `{file}` echo, use a static string.
5. **Remove `[dependency-groups]` from `pyproject.toml`** — redundant with `[project.optional-dependencies]`.

---

## Recommended `/jc` Fixes

1. **Kill the dead branch in `pipeline.py:334-337`** — remove 4 lines. Update the docstring to say "5 strategies" honestly (or "5 strategies, may add 6th"). The QA test that "covers" this branch needs its assertion tightened from "accept 0.8 or 1.0" to just "0.8".
2. **Replace `remove_edges_for_source` with a decision** — either document it as a planned public API (add a comment), or remove it. The method is orphaned from production code.
3. **Add named coupling score constants to `engine.py`** — six floating-point literals -> six named constants. Self-documenting ranking hierarchy.
4. **Improve `_row_to_symbol` and `_row_to_edge` type annotations** — either typed tuples or `sqlite3.Row` factory. Remove `# type: ignore[type-arg]`.

---

## Recommended `/build` Tasks

1. **Extract `_build_import_map` SQL and Strategy 4b SQL into LoomDB methods** — architectural smell: SQL belongs in the store layer. Two new methods needed:
   - `get_all_import_edges() -> list[Edge]`
   - `get_symbols_by_name_pattern(pattern: str) -> list[Symbol]`
   Then remove `self._db.conn.execute()` and `self._db._row_to_symbol()` calls from pipeline.

2. **Add `StatsResult` TypedDict for `get_stats()` return** — replace `dict[str, int | str | None]` with a named TypedDict. Fixes the type loss when server.py spreads it into `dict[str, Any]`.

---

## The Verdict

The foundation-data-model pipeline shipped clean code — no bugs, 94.89% coverage, zero ruff/mypy issues. But "passes linters" and "well-designed" are different bars. The biggest structural issue is the SQL leaking into the indexer layer (`_build_import_map`, Strategy 4b): the boundary that was supposed to keep SQL in `db.py` has two known holes, and both have `noqa` comments that paper over the smell instead of fixing it. The dead branch in `_resolve_single_edge` is a logic artifact — it was probably written as a safety net but is provably unreachable, and the QA test that was supposed to cover it actually covers Strategy 4a instead. That's a test that gives false confidence.

The type system gaps (`tuple` without type args, `dict[str, Any]` spread) are minor but they're the canary in the coal mine: when the row schema changes, these will break silently at runtime instead of loudly at type-check time. The codebase is in good shape for a new project — the hygiene debt is small and well-contained. Clean up the dead code and fix the layer violation and this becomes a codebase a new hire can trust.
