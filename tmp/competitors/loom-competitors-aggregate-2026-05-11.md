# Loom Competitors Aggregate Report - 2026-05-11

## Prompt And Refined Goal

Original user request:

> do an RR and find ALL Loom competitors. Then clone each of them (all) into tmp/competitors And spawn an agent to research and steal the good ideas and implementations they have, write the aggrigated report in a file

User correction:

> octocode for example is a strong one

User process correction:

> Did you actually spawned parallel agents on each or one on a bunch to do the scan!? that's gonna return much better result!

Refined goal:

Find public and source-available competitors or adjacent systems for Loom's local, agent-facing code intelligence niche: semantic code search, code graph, MCP retrieval, codebase memory, LSP navigation, static analysis/CPG, vector retrieval, and Sourcegraph/Zoekt-lineage exact search. Clone all relevant public repos under `tmp/competitors`, inspect source in parallel slices, classify serious competitors versus adjacent/thin/no-op projects, and extract concrete implementation ideas Loom should adapt.

## Execution Record

This final report supersedes the earlier single-worker rough pass. The final source teardown was done by six parallel workers over disjoint slices:

| Slice | Findings file |
|---|---|
| Octocode | `tmp/competitors/_agent-findings/octocode.md` |
| Graph/code intelligence MCP | `tmp/competitors/_agent-findings/graph-code-intelligence.md` |
| Vector/hybrid search | `tmp/competitors/_agent-findings/vector-hybrid-search.md` |
| LSP/navigation and agent workflow | `tmp/competitors/_agent-findings/lsp-agent-workflow.md` |
| Static analysis, CPG, Tree-sitter | `tmp/competitors/_agent-findings/static-analysis-tree-sitter.md` |
| Canonical code search and Sourcegraph lineage | `tmp/competitors/_agent-findings/canonical-code-search.md` |

Clone result: 72 public repos cloned under `tmp/competitors`. Failed/private/proprietary references were recorded in the earlier rough manifest and are treated as market references only: current Sourcegraph core/Cody, GitHub Copilot, Cursor, JetBrains AI, Windsurf/Codeium, Augment Code, and missing/dead GitHub URLs.

Inspection mode: source-level review only. No competitor tests or benchmarks were run.

## Loom Baseline

Loom is a Rust MCP server for local code intelligence:

- Tree-sitter parsing into symbols and edges.
- Local embeddings with Candle or configured fallback.
- SQLite store in `.loom/` using `rusqlite`, `sqlite-vec`, and blob fallback.
- Graph intelligence via `petgraph`.
- MCP tools: `search`, `related`, `impact`, `neighborhood`, `reindex`, `status`.
- North-star metric: useful symbols discovered per token spent.
- Non-negotiables: local-only privacy, no indexed source logs, structured bounded payloads, and read-only MCP tools unless mutation is explicit.

## Clone Inventory

