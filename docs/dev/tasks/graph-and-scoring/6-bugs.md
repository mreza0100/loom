> Author: qa

# Bug Report — graph-and-scoring

## Summary

407 tests, 0 failures. Coverage 93.44% (gate: 85%). Ruff clean. Mypy clean.

**QA test file:** `tests/test_qa_graph_and_scoring.py` (58 adversarial tests)

---

## Bugs Found

### BUG-001 — Semantic path in `impact()` does not filter `_GENERIC_CALL_TARGETS`

**Severity:** Medium  
**File:** `src/loom/search/engine.py`, lines 284–302  
**Status:** OPEN

**Description:**
The structural traversal path in `impact()` applies `_GENERIC_CALL_TARGETS` filtering at lines 215 and 260. The semantic vector search path (lines 284–302) does **not** apply this filter. A symbol named `map`, `forEach`, `filter`, etc. can appear in `impact()` results if it has a similar embedding to the target symbol, even when it would be filtered by the structural path.

**Reproduction:**
1. Insert a symbol named `forEach` with the same embedding as the target symbol.
2. No structural edge needed.
3. Call `engine.impact(target)`.
4. `forEach` appears in results via the semantic path.

**Root cause:** Line 287 in `impact()` fetches `sym = self._db.get_symbol_by_id(sym_id)` but does not check `sym.name.split(".")[-1] in _GENERIC_CALL_TARGETS` before appending to `dependents`.

**Fix:** Add the generic name filter in the semantic loop of `impact()`, analogous to the structural path guard.

**Test coverage:** `test_generic_call_target_semantic_path_gap` documents the gap. `test_generic_call_target_filtered_from_structural_impact` verifies the structural path filter works correctly.

---

### BUG-002 — `compute_semantic()` can return values > 1.0 for negative distances

**Severity:** Low  
**File:** `src/loom/search/scoring.py`, line 66  
**Status:** OPEN

**Description:**
`compute_semantic(distance)` is defined as `max(0.0, 1.0 - distance)`. For negative distance inputs (which would represent a miscalibrated vector search), the return value exceeds 1.0:
- `compute_semantic(-1.0)` → `2.0`
- `compute_semantic(-0.5)` → `1.5`

The function correctly clamps the lower bound at 0.0 but has no upper bound enforcement. Since sqlite-vec returns non-negative L2 distances in practice, this is unlikely to trigger in production, but it is a contract violation: all scoring functions are expected to return values in `[0.0, 1.0]`.

**Test coverage:** `test_negative_distance_exceeds_one` documents the actual behavior.

**Fix:** Change to `min(1.0, max(0.0, 1.0 - distance))`.

---

### BUG-003 — `compute_evolutionary()` returns negative values for negative frequency inputs

**Severity:** Low  
**File:** `src/loom/search/scoring.py`, line 79  
**Status:** OPEN

**Description:**
`compute_evolutionary(frequency, max_frequency)` is defined as `min(1.0, frequency / max_frequency)`. Negative frequency inputs (invalid but not guarded) produce negative return values:
- `compute_evolutionary(-5, max_frequency=10)` → `-0.5`

The `max_frequency <= 0` guard is present, but no equivalent guard for `frequency < 0` exists.

**Test coverage:** `test_negative_frequency_returns_negative_score` documents the actual behavior.

**Fix:** Change to `min(1.0, max(0.0, frequency / max_frequency))`.

---

### BUG-004 — `fuse_signals()` has a discontinuity at `evolutionary == 0.0`

**Severity:** Low (behavioral)  
**File:** `src/loom/search/scoring.py`, lines 106–127  
**Status:** OPEN (by design, but warrants documentation)

**Description:**
`fuse_signals()` uses a float equality check `if evolutionary == 0.0` to decide between two fundamentally different formulas: weight redistribution vs. raw weights. A near-zero evolutionary value (e.g., `1e-300`) is mathematically indistinguishable from 0.0 but switches to raw weights mode:

- `fuse_signals(0.8, 0.0, 0.0, config)` → combined ≈ 0.45 (redistribution: `0.8 × 0.5625`)
- `fuse_signals(0.8, 0.0, 1e-300, config)` → combined ≈ 0.36 (raw weights: `0.8 × 0.45`)

The ~0.09 difference is significant at the scoring level. If Phase 6 sources evolutionary signals from external data, floating-point noise near zero could produce inconsistent scores.

**Test coverage:** `test_evolutionary_tiny_nonzero_switches_to_raw_weights` documents the discontinuity.

**Suggested fix:** Use a threshold check (`if evolutionary < EVOLUTIONARY_EPSILON`) rather than strict equality. Alternatively, always use redistribution until evolutionary data is confirmed present and meaningful.

---

### BUG-005 — `centrality()` counts self-loops as real in-degree (semantic issue)

**Severity:** Low  
**File:** `src/loom/store/graph.py`, line 148  
**Status:** OPEN (potential false positive in centrality ranking)

**Description:**
NetworkX counts self-loops in `in_degree_centrality()`. A symbol with only a self-loop (A→A) receives `in_degree=1`, the same as a symbol that is genuinely depended on by one other symbol. In a graph where most symbols have in-degree 0, a self-looping symbol with no real dependents can appear at the top of centrality rankings.

This is a semantic distortion — self-loops represent no real relationship to other code, yet they inflate centrality scores. The existing `test_centrality_ranking` test verifies hubs rank above leaves, but does not test self-loop inflation against real dependencies.

**Test coverage:** `test_centrality_on_graph_with_only_self_loops` verifies it doesn't crash and produces valid scores. The semantic correctness gap is documented.

**Fix:** Pre-process the graph to remove self-loops before computing centrality: `G_no_selfloops = G.copy(); G_no_selfloops.remove_edges_from(nx.selfloop_edges(G_no_selfloops))`.

---

## Compliance Checks

- **BUG-MOCK-VIOLATION:** None. All internal deps (SQLite, NetworkX, scoring functions) are used directly. Only `Embedder` (external I/O) is mocked.
- **BUG-RAW-PRINT:** None. `grep -r "print(" src/loom/` returns empty.
- **BUG-COVERAGE:** None. Total coverage 93.44% ≥ 85% gate.

---

## Test File

**`tests/test_qa_graph_and_scoring.py`** — 58 adversarial tests in 6 classes:

| Class | Count | Focus |
|-------|-------|-------|
| `TestSymbolGraphBoundaries` | 10 | max_depth=0, same-node path, nonexistent nodes, top_n=0, self-loops, negative confidence |
| `TestSymbolGraphTopology` | 9 | diamond graph, long chain cutoff, mutual dependency cycles, build_from_db idempotency, large fan-in performance |
| `TestComputeStructuralBoundaries` | 7 | depth=0, negative depth, negative confidence, NaN, Inf, very large depth, empty relationship |
| `TestComputeSemanticBoundaries` | 3 | negative distance > 1.0, NaN distance, +inf distance |
| `TestComputeEvolutionaryBoundaries` | 4 | negative frequency, zero frequency, max_frequency=0, negative max_frequency |
| `TestFuseSignalsBoundaries` | 8 | near-zero evolutionary discontinuity, zero-weight config, over-unity weights, NaN propagation, negative structural, evolutionary_weight=0 |
| `TestCouplingScoreBreakdownEdgeCases` | 4 | all-zeros, unity formatting, tiny evolutionary in breakdown, negative values |
| `TestEngineGraphAdversarial` | 13 | no dependents, empty graph, unknown symbol, dangling sym_id, score range, sort order, incremental rebuild, structural keyword, generic filter gaps |
