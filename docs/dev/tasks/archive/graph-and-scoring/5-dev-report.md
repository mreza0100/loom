# Dev Report — graph-and-scoring

## Implementation Summary

### Phase 4: Build the Actual Graph

**`src/loom/store/graph.py`** (new, 75 lines, 100% coverage)
- `SymbolGraph` wraps `networkx.DiGraph`. Nodes are integer symbol IDs; edges carry `relationship` and `confidence`.
- `build_from_db(db)` clears and rebuilds from `SELECT ... WHERE target_id IS NOT NULL`. Duplicate `(source, target)` pairs collapse to the highest-confidence edge.
- BFS uses `nx.bfs_edges(G, source, depth_limit=N)` with a manual `depths: dict[int, int]` tracker — not `bfs_layers` (no depth_limit support) and not `MultiDiGraph` (complicates traversal).
- `centrality()` uses `nx.in_degree_centrality()` instead of `nx.pagerank()` because networkx 3.6+ routes pagerank through scipy by default and scipy is not in the dependency tree.
- `impact_radius()` score formula: `confidence × (1 / 2^(depth−1))`.
- All traversal methods exclude the queried node from results via a `seen` set initialized with `{source}` — self-loops terminate immediately.

**`pyproject.toml`** — added `networkx>=3.2` to dependencies; added `networkx` to mypy `ignore_missing_imports` overrides (no py.typed marker).

**`src/loom/indexer/pipeline.py`** — `IndexPipeline.__init__` gains `graph: SymbolGraph | None = None`. After `_resolve_all_edges()` in both `full_index()` and `incremental_index()`, calls `self._graph.build_from_db(self._db)` when graph is present. `graph=None` default preserves all existing tests.

**`src/loom/server.py`** — creates `SymbolGraph()` in `initialize()`, threads it through both `IndexPipeline` and `SearchEngine` constructors. Shared reference means incremental reindex keeps the engine's graph current automatically.

### Phase 5: Real Coupling Scores

**`src/loom/search/scoring.py`** (new, 31 lines code, 94% coverage)
- `CouplingScore` frozen dataclass with `structural`, `semantic`, `evolutionary`, `combined` fields.
- `breakdown()` returns `"structural=0.85 + semantic=0.42"` — "structural" always present, satisfying the existing `test_structural_results_capped` assertion.
- `RELATIONSHIP_WEIGHT` dict: calls/extends=1.0, called_by/extended_by=0.9, instantiates=0.85, imports=0.5, imported_by=0.4, co_located=0.2.
- `fuse_signals()`: when `evolutionary==0.0`, redistributes its weight proportionally between structural and semantic (100% utilization, not 80%). When `evolutionary>0.0`, uses all three raw weights directly.

**`src/loom/config.py`** — added `structural_weight=0.45`, `semantic_weight=0.35`, `evolutionary_weight=0.20` fields to `LoomConfig` frozen dataclass.

**`src/loom/search/engine.py`** — `SearchEngine.__init__` gains `graph: SymbolGraph | None = None` and `config: LoomConfig | None = None` (both default None for backward compatibility).

- `_find_coupled()`: when graph present, calls `graph.neighbors_with_metadata(target.id, max_depth=2)`, computes `compute_structural(relationship, confidence, depth)` per neighbor, then fuses with semantic vector hits via `fuse_signals()`. When a sym_id is found by both signals, the existing structural-only entry is updated in-place with the fused score. When graph absent, falls back to one-hop DB traversal with hardcoded 0.7/0.6.
- `impact()`: when graph present, calls `graph.impact_radius(target.id, max_depth=3)` for multi-hop structural hits with decay-weighted scores. Unresolved caller fallback (`get_edges_to_by_name`) always runs after graph traversal — the graph only contains resolved edges. Semantic hits use `compute_semantic(distance)` and fuse with structural where both signals are present.

## Bug Fixes (Post-QA)

Five bugs reported by QA were fixed. All test assertions that documented old buggy behavior were updated to assert correct behavior.

| Bug | File | Fix |
|-----|------|-----|
| BUG-001 (Medium) | `search/engine.py` line 290 | Added `_GENERIC_CALL_TARGETS` filter in semantic loop of `impact()` |
| BUG-002 (Low) | `search/scoring.py` line 66 | `compute_semantic()` now caps at 1.0: `min(1.0, max(0.0, 1.0 - distance))` |
| BUG-003 (Low) | `search/scoring.py` line 79 | `compute_evolutionary()` now guards lower bound: `min(1.0, max(0.0, frequency / max_frequency))` |
| BUG-004 (Low) | `search/scoring.py` line 106 | `fuse_signals()` uses `evolutionary < 1e-9` instead of `== 0.0` to eliminate float discontinuity |
| BUG-005 (Low) | `store/graph.py` line 148 | `centrality()` removes self-loops before `in_degree_centrality()` to prevent semantic inflation |

Three QA tests that documented buggy behavior (not gating behavior) were renamed and updated to assert the corrected contracts:
- `test_negative_frequency_returns_negative_score` → `test_negative_frequency_clamped_to_zero`
- `test_evolutionary_tiny_nonzero_switches_to_raw_weights` → `test_evolutionary_tiny_nonzero_uses_redistribution`
- `test_generic_call_target_semantic_path_gap` → `test_generic_call_target_filtered_from_semantic_path`

## Test Coverage

| File | Stmts | Cover |
|------|-------|-------|
| `store/graph.py` | 77 | **100%** |
| `search/scoring.py` | 31 | **100%** |
| `search/engine.py` | 241 | 90% |
| `config.py` | 16 | **100%** |
| `store/db.py` | 163 | 96% |
| **TOTAL** | 1102 | **93.38%** |

**407 tests, 0 failures.** Gate: 85% required, 93.38% achieved.

New test files:
- `tests/test_graph.py` — 30 unit tests covering SymbolGraph: build_from_db, transitive traversal, depth limits, shortest path, impact_radius decay, centrality ranking, neighbors_with_metadata, incremental add/remove, self-loops, empty graph.
- `tests/test_scoring.py` — 20 unit tests covering all scoring functions and CouplingScore.breakdown() format.
- `tests/test_engine_with_graph.py` — 9 integration tests covering graph-enabled engine paths: multi-hop related(), real coupling scores, structural keyword in reason, impact() traversal, depth decay, unresolved fallback, MAX_STRUCTURAL_RESULTS cap, semantic fusion, no-graph fallback.
- `tests/test_qa_graph_and_scoring.py` — 58 adversarial tests (QA-authored), updated for bug fixes.

## Runbook

```bash
# Install (includes networkx)
uv sync

# Run all tests
uv run pytest

# Lint + format
uv run ruff check && uv run ruff format

# Type check
uv run mypy src/loom
```

No configuration changes are needed to enable the graph — it's wired into `server.py:initialize()` by default. The graph is populated during `full_index()` and rebuilt on every `incremental_index()`.

To disable graph traversal (fallback to one-hop DB behavior), pass `graph=None` to `SearchEngine` and `IndexPipeline` constructors — all existing tests use this path.
