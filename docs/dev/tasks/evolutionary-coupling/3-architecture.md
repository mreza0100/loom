> Author: architect

# Architecture — evolutionary-coupling

## Overview

Phase 6 wires the third coupling signal — evolutionary coupling from git co-change history — into Loom's scoring pipeline. The work spans six files across four modules: a new `git_analyzer.py`, schema additions in `db.py`, config fields in `config.py`, pipeline orchestration in `pipeline.py`, and scoring integration in `engine.py`. No new external dependencies are introduced; all components use stdlib (`subprocess`, `collections.Counter`) or existing in-codebase infrastructure.

---

## File Structure Changes

```
src/loom/
├── config.py                  ← +3 fields (enable_git_analysis, git_max_commits,
│                                            git_max_files_per_commit)
├── indexer/
│   ├── git_analyzer.py        ← NEW — GitAnalyzer class
│   ├── pipeline.py            ← modified — git analysis block in full_index()
│   └── [embedder, parser, watcher unchanged]
├── search/
│   ├── engine.py              ← modified — _evolutionary_score() helper + 5 fuse_signals callsites
│   └── scoring.py             ← unchanged — compute_evolutionary() already correct
└── store/
    └── db.py                  ← modified — cochange DDL + 3 new methods + get_stats() extension
```

**New test file:**
```
tests/test_git_analyzer.py     ← NEW — all GitAnalyzer + DB cochange + scoring integration tests
```

---

## Module Responsibilities

### `src/loom/indexer/git_analyzer.py` (new)

Single class: `GitAnalyzer`. Responsibility: invoke `git log`, parse its output into file co-change pairs, return a frequency map. Zero knowledge of DB, config, or scoring.

**Constructor:**
```
GitAnalyzer(target_dir: Path, watch_extensions: frozenset[str])
```
Stores both. `target_dir` becomes `cwd` for all subprocess calls.

**`is_git_repo() -> bool`**

Runs `git rev-parse --is-inside-work-tree` with `check=False`. Returns `returncode == 0`. Catches `FileNotFoundError` (git absent from PATH) and returns `False`. No other exception handling — if git is on PATH but something else fails, that is unexpected and should propagate.

**`analyze_cochanges(max_commits: int = 500, max_files_per_commit: int = 20) -> dict[tuple[str, str], int]`**

Shell invocation:
```
git log --max-count={max_commits} --name-only --pretty=format:---COMMIT---
```

Parse strategy:
1. Split stdout on `---COMMIT---` to obtain per-commit file lists.
2. Strip blank lines from each block.
3. Skip commits with `< 2` files (no pair possible) or `> max_files_per_commit` files (noisy merge commits).
4. Skip individual files whose suffix is not in `watch_extensions`.
5. From surviving commits, emit all `O(n^2)` file pairs, sorted as `(min(a, b), max(a, b))` for dedup consistency.
6. Accumulate into `collections.Counter`. Return `dict(counter)`.

Subprocess call: `subprocess.run(..., capture_output=True, text=True, timeout=30, cwd=str(self._target_dir))`.

Exception handling:
- `subprocess.TimeoutExpired`: log warning at `WARNING` level, return `{}`. This is graceful degradation — the rest of indexing continues.
- Any other exception: log full stack trace via `log.exception(...)`, re-raise. No silent swallowing.

**Known limitation (docstring):** Git outputs paths relative to repo root. The pipeline stores file paths relative to `target_dir` via `path.relative_to(config.target_dir)`. When `target_dir` is a subdirectory of the repo, git paths carry an extra prefix and will not match stored paths — evolutionary scores degrade to 0.0 for all pairs. This is a Phase 6 scope constraint, not a bug.

---

### `src/loom/store/db.py` — Schema Addition

Append to the `SCHEMA` string after the `index_meta` block, before the FTS5 virtual table:

