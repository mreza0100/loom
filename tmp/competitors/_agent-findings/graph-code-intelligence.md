# Graph + Code Intelligence MCP Competitor Teardown

## Executive Verdict

The serious Loom competitors are not generic grep wrappers. The strongest ones build a durable symbol/fact store, resolve at least some dependency/call/reference edges, and expose agent-shaped tools such as impact, callers, refs, neighborhoods, and repo maps.

Top direct threats:

- `kuberstar__qartez-mcp`: closest Loom-shaped competitor. Rust, SQLite graph schema, symbol refs, edges, co-change, FTS, optional embeddings, watcher, PageRank/Leiden/blast-radius, many MCP tools.
- `sdsrss__code-graph-mcp`: Rust, compact local SQLite graph with FTS triggers, tree-sitter parser cache, AST node/search/callgraph/ref tools, and unresolved-call staging.
- `jgravelle__jcodemunch-mcp`: Python but very agent-aware: AST call references, import blast radius, MUNCH compact response format, watcher, runtime signals, and topology/community tooling.
- `SimplyLiz__CodeMCP`: Go, SCIP/LSP/git-first engine with tree-sitter fallback, incremental indexing, callgraph, impact, transitive invalidation, FTS, and multi-repo MCP server.
- `srclight__srclight`: Python, real SQLite/FTS/schema breadth, tree-sitter indexing, Louvain communities, execution flows, and unusually strong MCP usage guidance.
- `abhigyanpatwari__GitNexus`: broad TypeScript semantic graph with per-type and reverse adjacency indexes, SCC-aware finalize, and useful next-step hints in MCP responses.

Strong adjacent but less directly Loom-shaped:

- `bartolli__codanna`: strong Rust symbol indexer with relationships and MCP tools, but search/storage is Tantivy-first rather than graph-first.
- `repowise-dev__repowise`: real graph under a documentation/wiki product. Parser architecture is worth stealing, product direction is not.
- `FreePeak__LeanKG`: real KG with Android/Kotlin/infra depth, adjacent unless Loom wants domain-specific Android graph intelligence.
- `postrv__narsil-mcp`: serious broad code-intel engine, but it tries to be LSP/security/neural/RDF/remote/indexer all at once.
- `CodeGraphContext__CodeGraphContext`: real graph tooling but operationally heavy: external Neo4j/Falkor/Kuzu posture is anti-Loom for local-first MCP.
- `DeusData__codebase-memory-mcp`: technically serious, but high-risk C implementation and algorithmic embedding stack make it more evidence source than product model.

Weak/thin:

- `JudiniLabs__mcp-code-graph`: thin local wrapper over remote CodeGPT API endpoints. It does not perform local parsing, indexing, graph storage, or analysis in the repo. Useful as MCP naming inspiration only. Abstraction theater with an API key, which is still theater.

## Repo Manifest

