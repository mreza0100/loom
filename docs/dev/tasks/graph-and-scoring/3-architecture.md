> Author: architect

# Architecture — graph-and-scoring

## Overview

Two new modules slot into the existing layered architecture without schema changes:

- `src/loom/store/graph.py` — `SymbolGraph`, an in-memory NetworkX DiGraph over resolved DB edges
- `src/loom/search/scoring.py` — `CouplingScore` dataclass + pure functions for structural/semantic/evolutionary signal computation and fusion

All other files receive targeted edits to wire these in. No new MCP tools, no schema migrations.

---

## File Structure Changes

```
src/loom/
├── store/
│   ├── graph.py          NEW — SymbolGraph wrapping NetworkX DiGraph
│   ├── db.py             unchanged
│   └── models.py         unchanged
├── search/
│   ├── scoring.py        NEW — CouplingScore, compute_*, fuse_signals
│   └── engine.py         EDIT — accept graph, use scoring, replace hardcoded floats
├── indexer/
│   └── pipeline.py       EDIT — accept graph, call build_from_db after resolve
├── config.py             EDIT — add structural_weight, semantic_weight, evolutionary_weight
└── server.py             EDIT — create SymbolGraph, thread it through pipeline + engine

tests/
├── test_graph.py         NEW — 13 unit tests for SymbolGraph
└── test_scoring.py       NEW — 11 unit tests for scoring functions
```

---

## Module Responsibilities

### `src/loom/store/graph.py` — SymbolGraph

Single-responsibility: maintain an in-memory directed graph of resolved symbol relationships, and answer traversal queries over it.

**Internal state:**
- `self._g: nx.DiGraph` — nodes are integer symbol IDs; each directed edge carries `relationship: str` and `confidence: float` attributes.

**Public interface:**

```python
class SymbolGraph:
    def build_from_db(self, db: LoomDB) -> None: ...
    def add_edge(self, source_id: int, target_id: int, relationship: str, confidence: float) -> None: ...
    def remove_node(self, symbol_id: int) -> None: ...

    def dependents(self, symbol_id: int, max_depth: int = 3) -> list[tuple[int, int, str, float]]: ...
    def dependencies(self, symbol_id: int, max_depth: int = 3) -> list[tuple[int, int, str, float]]: ...
    def shortest_path(self, source_id: int, target_id: int) -> list[int] | None: ...
    def impact_radius(self, symbol_id: int, max_depth: int = 3) -> list[tuple[int, float]]: ...
    def centrality(self, top_n: int = 20) -> list[tuple[int, float]]: ...
    def neighbors_with_metadata(self, symbol_id: int, max_depth: int = 2) -> list[tuple[int, int, str, float]]: ...
```

Return type for traversal tuples: `(sym_id: int, depth: int, relationship: str, confidence: float)`.
Return type for `impact_radius`: `(sym_id: int, impact_score: float)` sorted descending.

**build_from_db behavior:**
- Clears `self._g` before rebuilding (full replace, not merge).
- Queries: `SELECT source_id, target_id, relationship, confidence FROM edges WHERE target_id IS NOT NULL`.
- Duplicate-edge handling: NetworkX `DiGraph` permits one edge per `(source, target)` pair. When two DB edges share the same `(source_id, target_id)` but different relationships (e.g., `calls` + `imports`), keep the one with highest `confidence`. Implementation: insert in DB row order; on collision, check existing edge's confidence and call `add_edge` only if the new confidence is higher.

**BFS implementation strategy:**
Use `nx.bfs_edges(G, source, depth_limit=max_depth)` which yields `(u, v)` tuples in BFS order and natively supports `depth_limit`. Track depth with a visited-with-depth dict (`seen: dict[int, int]` mapping `node -> depth`). This is more reliable than `bfs_layers` which lacks a `depth_limit` parameter in the current NetworkX API.

For `dependents()` (reverse traversal): call `nx.bfs_edges(self._g.reverse(copy=False), source, depth_limit=max_depth)`. Using `copy=False` avoids an O(E) copy.

Self-loop guard: the `seen` dict prevents revisiting any node, including self-loops. `bfs_edges` itself already skips seen nodes, but the depth-tracking dict provides an additional safety layer.

