# Wave: foundation-rebuild

**Source:** wave.md (repo root) — Professor-refined 6-phase foundation rebuild plan
**Date:** 2026-05-11
**Goal:** Fix every foundational flaw that limits Loom's ceiling before building anything on top.

## Phases → Pipelines

6 phases grouped into 3 pipelines:

| Pipeline | Phases | Tasks | Routing |
|----------|--------|-------|---------|
| `foundation-data-model` | 1 (ID Edges) + 3 (Call Expressions) + 2 (Two-Phase Index) | ID-based edge model, parser fix, cross-file resolution | store, indexer, search |
| `graph-and-scoring` | 4 (Graph) + 5 (Scores) | NetworkX graph, real coupling scores | store, search, server |
| `evolutionary-coupling` | 6 (Git Co-Change) | Git log analysis, third signal | indexer, store, config |

## Build Order

```
foundation-data-model → graph-and-scoring → evolutionary-coupling
```

All sequential — each pipeline depends on the previous.

## Success Criteria

| Metric | Current | Target |
|--------|---------|--------|
| impact() recall | 8% | >70% |
| F-grade call rate | 11% | <5% |
| Tokens/useful symbol | 376 | <150 |
| Coupling scores | flat 0.6/0.7 | continuous 0.15-1.0 |
| Edge resolution rate | ~30% | >70% |