| Repo | Inferred URL | Classification | Short verdict |
|---|---|---:|---|
| `tmp/competitors/kuberstar__qartez-mcp` | `https://github.com/kuberstar/qartez-mcp.git` | serious competitor | Closest Rust/SQLite graph + MCP threat. |
| `tmp/competitors/abhigyanpatwari__GitNexus` | `https://github.com/abhigyanpatwari/GitNexus.git` | serious competitor | Broad semantic graph and response-guided MCP. |
| `tmp/competitors/DeusData__codebase-memory-mcp` | `https://github.com/DeusData/codebase-memory-mcp.git` | serious competitor | Real C graph/index/semantic stack; risky but substantive. |
| `tmp/competitors/tirth8205__code-review-graph` | `https://github.com/tirth8205/code-review-graph.git` | serious competitor | Python graph store, tree-sitter extraction, Leiden/impact/incremental. |
| `tmp/competitors/CodeGraphContext__CodeGraphContext` | `https://github.com/CodeGraphContext/CodeGraphContext.git` | adjacent tool | External graph DB code finder/context builder. |
| `tmp/competitors/sdsrss__code-graph-mcp` | `https://github.com/sdsrss/code-graph-mcp.git` | serious competitor | Rust local graph MCP with parser cache and unresolved-call queue. |
| `tmp/competitors/JudiniLabs__mcp-code-graph` | `https://github.com/JudiniLabs/mcp-code-graph.git` | thin wrapper | Remote API client, no local code intelligence. |
| `tmp/competitors/bartolli__codanna` | `https://github.com/bartolli/codanna.git` | adjacent tool | Strong Rust symbol indexer with relationship tools. |
| `tmp/competitors/jgravelle__jcodemunch-mcp` | `https://github.com/jgravelle/jcodemunch-mcp.git` | serious competitor | Agent-shaped Python indexer with compact wire format and impact tools. |
| `tmp/competitors/srclight__srclight` | `https://github.com/srclight/srclight.git` | serious competitor | Python tree-sitter + SQLite/FTS + communities + flows. |
| `tmp/competitors/postrv__narsil-mcp` | `https://github.com/postrv/narsil-mcp.git` | adjacent tool | Real Rust code-intel engine with callgraph, but scope is sprawling. |
| `tmp/competitors/repowise-dev__repowise` | `https://github.com/repowise-dev/repowise.git` | adjacent tool | Wiki/doc product with a serious dependency graph under it. |
| `tmp/competitors/FreePeak__LeanKG` | `https://github.com/FreePeak/LeanKG.git` | adjacent tool | Android/Kotlin/infra knowledge graph, real but domain-biased. |
| `tmp/competitors/SimplyLiz__CodeMCP` | `https://github.com/SimplyLiz/CodeMCP.git` | serious competitor | Go SCIP/LSP/tree-sitter engine with incremental impact. |

## Source-Level Findings

### `kuberstar__qartez-mcp` - Serious Competitor

Qartez is the most Loom-shaped repo in this slice. It uses a local SQLite schema for files, symbols, symbol references, directed edges, co-changes, FTS, clusters, type hierarchy, and optional embeddings (`tmp/competitors/kuberstar__qartez-mcp/src/storage/schema.rs:5`, `:118`). The indexer has full and multi-root indexing paths and a two-pass shape that first gathers known paths before resolving relations (`src/index/mod.rs:52`, `:90`). It has a watcher with debounce/PageRank/WAL cadence constants and notify integration (`src/watch.rs:21`, `:171`).

Graph intelligence is explicit: deterministic Louvain plus Leiden refinement is implemented in Rust (`src/graph/leiden.rs:3`, `:67`), and blast radius builds reverse adjacency then BFSes dependents (`src/graph/blast.rs:102`). The MCP surface is broad: calls, refs, deps, impact, semantic, unused, clones, smells, and safe-delete style tools live under `src/server/tools/`.

Steal: graph schema breadth and co-change/community support. Avoid: tool sprawl and refactor-ish tools in a code-intelligence server unless Loom clearly marks them mutating.

### `abhigyanpatwari__GitNexus` - Serious Competitor

GitNexus is a large semantic graph system. Its `KnowledgeGraph` interface supports node/relationship adds, bulk file deletion, queries by ID/type/file, serialization, and metadata (`tmp/competitors/abhigyanpatwari__GitNexus/gitnexus/src/core/graph/types.ts:11`). The concrete graph keeps nodes/relationships plus per-type, reverse adjacency, and per-file indexes (`gitnexus/src/core/graph/graph.ts:11`). Relationship writes update forward and reverse indexes in one path (`graph.ts:48`), and file-level deletion removes affected nodes/relationships incrementally (`graph.ts:91`).

The finalize orchestrator is where it gets serious: it imports SCC-aware finalization and emits scope/definition/qualified-name/module/method dispatch indexes (`gitnexus/src/core/ingestion/finalize-orchestrator.ts:1`, `:92`). The MCP layer exposes tools/resources and appends next-step hints to tool responses, which is unusually agent-aware (`gitnexus/src/mcp/server.ts:1`, `:31`, `:155`).

Steal: per-file deletion indexes and response-level next-step hints. Avoid: making the graph model so broad that every query needs an ontology briefing.

### `DeusData__codebase-memory-mcp` - Serious Competitor

