# Pipeline: evolutionary-coupling

**Wave:** foundation-rebuild
**Phase:** 6 (Evolutionary Coupling — Git Co-Change)
**Depends on:** `graph-and-scoring` pipeline (scoring.py's fuse_signals must exist)

---

## What to create

**`src/loom/indexer/git_analyzer.py`** — `GitAnalyzer` class:
- `is_git_repo()` — check if target directory is a git repo
- `analyze_cochanges(max_commits=500, max_files_per_commit=20)` → `{(file_a, file_b): frequency}`
  - Parse `git log --name-only --pretty=format:---COMMIT---`
  - Skip commits with >max_files or <2 files
  - Only track files matching configured extensions
  - Pairs always sorted: `(min(a,b), max(a,b))` for consistent dedup
  - `timeout=30` on subprocess call
- No git-related dependencies needed — uses `subprocess` + `git` CLI

### Schema addition

**`src/loom/store/db.py`** — Add `cochange` table:
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

This is **file-level** co-change, not symbol-level (avoids combinatorial explosion).

### New DB methods
- `upsert_cochange(file_a, file_b, frequency)` — INSERT ON CONFLICT UPDATE
- `get_cochange_frequency(file_a, file_b)` → int
- `get_top_cochanges(file, limit=20)` → [(other_file, frequency)]

### Pipeline integration

**`src/loom/indexer/pipeline.py`** — After Phase 2 resolution in `full_index()`:
```python
if self._config.enable_git_analysis:
    git = GitAnalyzer(self._config.target_dir, self._config.watch_extensions)
    if git.is_git_repo():
        cochanges = git.analyze_cochanges(max_commits=self._config.git_max_commits)
        for (file_a, file_b), freq in cochanges.items():
            self._db.upsert_cochange(file_a, file_b, freq)
        self._db.commit()
```

### Scoring integration

**`src/loom/search/scoring.py`** — Update `compute_evolutionary()`:
- Query cochange table for file pair frequency
- `min(1.0, frequency / 10.0)` — 10+ co-changes = max score
- Same-file pairs return 0.0

**`src/loom/search/engine.py`** — In `_find_coupled()` and `impact()`:
- For each candidate symbol, compute evolutionary score from file co-change
- Pass to `fuse_signals(structural, semantic, evolutionary)`

### Config addition

**`src/loom/config.py`**:
- `enable_git_analysis: bool = True`
- `git_max_commits: int = 500`
- `git_max_files_per_commit: int = 20`

### Performance
- `git log --max-count=500 --name-only`: ~0.5s
- Processing 500 commits: ~0.1s
- Storing 1K-5K pairs: ~0.1s
- Total: ~0.7s — negligible vs embedding generation

### Tests (in `tests/test_git_analyzer.py`)
- `test_git_cochange_extraction` — Mock subprocess, verify pairs
- `test_large_commit_filtered` — >20 files excluded
- `test_single_file_commit_filtered` — <2 files excluded
- `test_extension_filtering` — Only configured extensions
- `test_cochange_pair_ordering` — Always (min, max)
- `test_not_a_git_repo` — Returns empty dict
- `test_git_timeout` — subprocess timeout doesn't crash
- `test_upsert_cochange` — Duplicate pair updates frequency
- `test_get_cochange_frequency` + `test_get_top_cochanges`
- `test_evolutionary_score_computation` — freq 10→1.0, freq 5→0.5, freq 0→0.0
- `test_same_file_evolutionary_zero`
- `test_scoring_with_evolutionary` — Full three-signal fusion

### Done when
- GitAnalyzer class works
- cochange table in schema
- DB methods for co-change CRUD
- Git analysis runs during full_index() when enabled
- compute_evolutionary() queries real data
- fuse_signals() properly weights all three signals
- Config flags present
- Git analysis <2s for 500 commits
- All tests pass with git analysis enabled and disabled
