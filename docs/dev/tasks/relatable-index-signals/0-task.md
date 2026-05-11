# Pipeline: relatable-index-signals

Wave: `semantic-proof-gate`

## Tasks

### Task 10 - Behavior fact indexing

Extend indexing beyond symbols to first-class behavior facts:

- Environment variables.
- Config JSON/YAML/TOML paths.
- Command names.
- Package/script names.
- Error strings.
- Feature flags.
- Important string literals.

Required behaviors:
- Facts link to file/line spans and nullable enclosing symbols.
- Manifests produce path-like facts.
- Repeated facts aggregate occurrences.
- Facts are searchable lexically and attachable to evidence packs.
- Do not add full data-flow analysis.
- Do not index secrets beyond local private storage already inside `.loom`.

### Task 11 - Callsite and argument-role indexing

Index not only that `A` calls `B`, but where the call occurs and what role the call carries.

Required fields:
- File/line/span.
- Callee.
- Receiver/base object.
- Unresolved target text.
- Resolved target when known.
- Argument literals/summaries.
- Imported aliases.
- Enclosing symbol.
- Confidence.
- Generic/downweighted flags.

Alias records must be normalized instead of hiding inside import edges. No whole-program type inference is required.

### Task 12 - File role cards and repository map summaries

Generate compact deterministic per-file and per-module role cards during indexing.

Required content:
- Primary responsibility.
- Exported symbols.
- Imported dependencies.
- Behavior facts.
- Centrality.
- Tests touching it.
- Top related files.

Role cards must be extractive, short, deterministic, incrementally updated, queryable, invalidated by file hash changes, and eligible for evidence packs before snippets when useful. No LLM summarization dependency.

## Verification

- Add parser/indexer/store/search tests for facts, callsites, aliases, and role cards.
- Verify incremental invalidation on file hash changes.