```sql
CREATE TABLE IF NOT EXISTS cochange (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_a TEXT NOT NULL,
    file_b TEXT NOT NULL,
    frequency INTEGER NOT NULL DEFAULT 1,
    UNIQUE(file_a, file_b)
);
CREATE INDEX IF NOT EXISTS idx_cochange_a ON cochange(file_a);
CREATE INDEX IF NOT EXISTS idx_cochange_b ON cochange(file_b);
```

Critical: use `CREATE TABLE IF NOT EXISTS`, not `DROP TABLE IF EXISTS + CREATE TABLE`. The existing `edges` table uses the destructive pattern as an intentional migration strategy from a prior pipeline. `cochange` must survive reconnects because co-change data is expensive to recompute and is not regenerated on incremental indexing. Replicating the `DROP + CREATE` pattern would silently erase all historical co-change data on every server restart.

---

### `src/loom/store/db.py` — New Methods

Three methods added to `LoomDB`:

**`upsert_cochange(file_a: str, file_b: str, frequency: int) -> None`**

Normalizes the pair to `(min, max)` before writing — double-enforces canonical ordering at the DB layer even if the caller forgets. Uses `INSERT ... ON CONFLICT(file_a, file_b) DO UPDATE SET frequency = excluded.frequency`. This replaces the stored frequency with the new value (the entire git log is re-analyzed on each full_index, so the incoming frequency is always the authoritative total count).

**`get_cochange_frequency(file_a: str, file_b: str) -> int`**

Normalizes to `(min, max)`, queries `SELECT frequency WHERE file_a = ? AND file_b = ?`. Returns `0` if no row — zero is the correct default for the engine's evolutionary score computation.

**`get_top_cochanges(file: str, limit: int = 20) -> list[tuple[str, int]]`**

Returns the top co-changed partners for a file, ranked by frequency descending. The SQL uses a `CASE` expression to return the partner's path regardless of which column holds the query file:
```sql
SELECT CASE WHEN file_a = ? THEN file_b ELSE file_a END AS other_file, frequency
FROM cochange WHERE file_a = ? OR file_b = ?
ORDER BY frequency DESC LIMIT ?
```
This method serves the `status()` MCP tool and future `related()` file-level expansion. Not used in Phase 6 search path directly, but the test coverage gates require it.

**`get_stats()` extension:** Add `"cochange_pairs": cochange_count` to the returned dict. Query: `SELECT COUNT(*) FROM cochange`. Low effort, makes `status()` MCP tool reflect evolutionary coupling completeness.

---

### `src/loom/config.py` — New Fields

Three fields appended to `LoomConfig` (frozen dataclass):

```python
enable_git_analysis: bool = True
git_max_commits: int = 500
git_max_files_per_commit: int = 20
```

Default `enable_git_analysis=True` — git analysis is on by default for any repo. Users with non-git projects or those who want deterministic indexing can disable it. The `False` path produces zero cochange rows; the engine's `_evolutionary_score()` returns 0.0 for all pairs; `fuse_signals()` redistributes weight to structural/semantic as before.

---

### `src/loom/indexer/pipeline.py` — Pipeline Integration

In `full_index()`, after `self._resolve_all_edges()` and before `self._graph.build_from_db()`:

```python
if self._config.enable_git_analysis:
    from loom.indexer.git_analyzer import GitAnalyzer  # deferred import
    git = GitAnalyzer(self._config.target_dir, self._config.watch_extensions)
    if git.is_git_repo():
        cochanges = git.analyze_cochanges(
            max_commits=self._config.git_max_commits,
            max_files_per_commit=self._config.git_max_files_per_commit,
        )
        for (file_a, file_b), freq in cochanges.items():
            self._db.upsert_cochange(file_a, file_b, freq)
        self._db.commit()
        log.info("Git analysis: stored %d co-change pairs", len(cochanges))
```

