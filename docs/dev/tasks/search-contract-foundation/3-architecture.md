# Architecture - search-contract-foundation

## Goals

- Make lexical search evidence explicit and bounded.
- Establish versioned response contracts for the search-family tools before later visible behavior changes.
- Prove the contract with tiny deterministic fixtures that cover exact hits, beyond-grep results, duplicate removal, empty lexical searches, ordering, reason codes, and file/line anchors.
- Reserve inspect/evidence-pack response contracts without adding the future MCP tools in this pipeline.

## File Responsibilities

- `crates/loom-core/src/models.rs`
  - Owns serializable contract structs: search, related, impact, neighborhood, reserved inspect, and reserved evidence-pack responses.
  - Owns `LexicalEvidence`, `FtsSearchResult`, `SymbolHit`, `CoupledHit`, and stable handle helpers.
- `crates/loom-core/src/store/mod.rs`
  - Adds `search_fts_with_evidence`.
  - Keeps `search_fts` as a compatibility wrapper.
  - Computes an index revision hash from indexed database facts.
  - Hardens query sanitization for quotes, punctuation-heavy identifiers, and empty sanitized queries.
- `crates/loom-core/src/search/engine.rs`
  - Builds `SearchResponse` with `exact_hits` and `beyond_grep`.
  - Deduplicates by symbol id and merges lexical/vector/graph reason codes.
  - Wraps related, impact, and neighborhood results in named response structs.
- `crates/loom-mcp/src/server.rs`
  - Continues to validate input and returns versioned response structs from the engine.
- `crates/loom-core/tests/search.rs`, `crates/loom-core/tests/foundation.rs`, and `crates/loom-mcp/src/server.rs` tests
  - Cover golden search fixtures, FTS evidence, and JSON shape contracts.

## Data Model / API Changes

- No SQLite schema migration is required.
- New top-level response fields:
  - `contract`
  - `version`
  - `index_revision`
  - `limit`
  - `truncated`
  - `inspect_required`
- Stable symbol handles use `symbol:{index_revision}:{symbol_id}` when a database id exists.
- Lexical evidence contains:
  - `snippet`
  - `matched_text`
  - `rank`
  - `field`
  - `reason`
  - `match_kind`
  - `sanitized_query`

## Algorithms

- FTS evidence query uses `bm25` and `snippet` over name, kind, file, and context columns, then selects the first highlighted field in deterministic priority order.
- Search fusion records candidate provenance in a per-symbol map. Lexical and vector duplicates merge into one candidate.
- Graph-only candidates are admitted from direct graph neighbors of exact hits when they were not already lexical/vector candidates.
- Ordering is score descending with stable file, line, name, and handle tie-breakers.
- Buckets are split by lexical evidence presence: lexical candidates go to `exact_hits`; semantic/graph-only candidates go to `beyond_grep`.

## Test Plan

- Store tests:
  - quoted strings
  - punctuation-heavy identifiers
  - field matches
  - empty sanitized queries
  - ranking and stable ordering
- Search tests:
  - exact hit bucket membership
  - beyond-grep bucket membership from semantic/graph signals
  - duplicate removal across FTS/vector
  - empty lexical results still returning beyond-grep candidates
  - reason codes, handles, file/line anchors, and stable ordering
- MCP tests:
  - JSON shape includes contract/version/revision/truncation metadata and named result buckets.
  - `inspect` and `evidence_pack` are not registered tools in this pipeline.

## Risks

- Response shape changes are intentionally breaking. Tests and docs make that explicit before downstream pipelines build inspect/evidence behavior.
- FTS snippets are bounded evidence, not full source inspection. Larger proof bundles remain delegated to the later evidence-pack pipeline.
