> Author: planner

# Plan - relatable-index-signals

## Feature Context

The semantic proof gate needs Loom to index source-derived signals beyond declarations: behavior facts, callsite details, alias records, and compact file role cards. These signals must stay local to `.loom`, avoid whole-program inference, and remain deterministic enough for search and evidence packs.

## Current State

- `crates/loom-core/src/parsers/` extracts symbols and coarse edges for JavaScript/TypeScript, Go, Java, Rust, and C#.
- `crates/loom-core/src/indexer/pipeline.rs` writes symbols, edges, embeddings, file hashes, and cochange rows through `LoomDb::replace_file_index`.
- `crates/loom-core/src/store/` stores symbols, edges, embeddings, index metadata, cochange, and symbol FTS.
- `crates/loom-core/src/search/engine.rs` uses symbol FTS, vectors, and graph neighborhoods, then builds bounded search, inspect, and evidence pack responses.
- Manifest/config files are not first-class indexed inputs today because the default watch extensions are adapter-derived code extensions only.

## Gaps & Needed Changes

- Add first-class models for behavior facts, callsites, aliases, and file role cards.
- Extend parsing/index preparation with deterministic extraction from source text and manifests:
  - environment variables, feature flags, config paths, commands, package/script names, error strings, and important string literals;
  - callsite location, callee, receiver, unresolved target text, argument summaries, imported aliases, enclosing symbol, confidence, and generic/downweighted flags;
  - normalized alias rows instead of only import edges.
- Extend SQLite schema with tables, FTS for facts, invalidation on file replacement/deletion, and revision hashing.
- Make fact matches searchable by promoting enclosing symbols where possible and attach facts/role cards to evidence packs.
- Generate deterministic per-file role cards during indexing and refresh aggregate fields after edge resolution.
- Add focused parser/indexer/store/search tests, including incremental invalidation.

## Integration Surface

- `models.rs`: new serializable records and evidence pack fields.
- `parsers/`: parse result additions plus text-based signal extraction.
- `config.rs` and `indexer/path.rs`: include manifest/config extensions for fact-only indexing.
- `indexer/pipeline.rs`: carry parsed signals through file replacement and refresh role cards after resolution.
- `store/migrations.rs` and `store/mod.rs`: schema version bump, signal insert/query/search, cleanup, and role card refresh.
- `search/engine.rs`: use behavior fact FTS as lexical evidence and include behavior facts/role cards in evidence packs.
- Tests under `crates/loom-core/tests/`.

## Risks & Dependencies

- The worktree already contains completed pipeline changes; edits must be additive and preserve current modified files.
- Search response contracts are already versioned. New evidence pack fields are additive, while symbol hit fields should remain compatible.
- Source extraction must not log or expose secrets. Values on secret-looking lines/keys should be skipped.
- Broad callsite extraction can over-index declarations or control-flow forms; confidence and generic/downweighted flags should make this explicit.
- Manifest extension indexing changes default file discovery and related tests must be updated deliberately.

## Research Needed

No new external library is required. The implementation can use existing tree-sitter parses for symbols/edges and deterministic string scanning for additional source facts, keeping compile cost stable.

Analysis complete.