**impact_radius score formula:**
`score = confidence × (1.0 / 2^(depth - 1))`
- depth 1, confidence 1.0 → 1.0
- depth 2, confidence 1.0 → 0.5
- depth 3, confidence 0.8 → 0.2

Results sorted descending by score.

**centrality:** `nx.pagerank(self._g)` returns `dict[int, float]`. Sort descending, return top N as `list[tuple[int, float]]`.

**neighbors_with_metadata:** Merge results from `dependents()` and `dependencies()`. Deduplicate by `sym_id`, keeping the entry with higher `confidence`. Does NOT merge depth values — use the entry that came first (dependents win on tie, as direct callers are more relevant than transitive dependencies).

**Memory:** At 10K symbols / 20K edges, NetworkX DiGraph uses approximately 4 MB. Build is O(E). All traversals up to depth 3 are O(V + E) in the worst case but practically sub-10ms for typical codebases.

---

### `src/loom/search/scoring.py` — CouplingScore

Pure functions — no I/O, no DB access, no state. Fully unit-testable in isolation.

**CouplingScore dataclass:**
```python
@dataclass(frozen=True)
class CouplingScore:
    structural: float
    semantic: float
    evolutionary: float
    combined: float

    def breakdown(self) -> str:
        # Returns: "structural=0.85 + semantic=0.42"
        # or: "structural=0.85 + semantic=0.42 + evolutionary=0.30"
        # evolutionary is omitted when 0.0
```

The word "structural" must appear in the `breakdown()` output. This preserves the existing `test_structural_results_capped` assertion: `assert "structural" in c.reason`.

**RELATIONSHIP_WEIGHT constant:**
```python
RELATIONSHIP_WEIGHT: dict[str, float] = {
    "calls": 1.0,
    "extends": 1.0,
    "called_by": 0.9,
    "extended_by": 0.9,
    "instantiates": 0.85,
    "imports": 0.5,
    "imported_by": 0.4,
    "co_located": 0.2,
}
DEFAULT_RELATIONSHIP_WEIGHT = 0.5
```

**compute_structural(relationship, confidence, depth) -> float:**
`min(1.0, RELATIONSHIP_WEIGHT.get(relationship, 0.5) * confidence * (1.0 / (2 ** (depth - 1))))`

**compute_semantic(distance) -> float:**
`max(0.0, 1.0 - distance)` — L2 distance to similarity.

**compute_evolutionary(frequency, max_frequency=10) -> float:**
`min(1.0, frequency / max_frequency)` — returns 0.0 in Phase 5 (no cochange table). Forward-compatible with Phase 6.

**fuse_signals(structural, semantic, evolutionary, config) -> CouplingScore:**
When `evolutionary == 0.0`, redistribute its weight proportionally to structural and semantic using their configured ratio:
```
total_base = config.structural_weight + config.semantic_weight
effective_structural_w = config.structural_weight / total_base
effective_semantic_w   = config.semantic_weight / total_base
combined = min(1.0, structural * effective_structural_w + semantic * effective_semantic_w)
```

When `evolutionary > 0.0`, use all three weights directly:
```
combined = min(1.0,
    structural * config.structural_weight +
    semantic   * config.semantic_weight   +
    evolutionary * config.evolutionary_weight
)
```

This means with defaults (0.45 / 0.35 / 0.20) and evolutionary=0.0:
- effective_structural_w = 0.45 / 0.80 = 0.5625
- effective_semantic_w   = 0.35 / 0.80 = 0.4375
- Total weight consumed = 1.0 (not 0.80 — redistribution fills the gap)

This is the correct behavior: when evolutionary signal is absent, structural + semantic still sum to a full 1.0 weighting. The test `test_evolutionary_zero_redistributes_weight` must verify that the weights are proportionally redistributed (not simply dropped), resulting in `combined` values consistent with using 100% of structural + semantic weight — not 80%.

---

### `src/loom/search/engine.py` — Edits

**Constructor change:**
```python
def __init__(self, db: LoomDB, embedder: Embedder, graph: SymbolGraph | None = None, config: LoomConfig | None = None) -> None:
```
`graph=None` and `config=None` default to backward-compatible no-graph behavior. Existing tests pass without modification.