```text
Aider-AI__aider
BeaconBay__ck
BumpyClock__lsp-mcp
CSCSoftware__AiDex
CodeGraphContext__CodeGraphContext
DEFENSE-SEU__Code2MCP
DEFENSE-SEU__MCP-Github-Agent
DeDeveloper23__codebase-mcp
DeusData__codebase-memory-mcp
FarhanAliRaza__claude-context-local
FreePeak__LeanKG
HarshalRathore__code-intel-mcp
Jakedismo__codegraph-rust
JudiniLabs__mcp-code-graph
MinishLab__semble
Muvon__octocode
PatrickSys__codebase-context
Ryandonofrio3__osgrep
ShiftLeftSecurity__codepropertygraph
SimplyLiz__CodeMCP
ViperJuice__Code-Index-MCP
Wildcard-Official__deepcontext-mcp
abhigyanpatwari__GitNexus
aimasteracc__tree-sitter-analyzer
aj47__auggie-context-mcp
bartolli__codanna
bgauryy__octocode-mcp
bobmatnyc__mcp-vector-search
colbymchenry__CodeGraph
ctoth__mcp_server_code_extractor
dudufcb1__codebase-index-cli
dudufcb1__semantic-search
edelauna__github-semantic-search-mcp
elastic__semantic-code-search-indexer
elastic__semantic-code-search-mcp-server
er77__code-graph-rag-mcp
glommer__codemogger
google__zoekt
jghiringhelli__codeseeker
jgravelle__jcodemunch-mcp
joernio__joern
johnhuang316__code-index-mcp
ktnyt__cclsp
kuberstar__qartez-mcp
lekssays__codebadger
livegrep__livegrep
m1rl0k__Context-Engine
mhalder__qdrant-mcp-server
mixedbread-ai__mgrep
nendotools__tree-sitter-mcp
omar-haris__smart-coding-mcp
oracle__opengrok
oraios__serena
pdavis68__RepoMapper
postrv__narsil-mcp
repowise-dev__repowise
sdsrss__code-graph-mcp
shivasurya__code-pathfinder
sourcebot-dev__sourcebot
sourcegraph__cody-public-snapshot
sourcegraph__zoekt
srclight__srclight
st3v3nmw__sourcerer-mcp
steiner385__qdrant-mcp-server
tecnomanu__pampa
tirth8205__code-review-graph
vrppaul__semantic-code-mcp
vymalo__code-graph
wrale__mcp-server-tree-sitter
yichuan-w__LEANN
yoanbernabeu__grepai
zilliztech__claude-context
```

## Verdict

The serious competitors are not "semantic search" labels. They are systems with one or more of these properties: exact lexical baseline, AST/symbol extraction, durable local index, relationship graph, edge provenance, hybrid ranking, bounded evidence payloads, incremental invalidation, and agent workflow rails.

Octocode is a real competitor, especially `Muvon/octocode`. The strongest direct technical threats are `Muvon/octocode`, `kuberstar/qartez-mcp`, `colbymchenry/CodeGraph`, `sdsrss/code-graph-mcp`, `SimplyLiz/CodeMCP`, `MinishLab/semble`, `BeaconBay/ck`, `zilliztech/claude-context`, `sourcegraph/zoekt`, `livegrep`, `sourcebot`, `Serena`, and `BumpyClock/lsp-mcp`.

Several cloned repos are theater or thin wrappers. The important result is not 72 "competitors"; it is a smaller set of serious systems plus a graveyard of mistakes Loom should avoid.

## Serious Direct Or Near-Direct Competitors

