> Author: planner

# Plan — graph-and-scoring

## Feature Context

Add a NetworkX-backed `SymbolGraph` for transitive traversal and implement real coupling score computation (structural/semantic/evolutionary fusion), replacing the hardcoded 0.7/0.6/0.8 constants currently scattered across `engine.py`.

## Current State

### Key files

- `/Users/reza/work/loom/src/loom/store/models.py` — `Edge` dataclass has `source_id`, `target_id`, `confidence`, `relationship`. `CoupledSymbol` has `score: float` and `reason: str`. Both are ready; no schema changes needed.
- `/Users/reza/work/loom/src/loom/store/db.py` — `get_edges_from()`, `get_edges_to()`, `get_unresolved_edges()`, and `update_edge_target()` all in place. The resolved-edge query needed for `build_from_db` is: `SELECT * FROM edges WHERE target_id IS NOT NULL`. No new DB methods needed for Phase 4.
- `/Users/reza/work/loom/src/loom/search/engine.py` — `SearchEngine.__init__` takes `(db, embedder)`. `_find_coupled()` does one-hop structural traversal with hardcoded scores `0.7` (outgoing) and `0.6` (incoming). `impact()` hardcodes `0.8` for all structural hits and `sim` (raw cosine distance converted to similarity) for semantic. No multi-hop traversal exists.
- `/Users/reza/work/loom/src/loom/indexer/pipeline.py` — `IndexPipeline.__init__` takes `(config, db, embedder)`. After `_resolve_all_edges()`, there is no graph build step.
- `/Users/reza/work/loom/src/loom/server.py` — `initialize()` creates `SearchEngine(db, embedder)` with no graph argument.
- `/Users/reza/work/loom/src/loom/config.py` — `LoomConfig` is a frozen dataclass. No coupling weights exist yet.
- `/Users/reza/work/loom/pyproject.toml` — `networkx` is not in dependencies.
- `/Users/reza/work/loom/tests/conftest.py` — `populated_db` fixture provides 7 symbols and 4 edges (3 resolved, 1 unresolved). `SearchEngine` is instantiated with `(populated_db, mock_embedder)` — constructor must remain backward-compatible or tests must be updated.

### Existing store layer
- `store/graph.py` does not exist — must be created.
- `search/scoring.py` does not exist — must be created.

### Current MCP tool surface
`search`, `related`, `impact`, `neighborhood` — all delegate to `SearchEngine`. The MCP layer (`server.py`) needs no new tools, only updated initialization wiring.

## Gaps & Needed Changes

### Phase 4 — Build the Actual Graph

**`pyproject.toml`**
- Add `networkx>=3.0` to `[project.dependencies]`.
- Add `networkx` to `[[tool.mypy.overrides]] ignore_missing_imports` (no stub package shipped by networkx at time of writing).

**`src/loom/store/graph.py`** (new file)
- `SymbolGraph` class wrapping `networkx.DiGraph`.
- Nodes = integer symbol IDs. Edge attributes: `relationship: str`, `confidence: float`.
- `build_from_db(db: LoomDB) -> None` — query `SELECT source_id, target_id, relationship, confidence FROM edges WHERE target_id IS NOT NULL`, call `add_edge` for each row. Handles duplicate edges (A→B via calls AND imports) by keeping highest confidence on the same directed edge. Clear existing graph before rebuild.
- `add_edge(source_id, target_id, relationship, confidence)` — if edge already exists with lower confidence, update it.
- `remove_node(symbol_id)` — delegates to `self._g.remove_node(symbol_id)`, no-op if absent.
- `dependents(symbol_id, max_depth=3)` — reverse BFS over `self._g.reverse()`, returns `list[tuple[int, int, str, float]]` = `(sym_id, depth, relationship, confidence)`.
- `dependencies(symbol_id, max_depth=3)` — forward BFS, same return type.
- `shortest_path(source_id, target_id)` — `nx.shortest_path(self._g, source, target)` wrapped in try/except, returns `list[int] | None`.
- `impact_radius(symbol_id, max_depth=3)` — BFS over dependents with score = `confidence * (1 / 2^(depth-1))`. Returns `list[tuple[int, float]]` = `(sym_id, score)` sorted descending.
- `centrality(top_n=20)` — `nx.pagerank(self._g)`, return top N `(sym_id, score)`.
- `neighbors_with_metadata(symbol_id, max_depth=2)` — merge `dependents()` + `dependencies()`, deduplicate by sym_id keeping highest score entry.
- Self-loop guard: BFS must not re-visit seen nodes.

**`src/loom/config.py`**
- Add three weight fields to `LoomConfig` (frozen dataclass, so use `field(default=...)`):
  - `structural_weight: float = 0.45`
  - `semantic_weight: float = 0.35`
  - `evolutionary_weight: float = 0.20`

**`src/loom/indexer/pipeline.py`**
- `IndexPipeline.__init__` gains optional `graph: SymbolGraph | None = None`.
- After `_resolve_all_edges()` in both `full_index()` and `incremental_index()`: if `self._graph is not None`, call `self._graph.build_from_db(self._db)`.

