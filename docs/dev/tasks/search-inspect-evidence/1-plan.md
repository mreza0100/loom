> Author: planner

# Plan - search-inspect-evidence

## Feature Context

This pipeline turns the contract foundation into a proof workflow. Search should first return compact, ranked handles and evidence buckets; source text should only appear through explicit `inspect` or `evidence_pack` calls with bounded budgets.

## Current State

- `search-contract-foundation` already added `exact_hits`, `beyond_grep`, lexical evidence, stable symbol handles, index revision hashes, and versioned response structs.
- `crates/loom-core/src/search/engine.rs` still serializes full `Symbol` payloads in hits, so default responses are not yet compact enough for containment.
- `crates/loom-core/src/models.rs` has reserved inspect and evidence-pack response structs, but they only contain placeholder fields.
- `crates/loom-mcp/src/server.rs` registers `search`, `related`, `impact`, `neighborhood`, `reindex`, and `status`; `inspect` and `evidence_pack` are intentionally not registered yet.
- Tool descriptions already discourage broad grep, but they do not yet teach the final sequence: search, inspect selected handles, then evidence pack before final answers.

## Gaps & Needed Changes

- Add compact hit metadata: rank, handle, file handle, file/line anchor, one-line summary, reason codes, and budget metadata.
- Keep exact/beyond bucket behavior and deterministic de-duplication while making JSON output source-contained by default.
- Add handle parsing for symbol and file handles tied to `index_revision`.
- Add bounded source inspection with stale-handle detection, path containment, line/character budgets, pagination metadata, and citable anchors.
- Add an `evidence_pack(query, budget_tokens)` workflow that combines search buckets, selected snippets, coverage checklist, omitted/truncated metadata, and missing concepts.
- Register `inspect` and `evidence_pack` in MCP with read-only annotations, action-oriented descriptions, and actionable validation errors.
- Update runtime docs, pipeline docs, MCP tests, and core search tests.

## Integration Surface

- Core models: compact anchors, budgets, snippets, inspect/evidence response contracts, and file-handle helpers.
- Store: symbol lookup by handle id already exists; no schema change is required.
- Search engine: compact hit assembly, inspect source reads, evidence pack orchestration, and budget accounting.
- MCP server: request schemas, validation limits, read-only tool metadata, and error mapping.
- Docs: runtime contract, build pipeline reports, and wave report.

## Risks & Dependencies

- Search-family JSON shape changes are intentionally breaking again; tests must assert the new compact contract.
- Source reads must stay contained under `target_dir` and must not silently return large files.
- Stale handle errors depend on the index revision changing when indexed facts change; the existing hash-based revision is the authority.
- Evidence pack is a proof bundle, not a natural-language answer. It should expose enough citable facts without inventing analysis.

## Research Needed

No external library research is needed. The implementation can use existing std file IO, SQLite-backed lookups, current handle conventions, and existing MCP schema annotations.

Analysis complete.