| Competitor | Why it matters |
|---|---|
| `Muvon/octocode` | Rust, tree-sitter, LanceDB, vector plus FTS, RRF, rerank, GraphRAG, LSP, watch, branch deltas, MCP, and a retrieval benchmark suite. Closest full-stack overlap. |
| `kuberstar/qartez-mcp` | Rust/SQLite graph schema with symbols, refs, edges, co-change, FTS, optional embeddings, watcher, PageRank/Leiden/blast radius, and many MCP tools. |
| `colbymchenry/CodeGraph` | Compact SQLite graph, unresolved references, confidence/provenance, framework-aware resolution, bounded MCP tools, watcher sync. Study this hard. |
| `sdsrss/code-graph-mcp` | Compact Rust graph MCP with SQLite/FTS, pending unresolved calls, tree-sitter parser cache, AST/callgraph/ref tools. |
| `SimplyLiz/CodeMCP` | Go engine with SCIP/LSP/tree-sitter, incremental indexing, callgraph, impact, transitive invalidation, FTS, and multi-repo MCP. |
| `jgravelle/jcodemunch-mcp` | Agent-aware graph/index with compact MUNCH output, import blast radius, runtime signals, git co-churn, watcher, topology/community tools. |
| `srclight/srclight` | Tree-sitter plus SQLite/FTS, communities, flows, and strong MCP usage guidance. |
| `tirth8205/code-review-graph` | Tree-sitter graph store, confidence tiers, Leiden communities, incremental review-oriented impact. |
| `MinishLab/semble` | Clean hybrid baseline: AST chunking, dense search, BM25, RRF, symbol/path boosts, and MCP instructions that tell agents to avoid grep/read. |
| `BeaconBay/ck` | Rust local search engine with Tantivy lexical search, semantic sidecars, hybrid RRF, chunk hash reuse, MCP search tools. |
| `zilliztech/claude-context` | Mature vector/hybrid MCP with AST chunking, Milvus dense+sparse search, BM25 functions, locks, periodic sync, and trigger watching. |
| Elastic semantic code search pair | Strong chunk/location split, queue semantics, semantic search, symbol mapping, file reconstruction from indexed chunks. Heavy, but well-shaped. |
| `bobmatnyc/mcp-vector-search` | LanceDB plus BM25, MMR, graph-ish strategies, rich payload schema, and hybrid handler with component scores. Overgrown but serious. |
| `mhalder/qdrant-mcp-server` | Qdrant dense plus sparse prefetch/RRF, tree-sitter chunking, secret skipping, rich payloads. |
| `oraios/serena` | Best symbolic-first agent workflow: memories, onboarding, hooks, staged symbolic retrieval, LSP tools, and broad-read nudges. |
| `BumpyClock/lsp-mcp` | Best LSP/navigation competitor: definitions, references, implementations, call hierarchy, diagnostics, codemap, pagination, candidate disambiguation. |
| `sourcegraph/zoekt` | Exact-search baseline: trigram indexes, regex query planning, shard eval, ctags symbols, ranking, branch masks. |
| `livegrep/livegrep` | Exact regex baseline: suffix arrays, RE2 planning, hard timeouts, match caps, symbol-looking query fast path. |
| `sourcebot` | Sourcegraph-lineage packaging around Zoekt: repo sync, permissions, streaming search, MCP grep/read/tree/symbol tools, agent surfaces. |
| `joern` and CPG schema | Heavy CPG/SAST baseline. Not Loom-shaped, but important for typed edge vocabulary, overlays, dataflow concepts, and query model. |
| `shivasurya/code-pathfinder` | SAST-adjacent with tree-sitter extraction, call graphs, CFG, variable dependency graph, interprocedural taint, MCP tools. |

## Strong Adjacent Systems

| Competitor | Useful idea |
|---|---|
| `bgauryy/octocode-mcp` | Not a local indexer, but excellent MCP UX: provider tools, local `rg`/find/read, LSP, clone cache, path validation, pagination, token hints, security package. |
| `Aider` and `RepoMapper` | Repo-map PageRank over tree-sitter def/ref tags under a token budget. This is still one of the cleanest compact context techniques. |
| `PatrickSys/codebase-context` | Compact/full output modes, edit preflight, search quality, evidence locks, memories, pattern summaries, and next hops. |
| `CSCSoftware/AiDex` | Agent rails: prefer signatures over full files, semantic/exact parallel search, CLAUDE.md setup block, privacy switch for code-to-LLM. |
| `bartolli/codanna` | Rust symbol indexer with parser modularity, relationships, staged indexing, Tantivy search, MCP tools. |
| `postrv/narsil-mcp` | Broad Rust code-intel engine with two-pass callgraph and feature flags, but too much scope. |
| `repowise` | Strong query-file parser config and graph builder under a wiki/doc product. |
| `FreePeak/LeanKG` | Domain extractor packs for Android/Kotlin/infra. Useful if Loom grows optional domain packs. |
| `LEANN` | Retrieval infrastructure: compact/pruned vector indexes, mutable indexes, offset maps. Not code-agent shaped. |
| `tecnomanu/pampa` | RRF, BM25, SQLite, Merkle state, watch, encrypted chunks, optional rerank. Small but substantive. |
| `m1rl0k/Context-Engine` | Strong result contract with `why`, component scores, snippets, doc IDs, tags, relations. Too sprawling to copy whole. |
| `glommer/codemogger` | Honest tiny API: index, search, reindex; tree-sitter chunks; hybrid search. |
| `sourcegraph/cody-public-snapshot` | Context assembly: local dirty-file overlay, stale indexed-context filtering, context de-duplication by path/range/content. |

## Thin, Weak, Or No-Op Theater

