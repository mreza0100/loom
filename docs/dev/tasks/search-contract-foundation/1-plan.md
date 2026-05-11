> Author: planner

# Plan - search-contract-foundation

## Feature Context

This pipeline locks the search contract before later waves change visible search behavior. The required foundation is deterministic evidence for lexical hits, explicit exact/beyond-grep buckets, and versioned MCP-facing response shapes with stable handles tied to an index revision.

## Current State

- `crates/loom-core/src/store/mod.rs` owns FTS insertion and `search_fts`, but it returns only `Symbol` rows.
- `crates/loom-core/src/search/engine.rs` fuses FTS and vector results into an unversioned `Vec<SearchResult>`.
- `crates/loom-mcp/src/server.rs` serializes raw engine internals for `search`, `related`, `impact`, and `neighborhood`.
- `inspect` and `evidence_pack` are planned by later pipelines; they do not exist yet and must not be exposed in this pipeline.

## Gaps & Needed Changes

- Add an FTS evidence row that carries bounded snippets, matched text, rank, matched field, reason, sanitized query, and exact-phrase/token-match classification.
- Preserve the existing `search_fts` compatibility path while adding an evidence-returning path for the search engine and tests.
- Add response contract models with `contract`, `version`, `index_revision`, stable result handles, truncation, and `inspect_required` metadata.
- Update the search engine to return `exact_hits` and `beyond_grep` buckets, de-duplicate candidates by symbol id, and keep deterministic ordering.
- Wrap `related`, `impact`, and `neighborhood` in named versioned response structs.
- Add focused golden fixture tests and JSON shape tests.

## Integration Surface

- Core models: response contract structs and lexical evidence types.
- Store: evidence-aware FTS query and deterministic sanitization for quoted and punctuation-heavy queries.
- Search: bucketed response assembly, graph/semantic beyond-grep candidates, stable handles, and compatibility with existing coupling logic.
- MCP: serialize named versioned responses rather than raw vectors.
- Docs: pipeline reports and wave status.

## Risks & Dependencies

- Changing engine return types requires updating existing core and MCP tests.
- FTS5 snippet behavior is tokenizer-dependent, so tests should assert stable contract fields without overfitting long snippets.
- Handle revision should avoid exposing source content while still changing when indexed facts change.
- Later `inspect` and `evidence_pack` pipelines need reserved structs/docs, but this pipeline must not register those tools.

## Research Needed

No external research needed. The implementation can use existing `rusqlite` FTS5 helpers, existing `sha2`, and current Loom store/search/server structure.

Analysis complete.