**_find_coupled() new logic (Phase 5):**
1. If `self._graph` is not None: call `self._graph.neighbors_with_metadata(target.id, max_depth=2)` to get structural neighbors as `list[tuple[sym_id, depth, relationship, confidence]]`.
2. For each structural neighbor: `structural_score = compute_structural(relationship, confidence, depth)`. Store in `structural_scores: dict[int, float]`.
3. Run `self._db.search_vec(embedding, limit=20)` for semantic neighbors. For each `(sym_id, distance)`: `semantic_score = compute_semantic(distance)`. Store in `semantic_scores: dict[int, float]`.
4. Union of all sym_ids from both sets (minus `target.id`, minus `_GENERIC_CALL_TARGETS` filtered).
5. For each sym_id: `cs = fuse_signals(structural_scores.get(sym_id, 0.0), semantic_scores.get(sym_id, 0.0), 0.0, self._config)`.
6. Build `CoupledSymbol(symbol=sym, score=cs.combined, reason=cs.breakdown())`.
7. Sort by combined descending, return top `MAX_STRUCTURAL_RESULTS` (30).

Fallback when graph is absent: retain current one-hop DB traversal with hardcoded 0.7/0.6. Phase 4 wires graph into `impact()` only; Phase 5 wires it into `_find_coupled()` too.

**impact() new logic (Phase 4+5):**
When `self._graph` is not None:
- `graph.impact_radius(target.id, max_depth=3)` returns `list[tuple[sym_id, decay_score]]`.
- For each `(sym_id, decay_score)`: fetch symbol via `db.get_symbol_by_id`, apply generic-name filter, build `CoupledSymbol(score=decay_score, reason=cs.breakdown())`.
- Still include unresolved-by-name fallback (current `get_edges_to_by_name` logic) for callers not in the graph.
- Semantic hits: `compute_semantic(distance)` instead of raw `1 - distance` (same result, explicit).
- Where sym_id appears in both structural and semantic: `fuse_signals` for combined score.

The generic-name filter (`_GENERIC_CALL_TARGETS`) is applied at the symbol-name level for sources, same as today.

**`self._config` field:** The engine stores a reference to `LoomConfig` for weight access. When `config=None`, use a default `LoomConfig` with a sentinel `target_dir` (never used for DB ops). Alternative: pass only the three weight floats. Preferred: pass full config for forward compatibility.

---

### `src/loom/config.py` — Edits

Add three fields to `LoomConfig` (frozen dataclass — add with `field(default=...)`):

```python
structural_weight: float = 0.45
semantic_weight: float = 0.35
evolutionary_weight: float = 0.20
```

No other changes.

---

### `src/loom/indexer/pipeline.py` — Edits

Constructor:
```python
def __init__(self, config: LoomConfig, db: LoomDB, embedder: Embedder, graph: SymbolGraph | None = None) -> None:
```

In both `full_index()` and `incremental_index()`, after `self._resolve_all_edges()`:
```python
if self._graph is not None:
    self._graph.build_from_db(self._db)
```

This placement ensures the graph sees all resolved edges from the just-completed Phase 2 resolution.

---

### `src/loom/server.py` — Edits

Module-level: add `_graph: SymbolGraph | None = None`.

In `initialize()`:
```python
_graph = SymbolGraph()
_pipeline = IndexPipeline(_config, _db, _embedder, graph=_graph)
_engine = SearchEngine(_db, _embedder, graph=_graph, config=_config)
```

`full_index()` is called after pipeline creation; `build_from_db` runs inside pipeline at the end of `full_index()`, so the graph is populated before `_engine` receives its first query. The shared reference means incremental re-index also keeps the engine's graph reference current without any additional wiring.

---

### `pyproject.toml` — Edits

Add to `[project.dependencies]`:
```
"networkx>=3.2",
```

Version pin rationale: `bfs_edges` with `depth_limit` is available since NetworkX 2.5+. `bfs_layers` was added in 2.6. Pinning `>=3.2` ensures a reasonably recent release with stable type annotations.

Add to `[[tool.mypy.overrides]]`:
```
module = ["networkx"]
ignore_missing_imports = true
```