| Repo | Why it is weak |
|---|---|
| `JudiniLabs/mcp-code-graph` | Local repo is a remote CodeGPT API client. No local parsing, graph storage, incremental indexing, or evidence construction. |
| `mixedbread-ai/mgrep` MCP side | CLI is a cloud client; MCP watcher returns `tools: []` and tool calls are not implemented. |
| `Ryandonofrio3/osgrep` MCP side | Underlying hybrid search is interesting, but MCP mode exposes zero tools and returns not implemented. |
| `DeDeveloper23/codebase-mcp` | Repomix wrapper that dumps whole repos. It packages the broad-read behavior Loom exists to replace. |
| `DEFENSE-SEU/MCP-Github-Agent` | Clone is only a move notice to Code2MCP. No implementation. |
| `DEFENSE-SEU/Code2MCP` | LLM-generated MCP wrapper pipeline, not a search/index engine. |
| `aj47/auggie-context-mcp` | Thin CLI wrapper around Auggie/Augment. No local implementation to inspect. |
| `steiner385/qdrant-mcp-server` | One point per file, OpenAI-only, no hybrid, deletion does not remove Qdrant points, docs list core features as future work. |
| `jghiringhelli/codeseeker` | Has real fragments, but visible CLI path indexes only first 50 changed files, chunks fixed windows, and searches text in embedded mode. |
| `ctoth/mcp_server_code_extractor` | Exact extractor, not graph/search competitor. Re-parses and has narrow call queries. |
| `nendotools/tree-sitter-mcp` | Useful AST search wrapper, but no resolved references, calls, dataflow, or durable graph. |
| `aimasteracc/tree-sitter-analyzer` | Good parser wrapper, but impact is ripgrep and edges are lightweight regex tuples. |
| `google/zoekt` | Historical pointer in this checkout; active source is `sourcegraph/zoekt`. |

## P0 Ideas Loom Should Steal

### 1. Exact Search Baseline Inside Loom

Loom needs an internal exact lexical stage, not just semantic vectors. The baseline should learn from Zoekt/livegrep without becoming them:

- Code-aware tokenization: preserve `foo.bar`, `foo-bar`, path fragments, snake/camel splits, symbols.
- Exact hits separated from beyond-grep results.
- Regex/path/language/symbol filters with explicit limits.
- Component scores and reason codes.
- Truncation proof: `exhaustive=false`, omitted counts, continuation handles.

Relevant sources: `sourcegraph__zoekt/doc/design.md`, `sourcegraph__zoekt/index/eval.go`, `livegrep__livegrep/src/query_planner.cc`, `bobmatnyc__mcp-vector-search/core/bm25_backend.py`, `MinishLab__semble/src/semble/search.py`.

### 2. Hybrid Retrieval As Dense + Lexical + Graph, Not Vector Theater

Real competitors use RRF or equivalent fusion. Loom should rank from:

- `exact_hits`: lexical/token/path/symbol matches.
- `semantic_candidates`: local embedding results.
- `graph_candidates`: callers/callees/imports/refs/co-change/communities.
- `lsp_candidates`: optional precise refs/defs/call hierarchy when available.

Return component scores and "why this result" fields. Do not hide the evidence under a single mystical score.

### 3. Edge Provenance, Confidence, And Unresolved References

This is the biggest graph lesson:

- Store unresolved refs during parsing.
- Resolve later with a mode: `exact`, `import`, `qualified_name`, `framework`, `fuzzy`, `instance_method`, `lsp`, `text_heuristic`.
- Attach confidence and provenance to every edge.
- Keep unresolved refs visible in status/debug output.

Best sources: `colbymchenry__CodeGraph/src/resolution/types.ts`, `colbymchenry__CodeGraph/src/db/schema.sql`, `sdsrss__code-graph-mcp/src/storage/schema.rs`, `tirth8205__code-review-graph/code_review_graph/graph.py`.

### 4. Evidence-Rich MCP Response Contracts

Every search-family response should carry enough proof that an agent does not reflexively shell out:

- file path, line/span, symbol id, snippet/signature, edge reasons.
- index revision/fingerprint.
- matched query mode and backend.
- component scores.
- `next_tool_suggestions`.
- continuation handles.
- `inspect_required` when source is omitted.