This is real, not polished. The C graph buffer defines node/edge structures, upsert/find/delete-by-qualified-name/file APIs, edge dedupe, vector storage, and SQLite dump/merge for incremental indexing (`tmp/competitors/DeusData__codebase-memory-mcp/src/graph_buffer/graph_buffer.h:23`, `:71`, `:121`, `:151`). The store schema creates project/file-hash/node persistence (`src/store/store.c:214`).

The semantic layer is custom and ambitious: TF-IDF, random indexing, API/type/decorator signatures, and graph diffusion are in the source (`src/semantic/semantic.c:1`). Signal weights are explicit (`semantic.c:40`). The MCP server is a C JSON-RPC server advertising 14 graph tools (`src/mcp/mcp.c:1`, `:120`).

Steal: explicit graph-buffer/dump-merge architecture as a design pattern. Avoid: C memory-safety surface for an MCP server that will ingest untrusted-ish local repos.

### `tirth8205__code-review-graph` - Serious Competitor

This is a focused Python graph tool for review intelligence. The graph module says the model stores file/function/class nodes, imports/calls/inherits edges, confidence tiers, and impact radius (`tmp/competitors/tirth8205__code-review-graph/code_review_graph/graph.py:1`). SQLite schema includes nodes/edges with confidence fields and indexes (`graph.py:31`), and the `GraphStore` enables WAL and keeps a NetworkX cache (`graph.py:142`).

Extraction uses tree-sitter for structural nodes and edges (`parser.py:1`), with a broad extension-to-language map (`parser.py:83`) and many class/function node-type mappings (`parser.py:182`). Community detection uses Leiden through igraph when available and has weighted graph construction and batch cohesion scoring (`communities.py:1`, `:37`, `:155`). Incremental update is git-diff driven and can use process/thread executors (`incremental.py:1`, `:25`).

Steal: confidence tiers on graph facts and review-shaped impact vocabulary. Avoid: maximal language list if extraction depth is uneven.

### `CodeGraphContext__CodeGraphContext` - Adjacent Tool

This is real code graph tooling, but it is graph-DB-first rather than local-index-first. The database singleton is Neo4j-oriented and validates credentials/reachability (`tmp/competitors/CodeGraphContext__CodeGraphContext/src/codegraphcontext/core/database.py:50`, `:85`). `GraphBuilder` supports Neo4j/Falkor/Kuzu and maps extensions to parsers (`src/codegraphcontext/tools/graph_builder.py:26`, `:36`).

It pre-scans imports and dispatches through language-specific parsing branches (`graph_builder.py:120`, `:137`). The watcher performs an initial scan, pre-scan, parse, and link-function-calls/link-inheritance workflow (`src/codegraphcontext/core/watcher.py:22`, `:122`). `CodeFinder` issues Cypher fulltext queries for graph nodes (`src/codegraphcontext/tools/code_finder.py:127`).

Steal: optional graph-DB adapter boundaries if Loom ever exports graphs. Avoid: requiring Neo4j/Falkor/Kuzu for normal agent use. Local-first is Loom's advantage.

### `sdsrss__code-graph-mcp` - Serious Competitor

This Rust repo is compact and serious. SQLite schema uses files, nodes, edges, FTS5, meta, and a `pending_unresolved_calls` table; FTS triggers are declared beside schema setup (`tmp/competitors/sdsrss__code-graph-mcp/src/storage/schema.rs:1`, `:29`). The parser has a thread-local tree-sitter parser cache with parse timeout (`src/parser/treesitter.rs:27`) and extracts functions/classes/methods plus test context (`treesitter.rs:56`, `:154`).

Relation extraction includes generic and Python import extraction with metadata (`src/parser/relations/imports.rs:1`, `:99`). MCP tools are split into advanced, AST node, AST search, callgraph, refs, search, and project-map modules (`src/mcp/server/tools.rs:1`).

Steal: unresolved-call staging as a first-class table. That is a clean answer to incremental/later resolution. Avoid: letting AST-node tools expose too much raw syntax without ranked neighborhoods.

### `JudiniLabs__mcp-code-graph` - Thin Wrapper