**Deferred import:** `GitAnalyzer` is imported inside `full_index()` rather than at module top. This avoids loading subprocess machinery at module import time and isolates the import behind the runtime feature flag check.

**Placement rationale:** After Phase 2 resolution (all edges settled) but before graph build. Graph construction can proceed with cochange data already in DB for future extensions that join the graph over cochange. The commit on the co-change writes is separate from the Phase 2 commit — if git analysis fails mid-loop, Phase 2 data remains intact.

**`incremental_index()` exclusion:** Git analysis does not run during incremental indexing. Co-change is a bulk historical measure that spans hundreds of commits. Re-running it on every file change would add 0.7s of latency to every keystroke-triggered re-index and produce no meaningful signal update (a single new commit barely moves the frequency distribution). Co-change data from the last `full_index()` remains valid for the lifetime of the index.

---

### `src/loom/search/engine.py` — Scoring Integration

**New private helper:**

```python
def _evolutionary_score(self, file_a: str, file_b: str) -> float:
    freq = self._db.get_cochange_frequency(file_a, file_b)
    return compute_evolutionary(freq)
```

Single-line delegation. Requires adding `compute_evolutionary` to the import at the top of `engine.py` — currently the import reads `from loom.search.scoring import compute_semantic, compute_structural, fuse_signals`. Add `compute_evolutionary` to that import.

**Five `fuse_signals` callsites to update:**

All five currently pass `0.0` as the evolutionary argument. Each must be replaced with `self._evolutionary_score(target.file, sym.file)` (or `source_sym.file` / `c.symbol.file` where `sym` is not in scope):

| Method | Location | Structural source | Candidate file |
|--------|----------|-------------------|----------------|
| `_find_coupled` | Graph path — initial structural build | `target.file` | `sym.file` |
| `_find_coupled` | Graph path — semantic update for already-seen sym | `target.file` | `c.symbol.file` |
| `_find_coupled` | Graph path — new semantic-only entry | `target.file` | `sym.file` |
| `impact` | Graph path — structural hits | `target.file` | `source_sym.file` |
| `impact` | Semantic pass | `target.file` | `sym.file` |

The fallback path in `_find_coupled` (no graph) uses hardcoded `0.7` / `0.6` scores and does not call `fuse_signals` — it is left unchanged. This path is the pre-Phase-5 fallback and is exercised only when `SearchEngine` is constructed without a `SymbolGraph`. It remains a hardcoded score path; wiring evolutionary into it would require refactoring that path's call sites into `fuse_signals`, which is out of Phase 6 scope.

The unresolved callers path in `impact()` also uses hardcoded scores and does not call `fuse_signals` — same reasoning, left unchanged.

---

## Data Flow

```
full_index() call
    │
    ├── Phase 1: _parse_all_files()    [unchanged]
    ├── Phase 2: _resolve_all_edges()  [unchanged]
    │
    ├── Phase 3 (new): Git Analysis
    │   ├── GitAnalyzer.is_git_repo()
    │   │     subprocess: git rev-parse --is-inside-work-tree
    │   │
    │   ├── GitAnalyzer.analyze_cochanges(max_commits=500)
    │   │     subprocess: git log --max-count=500 --name-only --pretty=format:---COMMIT---
    │   │     parse stdout → Counter{(file_a, file_b): frequency}
    │   │
    │   └── LoomDB.upsert_cochange() × N
    │         INSERT INTO cochange ... ON CONFLICT DO UPDATE
    │         db.commit()
    │
    └── Phase 4: graph.build_from_db() [unchanged]

search query / related / impact / neighborhood call
    │
    └── SearchEngine._find_coupled(target)
          ├── structural pass: graph.neighbors_with_metadata()
          │     compute_structural() → struct_score
          │
          ├── semantic pass: db.search_vec()
          │     compute_semantic() → semantic_score
          │
          ├── evolutionary lookup (new):
          │     _evolutionary_score(target.file, sym.file)
          │       → db.get_cochange_frequency(file_a, file_b) → int
          │       → compute_evolutionary(freq) → float [0, 1]
          │
          └── fuse_signals(struct, semantic, evolutionary, config)
                → CouplingScore.combined
```

