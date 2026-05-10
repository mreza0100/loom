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

*(updated after each pipeline completes)*