The local repo is a client for a remote graph service. Config is basically API key, API URL, and graph ID (`tmp/competitors/JudiniLabs__mcp-code-graph/src/config.ts:1`). The server hard-codes the CodeGPT API base (`src/index.ts:22`) and calls remote endpoints for listing graphs, getting code, direct connections, and semantic search (`index.ts:64`, `:107`, `:186`, `:269`).

There is no local AST extraction, graph storage, incremental indexing, impact model, or evidence construction. For Loom, this is not a code-intelligence competitor. It is a network adapter wearing a graph hat.

Steal: simple tool naming, maybe. Avoid: shipping a "local MCP" that secretly depends on a remote graph API for all intelligence.

### `bartolli__codanna` - Adjacent Tool

Codanna is a serious Rust symbol indexer with MCP support. Parser modules cover language parsers, behavior, and resolution (`tmp/competitors/bartolli__codanna/src/parsing/mod.rs:1`). Persistence is simplified around Tantivy metadata rather than a graph-native schema (`src/storage/persistence.rs:1`, `:69`). Relationships are modeled explicitly as calls, extends, uses, and references, with inverse names (`src/relationship/mod.rs:4`).

The indexing pipeline is high-performance and staged (`src/indexing/pipeline/mod.rs:1`, `:68`). MCP supports find-symbol, calls, callers, impact, symbol search, and semantic search (`src/mcp/mod.rs:1`, `:74`). This is not a wrapper; it is just less graph-first than Loom.

Steal: Rust parser modularity and staged indexing pipeline. Avoid: making search engine persistence the center of gravity if the north-star metric is symbol neighborhoods per token.

### `jgravelle__jcodemunch-mcp` - Serious Competitor

jCodemunch is noisy, but the graph/intelligence core is real. The SQLite backend uses WAL, symbols/files, branch deltas, runtime calls, runtime edges, runtime imports, unmapped runtime facts, redaction logs, runtime columns, and stack events (`tmp/competitors/jgravelle__jcodemunch-mcp/src/jcodemunch_mcp/storage/sqlite_store.py:1`, `:32`, `:100`). `CodeIndex` caches symbol lookup, import-name indexes, and lazy reverse caller indexes over stored `call_references` (`src/jcodemunch_mcp/storage/index_store.py:155`, `:187`, `:212`).

Extraction uses tree-sitter language pack and AST call-node mappings by language (`src/jcodemunch_mcp/parser/extractor.py:1`, `:13`). It collects calls iteratively, attributes them to enclosing symbols, and stores call references (`extractor.py:89`, `:150`, `:180`). The call-graph module prefers stored AST call refs, falls back to import/text heuristics, and can incorporate LSP-resolved edges from metadata (`src/jcodemunch_mcp/tools/_call_graph.py:1`, `:72`, `:121`, `:192`).

Impact/blast-radius is import reverse-adjacency BFS plus symbol-name confirmation and optional call-depth expansion (`src/jcodemunch_mcp/tools/get_blast_radius.py:16`, `:117`, `:223`). Community/topology analysis fuses structural imports, shared symbol references, and git co-churn, then label-propagates plates (`src/jcodemunch_mcp/tools/get_tectonic_map.py:1`, `:41`, `:151`, `:178`). The watcher does initial incremental indexing and then watchfiles-driven reindexing with debounce and locks (`src/jcodemunch_mcp/watcher.py:1`, `:133`, `:165`, `:202`). Its MUNCH response format uses legends/tables/scalars to compress agent payloads (`src/jcodemunch_mcp/encoding/format.py:1`, `:37`).

Steal: compact response encoding ideas, runtime signal tables, and clear confidence/fallback metadata. Avoid: the everything-tool surface. It has every button because someone was afraid of choosing.

### `srclight__srclight` - Serious Competitor

Srclight has real indexing and agent guidance. The indexer is tree-sitter based and incremental by content hash (`tmp/competitors/srclight__srclight/src/srclight/indexer.py:1`, `:120`). The DB schema covers files, symbols, three FTS tables, symbol edges, index state, communities, and flows (`src/srclight/db.py:1`, `:72`). Community analysis uses Louvain and includes flow tracing/impact from entry points (`src/srclight/community.py:1`, `:60`, `:195`).