**`src/loom/search/engine.py`**
- `SearchEngine.__init__` gains optional `graph: SymbolGraph | None = None` stored as `self._graph`.
- `_find_coupled()`: if graph present, replace one-hop DB edge calls with `graph.neighbors_with_metadata(target.id, max_depth=2)`. Structural score computed via `compute_structural()` (Phase 5). Without Phase 5, keep DB-based one-hop but accept graph for impact. (Phase 4 can wire graph into impact only, keeping _find_coupled unchanged; Phase 5 wires _find_coupled.)
- `impact()`: replace one-hop `get_edges_to()` loop with `graph.impact_radius(target.id, max_depth=3)` when graph is available; fetch symbol objects via `db.get_symbol_by_id()`.

**`src/loom/server.py`**
- Import `SymbolGraph`.
- Add `_graph: SymbolGraph | None = None` module-level.
- In `initialize()`: create `SymbolGraph()`, pass to both `IndexPipeline` and `SearchEngine` constructors. After `full_index()`, graph is already built inside pipeline.

**`tests/test_graph.py`** (new file)
All tests listed in the task spec. Use the `db` fixture from conftest; build a small graph with known edges. Key coverage:
- `test_graph_build_from_resolved_edges` — 5 resolved edges, verify `len(graph._g.nodes)` and `len(graph._g.edges)`.
- `test_graph_ignores_unresolved_edges` — insert one unresolved edge (target_id=None), assert it's absent from graph.
- `test_transitive_dependents` — A→B→C chain, `dependents(C_id)` returns B at depth 1 and A at depth 2.
- `test_transitive_dependencies` — same chain, `dependencies(A_id)` returns B at depth 1, C at depth 2.
- `test_dependents_max_depth` — max_depth=1 returns only B, not A.
- `test_shortest_path` — A→B→C, `shortest_path(A,C)` = [A,B,C].
- `test_shortest_path_no_path` — disconnected nodes, returns None.
- `test_impact_radius_decay` — depth 1 node with confidence 1.0 scores 1.0; depth 2 node scores 0.5.
- `test_centrality_ranking` — hub node (many inbound edges) ranks higher than leaf.
- `test_neighbors_with_metadata` — bidirectional, deduplication.
- `test_incremental_add_edge` — call `add_edge`, verify node and edge appear.
- `test_incremental_remove_node` — call `remove_node`, verify gone.
- `test_empty_graph` — all traversals on empty graph return empty lists, no exceptions.
- `test_self_loop_handling` — insert A→A edge, `dependents(A)` does not loop infinitely.

### Phase 5 — Real Coupling Scores

**`src/loom/search/scoring.py`** (new file)

```python
@dataclass(frozen=True)
class CouplingScore:
    structural: float
    semantic: float
    evolutionary: float
    combined: float

    def breakdown(self) -> str: ...
```

- `RELATIONSHIP_WEIGHT: dict[str, float]` — calls=1.0, extends=1.0, called_by=0.9, extended_by=0.9, instantiates=0.85, imports=0.5, imported_by=0.4, co_located=0.2. Default for unknown relationships: 0.5.
- `compute_structural(relationship: str, confidence: float, depth: int) -> float` — `RELATIONSHIP_WEIGHT.get(relationship, 0.5) * confidence * (1.0 / (2 ** (depth - 1)))`, capped at 1.0.
- `compute_semantic(distance: float) -> float` — `max(0.0, 1.0 - distance)`.
- `compute_evolutionary(frequency: int, max_frequency: int = 10) -> float` — `min(1.0, frequency / max_frequency)`. Returns 0.0 for Phase 5 (no cochange table yet); function signature is forward-compatible with Phase 6.
- `fuse_signals(structural: float, semantic: float, evolutionary: float, config: LoomConfig) -> CouplingScore` — when evolutionary=0.0, redistribute its weight proportionally between structural and semantic using `config.structural_weight` and `config.semantic_weight`. Combined = weighted sum, capped at 1.0.

**`src/loom/search/engine.py`** — major update to `_find_coupled()` and `impact()`:

`_find_coupled()` new logic:
1. Collect structural neighbors via `graph.neighbors_with_metadata(target.id, max_depth=2)` if graph present; fall back to one-hop DB traversal if not.
2. For each structural neighbor: call `compute_structural(relationship, confidence, depth)`, store in `structural_scores: dict[int, float]`.
3. Run `db.search_vec(embedding, limit=20)` for semantic neighbors. For each: `compute_semantic(distance)`, store in `semantic_scores: dict[int, float]`.
4. Union of sym_ids from both sets. For each: `fuse_signals(structural_scores.get(sym_id, 0.0), semantic_scores.get(sym_id, 0.0), 0.0, config)`.
5. Build `CoupledSymbol(symbol=sym, score=cs.combined, reason=cs.breakdown())`.
6. Sort by combined descending, return top 30.

