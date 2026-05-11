> Author: planner

# Plan — evolutionary-coupling

## Feature Context

Mine git log for file-level co-change pairs, persist them in a `cochange` table, and wire the frequency data into the existing `compute_evolutionary()` / `fuse_signals()` pipeline so all three coupling signals (structural, semantic, evolutionary) contribute to every search result.

---

## Current State

### Config — `/Users/reza/work/loom/src/loom/config.py`
`LoomConfig` already carries `structural_weight=0.45`, `semantic_weight=0.35`, `evolutionary_weight=0.20`. The three git-analysis flags listed in the task spec (`enable_git_analysis`, `git_max_commits`, `git_max_files_per_commit`) are **absent** — this is the only gap in config.

### Store — `/Users/reza/work/loom/src/loom/store/db.py`
- `SCHEMA` string uses `CREATE TABLE IF NOT EXISTS` for `symbols` and `index_meta`, but **`DROP TABLE IF EXISTS` + plain `CREATE TABLE` for `edges`** (schema migration strategy inherited from previous pipeline). The `cochange` table and its three methods do not exist yet.
- `get_stats()` reports `symbols`, `edges`, `files`, `vectors`, `last_indexed`, `stale_files` — no cochange count. This is a nice-to-have addition, not a blocker.
- `LoomDB` has no cochange-related methods.

### Scoring — `/Users/reza/work/loom/src/loom/search/scoring.py`
`compute_evolutionary(frequency, max_frequency=10)` is already implemented and correct — returns `min(1.0, max(0.0, frequency / max_frequency))`. It is called nowhere yet (always receives `0` by convention). `fuse_signals()` correctly redistributes weight when `evolutionary < 1e-9`. No changes to these functions are needed; the only gap is that the engine never feeds real frequency data into them.

### Engine — `/Users/reza/work/loom/src/loom/search/engine.py`
`_find_coupled()` calls `fuse_signals(struct_score, semantic_score, 0.0, self._config)` — evolutionary hardcoded to 0.0 in five separate call sites. `impact()` has the same pattern (two `fuse_signals` calls, both with `0.0`). No reference to cochange data anywhere.

### Pipeline — `/Users/reza/work/loom/src/loom/indexer/pipeline.py`
`full_index()` calls `_parse_all_files()`, `_resolve_all_edges()`, then optionally `self._graph.build_from_db()`. No git analysis step. `IndexPipeline.__init__` takes `config, db, embedder, graph` — adding `GitAnalyzer` as a local within `full_index()` is straightforward.

### Indexer modules — `/Users/reza/work/loom/src/loom/indexer/`
Contains `embedder.py`, `parser.py`, `pipeline.py`, `watcher.py`. No `git_analyzer.py` — this is the primary new file.

### Tests
- `tests/test_scoring.py` has `TestComputeEvolutionary` testing the function with hardcoded inputs; it passes already. No test for DB cochange methods or `GitAnalyzer`.
- `tests/conftest.py` provides `db` and `config` fixtures that the new DB tests can reuse directly.
- Test pattern across the suite: mock heavy deps (fastembed, subprocess) with `unittest.mock.MagicMock`/`patch`.

---

## Gaps & Needed Changes

### 1. New file: `src/loom/indexer/git_analyzer.py`

Create `GitAnalyzer` class with:
- `__init__(self, target_dir: Path, watch_extensions: frozenset[str])` — stores both; uses `target_dir` as `cwd` for subprocess calls.
- `is_git_repo(self) -> bool` — run `git rev-parse --is-inside-work-tree` with `check=False`; return `returncode == 0`. Catches `FileNotFoundError` (git not on PATH) and returns `False`.
- `analyze_cochanges(self, max_commits: int = 500, max_files_per_commit: int = 20) -> dict[tuple[str, str], int]` — shell call: `git log --max-count={max_commits} --name-only --pretty=format:---COMMIT---`. Parse stdout: split on `---COMMIT---`, strip blank lines, collect file paths per commit. Filter: skip commits with `< 2` or `> max_files_per_commit` files; skip files whose suffix is not in `watch_extensions`. For surviving commits, emit all file pairs sorted as `(min(a,b), max(a,b))`, accumulate into `Counter`. Return `dict(counter)`. Use `subprocess.run(..., capture_output=True, text=True, timeout=30, cwd=str(self._target_dir))`. On `subprocess.TimeoutExpired`, log warning and return `{}`. On any other exception, log full stack trace and re-raise.

