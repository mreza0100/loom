# Rust Runtime Contract

Loom's active runtime is the Rust `loom-mcp` binary. Python runtime paths are historical only and must not appear in active benchmark configs, MCP configs, or command manuals.

## MCP Server JSON

Use the Rust binary built from this workspace:

```json
{
  "mcpServers": {
    "loom": {
      "command": "/path/to/loom/target/debug/loom-mcp",
      "args": ["--target", "/path/to/indexed/project"],
      "cwd": "/path/to/loom"
    }
  }
}
```

Release configs may use `target/release/loom-mcp`. Development configs may use `target/debug/loom-mcp` after `cargo build --workspace`.

## Tool Contracts

Loom exposes these MCP tools from `crates/loom-mcp`:

| Tool | Input JSON | Result semantics |
|---|---|---|
| `search` | `{"query": "text", "limit": 10, "kind": "function"}` | Read-only first step. Returns compact `exact_hits` and `beyond_grep` handles, anchors, summaries, reason codes, lexical evidence, and budget metadata. `kind` is optional. |
| `related` | `{"symbol": "name", "file": "path", "kind": "function"}` | Read-only compact symbols structurally, semantically, or evolutionarily coupled to the named symbol. `file` and `kind` are optional filters. |
| `impact` | `{"symbol": "name", "file": "path", "kind": "function"}` | Read-only likely dependents and blast-radius symbols for a change to the named symbol. |
| `neighborhood` | `{"file": "path", "line": 42}` | Read-only anchor symbol at a file/line plus nearby compact coupled handles. |
| `inspect` | `{"handle": "symbol:idx-...:42", "line_budget": 24, "char_budget": 4000, "line_offset": 0}` | Read-only bounded source inspection for one selected symbol or file handle. Returns stale-handle guidance and pagination metadata. |
| `evidence_pack` | `{"query": "text", "budget_tokens": 1200}` | Read-only proof bundle. Orchestrates search buckets, graph/semantic evidence, inspected snippets, citations, coverage, omissions, and missing concepts within budget. |
| `reindex` | `{}` | Full target reindex; mutates only the local `.loom/` index state. |
| `status` | `{}` | Structured index health and runtime metadata. |

Search-family responses are versioned JSON objects, not raw internal arrays. This is an intentional contract break introduced by `search-contract-foundation` and extended by `search-inspect-evidence`.

Common response fields:

| Field | Meaning |
|---|---|
| `contract` | Stable response contract name, for example `loom.search.response`. |
| `version` | Numeric contract version. Current version is `1`. |
| `index_revision` | Hash-derived revision for the indexed facts used to build result handles. |
| `limit` | Effective result limit represented by the response. |
| `truncated` | `true` when additional results were omitted by the limit. |
| `inspect_required` | `true` when handles/snippets are intentionally compact and full source inspection requires an explicit follow-up workflow. |
| `budget` | Structured budget metadata with `unit`, `requested`, `returned`, `omitted`, and `truncated`. |

Compact hit entries expose `handle`, `file_handle`, `rank`, `name`, `kind`, `language`, `anchor`, `summary`, `score`, `reason_codes`, and compact coupled entries. Full symbol context is internal to Rust callers and is not serialized by default. Source text beyond bounded lexical snippets requires `inspect` or `evidence_pack`.

`search` returns `exact_hits` and `beyond_grep` buckets. Exact hits include bounded `lexical_evidence` with snippet, matched text, rank, matched field, reason, match kind, and sanitized query. Beyond-grep hits are semantic or graph candidates that were not duplicate lexical hits. Symbol handles use `symbol:{index_revision}:{symbol_id}` and file handles use `file:{index_revision}:{hex_repo_relative_path}`. Both are stable within the index revision.

`related`, `impact`, and `neighborhood` return named response objects with the common fields above and handle-bearing result entries.

`inspect` resolves one handle into a bounded snippet with `anchor`, `start_line`, `end_line`, `text`, `chars`, `page.next_line_offset`, and stale-handle errors that tell callers to rerun search. It refuses invalid or oversized budgets through MCP validation.

`evidence_pack` provides evidence, not a final natural-language answer. It includes exact matches, grep-missed findings, inspected snippets, file/line citations, coverage checklist, omitted/truncated metadata, and missing concepts. It must not claim whole-file grep equivalence; current exact matching is indexed symbol FTS unless a later pipeline adds bounded full-file scanning.

## Status Fields

`status` returns:

| Field | Meaning |
|---|---|
| `stats` | Store counts from the active `.loom/loom.db`: symbols, edges, embeddings, files, and co-change rows. |
| `graph_nodes`, `graph_edges` | In-memory relationship graph size loaded from the store. |
| `vector_backend` | Active vector store backend, currently `sqlite-vec` or `blob`. |
| `embedder_backend` | Active embedding backend, currently `candle` or `hashing`. |
| `embedder_degraded` | `true` only when the configured backend had to degrade. Hashing fallback is not automatic unless explicitly configured. |
| `embedder_model` | Model identifier for semantic backends, normally `jinaai/jina-embeddings-v2-base-code`; `null` for hashing. |
| `embedder_dimensions` | Vector dimensions, currently `768` by default. |
| `schema_version` | SQLite `PRAGMA user_version` after migrations. Current Rust schema version is `5`. |
| `watcher_active`, `auto_watch` | File-watcher runtime state and configuration. |

## Storage And Schema

The active store path is `<target>/.loom/loom.db`. Target-local config lives at `<target>/.loom/config.toml`. Model files are cached under `~/.loom/models` unless `model_cache_dir` overrides it.

Schema migrations are Rust-owned in `crates/loom-core/src/store/migrations.rs`. The current version is `CURRENT_SCHEMA_VERSION = 5`, with idempotent migrations for symbols, edges, FTS, co-change, vector storage, `index_meta.embedding_fingerprint`, behavior facts, aliases, callsites, and file role cards.

## Scoring Semantics

Loom combines three bounded signals:

| Signal | Semantics |
|---|---|
| Structural | Relationship-specific weight times parser confidence, with depth decay. Calls and inheritance are strongest; imports and co-location are weaker. |
| Semantic | `1.0 - vector_distance`, clamped to `[0.0, 1.0]`. |
| Evolutionary | Weighted blend of normalized co-change frequency and recency. |

The combined coupling score uses `structural_weight = 0.45`, `semantic_weight = 0.35`, and `evolutionary_weight = 0.20` by default. When no evolutionary evidence exists, structural and semantic weights are renormalized so missing git history does not suppress otherwise valid evidence.

## Benchmark Metric Contract

Active benchmark configs must launch Rust `loom-mcp` and must store indexes under `.loom/loom.db`. A benchmark run must record, when available:

- wall time, exit status, model, sandbox, prompt hash, repo commit, and setup failures;
- MCP call count, per-tool latency, request/response chars, and tool error count;
- shell call count, shell output chars, file-read count, and shell-escape cause labels;
- input, output, reasoning, and cached token counts;
- index build time, index size, vector backend, embedder backend/status, schema version, and embedding fingerprint;
- required evidence coverage, exact-hit useful symbols, beyond-grep useful symbols, useful symbols per token, and final answer quality verdict.

Historical Python/Rust comparison artifacts may remain under `tmp/benchmark/previous-benchmarks/` or clearly labeled historical research files. They are not active runtime documentation.