Rationale: NetworkX does not ship a `py.typed` marker. The `types-networkx` package on PyPI (version 3.6.1.20260408) provides stubs, but adding it as an optional dev dependency introduces version coupling risk. The `ignore_missing_imports` override is the lowest-friction path for strict mypy compliance. Developer can opt in to `types-networkx` if they want richer IDE completion.

---

## Data Flow

### Indexing (server startup / reindex)

```
server.initialize()
  → SymbolGraph()               # empty graph
  → IndexPipeline(graph=graph)
  → pipeline.full_index()
      → _parse_all_files()      # Phase 1: symbols + raw edges (target_id=None)
      → _resolve_all_edges()    # Phase 2: target_id + confidence filled in
      → graph.build_from_db()   # loads all resolved edges into DiGraph
```

### Query: related(symbol)

```
engine.related(symbol_name)
  → db.get_symbol_by_name_fuzzy()    # find target Symbol
  → _find_coupled(target)
      → graph.neighbors_with_metadata(target.id, max_depth=2)
          → bfs_edges(G, target.id, depth_limit=2)         # forward
          → bfs_edges(G.reverse(), target.id, depth_limit=2) # reverse
          → merge + deduplicate by sym_id
      → for each neighbor: compute_structural(rel, conf, depth)
      → db.search_vec(embedding, limit=20)
      → for each vec hit: compute_semantic(distance)
      → union of sym_ids → fuse_signals() → CoupledSymbol(score, reason=breakdown())
      → sort desc, top 30
```

### Query: impact(symbol)

```
engine.impact(symbol_name)
  → db.get_symbol_by_name_fuzzy()    # find target Symbol
  → graph.impact_radius(target.id, max_depth=3)
      → bfs_edges(G.reverse(), target.id, depth_limit=3)
      → score = confidence × decay(depth)
  → fetch Symbol objects via db.get_symbol_by_id()
  → apply generic-name filter
  → db.get_edges_to_by_name() fallback for unresolved callers
  → db.search_vec() for semantic hits
  → fuse where sym_id appears in both structural + semantic
  → sort desc
```

---

## Test Architecture

### `tests/test_graph.py` (new)

All tests use the existing `db` fixture from `conftest.py`. Each test inserts its own minimal symbol + edge set to control graph state precisely.

Key design decisions:
- `test_graph_build_from_resolved_edges`: insert 5 resolved edges and 1 unresolved; verify `len(graph._g.nodes)` and `len(graph._g.edges)` — unresolved must not appear.
- `test_transitive_dependents` / `test_transitive_dependencies`: A→B→C chain via DB; assert depth values on returned tuples.
- `test_impact_radius_decay`: Two nodes at depth 1 and depth 2 with known confidence; assert exact float scores.
- `test_centrality_ranking`: Hub node (3+ inbound edges) must rank above leaf; `nx.pagerank` converges even on small graphs.
- `test_self_loop_handling`: Insert A→A; call `dependents(A)` — must return empty list (A is excluded from its own results by the `seen` set initialized with `{symbol_id}`).

### `tests/test_scoring.py` (new)

Pure unit tests — no fixtures, no DB. All inputs are literal floats passed directly to scoring functions.

Key design decisions:
- `test_evolutionary_zero_redistributes_weight`: Pass `structural=1.0, semantic=1.0, evolutionary=0.0` with default config. Expected `combined` = 1.0 (capped), verifying redistribution produces 100% of structural+semantic weight, not 80%.
- `test_score_capped_at_one`: Inputs designed to exceed 1.0 before capping — verify `min(1.0, ...)` is enforced.
- `test_coupling_score_breakdown_string`: Assert `"structural"` appears in `breakdown()` output (required by existing `test_structural_results_capped`).

### `tests/test_engine.py` (existing — compatibility constraints)

Constructor changes (`graph=None`, `config=None` optional) preserve all existing call sites. No test currently asserts exact `reason` strings except:
- `test_structural_results_capped`: `assert "structural" in c.reason` — preserved by `CouplingScore.breakdown()` format `"structural=X.XX + semantic=Y.YY"`.

No other existing engine tests need modification.

---

## Trade-off Decisions

### DiGraph vs. MultiDiGraph

**Decision:** Use `nx.DiGraph`, not `nx.MultiDiGraph`.