Type hints: return type is `dict[tuple[str, str], int]`.

### 2. Schema addition in `src/loom/store/db.py`

Add to the `SCHEMA` string (append after `index_meta` block, before the closing of the string):

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

Note the `SCHEMA` string currently ends with the FTS5 virtual table. The `cochange` DDL must use `CREATE TABLE IF NOT EXISTS` (not `DROP + CREATE`) to avoid destroying co-change data on reconnect. The existing `edges` table uses `DROP + CREATE` as a migration strategy from a previous pipeline — do not replicate that pattern here.

### 3. New DB methods in `src/loom/store/db.py`

Add three methods to `LoomDB`:

```python
def upsert_cochange(self, file_a: str, file_b: str, frequency: int) -> None:
    # Always store as (min, max) pair — enforces canonical ordering at DB level too
    a, b = (min(file_a, file_b), max(file_a, file_b))
    self.conn.execute(
        "INSERT INTO cochange (file_a, file_b, frequency) VALUES (?, ?, ?) "
        "ON CONFLICT(file_a, file_b) DO UPDATE SET frequency = excluded.frequency",
        (a, b, frequency),
    )

def get_cochange_frequency(self, file_a: str, file_b: str) -> int:
    a, b = (min(file_a, file_b), max(file_a, file_b))
    row = self.conn.execute(
        "SELECT frequency FROM cochange WHERE file_a = ? AND file_b = ?",
        (a, b),
    ).fetchone()
    return row[0] if row else 0

def get_top_cochanges(self, file: str, limit: int = 20) -> list[tuple[str, int]]:
    rows = self.conn.execute(
        "SELECT CASE WHEN file_a = ? THEN file_b ELSE file_a END AS other_file, frequency "
        "FROM cochange WHERE file_a = ? OR file_b = ? "
        "ORDER BY frequency DESC LIMIT ?",
        (file, file, file, limit),
    ).fetchall()
    return [(row[0], row[1]) for row in rows]
```

Optionally extend `get_stats()` to include `"cochange_pairs": int` for the `status()` MCP tool — low effort, useful for observability.

### 4. Config additions in `src/loom/config.py`

Add three fields to `LoomConfig`:

```python
enable_git_analysis: bool = True
git_max_commits: int = 500
git_max_files_per_commit: int = 20
```

Since `LoomConfig` is a frozen dataclass, these are just additional fields with defaults — no structural change needed.

### 5. Pipeline integration in `src/loom/indexer/pipeline.py`

In `full_index()`, after `self._resolve_all_edges()` and before the graph build:

```python
if self._config.enable_git_analysis:
    from loom.indexer.git_analyzer import GitAnalyzer
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

The import is deferred to avoid importing subprocess machinery at module load time. Do not run git analysis during `incremental_index()` — full re-analysis on each file change would be prohibitively slow and the co-change signal is a bulk/historical measure.

### 6. Scoring integration in `src/loom/search/engine.py`

The engine currently passes `0.0` for evolutionary in every `fuse_signals()` call. To wire in real data, the engine needs access to the DB for cochange lookups — it already holds `self._db`.

Two integration points:

**`_find_coupled()`** — after building `structural_scores` and the initial coupled list from structural neighbors, and after the semantic pass loop, retroactively compute evolutionary score for each coupled symbol:

```python
def _evolutionary_score(self, file_a: str, file_b: str) -> float:
    freq = self._db.get_cochange_frequency(file_a, file_b)
    return compute_evolutionary(freq)