The MCP server embeds detailed tool-selection guidance and dynamic stats in its instructions (`src/srclight/server.py:26`) and uses FastMCP (`server.py:174`). That is useful: the tool does not merely expose graph data; it coaches agents toward the right retrieval primitive.

Steal: MCP instruction quality and entry-point flow tracing. Avoid: Python graph algorithms on large monorepos unless bounded hard.

### `postrv__narsil-mcp` - Adjacent Tool

Narsil is a broad Rust code-intelligence engine. Parser config uses tree-sitter language configs and supports many languages (`tmp/competitors/postrv__narsil-mcp/src/parser.rs:1`, `:67`). Engine options include git, callgraph, persistence, watch, remote, LSP, neural, and graph features (`src/index.rs:82`), and the engine keeps symbol indexes (`index.rs:137`).

The callgraph module is direct: call graph is called critical for AI/impact, structs model nodes/edges/graph, and build is two-pass over files (`src/callgraph.rs:1`, `:11`, `:96`). Call extraction covers function declarations and call expressions (`callgraph.rs:163`). The MCP server runs JSON-RPC over stdio with tool registry, max request size, and notification handling (`src/mcp.rs:76`, `:133`).

Steal: two-pass callgraph builder and feature flags around expensive subsystems. Avoid: turning Loom into security scanner + neural engine + remote indexer + graph DB + LSP server. That way lies architectural soup.

### `repowise-dev__repowise` - Adjacent Tool

Repowise is a wiki/documentation product with a real graph underneath. The graph builder creates a dependency graph over files and symbols with caches and a NetworkX DiGraph (`tmp/competitors/repowise-dev__repowise/packages/core/src/repowise/core/ingestion/graph.py:1`, `:84`). It adds file/symbol nodes, defines/has-method edges, module anchors, and then resolves imports, inheritance, and calls (`graph.py:122`, `:190`).

Its parser architecture is stronger than many direct competitors: a unified AST parser loads language configs and `.scm` queries instead of hardcoding every language branch (`packages/core/src/repowise/core/ingestion/parser.py:1`, `:73`, `:117`, `:178`). It has dead-code analysis from graph plus git signals (`packages/core/src/repowise/core/analysis/dead_code/analyzer.py:1`, `:93`) and migrations for symbol graph columns, typed edges with confidence, and community metadata (`packages/core/alembic/versions/0015_symbol_graph.py:1`, `:22`; `0016_community_meta.py:1`, `:20`).

Steal: query-file driven parser config. Avoid: burying graph intelligence under LLM-generated wiki flows.

### `FreePeak__LeanKG` - Adjacent Tool

LeanKG is a serious domain knowledge graph, especially for Android/Kotlin/infra. The indexer modules include Android, Hilt, Room, WorkManager, Gradle, Terraform, CI, and config extraction (`tmp/competitors/FreePeak__LeanKG/src/indexer/mod.rs:1`). File discovery covers many source/config types (`src/indexer/mod.rs:64`), language detection is explicit (`mod.rs:128`), and element extraction dispatches through Terraform, CI YAML, config, Gradle, XML, and Android-specific paths (`mod.rs:171`).

Graph modules include cache, clustering, context, layout, query, and traversal (`src/graph/mod.rs:1`). The orchestrator has impact-query and related-relationship paths (`src/orchestrator/mod.rs:113`). MCP modules exist under `src/mcp/mod.rs:1`.

Steal: domain-specific extractors if Loom wants "deep mode" for Android/infra ecosystems. Avoid: encoding ecosystem-specific ontology into Loom core.

### `SimplyLiz__CodeMCP` - Serious Competitor

CodeMCP is one of the more mature serious competitors. The storage schema has versions for incremental indexing, callgraph, transitive invalidation, FTS, and metrics (`tmp/competitors/SimplyLiz__CodeMCP/internal/storage/schema.go:8`). Schema creation includes symbol mappings, modules, dependency edges, caches, ownership, telemetry, incremental tables, callgraph, transitive invalidation, FTS, and metrics (`schema.go:22`).

