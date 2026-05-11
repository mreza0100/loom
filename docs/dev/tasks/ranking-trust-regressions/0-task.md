# Pipeline: ranking-trust-regressions

Wave: `semantic-proof-gate`

## Tasks

### Task 14 - Payload budgets and anti-rabbit-hole caps

Enforce strict per-tool output budgets, default result limits, coupled-result caps, graph expansion diversity caps, pagination, and capping metadata now that handles and `inspect` exist.

Required metadata:
- `truncated`.
- `next_handle`.
- `inspect_required`.
- Omitted counts.

Broad queries return summaries and handles, not huge snippets. Advanced detail access remains available through explicit inspection.

### Task 15 - Staged hybrid ranking with query intent routing

Replace single-pass RRF behavior with a staged deterministic pipeline:

1. Classify query intent.
2. Retrieve lexical/fact/vector/graph/role-card candidates.
3. Rerank with intent-aware weights.
4. Cap by evidence diversity and budget.

Required behaviors:
- Exact symbol/string queries favor lexical and facts.
- Conceptual queries favor vectors and role cards.
- Impact-analysis questions favor dependents, calls, and tests.
- Tie-breaks prefer central files and high-confidence edges.
- Diversity prevents ten variants of one file.
- No learned reranker required if deterministic heuristics are benchmarked.

### Task 16 - Trust signals and coverage accounting

Add explicit coverage metadata to search and evidence outputs after evidence packs exist.

Required fields:
- Matched concepts.
- Missing concepts.
- Exact-vs-inferred distinction.
- Confidence.
- Inspection-needed state.
- Stop-condition signal based on evidence coverage.

### Task 17 - Benchmark-gated ranking and containment regression suite

Add focused regression tests that prove ranking, budgets, evidence packs, and stop conditions improve north-star behavior before running the final Corepack gate.

Required test coverage:
- Expected evidence sets.
- Exact-hit usefulness.
- Beyond-grep usefulness.
- Shell-escape causes.
- Payload budget assertions.
- Final-answer evidence coverage.
- Failures identify whether the gap is retrieval, evidence, confidence, or model habit.

## Verification

- Run ranking/search/evidence tests.
- Run benchmark regression suite with deterministic fixtures.
- Run workspace gates when feasible.