```

In `_find_coupled()`, replace all `fuse_signals(struct, sem, 0.0, ...)` calls with `fuse_signals(struct, sem, self._evolutionary_score(target.file, sym.file), ...)`.

**`impact()`** — same pattern: wherever `fuse_signals` is called with `0.0` as evolutionary, compute and substitute the real value.

There are five `fuse_signals` call sites total across the two methods. Each must be updated. The target symbol's file is always `target.file`; the candidate symbol's file is `sym.file` or `source_sym.file` depending on context.

Add a private helper `_evolutionary_score(self, file_a: str, file_b: str) -> float` — a single-line delegation to `compute_evolutionary(self._db.get_cochange_frequency(file_a, file_b))`. Import `compute_evolutionary` from `loom.search.scoring` (it's already imported alongside `compute_semantic`, `compute_structural`, `fuse_signals` at the top of `engine.py`).

---

## Risks & Dependencies

**Ordering constraint:** `cochange` DDL must land in `SCHEMA` before any test that calls `db.connect()` — the schema is applied at connect-time. The `upsert_cochange`/`get_*` methods depend on the table existing.

**`DROP TABLE IF EXISTS edges` pattern in `SCHEMA`:** The current schema drops and recreates `edges` on every `connect()` call. This is intentional migration behavior from a prior pipeline. Do not replicate it for `cochange` — use `CREATE TABLE IF NOT EXISTS` so historical co-change data survives reconnects. This distinction is already noted in the task spec.

**Subprocess isolation in tests:** All tests that instantiate `GitAnalyzer` must mock `subprocess.run`. The `is_git_repo()` and `analyze_cochanges()` methods share the same subprocess boundary — mock at `subprocess.run` via `unittest.mock.patch("subprocess.run", ...)`.

**Non-git repos:** `is_git_repo()` returns `False`, `analyze_cochanges()` is never called, no cochange rows are written. The engine's `_evolutionary_score()` returns `0.0` for all pairs (no rows in table), and `fuse_signals()` redistributes weight to structural/semantic as before. This is the correct graceful degradation.

**`enable_git_analysis=False`:** Same graceful degradation — git analysis block is skipped entirely. Zero cochange rows means zero evolutionary contribution everywhere.

**File path format:** Git outputs paths relative to repo root. The pipeline stores file paths relative to `target_dir` (via `path.relative_to(self._config.target_dir)`). If `target_dir` is the repo root, paths match directly. If it is a subdirectory, git paths will have an extra prefix. The `analyze_cochanges()` method should accept this as a known limitation (Phase 6 scope) — mismatched paths simply produce no cochange matches, degrading gracefully to zero evolutionary score. Document in docstring.

**Git not on PATH:** `is_git_repo()` catches `FileNotFoundError` and returns `False`. No crash.

**Large commits:** Already handled by `max_files_per_commit` filter.

**Coverage gate:** `pyproject.toml` enforces 85% coverage. The new `git_analyzer.py` file must be covered by tests; `tests/test_git_analyzer.py` handles this. The `_evolutionary_score()` helper in engine.py will be exercised by the existing `test_engine_with_graph.py` fixture path once the scoring integration test is added.

---

## Research Needed

None. All required components are already in the codebase or standard library:
- `subprocess` — stdlib, already used implicitly (watchdog uses it internally); no new dep
- `collections.Counter` — stdlib, no import needed beyond standard Python
- `compute_evolutionary()` — already implemented correctly in `scoring.py`
- `fuse_signals()` — already handles three-signal case when evolutionary > 0
- DB layer pattern — `LoomDB` methods follow an established pattern; `upsert_cochange` follows the same style as `set_file_hash` (INSERT OR REPLACE)