Borrow from Octocode MCP pagination/security, BumpyClock markdown shaping, Sourcebot grep metadata, Elastic read-from-chunks, and Context-Engine's `why`/score contract.

### 5. Bounded Output Modes

Default response should be compact. Then allow expansion:

- `signatures`
- `summary`
- `partial`
- `full`
- `evidence_pack`

Muvon Octocode's `signatures/partial/full`, Codebase Context's compact/full modes, Serena's shortened-result factories, and BumpyClock's summary/detail modes all point the same way.

### 6. Incremental Indexing With Real Invalidation

Do not merely append new chunks. Loom should invalidate by file/symbol and prevent stale vectors/edges:

- content hash per chunk and file.
- model/tokenizer/index version stamps.
- vector dimension guards.
- stale edge deletion and unresolved-ref re-resolution.
- branch/working-tree overlay support.
- deletion-correct vector storage.

Sources: Muvon dimension guard, Elastic WAL queue, GitNexus per-file deletion, CodeMCP transitive invalidation, Qartez watcher, jCodemunch watcher.

### 7. Local-Only Privacy And MCP Security Hygiene

Several competitors default to cloud embeddings or log query/source snippets. Loom should make privacy a weapon:

- remote embeddings/rerankers opt-in only.
- no indexed source in logs.
- path realpath/symlink validation for content tools.
- secret redaction for any content-like output.
- command timeouts and safe command builders if shell-adjacent tools exist.
- no mutating MCP tools in search surfaces.

Octocode MCP's security package is worth copying. Muvon's remote default and mutating structural search are worth avoiding.

### 8. Benchmark Useful Symbols Per Token

Copy Muvon's benchmark seriousness, not just its feature list:

- pinned tasks and ground truth spans.
- exact-hit usefulness, beyond-grep usefulness, graph usefulness.
- Hit@k, MRR, NDCG, Recall, and line-overlap scoring.
- agent-level metrics: shell calls, shell output chars, token use, final evidence coverage.
- head-to-head: grep/no-MCP, Zoekt/exact baseline, Octocode, Semble, Loom.

## P1 Ideas

- Add optional LSP precision layer for definitions, references, implementations, and call hierarchy. Label results as `lsp`, `tree_sitter`, `heuristic`, or `mixed`.
- Add repo-map/neighborhood under a hard token budget: PageRank over def/ref/import/call edges plus mentioned-file/symbol boosts.
- Add community/co-change neighborhoods: Leiden/Louvain and git co-change as separate reasons, not opaque clustering.
- Add raw Tree-sitter query/debug tool for parser diagnostics, bounded and expert-facing.
- Add call hierarchy and impact as first-class read-only tools, not side effects of search.
- Add low-confidence rescue and abstention: say when the index is stale, incomplete, or low-confidence.
- Add tool presets or schema modes to avoid a giant MCP menu.

## P2 Ideas

- Optional security/SAST pack with source/sink/sanitizer profiles. Do not ship Joern-lite in core.
- Optional domain packs for Android, Terraform, CI, SQL, etc.
- Optional graph export to Graphviz/Neo4j/Kuzu. Do not require a graph DB.
- Runtime evidence ingestion as a separate edge source.
- Remote repo clone/cache workflow only after local retrieval is excellent.

## Concrete Loom Implementation Implications

### Store And Schema

- Add tables or columns for edge provenance, confidence, unresolved refs, source spans, and index revision.
- Add tokenizer/model/schema version stamps for lexical and vector indexes.
- Add deletion-correct incremental indexes keyed by file and symbol.
- Consider a chunk/location split if Loom stores content snippets: chunk identity separate from file occurrence.

### Parser And Graph

- Move toward declarative tree-sitter query files where possible.
- Keep supported edge vocabulary small: `contains`, `defines`, `imports`, `calls`, `references`, `implements`, `extends`, `instantiates`, `co_changes`.
- Resolve references in a second pass and store unresolved leftovers.
- Add co-change/community as graph overlays, not replacements for exact edges.

### Search

