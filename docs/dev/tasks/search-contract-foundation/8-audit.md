# Audit - search-contract-foundation

Scope: changed search contract, FTS evidence, store revision, MCP response serialization, and focused tests.

## Findings

No audit findings.

## Checks

- No `unsafe` added.
- No source snippets or indexed source content are logged.
- FTS evidence is bounded by FTS snippet token count and an additional character cap.
- Exact/beyond buckets use structured fields and reason codes rather than free-form serialized internals.
- Handles include the index revision and database symbol id; they do not embed source content.
- `inspect` and `evidence_pack` response structs are reserved, but no mutating or future MCP tools were registered in this pipeline.

## Residual Risk

The response shape is intentionally breaking for search-family tools. The break is covered by tests and documented in `docs/dev/runtime-contract.md`; downstream pipelines must build on the versioned contract rather than reusing the old raw arrays.