**Reasoning:** MultiDiGraph allows multiple edges per `(source, target)` pair (one per relationship type). It complicates every traversal: `bfs_edges` yields `(u, v, key)` tuples; PageRank semantics differ. The value of tracking both `calls` and `imports` edges between the same pair is marginal — the dominant signal is already captured by the highest-confidence edge. Keeping `DiGraph` preserves simplicity at negligible cost.

### graph=None Optional Parameters

**Decision:** Both `IndexPipeline` and `SearchEngine` accept `graph: SymbolGraph | None = None` and degrade to current behavior when absent.

**Reasoning:** The 85% coverage gate requires all existing tests to pass. Making graph optional means 40+ existing tests need zero changes. The degraded path (one-hop DB traversal with hardcoded 0.7/0.6) is the current production behavior and remains valid as a fallback.

### BFS Implementation: bfs_edges vs. bfs_layers

**Decision:** Use `nx.bfs_edges(G, source, depth_limit=N)` with a manual depth-tracking dict. Do NOT use `bfs_layers`.

**Reasoning:** `bfs_layers` does not support a `depth_limit` parameter — it iterates all reachable layers. Using `bfs_edges` with `depth_limit` gives direct depth control. Depth tracking: maintain `depths: dict[int, int]`; when `bfs_edges` yields `(u, v)`, depth of `v = depths[u] + 1`. Initialize `depths = {source: 0}`. The yielded `(u, v)` pairs are tree edges, so `u` is always already in `depths` before `v` is processed.

### LoomConfig passed to engine (not just weight floats)

**Decision:** Pass `config: LoomConfig` to `SearchEngine` rather than individual weight floats.

**Reasoning:** Forward-compatible. When Phase 6 adds evolutionary signal weights and Phase 7 may add per-language boost factors, the engine already holds the full config reference. The alternative (passing three floats) would require constructor churn at each phase.

### Unresolved caller fallback in impact()

**Decision:** Keep the `get_edges_to_by_name` fallback for unresolved callers even after graph integration.

**Reasoning:** The graph only contains resolved edges. Unresolved callers (target_id=None, target_name matches) represent real structural dependents that the Phase 2 resolver failed to link. Dropping them would regress `test_impact_includes_unresolved_name_matches`. The fallback runs after graph traversal and deduplicates via the `seen` set.

---

## Research Notes

### NetworkX BFS API

| API | depth_limit support | Return type | Use case |
|-----|--------------------|-----------:|---------|
| `bfs_edges(G, source, depth_limit=N)` | Yes — native | `(u, v)` tuples | Primary — depth-limited traversal |
| `bfs_layers(G, sources)` | No — iterates all layers | lists of nodes per layer | Inappropriate without post-filter |
| `bfs_tree(G, source, depth_limit=N)` | Yes | DiGraph | Returns new subgraph, wasteful |
| `bfs_predecessors(G, source, depth_limit=N)` | Yes | `(v, u)` tuples | Reverse traversal alternative |

Decision: `bfs_edges` on `G.reverse(copy=False)` is cleaner than `bfs_predecessors` since it uses the same call convention as forward traversal.

### NetworkX Type Stubs

| Package | Version | Notes |
|---------|---------|-------|
| `networkx` | 3.x | No `py.typed` marker — mypy will complain without stubs |
| `types-networkx` | 3.6.1.20260408 | Full stubs on PyPI, maintained by typeshed community |
| `networkx-stubs` | separate project | Older, less maintained |

Decision: Add `networkx` to `mypy.overrides ignore_missing_imports`. Developer can optionally add `types-networkx` to dev dependencies if IDE completions are wanted. Not added to `pyproject.toml` as a required dev dep to avoid version coupling.

Sources consulted:
- [bfs_layers — NetworkX 3.6.1](https://networkx.org/documentation/stable/reference/algorithms/generated/networkx.algorithms.traversal.breadth_first_search.bfs_layers.html)
- [bfs_edges — NetworkX 3.6.1](https://networkx.org/documentation/stable/reference/algorithms/generated/networkx.algorithms.traversal.breadth_first_search.bfs_edges.html)
- [types-networkx on PyPI](https://pypi.org/project/types-networkx/)