The SCIP backend models call graph structs and finds callees/callers from SCIP occurrences and function ranges, with lazy caller index building (`internal/backends/scip/callgraph.go:14`, `:54`, `:138`). Impact analysis walks direct and transitive callers and computes risk/blast radius (`internal/impact/analyzer.go:8`, `:67`). Tree-sitter is used as a fallback extractor (`internal/symbols/treesitter.go:1`, `:42`, `:205`). Incremental indexing includes language capability checks, change detection, delta apply, and large-SCIP streaming threshold (`internal/incremental/indexer.go:70`, `:229`). The MCP server supports multi-repo/lazy engine initialization and tool registration (`internal/mcp/server.go:35`, `:73`).

Steal: SCIP/LSP as optional precision layer and transitive invalidation tables. Avoid: requiring SCIP before Loom is useful; fallback-first matters for agent ergonomics.

## Strong Ideas Loom Should Steal

### P0

- Response contracts that include evidence and next-step hints. GitNexus appends next-step guidance at the MCP layer (`gitnexus/src/mcp/server.ts:31`), and Srclight embeds tool-selection guidance in server instructions (`srclight/src/srclight/server.py:26`). Loom should make "what to do next" a first-class response field, not a README wish.
- Compact, bounded graph payloads. jCodemunch's MUNCH format interns path/symbol legends and table rows (`jcodemunch_mcp/encoding/format.py:1`). Loom does not need that exact format, but should have a compact neighborhood response mode with deterministic field order, legends, and truncation metadata.
- Incremental fact invalidation by file/symbol. GitNexus has per-file deletion indexes (`gitnexus/src/core/graph/graph.ts:91`), sdsrss has `pending_unresolved_calls` (`sdsrss/src/storage/schema.rs:29`), and CodeMCP has transitive invalidation schema versions (`SimplyLiz__CodeMCP/internal/storage/schema.go:8`). Loom should avoid whole-index rebuilds when a bounded file delta is enough.
- Confidence/provenance on edges. `code-review-graph` stores confidence and confidence tiers (`code_review_graph/graph.py:31`), Repowise stores typed edges with confidence (`0015_symbol_graph.py:22`), and jCodemunch marks resolution modes (`tools/_call_graph.py:72`). Loom should distinguish AST-resolved, import-inferred, text-heuristic, LSP-resolved, and runtime-observed edges.
- Community detection plus co-change. Qartez has Leiden/Louvain and co-change tables (`qartez-mcp/src/graph/leiden.rs:3`, `src/storage/schema.rs:118`), jCodemunch fuses structural/behavioral/temporal coupling (`get_tectonic_map.py:41`), and Srclight includes Louvain communities (`community.py:60`). Loom should provide "logical neighborhoods" beyond lexical/semantic search.

### P1

- Optional precision adapters: SCIP/LSP/runtime signals. CodeMCP's SCIP call graph and jCodemunch's LSP/runtime tables show how to layer precision without making it mandatory (`SimplyLiz__CodeMCP/internal/backends/scip/callgraph.go:54`, `jcodemunch_mcp/storage/sqlite_store.py:100`).
- Query-file driven parser extraction. Repowise's `.scm` language configs reduce branch soup (`repowise/core/ingestion/parser.py:1`). Loom's tree-sitter support should move toward declarative per-language query files where possible.
- Unresolved edge staging. sdsrss's pending unresolved calls table is a very clean primitive for "index now, resolve later" (`sdsrss/src/storage/schema.rs:29`).
- Review/impact vocabulary. `code-review-graph` and CodeMCP both speak in blast radius, direct/transitive callers, and risk (`code_review_graph/graph.py:1`, `SimplyLiz__CodeMCP/internal/impact/analyzer.go:67`). Loom should make impact tools crisp and bounded.
- Watcher with explicit debounce/lock/status. Qartez and jCodemunch both have real watcher flows (`qartez-mcp/src/watch.rs:21`, `jcodemunch_mcp/watcher.py:133`). Loom should expose watch status as data, not logs.