---

## Trade-off Decisions

### File-level vs symbol-level co-change

**Decision:** File-level only.

Symbol-level co-change would require identifying which symbols changed within each commit, which demands parsing every historical commit's diff at the character level — complexity that multiplies the git analysis cost by 10-100x and introduces tree-sitter parsing for historical blobs. File-level pairs capture the dominant signal (if `order.js` and `validation.js` always change together, every symbol in those files is co-changed) at O(files_per_commit^2) cost per commit, which is fast. The symbol-level refinement is a future optimization, not Phase 6 scope.

### Bulk recompute on full_index vs incremental update

**Decision:** Bulk recompute only, never incremental.

Co-change is a historical, aggregate signal. A single new commit moves no individual frequency counter by more than 1 out of potentially hundreds. Computing the delta per incremental file change would add complexity (tracking which commits are new, diffing the counter) for near-zero signal improvement. The full `git log --max-count=500` run is 0.5s — acceptable for `full_index()`, prohibitive for keystroke-triggered `incremental_index()`.

### ON CONFLICT DO UPDATE vs INSERT OR REPLACE

**Decision:** `ON CONFLICT DO UPDATE SET frequency = excluded.frequency`.

`INSERT OR REPLACE` is syntactic sugar for `DELETE + INSERT`, which would reset the AUTOINCREMENT `id` column. While that matters little for correctness, it creates unnecessary churn and differs from the explicit semantics we want (update the frequency value, leave everything else alone). The explicit upsert form is clearer.

### Deferred import of GitAnalyzer in pipeline.py

**Decision:** Import inside `full_index()`, not at module top.

`GitAnalyzer` imports `subprocess` transitively. Moving the import to module top would mean every test that instantiates `IndexPipeline` pulls in subprocess — a minor but avoidable coupling. The deferred import also means that if `enable_git_analysis=False`, the `git_analyzer` module is never loaded, which is the correct behavior for performance-sensitive environments that disable git analysis entirely.

### Evolutionary score capping at frequency=10

**Decision:** Inherit `compute_evolutionary(freq, max_frequency=10)` as-is.

10 co-changes as the saturation point means a file pair that changes together once per sprint (for a mid-size repo with regular releases) reaches maximum evolutionary score in about 2-3 months. This calibration was chosen in `scoring.py` in Phase 5 — the formula is already correct and tested. No change needed.

### Same-file pairs

**Decision:** `get_cochange_frequency(file, file)` returns 0; git never records intra-file co-change.

Canonical ordering means `min(a, a) == max(a, a)` would produce `(file, file)` pairs, which git never generates (it cannot report a file as co-changing with itself). The engine never queries same-file pairs because `_find_coupled` excludes `target.id` from candidate sets via `seen`. Belt-and-suspenders: even if queried, the row would not exist, returning 0.

---

## Dependency Map (Phase 6 additions only)

```
git_analyzer.py
    ← subprocess (stdlib)
    ← collections.Counter (stdlib)
    ← pathlib.Path (stdlib)
    ← logging (stdlib)

db.py (cochange additions)
    ← sqlite3 (stdlib, already imported)

config.py (three new fields)
    ← no new imports

pipeline.py (git analysis block)
    ← git_analyzer.GitAnalyzer (new, deferred import)
    ← db.upsert_cochange (new method)

engine.py (scoring integration)
    ← scoring.compute_evolutionary (existing function, new import)
    ← db.get_cochange_frequency (new method)
```

---

## Test Architecture (`tests/test_git_analyzer.py`)

All subprocess interactions are mocked via `unittest.mock.patch("subprocess.run", ...)`. The `db` and `config` fixtures from `conftest.py` are reused directly for DB method tests. No new fixtures are required.

