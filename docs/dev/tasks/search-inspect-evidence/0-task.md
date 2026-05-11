# Pipeline: search-inspect-evidence

Wave: `semantic-proof-gate`

## Tasks

### Task 6 - Internal lexical baseline plus beyond-grep split

Add a Loom-owned exact lexical retrieval stage before semantic/graph expansion and return explicit buckets:

- `exact_hits` for Loom lexical results.
- `beyond_grep` for semantically or structurally relevant symbols/files after excluding exact-hit ids and duplicate file/line spans.

Required behaviors:
- Exact hits preserve file, line, matched text/snippet, and match reason.
- Beyond-grep results include why they survived exclusion.
- Duplicates across buckets are removed deterministically.
- Empty exact results still produce semantic/graph results.
- Do not claim full grep equivalence unless file-text indexing or bounded scanning covers whole files.

### Task 7 - Compact result handles and inspect workflow

Change search-family responses to return compact handles, rankings, file/line anchors, one-line summaries, reason codes, and budget/truncation metadata by default.

Add an `inspect` tool that resolves handles into bounded source snippets only when requested.

Required behaviors:
- Handles are stable within an index revision.
- Stale handles return actionable errors.
- `inspect` supports symbol/file handles, line or character budgets, pagination/refusal for large snippets, and citable file/line anchors.

### Task 8 - MCP tool descriptions, schemas, and containment policy

Rewrite the MCP surface so tool descriptions teach the intended sequence:

- `search` or beyond-grep first.
- `inspect` only selected handles.
- `evidence_pack` before final answer.
- Shell only as last resort.

Required behaviors:
- Descriptions are compact, explicit, and action-oriented.
- Outputs separate machine-readable content from display text where possible.
- Read-only tools advertise read-only behavior where supported.
- `reindex` is explicitly non-read-only.
- Errors are actionable for retry.

### Task 9 - Evidence pack tool for final citable proof

Add `evidence_pack(query, budget_tokens)` after exact buckets, handles, and `inspect` exist.

Required behaviors:
- Orchestrates lexical hits, beyond-grep results, graph neighbors, role cards when useful, and inspected snippets into a compact bundle.
- Includes exact matches, grep-missed findings, source snippets, file/line citations, coverage checklist, omitted/truncated metadata, and missing concepts.
- Obeys caller-provided token/character budget.
- Never reads unrelated large files wholesale.
- Provides evidence, not the final natural-language answer.

## Verification

- Run MCP server tests, search tests, and JSON compatibility tests.
- Exercise tool-level errors and budget paths.

