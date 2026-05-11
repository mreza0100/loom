# Pipeline: search-contract-foundation

Wave: `semantic-proof-gate`

## Tasks

### Task 3 - Search contract golden fixtures

Add tiny deterministic in-repo fixtures and tests that prove the beyond-grep contract before and during search-output changes.

Required fixture behaviors:
- At least one exact lexical hit in `exact_hits`.
- At least one semantic or graph-only result in `beyond_grep`.
- One duplicate candidate removed deterministically.
- One query with empty lexical results that still returns semantic/graph candidates.
- Tests assert bucket membership, ordering stability, reason codes, file/line anchors, and absence of duplicate file/line spans.

### Task 4 - FTS evidence substrate for exact hits

Extend lexical retrieval so exact-hit candidates carry evidence, not just `Symbol` rows.

Required behaviors:
- Lexical results expose bounded snippet or matched text, rank, field/reason, exact phrase vs token-match classification where possible, deterministic punctuation sanitization, and stable ordering.
- Cover quoted strings, punctuation-heavy identifiers, field matches, empty sanitized queries, and ranking.
- Do not create `evidence_pack` in this pipeline.
- Do not read unrelated large files wholesale.

### Task 5 - Versioned MCP response contracts and handle compatibility

Introduce explicit versioned response structs before changing search-family behavior.

Required behaviors:
- Search, related, impact, neighborhood, inspect, and evidence-pack responses use named structs rather than ad hoc serialized internals.
- Responses include contract/version field, stable handle format tied to index revision, truncation/inspect-required metadata, and JSON shape tests.
- Breaking changes are explicit in tests and docs.

## Verification

- Add focused fixtures and JSON shape tests.
- Run focused search/store/server tests, then workspace gates when feasible.

