# Wave Report: foundation-rebuild

**Task file:** wave.md (repo root) | **Started:** 2026-05-11
**Total tasks:** 6 phases → 0 via /jc + 3 pipelines | **Waves:** 1 (all sequential)

## Grouping Summary

| Pipeline | Tasks included | Routing |
|----------|---------------|---------|
| `foundation-data-model` | Phase 1 (ID Edges) + Phase 3 (Call Expressions) + Phase 2 (Two-Phase Index) | store, indexer, search |
| `graph-and-scoring` | Phase 4 (Graph) + Phase 5 (Scores) | store, search, server |
| `evolutionary-coupling` | Phase 6 (Git Co-Change) | indexer, store, config |

## Execution Plan

### Wave 1 (sequential — strict dependencies)
- [ ] `foundation-data-model` — ID-based edge model + parser fix + two-phase indexing (3 phases)
- [ ] `graph-and-scoring` — NetworkX graph + real coupling scores (2 phases)
- [ ] `evolutionary-coupling` — Git co-change analysis (1 phase)

## Execution Log

- [x] `foundation-data-model` — **DONE** ✓ (319 tests, 94.89% coverage, commit f658029, docs 9531909)
- [x] `graph-and-scoring` — **DONE** ✓ (440 tests, 94.46% coverage, commit c0b77e0, docs c83cc9b)
- [x] `evolutionary-coupling` — **DONE** ✓ (537 tests, 94.71% coverage, commit 76870d5)

## Final Summary
**Completed:** 2026-05-11 | **Pipelines:** 3 succeeded, 0 failed, 0 deferred

| Pipeline | Tasks | Status | Notes |
|----------|-------|--------|-------|
| foundation-data-model | Phase 1+3+2 | DONE | ID edges, parser fix, two-phase indexing |
| graph-and-scoring | Phase 4+5 | DONE | NetworkX graph, real coupling scores |
| evolutionary-coupling | Phase 6 | DONE | Git co-change analysis, third signal |