- Implement lexical exact stage before semantic/graph expansion.
- Use RRF or another transparent fusion strategy.
- Adapt weight for symbol-like queries toward lexical/symbol matches.
- Deduplicate by symbol id and file/span.
- Return exact and beyond-grep buckets separately.

### MCP Surface

- Keep the core surface small, but make responses smarter:
  - `search`
  - `related`
  - `impact`
  - `neighborhood`
  - future `inspect`
  - future `evidence_pack`
- Add `detail`/`budget`/`cursor` parameters.
- Add `next_tool_suggestions`.
- Add `read_only` annotations and make mutating tools impossible to confuse with retrieval.

### Benchmarks

Add a competitor gate under `tmp/benchmark`:

- Corepack task: grep/no-MCP vs Loom exact/beyond vs Octocode/Semble where practical.
- Exact-search task: Loom lexical baseline vs Zoekt/livegrep.
- Graph task: Loom vs CodeGraph/Qartez style result coverage.
- Agent containment task: measure shell calls and useful symbols per token.

## What Loom Should Avoid

- Whole-repo dump tools.
- Remote API forwarding wearing an MCP costume.
- Cloud embeddings/reranking by default.
- Unbounded AST dumps.
- Silent watcher/indexer failure.
- Silent stale vectors after deletes or model changes.
- Treating line-proximity, regex imports, or text matches as true references.
- External graph DBs as the default local path.
- Tool sprawl before the retrieval contract is sharp.
- Mutating rewrite/refactor tools inside read-oriented code intelligence.

## Open Questions

- Should Loom implement a real trigram index, a Tantivy/BM25 exact index, or a smaller code-aware lexical stage first?
- What is the minimal edge schema that works across JavaScript, TypeScript, Go, Java, Rust, and C# without becoming CPG cosplay?
- Should LSP be an optional precision adapter or a first-class required layer?
- How should branch/working-tree overlays interact with `.loom/` index revisions?
- Should Loom store snippets/content in SQLite, or reconstruct evidence by path/span at inspect time?
- Which competitors should be included in an executable benchmark gate: Octocode, Semble, Zoekt, CodeGraph/Qartez?
- How much community detection/co-change is useful before it becomes pretty graph noise?

## Source Trail

Primary aggregate inputs:

- `tmp/competitors/_agent-findings/octocode.md`
- `tmp/competitors/_agent-findings/graph-code-intelligence.md`
- `tmp/competitors/_agent-findings/vector-hybrid-search.md`
- `tmp/competitors/_agent-findings/lsp-agent-workflow.md`
- `tmp/competitors/_agent-findings/static-analysis-tree-sitter.md`
- `tmp/competitors/_agent-findings/canonical-code-search.md`

Key source refs inside clones:

- `tmp/competitors/Muvon__octocode/src/store/mod.rs`
- `tmp/competitors/Muvon__octocode/src/indexer/search.rs`
- `tmp/competitors/Muvon__octocode/src/indexer/graphrag/relationships.rs`
- `tmp/competitors/bgauryy__octocode-mcp/packages/octocode-security/src/pathValidator.ts`
- `tmp/competitors/bgauryy__octocode-mcp/packages/octocode-mcp/src/tools/local_fetch_content/fetchContent.ts`
- `tmp/competitors/colbymchenry__CodeGraph/src/db/schema.sql`
- `tmp/competitors/colbymchenry__CodeGraph/src/resolution/types.ts`
- `tmp/competitors/kuberstar__qartez-mcp/src/storage/schema.rs`
- `tmp/competitors/sourcegraph__zoekt/doc/design.md`
- `tmp/competitors/sourcegraph__zoekt/index/eval.go`
- `tmp/competitors/livegrep__livegrep/src/query_planner.cc`
- `tmp/competitors/oraios__serena/src/serena/resources/config/prompt_templates/system_prompt.yml`
- `tmp/competitors/BumpyClock__lsp-mcp/src/mcp/references.rs`
- `tmp/competitors/MinishLab__semble/src/semble/search.py`
- `tmp/competitors/elastic__semantic-code-search-mcp-server/src/mcp_server/server.ts`

