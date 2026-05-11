# Architecture - relatable-index-signals

## Goals

- Index source-derived behavior facts with file/line spans, optional enclosing symbols, occurrence aggregation, and lexical search.
- Store callsites and imported aliases as normalized rows with argument-role summaries and confidence metadata.
- Generate short deterministic file role cards that can be queried and attached to evidence packs before source snippets.
- Keep all new extraction local, deterministic, bounded, and free of whole-program type inference.

## File Responsibilities

- `crates/loom-core/src/models.rs`: add `BehaviorFact`, `BehaviorFactHit`, `ParsedBehaviorFact`, `ParsedCallsite`, `ParsedAlias`, `Callsite`, `AliasRecord`, `FileRoleCard`, and evidence pack fields for facts/cards.
- `crates/loom-core/src/parsers/signals.rs`: extract behavior facts, callsites, and aliases from source text and manifest/config files.
- `crates/loom-core/src/parsers/parser.rs`: run signal extraction after language adapter parsing, including unsupported manifest/config extensions.
- `crates/loom-core/src/config.rs`: include manifest/config extensions in default indexing.
- `crates/loom-core/src/indexer/pipeline.rs`: aggregate repeated facts, map enclosing symbols to IDs, write signals with each file index, and refresh role cards after resolver passes.
- `crates/loom-core/src/store/migrations.rs`: add schema version 5 with signal tables and fact FTS.
- `crates/loom-core/src/store/mod.rs`: insert/query/search signal rows, remove them on file invalidation, hash them into `index_revision`, and refresh role-card aggregate fields.
- `crates/loom-core/src/search/engine.rs`: promote fact matches into symbol search when possible and add role-card/fact evidence to evidence packs.

## Data Model / API Changes

- `behavior_facts`: `fact_type`, `value`, `file`, `line`, `end_line`, nullable `enclosing_symbol_id`, `occurrence_count`.
- `behavior_facts_fts`: FTS over fact type, value, and file.
- `callsites`: file/span, callee, receiver, unresolved text, nullable resolved target, JSON argument summaries and imported aliases, nullable enclosing symbol, confidence, generic, downweighted.
- `aliases`: file/span, local name, imported name, source module/path, alias kind, nullable enclosing symbol.
- `file_role_cards`: file hash plus JSON arrays for exported symbols, imports, facts, tests, and related files; primary responsibility and centrality remain scalar.
- `SearchEngine::search` uses fact FTS to seed enclosing symbols. `EvidencePackResponse` carries bounded `behavior_facts` and `role_cards`.

## Algorithms

- Behavior fact extraction is a bounded line scanner:
  - skip secret-looking lines/keys;
  - classify env var calls, feature flag calls, config path literals, command calls, package/script manifest entries, error strings, and useful string literals;
  - aggregate repeated facts by type/value/file/enclosing symbol before storing.
- Callsite extraction scans balanced single-line call expressions, ignores declarations/control flow, records argument summaries, and marks generic calls as downweighted.
- Alias extraction normalizes common import/use/using/require forms from supported languages.
- Role-card generation is extractive:
  - primary responsibility comes from top exported symbols or fact-only manifest role;
  - exports come from file symbols;
  - imports come from alias/import edges;
  - behavior facts are compact `type:value` labels;
  - centrality and top related files refresh from stored graph/cochange data after edge resolution.

## Test Plan

- Parser tests for facts, callsites, aliases, and manifest path/script facts.
- Store tests for schema migration, fact FTS, callsite/alias queries, role-card queries, revision hashing, and file removal cleanup.
- Indexer tests for full index, repeated fact aggregation, role-card creation, and incremental invalidation on file hash changes.
- Search tests for fact-backed search hits and evidence packs containing role cards before inspected snippets.
- Workspace gates: targeted tests first, then `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, and `cargo fmt --all -- --check` as time allows.

## Risks

- Regex-like text scanning can over-collect calls. This is acceptable only when the rows are explicitly marked generic/downweighted and bounded.
- Adding manifest/config extensions increases indexed file counts. Tests and docs must reflect that this is intentional.
- Evidence pack additions should stay additive and bounded so existing MCP consumers still receive compact payloads.
