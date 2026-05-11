# Pipeline: graph-and-scoring

**Wave:** foundation-rebuild
**Phases:** 4 (Build the Actual Graph) + 5 (Real Coupling Scores)
**Build order within pipeline:** Phase 4 first, then Phase 5
**Depends on:** `foundation-data-model` pipeline (ID-based edges + two-phase resolution must be merged)

---

## Phase 4: Build the Actual Graph

**Priority:** HIGH — required for real coupling scores and transitive queries

### What to create

**`src/loom/store/graph.py`** — `SymbolGraph` class wrapping NetworkX DiGraph:
- Nodes = symbol IDs (integers)
- Edge attributes: relationship (str), confidence (float)
- `build_from_db(db)` — load all resolved edges (target_id IS NOT NULL)
- `add_edge(source_id, target_id, relationship, confidence)` — incremental update
- `remove_node(symbol_id)` — incremental removal
- `dependents(symbol_id, max_depth=3)` — reverse BFS, returns [(sym_id, depth, relationship, confidence)]
- `dependencies(symbol_id, max_depth=3)` — forward BFS
- `shortest_path(source_id, target_id)` — returns symbol ID chain or None
- `impact_radius(symbol_id, max_depth=3)` — blast radius with exponential decay (depth 1=1.0, depth 2=0.5, depth 3=0.25) weighted by confidence
- `centrality(top_n=20)` — PageRank ranking
- `neighbors_with_metadata(symbol_id, max_depth=2)` — both dependents + dependencies merged
- Handle duplicate edges (A calls B AND A imports B): keep highest confidence

### What to change

**`pyproject.toml`** — Add `networkx>=3.0` to dependencies.

**`src/loom/server.py`** — Initialize `SymbolGraph` alongside engine:
- Create graph, pass to SearchEngine constructor
- After indexing: `graph.build_from_db(db)`

**`src/loom/indexer/pipeline.py`** — Accept `graph: SymbolGraph` parameter:
- After `_resolve_all_edges()`: `self._graph.build_from_db(self._db)`

**`src/loom/search/engine.py`** — Accept `graph: SymbolGraph` in constructor:
- `impact()`: Use `graph.impact_radius()` for transitive traversal instead of one-hop SQL
- `_find_coupled()`: Use `graph.neighbors_with_metadata()` for multi-hop discovery

### Memory budget
- 10K symbols / 20K edges = ~4 MB. Fine for MCP server.
- Rebuild from DB is O(edges). Should be <1s for 20K edges.

### Tests (in `tests/test_graph.py`)
- `test_graph_build_from_resolved_edges` — 5 edges, verify node/edge count
- `test_graph_ignores_unresolved_edges` — Unresolved edges not in graph
- `test_transitive_dependents` — A→B→C, dependents(C) = [B@1, A@2]
- `test_transitive_dependencies` — A→B→C, dependencies(A) = [B@1, C@2]
- `test_dependents_max_depth` — max_depth respected
- `test_shortest_path` + `test_shortest_path_no_path`
- `test_impact_radius_decay` — depth decay × confidence
- `test_centrality_ranking` — hub node ranks higher
- `test_neighbors_with_metadata` — merged + deduplicated
- `test_incremental_add_edge` + `test_incremental_remove_node`
- `test_empty_graph` — no crashes
- `test_self_loop_handling` — no infinite traversal

### Done when
- networkx in dependencies
- SymbolGraph class in graph.py
- Graph built from resolved edges after indexing
- impact() uses graph traversal
- _find_coupled() uses multi-hop discovery
- All tests pass
- Graph build <1s for 20K edges, traversals <10ms per query

---

## Phase 5: Real Coupling Scores

**Priority:** HIGH — the entire value proposition
**Do this AFTER Phase 4**

### What to create

**`src/loom/search/scoring.py`** — Coupling score computation:

```
CouplingScore(structural, semantic, evolutionary, combined)
```

- `compute_structural(relationship, confidence, depth)` — `base_weight × confidence × depth_decay`
  - RELATIONSHIP_WEIGHT: calls=1.0, extends=1.0, called_by=0.9, extended_by=0.9, instantiates=0.85, imports=0.5, imported_by=0.4, co_located=0.2
  - depth_decay: 1/(2^(depth-1)) — 1.0 at depth 1, 0.5 at depth 2, 0.25 at depth 3
- `compute_semantic(distance)` — `max(0, 1 - distance)` (L2 to similarity)
- `compute_evolutionary(frequency, max_frequency=10)` — normalized. Returns 0.0 until Phase 6.
- `fuse_signals(structural, semantic, evolutionary)` — weighted fusion
  - W_STRUCTURAL=0.45, W_SEMANTIC=0.35, W_EVOLUTIONARY=0.20
  - When evolutionary=0.0, redistribute weight proportionally to structural+semantic
- `CouplingScore.breakdown()` — readable signal decomposition: `"structural=0.85 + semantic=0.42"`

### What to change

**`src/loom/search/engine.py`**:
- `_find_coupled()`: Use `compute_structural()` + `compute_semantic()` + `fuse_signals()` instead of hardcoded 0.7/0.6
  1. Get graph neighbors → compute structural score per neighbor
  2. Get semantic neighbors from vec search → compute semantic score
  3. For neighbors found by both signals, fuse them
  4. Sort by combined score, return top 30
- `impact()`: Use coupling scores from graph traversal, not flat 0.8
- `CoupledSymbol.reason` field: show score breakdown instead of flat labels

**`src/loom/config.py`** — Add configurable weights:
- `structural_weight: float = 0.45`
- `semantic_weight: float = 0.35`
- `evolutionary_weight: float = 0.20`

### Expected score distribution (AFTER)
- Direct call, confidence 1.0: combined ~0.56-0.80
- Import, confidence 0.95: combined ~0.27-0.55
- 2-hop transitive: combined ~0.23-0.50
- Semantic-only: combined ~0.31
- Co-located: combined ~0.12-0.40

### Tests (in `tests/test_scoring.py`)
- `test_structural_score_calls_vs_imports` — calls > imports
- `test_structural_score_depth_decay` — depth 1 > depth 2 > depth 3
- `test_structural_score_confidence_weighting` — confidence 1.0 > 0.6
- `test_semantic_score_from_distance` — distance 0→1.0, 0.5→0.5, 1.0→0.0
- `test_fuse_signals_structural_only`, `_semantic_only`, `_both`
- `test_fuse_signals_with_evolutionary`
- `test_score_capped_at_one`
- `test_coupling_score_breakdown_string`
- `test_evolutionary_zero_redistributes_weight`

### Done when
- CouplingScore dataclass with breakdown
- scoring.py module with all compute functions
- _find_coupled uses real computation
- impact() uses coupling scores from graph
- reason field shows score breakdown
- Score distribution is continuous, not flat
- Weights configurable via LoomConfig