Test classes:

**`TestGitAnalyzerCochangeExtraction`** — pure unit tests on `analyze_cochanges()` parsing logic:
- `test_git_cochange_extraction` — mock subprocess with 2-commit output, verify pairs and frequencies
- `test_large_commit_filtered` — commit with >20 files produces zero pairs
- `test_single_file_commit_filtered` — commit with 1 file produces zero pairs
- `test_extension_filtering` — files with extensions not in `watch_extensions` are dropped; surviving files still form pairs
- `test_cochange_pair_ordering` — `(b, a)` input produces `(a, b)` in output (lexicographic min/max)
- `test_not_a_git_repo` — `is_git_repo()` returns `False` when subprocess returncode != 0
- `test_git_not_on_path` — `is_git_repo()` returns `False` when `FileNotFoundError` raised
- `test_git_timeout` — `subprocess.TimeoutExpired` causes `analyze_cochanges()` to return `{}` without raising

**`TestCochangeDB`** — uses `db` fixture:
- `test_upsert_cochange` — insert pair, verify frequency
- `test_upsert_cochange_updates_on_conflict` — upsert same pair twice, verify frequency updated (not duplicated)
- `test_upsert_cochange_canonical_ordering` — upsert `(b, a)` and `(a, b)` lands as same row
- `test_get_cochange_frequency_missing` — returns 0 for unknown pair
- `test_get_cochange_frequency_exists` — returns correct frequency
- `test_get_top_cochanges` — returns partner files ordered by frequency desc

**`TestEvolutionaryScoringIntegration`** — uses `db` fixture, no subprocess needed:
- `test_evolutionary_score_computation` — freq 10 → 1.0, freq 5 → 0.5, freq 0 → 0.0
- `test_same_file_evolutionary_zero` — `get_cochange_frequency(f, f)` → 0
- `test_scoring_with_evolutionary` — call `fuse_signals` with real cochange data from DB; verify breakdown string includes `evolutionary=` when freq > 0

**`TestPipelineGitIntegration`** — uses `tmp_dir`, mocks subprocess and embedder:
- `test_full_index_runs_git_analysis_when_enabled` — mock git subprocess, verify `upsert_cochange` called
- `test_full_index_skips_git_analysis_when_disabled` — `enable_git_analysis=False`, verify subprocess never called
- `test_full_index_skips_when_not_git_repo` — `is_git_repo()` returns False, verify no cochange writes

---

## Research Notes

No new libraries are introduced in this phase. All components are either stdlib or already in the project dependency graph:

| Component | Decision | Rationale |
|-----------|----------|-----------|
| Git invocation | `subprocess.run` (stdlib) | No git Python library needed; `git log --name-only` output is simple text; no new dependency |
| Pair accumulation | `collections.Counter` (stdlib) | Exact fit for frequency counting; no external dep |
| Co-change store | SQLite via existing `LoomDB` | Already present; `cochange` table is a natural addition to the unified store |

Two git Python libraries were evaluated and rejected:

| Criteria | GitPython | pygit2 |
|----------|-----------|--------|
| PyPI downloads | ~6M/week | ~350k/week |
| Last commit | Active | Active |
| Python 3.12+ | Yes | Yes |
| Type hints | Partial stubs | Partial |
| License | BSD-3 | GPL-2 + linking exception |
| Bundle size | Heavy (git subprocess + object model) | Heavy (libgit2 C library) |
| Verdict | Rejected | Rejected |

**Rejection rationale:** Both libraries provide a full git object model — branches, commits, trees, blobs. Phase 6 needs exactly one thing: `git log --name-only` output. Wrapping that in a library adds significant dependency weight and an abstraction layer over a single shell command. The subprocess approach is 3 lines of code, has no install footprint, and degrades gracefully to `is_git_repo() = False` when git is absent from PATH.