`impact()` new logic:
- If graph present: use `graph.impact_radius(target.id, max_depth=3)` for structural hits. Each `(sym_id, decay_score)` becomes a `CoupledSymbol` with `score=decay_score` and `reason` including the relationship type.
- Semantic hits: same as before but use `compute_semantic(distance)` instead of raw `1.0 - distance` (equivalent but explicit).
- Fuse where sym_id appears in both structural and semantic.

`CoupledSymbol.reason` field: populated with `CouplingScore.breakdown()` string, e.g. `"structural=0.85 + semantic=0.42"`.

**`src/loom/config.py`** (weights added in Phase 4 above — Phase 5 just uses them):
- `fuse_signals` receives `config: LoomConfig` to read `structural_weight`, `semantic_weight`, `evolutionary_weight`.

**`tests/test_scoring.py`** (new file)
All tests from task spec:
- `test_structural_score_calls_vs_imports` — calls score > imports score, same confidence/depth.
- `test_structural_score_depth_decay` — depth 1 > depth 2 > depth 3 for same relationship.
- `test_structural_score_confidence_weighting` — confidence 1.0 > 0.6 at same depth/relationship.
- `test_semantic_score_from_distance` — distance 0→1.0, 0.5→0.5, 1.0→0.0.
- `test_fuse_signals_structural_only` — semantic=0, evolutionary=0, combined = structural × redistributed_weight.
- `test_fuse_signals_semantic_only` — structural=0, evolutionary=0.
- `test_fuse_signals_both` — structural and semantic both nonzero.
- `test_fuse_signals_with_evolutionary` — evolutionary > 0 uses all three weights.
- `test_score_capped_at_one` — inputs that would exceed 1.0 are capped.
- `test_coupling_score_breakdown_string` — breakdown() returns expected format.
- `test_evolutionary_zero_redistributes_weight` — with evolutionary=0, total weight used = structural_weight + semantic_weight = 0.80 (not 1.0).

**`tests/test_engine.py`** — existing tests must still pass. Constructor changes (`graph=None` is optional) preserve backward compatibility. Assertions on `reason` field should be loosened if they check for exact strings like `"calls (structural)"` — post-Phase-5 reason will be `CouplingScore.breakdown()` format. Check current tests: `test_related`, `test_impact_sorted_by_score`, etc. do not assert on exact `reason` strings, so no breaking changes expected. The one risk is `test_structural_results_capped` which checks `"structural" in c.reason` — the breakdown format must include the word "structural" or that assertion needs updating.

## Risks & Dependencies

**Ordering constraint:** Phase 5 depends on Phase 4. `SymbolGraph` must exist before `_find_coupled` can call `graph.neighbors_with_metadata()`. Implement and test Phase 4 fully before Phase 5 touches `engine.py`.

**Constructor backward-compatibility:** `SearchEngine(db, embedder)` and `IndexPipeline(config, db, embedder)` are called in tests without a graph argument. Both must accept `graph=None` (optional, default None) and degrade gracefully to current behavior when graph is absent. This lets existing tests pass without modification during Phase 4.

**`test_structural_results_capped` string check:** Line `assert "structural" in c.reason` — `CouplingScore.breakdown()` must include the word "structural" in its output string (e.g., `"structural=0.85 + semantic=0.42"`). Plan the breakdown format to preserve this.

**NetworkX mypy stubs:** networkx ships type stubs since 3.x but mypy may still complain. Add `networkx` to `[[tool.mypy.overrides]]` ignore_missing_imports as a safety fallback, or install `networkx-stubs` as a dev dep. Check at implementation time.

**Duplicate-edge handling in `build_from_db`:** A symbol A may call AND import B. The DB has two edges with different relationships. NetworkX `DiGraph` allows only one edge per (source, target) pair. Strategy: first pass inserts all edges; on collision, compare confidence and keep max. Alternatively use `MultiDiGraph` — but that complicates BFS. Stick with `DiGraph` and max-confidence deduplication.

**`impact_radius` and `_find_coupled` overlap:** Currently `impact()` does its own separate logic from `_find_coupled`. After Phase 4+5, both should use graph traversal and scoring. The separation is intentional: `related()` (via `_find_coupled`) is bidirectional neighborhood; `impact()` is upstream-only (who depends on me?). Maintain this distinction: `impact_radius` is reverse-only BFS; `neighbors_with_metadata` is bidirectional.

**Coverage gate:** `pyproject.toml` requires 85% coverage. Two new modules (`graph.py`, `scoring.py`) with comprehensive test suites should exceed this. If existing tests currently sit near the 85% floor, the new files must be well-covered or the gate fails.

## Research Needed

- **networkx BFS API:** `nx.bfs_edges(G, source, depth_limit=N)` returns `(u, v)` pairs in BFS order with depth accessible via `nx.bfs_layers`. Use `nx.bfs_layers(G, source)` to iterate by depth level — available since networkx 3.1. Confirm version ≥ 3.1 when adding the dependency pin, or implement manual BFS.
- **networkx type stubs:** Verify whether `networkx>=3.0` ships inline stubs or requires a separate `networkx-stubs` package for mypy strict mode. If stubs are missing, add to mypy overrides.
