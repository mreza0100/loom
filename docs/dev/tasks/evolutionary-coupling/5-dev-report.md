> Author: developer

# Dev Report — evolutionary-coupling

## Implementation Summary

Phase 6 wires evolutionary coupling (git co-change) as a real third signal into Loom's scoring pipeline. Six files were touched; one new file created.

### Files changed

| File | Change |
|------|--------|
| `src/loom/indexer/git_analyzer.py` | **NEW** — `GitAnalyzer` class with `is_git_repo()` and `analyze_cochanges()` |
| `src/loom/config.py` | +3 fields: `enable_git_analysis`, `git_max_commits`, `git_max_files_per_commit` |
| `src/loom/store/db.py` | `cochange` table DDL + `upsert_cochange`, `get_cochange_frequency`, `get_top_cochanges` + `cochange_pairs` in `get_stats()` |
| `src/loom/indexer/pipeline.py` | Git analysis block in `full_index()` (deferred import, guarded by `enable_git_analysis`) |
| `src/loom/search/engine.py` | `_evolutionary_score()` helper + `compute_evolutionary` import + 5 `fuse_signals` callsites updated |
| `tests/test_git_analyzer.py` | **NEW** — 35 tests across 4 test classes |

### Key design decisions carried forward from architecture doc

- `cochange` uses `CREATE TABLE IF NOT EXISTS` — survives reconnects, preserves historical data
- Pair ordering enforced at both `GitAnalyzer` (Counter key) and DB (`upsert_cochange`) layers
- Git analysis only runs in `full_index()`, not `incremental_index()` — co-change is a bulk historical signal
- Deferred import of `GitAnalyzer` in `pipeline.py` — subprocess machinery not loaded when feature is disabled
- Same-file pairs never exist in DB (git doesn't emit them); `get_cochange_frequency(f, f)` correctly returns 0

### One deviation from plan

The plan's callsite 2 in `_find_coupled` had a logic error: `c` (the loop variable) was referenced before the loop began. Fixed by moving `fuse_signals` inside the `for c in coupled` loop where `c` is defined, computing `evo` per matching entry.

## Test Coverage

- **Total:** 442 tests passing, 93.69% coverage (gate: 85%)
- **New tests:** 35 in `tests/test_git_analyzer.py`
  - `TestGitAnalyzerCochangeExtraction` (15 tests) — subprocess mocked throughout
  - `TestCochangeDB` (11 tests) — real SQLite, no mocks
  - `TestEvolutionaryScoringIntegration` (8 tests) — real DB + scoring math
  - `TestPipelineGitIntegration` (3 tests) — full pipeline with mocked subprocess/embedder

## Runbook

```bash
# Run all tests
uv run pytest

# Run only Phase 6 tests
uv run pytest tests/test_git_analyzer.py -v

# Lint + format + types
uv run ruff check && uv run ruff format --check && uv run mypy src/
```

### Config flags

```python
LoomConfig(
    target_dir=Path("/your/project"),
    enable_git_analysis=True,      # default: True
    git_max_commits=500,           # default: 500 (≈0.5s to run)
    git_max_files_per_commit=20,   # default: 20 (filters mega-commits)
)
```

### Expected behavior

- **Git repo, analysis enabled:** `full_index()` runs `git log`, stores co-change pairs in `cochange` table, all 5 `fuse_signals` calls in `engine.py` receive real evolutionary scores.
- **Not a git repo:** `is_git_repo()` returns False, no DB writes, evolutionary scores all 0.0, `fuse_signals` redistributes weight to structural+semantic.
- **`enable_git_analysis=False`:** Git block never entered, same graceful degradation.
- **git not on PATH:** `FileNotFoundError` caught in `is_git_repo()`, returns False, no crash.
- **git log timeout:** `TimeoutExpired` caught, returns `{}`, indexing continues.