### P2

- Runtime evidence ingestion. jCodemunch's runtime calls/edges/imports/columns/stack events are overbuilt but interesting (`sqlite_store.py:100`). Loom could eventually accept external runtime facts as another edge source.
- Flow tracing from entry points. Srclight's execution flow tracing is a useful product shape for agents (`srclight/src/srclight/community.py:195`).
- Domain-specific extractor packs. LeanKG's Android/Hilt/Room/Gradle/Terraform extractors show value in ecosystem packs (`FreePeak__LeanKG/src/indexer/mod.rs:1`). Keep it plugin-like, not core.
- Graph export adapters. CodeGraphContext's graph DB support can be useful for export/import, but should not define the default architecture (`CodeGraphContext/src/codegraphcontext/tools/graph_builder.py:26`).

## Weak/No-Op Repos and Why

- `JudiniLabs__mcp-code-graph`: thin wrapper. All meaningful intelligence is remote: config points at CodeGPT API (`src/config.ts:1`), and tools call remote graph endpoints (`src/index.ts:64`, `:107`, `:186`, `:269`). It does little or nothing useful for Loom's local graph/indexing goals.
- `CodeGraphContext__CodeGraphContext`: not no-op, but weak for Loom's north star because normal use assumes an external graph database and Cypher-style queries. Useful as an export story, not as a default agent substrate.
- `repowise-dev__repowise`: not no-op, but product energy goes into generated docs/wiki. The parser and graph builder are valuable; the product wrapper is adjacent.
- `FreePeak__LeanKG`: not no-op, but much of its advantage is Android/infra ontology. That is useful only if Loom deliberately adds domain packs.
- `bartolli__codanna`: not no-op, but more symbol/search-index competitor than graph-neighborhood competitor. Its relationship model is real; its graph storage story is less central.

No other inspected repo was literally empty theater in the graph/code-intelligence slice. A surprising number are overbuilt, which is a different disease.

## What Loom Should Avoid

- Remote-first "MCP" that is just API forwarding. JudiniLabs shows the failure mode plainly.
- External graph DB as a requirement. Neo4j/Falkor/Kuzu are good export targets and bad defaults for local agent use.
- Raw AST dump tools without ranking. Agents need compact neighborhoods and evidence, not syntax confetti.
- Tool sprawl. Qartez, jCodemunch, and Narsil all show how quickly code intelligence turns into a junk drawer of commands. Loom should keep a small stable surface: search, related/neighborhood, refs/calls, impact, reindex/status.
- Silent heuristic edges. If a relationship came from import inference, text match, AST call ref, LSP, or runtime trace, say so.
- Core-domain ontology bloat. LeanKG's Android depth is useful as a pack, not as mandatory schema.
- Python-only in-memory graph algorithms on big repos without hard bounds. Great demos, expensive mornings.
- Storing or logging indexed source in operational logs. Several repos are casual about content-adjacent traces; Loom's privacy posture should stay boring and local. Boring is how you keep secrets.

## Gaps / Open Questions

- Did not execute each competitor's test suite or benchmark index a common repo; this is source-level inspection only.
- Need empirical comparison on the north-star metric: useful symbols discovered per token spent. Candidate benchmark: same repo, same tasks, compare shell grep vs Loom vs Qartez vs sdsrss vs jCodemunch vs CodeMCP.
- Need verify how much of Qartez's broad tool surface is fully wired versus partially exposed.
- Need inspect runtime behavior for jCodemunch's MUNCH format: compact on paper, but agents may pay decoding complexity if the format is not self-explanatory.
- Need test CodeMCP's SCIP fallback path on repos without generated SCIP indexes; if fallback is weak, Loom's out-of-box advantage grows.
- Need evaluate parser correctness by language. Many repos advertise broad language support; the competitive threat is only real where extraction quality is deep.
- Need compare storage footprint and cold/warm query latency across Rust SQLite (`Qartez`, `sdsrss`, Loom), Go SQLite (`CodeMCP`), and Python SQLite/NetworkX (`srclight`, `code-review-graph`, `jCodemunch`).
