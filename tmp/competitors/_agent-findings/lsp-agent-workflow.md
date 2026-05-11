# LSP / Navigation + Agent Workflow Competitor Teardown

## Executive Verdict

The strongest direct threats to Loom are **Serena** and **BumpyClock/lsp-mcp**, but for different reasons.

Serena is the best agent-workflow competitor: it weaponizes symbolic tools, memories, onboarding, tool descriptions, and hook-based nudges to keep agents out of broad file reads. BumpyClock/lsp-mcp is the best protocol competitor: it exposes definitions, references, implementations, call hierarchy, diagnostics, codemap, and shaped markdown output with pagination and candidate disambiguation.

The strongest adjacent threat is **Aider's repo map lineage**: PageRank over tree-sitter definition/reference tags, token-budget binary search, and conversation-aware boosts. **RepoMapper** is a direct MCP extraction of that idea, less mature than Aider. **Codebase Context** and **AiDex** are important workflow/search competitors, especially around compact output, edit preflight, memories, pattern guidance, and session governance, but they are not true LSP/navigation systems.

Most "code index MCP" repos are not serious navigation products. Several are semantic chunk searchers, whole-repo dumpers, or wrappers around other tools. Useful, sometimes, but the abstraction is wearing shoulder pads.

Loom should not copy any single product. The winning shape is:

- persistent local graph + embeddings from Loom,
- LSP-on-demand precision for definitions/references/call hierarchy,
- repo-map style ranked neighborhoods under a hard token budget,
- staged compact/full responses with continuation,
- agent rails that encourage narrow retrieval without trapping users in one tool.

## Repo Manifest

| Repo | Inferred URL | Classification |
|---|---|---|
| `tmp/competitors/oraios__serena` | `https://github.com/oraios/serena.git` | serious competitor |
| `tmp/competitors/BumpyClock__lsp-mcp` | `https://github.com/BumpyClock/lsp-mcp.git` | serious competitor |
| `tmp/competitors/ktnyt__cclsp` | `https://github.com/ktnyt/cclsp.git` | adjacent tool / thin LSP wrapper |
| `tmp/competitors/johnhuang316__code-index-mcp` | `https://github.com/johnhuang316/code-index-mcp.git` | adjacent tool |
| `tmp/competitors/ViperJuice__Code-Index-MCP` | `https://github.com/ViperJuice/Code-Index-MCP.git` | adjacent tool / overbuilt partial wrapper |
| `tmp/competitors/HarshalRathore__code-intel-mcp` | `https://github.com/HarshalRathore/code-intel-mcp.git` | serious adjacent graph tool |
| `tmp/competitors/CSCSoftware__AiDex` | `https://github.com/CSCSoftware/AiDex.git` | serious adjacent workflow/search tool |
| `tmp/competitors/aj47__auggie-context-mcp` | `https://github.com/aj47/auggie-context-mcp.git` | thin wrapper |
| `tmp/competitors/omar-haris__smart-coding-mcp` | `https://github.com/omar-haris/smart-coding-mcp.git` | adjacent semantic search tool |
| `tmp/competitors/pdavis68__RepoMapper` | `https://github.com/pdavis68/RepoMapper.git` | adjacent repo-map tool |
| `tmp/competitors/Aider-AI__aider` | `https://github.com/Aider-AI/aider.git` | serious adjacent repo-map/workflow competitor |
| `tmp/competitors/PatrickSys__codebase-context` | `https://github.com/PatrickSys/codebase-context.git` | serious adjacent workflow/search competitor |
| `tmp/competitors/DeDeveloper23__codebase-mcp` | `https://github.com/DeDeveloper23/codebase-mcp.git` | thin wrapper / no-op theater for navigation |

## Source-Level Findings

### Serena - serious competitor

Serena is the clearest "agent should not read files blindly" design in this set.

- Symbolic navigation is first-class. `get_symbols_overview` returns compact top-level symbol maps with depth and answer-size controls, while `find_symbol` supports `name_path`, `relative_path`, `depth`, `include_body`, `include_info`, and match caps (`tmp/competitors/oraios__serena/src/serena/tools/symbol_tools.py:36-240`).
- References and implementations are LSP-backed and output-shaped. References can include body/context snippets, group by symbol/file, and degrade into shorter summaries or counts when output exceeds the budget (`tmp/competitors/oraios__serena/src/serena/tools/symbol_tools.py:243-328`).
- Declaration lookup uses regex capture to find a symbol occurrence and then calls LSP declaration, letting the agent say "this text in this file" rather than manually locate byte offsets (`tmp/competitors/oraios__serena/src/serena/tools/symbol_tools.py:386-450`).
- Diagnostics are structured by path/severity/symbol, which is much more agent-useful than dumping raw language server output (`tmp/competitors/oraios__serena/src/serena/tools/symbol_tools.py:467-517`).
- File reads are deliberately demoted. `ReadFileTool` documentation tells agents to prefer symbolic operations when they know the symbol, and it supports `start_line`, `end_line`, and `max_answer_chars` (`tmp/competitors/oraios__serena/src/serena/tools/file_tools.py:20-48`).
- Memory is not decorative. It has project/global scope, filename relevance, char limits, explicit edit/delete/rename tools, and guards around memory mutation (`tmp/competitors/oraios__serena/src/serena/tools/memory_tools.py:6-115`).
- Onboarding is a workflow gate: if no project memories exist, the tool tells the agent to run onboarding and collect project structure, commands, style, and completion instructions (`tmp/competitors/oraios__serena/src/serena/tools/workflow_tools.py:10-74`).
- LSP server management is robust enough to matter: language servers start in parallel, startup failures are surfaced, and suitable servers are selected per file (`tmp/competitors/oraios__serena/src/serena/ls_manager.py:42-171`).
- Name matching is good agent UX: name paths support absolute/suffix/simple matching, overload indexes, and substring matching on the last symbol component (`tmp/competitors/oraios__serena/src/serena/symbol.py:142-249`).
- The system prompt is unusually explicit about staged symbolic retrieval: overview, then `find_symbol` with `include_body=false`, then targeted `include_body=true`, then `find_referencing_symbols` (`tmp/competitors/oraios__serena/src/serena/resources/config/prompt_templates/system_prompt.yml:8-47`).
- The Claude Code override tells agents to use Serena tools before built-in `Read`, `Glob`, `Grep`, or edit tools and includes a tool-choice table (`tmp/competitors/oraios__serena/src/serena/resources/config/prompt_templates/system_prompt.yml:67-123`).
- Hooks detect broad-read behavior and push the agent back toward symbolic tools after grep/read bursts; symbolic tools reset the counters (`tmp/competitors/oraios__serena/src/serena/hooks.py:103-151`, `tmp/competitors/oraios__serena/src/serena/hooks.py:206-272`, `tmp/competitors/oraios__serena/src/serena/hooks.py:411-484`).
- Output length handling is systematic: tools use shortened-result factories instead of just truncating useful data off the bottom (`tmp/competitors/oraios__serena/src/serena/tools/tools_base.py:267-297`).

Weaknesses: Serena does not have Loom's persistent graph/index story. It depends heavily on live LSP availability and agent compliance. The hooks can feel heavy-handed. It is a workflow product with symbolic tools, not a compact ranked-neighborhood engine.

### BumpyClock/lsp-mcp - serious competitor

BumpyClock/lsp-mcp is the most directly comparable LSP navigation server.

- Tool registry is broad and concrete: document symbols, go-to-definition, get-symbol-definition, find references, hover, workspace symbol, implementations, call hierarchy, referenced-symbol expansion, diagnostics, semantic search, and codemap (`tmp/competitors/BumpyClock__lsp-mcp/src/tool_registry.rs:7-43`).
- It has presets: minimal, standard, full. That is a good way to keep agent tool menus bounded while leaving power tools available (`tmp/competitors/BumpyClock__lsp-mcp/src/tool_registry.rs:45-77`).
- Server instructions tell agents about 1-based positions, diagnostics after edits, request IDs/logs for debugging, initial setup, and semantic search parameters (`tmp/competitors/BumpyClock__lsp-mcp/src/mcp/mod.rs:105-169`).
- LSP manager handles server startup, watchers, ast-grep, language detection/config, async pending startup, and server availability warnings (`tmp/competitors/BumpyClock__lsp-mcp/src/lsp/manager/core.rs:30-260`).
- Definition handling is not naive. It caps source return at 100 lines and retries definition lookup using selection ranges/document symbols, with TypeScript overload handling (`tmp/competitors/BumpyClock__lsp-mcp/src/service/operations/definitions.rs:36-260`).
- References support `context_lines`, `limit`, and `offset`, combine AST identifier discovery with LSP references, compute type counts before pagination, and group by file (`tmp/competitors/BumpyClock__lsp-mcp/src/service/operations/references.rs:25-176`).
- `find_referenced_symbols` expands outward from a symbol body: it resolves identifiers referenced by the symbol, dedupes workspace symbols, and separates external/not-found entries (`tmp/competitors/BumpyClock__lsp-mcp/src/service/operations/references.rs:179-320`).
- Call hierarchy is real: prepare, incoming callers, outgoing callees, snippets, context lines, and external markers (`tmp/competitors/BumpyClock__lsp-mcp/src/service/operations/call_hierarchy.rs:24-260`).
- Pagination has a shared helper and default limit (`tmp/competitors/BumpyClock__lsp-mcp/src/service/utils/pagination.rs:4-31`).
- MCP reference lookup is agent-friendly: symbol search prefers exact candidates, accepts fuzzy fallback, narrows by path, ranks candidates deterministically, and returns alternatives when ambiguous (`tmp/competitors/BumpyClock__lsp-mcp/src/mcp/references.rs:13-166`, `tmp/competitors/BumpyClock__lsp-mcp/src/mcp/references.rs:182-254`).
- Markdown formatters are strong: references have summary and detailed modes, selected candidate, definitions/reexports, by-type counts, top files, snippets, and truncation messages (`tmp/competitors/BumpyClock__lsp-mcp/src/markdown_formatter/references.rs:8-240`). Definitions include signature/docs/ref count/package/related symbols and source truncation (`tmp/competitors/BumpyClock__lsp-mcp/src/markdown_formatter/definition.rs:8-233`).
- Codemap is promising: overview/impact/context modes with depth, edge type, detail, scope, external inclusion, and limit/offset (`tmp/competitors/BumpyClock__lsp-mcp/src/mcp/codemap.rs:10-69`). The indexer builds file/symbol nodes and defines/calls/import edges from LSP symbols (`tmp/competitors/BumpyClock__lsp-mcp/src/codemap/indexer.rs:76-260`).

Weaknesses: it is more API surface than agent workflow. It does not have Serena-grade memory/onboarding/hook pressure. Codemap/semantic search appear secondary/optional, not the core product center. Loom can beat it by returning smaller ranked neighborhoods and integrating graph+embedding evidence more naturally.

### ktnyt/cclsp - adjacent tool / thin LSP wrapper

cclsp is a real LSP wrapper, but it is thin.

- It registers navigation and symbol tools with generic error handling (`tmp/competitors/ktnyt__cclsp/src/tools/registry.ts:17-50`).
- `find_definition` searches symbols by name in a file, then calls `textDocument/definition` at each found position (`tmp/competitors/ktnyt__cclsp/src/tools/navigation.ts:6-84`).
- `find_references` repeats the same name-to-position pattern and calls `textDocument/references` for all matches (`tmp/competitors/ktnyt__cclsp/src/tools/navigation.ts:86-169`).
- It supports implementations and call hierarchy tools (`tmp/competitors/ktnyt__cclsp/src/tools/navigation.ts:172-226`, `tmp/competitors/ktnyt__cclsp/src/tools/symbols.ts:46-242`).
- The underlying LSP operations flatten document symbols, find symbol positions using file text, and call LSP definition/references (`tmp/competitors/ktnyt__cclsp/src/lsp/operations.ts:117-301`).
- Server manager caches processes, spawns adapters, initializes JSON-RPC, and sets default pylsp settings (`tmp/competitors/ktnyt__cclsp/src/lsp/server-manager.ts:28-280`).
- Project scanning is shallow and pattern-based, with a default max depth of 3 (`tmp/competitors/ktnyt__cclsp/src/file-scanner.ts:8-170`).

Weaknesses: no staged retrieval workflow, no repo map, no memory, no strong compaction, no output budget strategy. It also catches and continues in some reference paths, which risks hiding partial failure (`tmp/competitors/ktnyt__cclsp/src/tools/navigation.ts:154-156`). Useful baseline, not a strategic threat.

### johnhuang316/code-index-mcp - adjacent tool

This is a practical index/search MCP, not an LSP competitor.

- `analyze_file` returns either deep index summaries or `needs_deep_index`, which is a good readiness shape (`tmp/competitors/johnhuang316__code-index-mcp/src/code_index_mcp/services/code_intelligence_service.py:35-77`).
- `get_symbol_body` handles exact and short-name matching, ambiguity, line-range reads, 150-line caps, and available-symbol hints on miss (`tmp/competitors/johnhuang316__code-index-mcp/src/code_index_mcp/services/code_intelligence_service.py:14-17`, `tmp/competitors/johnhuang316__code-index-mcp/src/code_index_mcp/services/code_intelligence_service.py:105-280`).
- Search supports context lines, file patterns, fuzzy/regex flags, pagination via `start_index` and `max_results`, and clear paging metadata (`tmp/competitors/johnhuang316__code-index-mcp/src/code_index_mcp/services/search_service.py:21-83`, `tmp/competitors/johnhuang316__code-index-mcp/src/code_index_mcp/services/search_service.py:134-217`).
- The SQLite index builder validates inputs, uses parallel workers, tracks timeout/running/cancelled states, writes file/symbol rows, stores metadata, resolves pending calls, and returns stats (`tmp/competitors/johnhuang316__code-index-mcp/src/code_index_mcp/indexing/sqlite_index_builder.py:43-263`).
- Python indexing is AST-based and extracts classes/functions/signatures/docstrings/calls with a single-pass visitor (`tmp/competitors/johnhuang316__code-index-mcp/src/code_index_mcp/indexing/strategies/python_strategy.py:26-239`).
- Server has a FIFO concurrency limiter and stderr-only logging (`tmp/competitors/johnhuang316__code-index-mcp/src/code_index_mcp/server.py:41-184`).

Weaknesses: no LSP definitions/references/call hierarchy; language intelligence is strategy-by-strategy AST extraction. Agent workflow is ordinary tool description, not a retrieval discipline.

### ViperJuice/Code-Index-MCP - adjacent tool / overbuilt partial wrapper

This repo has useful readiness/output ideas, but the navigation claims are much weaker than the architecture suggests.

- The simple dispatcher does symbol lookup and BM25-ish search, but graph/mutation/cross-repo methods explicitly raise `NotImplementedError` (`tmp/competitors/ViperJuice__Code-Index-MCP/mcp_server/dispatcher/simple_dispatcher.py:15-21`, `tmp/competitors/ViperJuice__Code-Index-MCP/mcp_server/dispatcher/simple_dispatcher.py:41-100`, `tmp/competitors/ViperJuice__Code-Index-MCP/mcp_server/dispatcher/simple_dispatcher.py:117-231`).
- Stdio runner has a useful server instruction: pre-built BM25+semantic index, readiness, and safe fallback (`tmp/competitors/ViperJuice__Code-Index-MCP/mcp_server/cli/stdio_runner.py:40-48`).
- Tool schemas expose symbol lookup, search, reindex, summarize, and handshake; search has `semantic`, `fuzzy`, `limit`, and repository/path handling (`tmp/competitors/ViperJuice__Code-Index-MCP/mcp_server/cli/stdio_runner.py:61-245`).
- Handler output is consistently JSON and protects against empty/serialization failures (`tmp/competitors/ViperJuice__Code-Index-MCP/mcp_server/cli/tool_handlers.py:49-68`).
- Lookup/search validate path allowlists and index readiness, surface `index_unavailable` with `safe_fallback=native_search`, enforce a search timeout, and include no-result/remediation shapes (`tmp/competitors/ViperJuice__Code-Index-MCP/mcp_server/cli/tool_handlers.py:145-165`, `tmp/competitors/ViperJuice__Code-Index-MCP/mcp_server/cli/tool_handlers.py:222-493`).

Weaknesses: lots of surface area, little direct LSP/navigation. Many graph/cross-repo methods are unimplemented in the simple path. The repeated `_usage_hint` telling agents to `Read(file, offset, limit=20/30)` is useful operationally but trains broad reads back into the loop. Loom should steal readiness/error shapes, not the sprawl.

### HarshalRathore/code-intel-mcp - serious adjacent graph tool

This is not LSP, but it is serious for call/data/impact analysis.

- It hard-depends on Joern and ArangoDB, failing startup if Joern CLI is missing (`tmp/competitors/HarshalRathore__code-intel-mcp/src/index.ts:17-34`).
- It wires Joern, Arango, live watcher, and MCP server startup together (`tmp/competitors/HarshalRathore__code-intel-mcp/src/index.ts:48-69`).
- Tools cover symbol search, callers, callees, call chain, data flow, impact analysis, and project indexing (`tmp/competitors/HarshalRathore__code-intel-mcp/src/index.ts:71-306`).
- Arango queries implement symbol search and graph traversals for callers/callees (`tmp/competitors/HarshalRathore__code-intel-mcp/src/arango-client.ts:71-255`).

Weaknesses: heavyweight external infrastructure is the product. That is a non-starter for Loom's local, lightweight positioning. Output is raw-ish JSON, not agent-compact. But the call/data/impact ambition is real.

### CSCSoftware/AiDex - serious adjacent workflow/search tool

AiDex is not LSP navigation, but its agent workflow is worth studying.

- Tool schema explicitly frames `aidex_query` as preferred over grep/glob, with filters for mode, file/type, modified-before/after, and limit (`tmp/competitors/CSCSoftware__AiDex/src/server/tools.ts:17-105`).
- Signature tools are explicitly positioned as alternatives to reading full files (`tmp/competitors/CSCSoftware__AiDex/src/server/tools.ts:121-160`).
- It exposes summary/tree/context-ish tools plus link/unlink/links/scan for cross-project handling (`tmp/competitors/CSCSoftware__AiDex/src/server/tools.ts:198-317`).
- Query implementation loads occurrences in batches, applies file/type/time filters, dedups, sorts, limits, and reports truncation (`tmp/competitors/CSCSoftware__AiDex/src/commands/query.ts:46-134`).
- Signature extraction returns header comments, types, and methods for one or many files (`tmp/competitors/CSCSoftware__AiDex/src/commands/signature.ts:64-223`).
- Setup writes a CLAUDE.md block that says `.aidex exists -> STOP use AiDex`, use `aidex_init` if absent, and prefer `aidex_signature` over file reads (`tmp/competitors/CSCSoftware__AiDex/src/commands/setup.ts:62-145`).
- Search pipeline has semantic/exact parallel search, query rewriting, RRF merge, optional rerank, and a privacy switch for whether code can be sent to an LLM (`tmp/competitors/CSCSoftware__AiDex/src/embeddings/search.ts:1-305`).
- Parser uses tree-sitter to extract items, methods, types, header comments, and identifiers (`tmp/competitors/CSCSoftware__AiDex/src/parser/extractor.ts:65-208`).

Weaknesses: no LSP-grade references/definitions/call hierarchy. Its prompt/governance surface is broader than its navigation capability. Good agent rails; not a direct symbol-graph competitor.

### aj47/auggie-context-mcp - thin wrapper

This is a wrapper around Auggie/Augment's context engine.

- It checks for the Auggie CLI, builds `auggie --print --quiet` command lines, applies workspace/model/rules/output arguments, and manages timeout/stdout/stderr (`tmp/competitors/aj47__auggie-context-mcp/src/index.ts:38-172`).
- It exposes one MCP tool, `query_codebase`, plus list/call handlers and startup/auth messaging (`tmp/competitors/aj47__auggie-context-mcp/src/index.ts:190-322`).
- README states the official Augment Code Context Engine MCP now exists, which weakens the repo's independent value (`tmp/competitors/aj47__auggie-context-mcp/README.md:1-2`).

Weaknesses: no local source-level code intelligence implementation. It may be useful if Auggie is useful. For Loom, this is a distribution/wrapper lesson only.

### omar-haris/smart-coding-mcp - adjacent semantic search tool

This is a semantic chunk searcher with decent operational thinking, but not navigation.

- Server lazy-loads model/cache on first use, avoiding IDE startup blocking, and starts progressive background indexing after a configurable delay (`tmp/competitors/omar-haris__smart-coding-mcp/index.js:107-213`).
- Feature registry prioritizes semantic search and includes index/status/cache/workspace/version tools (`tmp/competitors/omar-haris__smart-coding-mcp/index.js:73-105`, `tmp/competitors/omar-haris__smart-coding-mcp/index.js:229-260`).
- Hybrid search uses vector similarity, exact-match boost, partial-word boost, and returns indexing-in-progress warnings when results are partial (`tmp/competitors/omar-haris__smart-coding-mcp/features/hybrid-search.js:12-83`).
- Tool definition includes read-only annotations (`tmp/competitors/omar-haris__smart-coding-mcp/features/hybrid-search.js:87-140`).
- AST chunker uses tree-sitter where possible, falls back to smart chunking, extracts semantic nodes, splits large nodes, and preserves line ranges (`tmp/competitors/omar-haris__smart-coding-mcp/lib/ast-chunker.js:1-257`).
- Indexer uses worker threads when safe, single-thread fallback, file mtime/hash checks, watcher setup, progress tracking, and chunk embedding (`tmp/competitors/omar-haris__smart-coding-mcp/features/index-codebase.js:13-360`).
- Resource throttle limits worker count and batch delay, though CPU monitoring is admitted future work (`tmp/competitors/omar-haris__smart-coding-mcp/lib/resource-throttle.js:1-78`).
- SQLite cache stores embeddings, file hashes, line ranges, and content with WAL enabled (`tmp/competitors/omar-haris__smart-coding-mcp/lib/sqlite-cache.js:1-300`).

Weaknesses: no definitions, references, call hierarchy, or repo graph. Search scores all vectors in memory via `getVectorStore`, so large-repo scaling is suspect. Output is snippets, not compact neighborhoods.

### pdavis68/RepoMapper - adjacent repo-map tool

RepoMapper is basically Aider's repo map concept exposed as MCP.

- MCP tool `repo_map` accepts chat files, other files, token limit, mentioned files/idents, max context window, exclusion flags, and returns map plus report stats (`tmp/competitors/pdavis68__RepoMapper/repomap_server.py:53-175`).
- If no `other_files` are provided, it scans the root directory for context; that is convenient but dangerous for token discipline (`tmp/competitors/pdavis68__RepoMapper/repomap_server.py:108-124`).
- `search_identifiers` searches tree-sitter tags and returns context around def/ref matches (`tmp/competitors/pdavis68__RepoMapper/repomap_server.py:177-270`).
- RepoMap caches tags, extracts definitions/references via tree-sitter query files, and falls back carefully on missing languages/errors (`tmp/competitors/pdavis68__RepoMapper/repomap_class.py:41-252`).
- It builds a directed graph from reference files to definition files, runs PageRank, boosts mentioned identifiers/files/chat files, and ranks definition tags (`tmp/competitors/pdavis68__RepoMapper/repomap_class.py:254-400`).
- It renders lines of interest through `grep_ast.TreeContext`, then binary-searches the number of tags to fit the token budget (`tmp/competitors/pdavis68__RepoMapper/repomap_class.py:402-616`).

Weaknesses: no LSP precision. Ranking is file/symbol-tag oriented and may miss semantic edges. It inherits Aider's strengths but with less polish.

### Aider - serious adjacent repo-map/workflow competitor

Aider is not an MCP navigation server, but its repo map is a real token-budgeted code intelligence primitive.

- RepoMap caches tree-sitter tags, estimates token count by sampling long text, and expands repo-map budget when no files are already in chat (`tmp/competitors/Aider-AI__aider/aider/repomap.py:42-167`).
- Tag extraction supports tree-sitter query APIs across versions and backfills references with Pygments when languages only provide definitions (`tmp/competitors/Aider-AI__aider/aider/repomap.py:233-363`).
- Ranking builds a graph from references to definitions, boosts chat files, mentioned identifiers, and significant identifier styles, downweights private/high-fanout identifiers, and runs PageRank over weighted edges (`tmp/competitors/Aider-AI__aider/aider/repomap.py:365-574`).
- It caches repo-map results, uses refresh modes, and binary-searches tag count against token budget with a tolerance (`tmp/competitors/Aider-AI__aider/aider/repomap.py:576-706`).
- Rendering uses `TreeContext` around lines of interest and caches per file/mtime (`tmp/competitors/Aider-AI__aider/aider/repomap.py:710-746`).
- The coder integrates repo maps into chat automatically: current message mentions become file/identifier boosts, chat files are excluded from "other files", and there are fallbacks to global/unhinted repo maps (`tmp/competitors/Aider-AI__aider/aider/coders/base_coder.py:678-748`).

Weaknesses: no LSP references/call hierarchy and no persistent SQLite graph/embedding layer like Loom. It was built for Aider's chat loop, not a reusable MCP ranking service. But its "ranked map under budget" mechanic is one of the best things in this teardown.

### PatrickSys/codebase-context - serious adjacent workflow/search competitor

Codebase Context is not LSP, but it is the most ambitious compact-output/edit-preflight system here.

- Runtime tracks known roots, discovered project paths, active project selection, watched-project limits, debounce, and idle tracking (`tmp/competitors/PatrickSys__codebase-context/src/index.ts:135-171`, `tmp/competitors/PatrickSys__codebase-context/src/index.ts:224-300`).
- Tool registry includes search, metadata, status, refresh, style guide, team patterns, symbol references, cycle detection, remember, get memory, and health; project selection is injected into every tool schema (`tmp/competitors/PatrickSys__codebase-context/src/tools/index.ts:1-93`).
- Search tool defaults to compact mode: max 6 results, graph context, pattern summary, best example, next hops, and budget metadata. It also has edit/refactor/migrate preflight (`tmp/competitors/PatrickSys__codebase-context/src/tools/search-codebase.ts:70-141`).
- Search handles indexing/error states and auto-heals corrupt indexes by starting a background reindex (`tmp/competitors/PatrickSys__codebase-context/src/tools/search-codebase.ts:143-246`).
- Search loads memories, pattern intelligence, relationships sidecars, health, and reranker status to enrich output (`tmp/competitors/PatrickSys__codebase-context/src/tools/search-codebase.ts:248-280`, `tmp/competitors/PatrickSys__codebase-context/src/tools/search-codebase.ts:1159-1169`).
- Compact output is aggressively shaped: `status`, `searchQuality`, `budget`, optional `preflight`, `patternSummary`, `bestExample`, `nextHops`, compact file ranges, summaries, scores, graph counts, exports, layer, symbol metadata, scope, health, and signature preview (`tmp/competitors/PatrickSys__codebase-context/src/tools/search-codebase.ts:1171-1255`).
- Full mode keeps richer relationships/hints/snippets/imports/exports/complexity but still carries budget metadata (`tmp/competitors/PatrickSys__codebase-context/src/tools/search-codebase.ts:1257-1310`).
- Output budget warning is computed after rendering and adjusted for compact/full mode (`tmp/competitors/PatrickSys__codebase-context/src/tools/search-payload-budget.ts:1-69`).
- Search core supports semantic + keyword search, intent classification, query expansion, low-confidence rescue, candidate floor, cross-encoder rerank, and keyword fallback (`tmp/competitors/PatrickSys__codebase-context/src/core/search.ts:24-62`, `tmp/competitors/PatrickSys__codebase-context/src/core/search.ts:916-1165`).
- Symbol references are syntactic, not LSP: it prefilters candidate files from the keyword index, then uses tree-sitter identifier walking on file contents with regex fallback and bounded results (`tmp/competitors/PatrickSys__codebase-context/src/core/symbol-references.ts:104-254`).
- Codebase map reads intelligence/relationships/index artifacts, derives layers, entrypoints, hubs, key interfaces, API surface, hotspots, active patterns, best examples, graph stats, and suggested next calls (`tmp/competitors/PatrickSys__codebase-context/src/core/codebase-map.ts:1-265`).
- Memory has type/category/scope normalization, file/symbol scoped memories, filtering, recency sorting, and confidence decay by half-life (`tmp/competitors/PatrickSys__codebase-context/src/memory/store.ts:1-220`).
- Evidence lock blocks or warns before edits based on code/pattern/memory evidence, low search quality, stale index, contradictory patterns, and caller coverage (`tmp/competitors/PatrickSys__codebase-context/src/preflight/evidence-lock.ts:1-220`).
- Its AGENTS file contains useful product constraints: load team memory at session start, if retrieval is bad say so, optimize output for first read, and never overclaim evals (`tmp/competitors/PatrickSys__codebase-context/AGENTS.md:115-204`).

Weaknesses: no real LSP definitions/references/call hierarchy. There is a lot of policy and product surface, and some graceful degradation catches can hide missing artifact quality. It is a serious workflow competitor, not a direct navigation competitor.

### DeDeveloper23/codebase-mcp - thin wrapper / no-op theater for navigation

This repo is a Repomix wrapper plus what appears to be copied MCP SDK source.

- `getCodebase` shells out to `npx repomix --output stdout` and returns the whole packed repo, with a 50MB buffer (`tmp/competitors/DeDeveloper23__codebase-mcp/src/tools/codebase.ts:15-108`).
- `getRemoteCodebase` does the same for remote repos (`tmp/competitors/DeDeveloper23__codebase-mcp/src/tools/codebase.ts:110-199`).
- `saveCodebase` writes a Repomix dump to a file (`tmp/competitors/DeDeveloper23__codebase-mcp/src/tools/codebase.ts:201-310`).
- The rest of `src/server`, `src/client`, and `src/shared` is generic MCP SDK machinery, not code intelligence. For example, `src/server/index.ts` is an MCP server implementation with initialization/capability plumbing (`tmp/competitors/DeDeveloper23__codebase-mcp/src/server/index.ts:1-220`).

Weaknesses: this actively encourages the broad file read pattern Loom exists to replace. No symbols, no references, no graph, no search ranking, no staged retrieval. Useful only as an example of what not to become.

## Strong Ideas Loom Should Steal

### P0

- **Symbolic-first staged retrieval.** Serena's overview -> targeted symbol -> body only when needed -> references flow is exactly the anti-broad-read behavior Loom wants (`tmp/competitors/oraios__serena/src/serena/resources/config/prompt_templates/system_prompt.yml:28-37`).
- **Shortened-result factories.** Do not truncate dumbly. Return alternate compact shapes: counts, file groups, candidate lists, snippets stripped, then summaries (`tmp/competitors/oraios__serena/src/serena/tools/tools_base.py:267-297`).
- **LSP result shaping.** BumpyClock's selected candidate, alternatives, definitions/reexports, type counts, file groups, detail/summary modes, context lines, and `limit`/`offset` should be a baseline for Loom navigation tools (`tmp/competitors/BumpyClock__lsp-mcp/src/mcp/references.rs:13-166`, `tmp/competitors/BumpyClock__lsp-mcp/src/markdown_formatter/references.rs:8-240`).
- **Repo-map ranking under budget.** Aider's PageRank over def/ref tags plus chat-file and mentioned-ident boosts is still one of the cleanest compact codebase-context techniques (`tmp/competitors/Aider-AI__aider/aider/repomap.py:365-706`).
- **Agent memory with scope and decay.** Serena's project/global memories and Codebase Context's file/symbol scopes plus confidence decay are both useful. Loom should keep memories local, small, and explicit (`tmp/competitors/oraios__serena/src/serena/tools/memory_tools.py:6-115`, `tmp/competitors/PatrickSys__codebase-context/src/memory/store.ts:1-220`).
- **Readiness and safe fallback fields.** ViperJuice's `index_unavailable` + `safe_fallback` shape and Codebase Context's indexing/error/auto-heal responses make failures actionable (`tmp/competitors/ViperJuice__Code-Index-MCP/mcp_server/cli/tool_handlers.py:145-165`, `tmp/competitors/PatrickSys__codebase-context/src/tools/search-codebase.ts:178-246`).
- **Compact/full response modes.** Codebase Context's default compact mode with optional full mode is the right default for MCP agents (`tmp/competitors/PatrickSys__codebase-context/src/tools/search-codebase.ts:1171-1310`).

### P1

- **Tool presets.** BumpyClock's minimal/standard/full tool sets keep the MCP surface manageable while allowing deeper tools when needed (`tmp/competitors/BumpyClock__lsp-mcp/src/tool_registry.rs:45-77`).
- **Broad-read nudges, but lighter than Serena.** Detect repeated grep/read patterns and recommend symbolic/graph tools. Do not hard-deny unless the user configured strict mode (`tmp/competitors/oraios__serena/src/serena/hooks.py:103-151`).
- **Call hierarchy as first-class graph query.** BumpyClock's incoming/outgoing callers with snippets is a strong shape to blend with Loom's persistent graph (`tmp/competitors/BumpyClock__lsp-mcp/src/service/operations/call_hierarchy.rs:24-260`).
- **Candidate disambiguation.** Exact, path-scoped, fuzzy fallback, and alternatives avoid "wrong symbol, confident answer" failures (`tmp/competitors/BumpyClock__lsp-mcp/src/mcp/references.rs:93-166`).
- **Low-confidence rescue and abstention.** Codebase Context's search quality and evidence lock are a good north star, if kept smaller and less product-y (`tmp/competitors/PatrickSys__codebase-context/src/core/search.ts:972-1006`, `tmp/competitors/PatrickSys__codebase-context/src/preflight/evidence-lock.ts:198-220`).
- **Progressive/lazy indexing.** Smart Coding MCP's lazy model loading and background indexing help avoid MCP startup pain (`tmp/competitors/omar-haris__smart-coding-mcp/index.js:107-213`).
- **Concurrency/timeout discipline.** johnhuang's FIFO limiter and indexer timeouts are worth stealing for predictable local behavior (`tmp/competitors/johnhuang316__code-index-mcp/src/code_index_mcp/server.py:41-160`, `tmp/competitors/johnhuang316__code-index-mcp/src/code_index_mcp/indexing/sqlite_index_builder.py:60-166`).

### P2

- **Session notes and setup-file insertion.** AiDex's CLAUDE.md block and Codebase Context's AGENTS governance can help onboarding, but Loom should make it opt-in and small (`tmp/competitors/CSCSoftware__AiDex/src/commands/setup.ts:62-145`).
- **Cross-project links.** AiDex/ViperJuice gesture at cross-repo relationships; useful later, not core now (`tmp/competitors/CSCSoftware__AiDex/src/server/tools.ts:265-317`).
- **Resource throttling controls.** Smart Coding MCP's worker-count/batch-delay knobs are simple and practical, though actual CPU monitoring is still future work (`tmp/competitors/omar-haris__smart-coding-mcp/lib/resource-throttle.js:1-78`).
- **Pattern/golden-file guidance.** Codebase Context's best examples and pattern summaries are useful for edit preflight, but should not pollute basic symbol search (`tmp/competitors/PatrickSys__codebase-context/src/tools/search-codebase.ts:1190-1238`).

## Weak / No-Op Repos And Why

- **DeDeveloper23/codebase-mcp:** thin Repomix shell wrapper. It dumps entire repos to stdout or files. No navigation, no index, no compaction beyond Repomix options, no staged retrieval. It is the broad-read failure mode packaged as a tool (`tmp/competitors/DeDeveloper23__codebase-mcp/src/tools/codebase.ts:15-310`).
- **aj47/auggie-context-mcp:** thin CLI wrapper over Auggie. No local implementation to evaluate; value depends entirely on Augment's context engine (`tmp/competitors/aj47__auggie-context-mcp/src/index.ts:38-322`).
- **ktnyt/cclsp:** real LSP calls, but thin. It lacks Loom-grade ranking, compaction, memory, continuation, or workflow rails (`tmp/competitors/ktnyt__cclsp/src/tools/navigation.ts:6-226`).
- **ViperJuice/Code-Index-MCP:** not no-op, but much of the impressive surface is partial. The simple dispatcher explicitly leaves graph/mutation/cross-repo methods unimplemented (`tmp/competitors/ViperJuice__Code-Index-MCP/mcp_server/dispatcher/simple_dispatcher.py:117-231`).
- **omar-haris/smart-coding-mcp:** useful semantic search, but no definitions/references/call hierarchy. It is an embedding snippet retriever, not a navigation competitor (`tmp/competitors/omar-haris__smart-coding-mcp/features/hybrid-search.js:12-83`).

## What Loom Should Avoid

- **Whole-repo dumping as an MCP tool.** It makes agents worse at the exact behavior Loom is trying to fix (`tmp/competitors/DeDeveloper23__codebase-mcp/src/tools/codebase.ts:15-108`).
- **A single magic `query_codebase` wrapper.** It hides retrieval stages and prevents agents from knowing whether they got semantic search, symbol lookup, graph impact, or file content (`tmp/competitors/aj47__auggie-context-mcp/src/index.ts:190-231`).
- **Heavy infrastructure as the default path.** Joern + Arango may be powerful, but it is not Loom's default local developer experience (`tmp/competitors/HarshalRathore__code-intel-mcp/src/index.ts:17-34`).
- **Silent partial failures.** cclsp's catch-and-continue reference path is the wrong instinct; agents need explicit partial status (`tmp/competitors/ktnyt__cclsp/src/tools/navigation.ts:154-156`).
- **Overgrown product surfaces before core quality.** ViperJuice and Codebase Context both show the risk: many modes/tools/sidecars can bury the core retrieval contract.
- **Training agents back to file reads after search.** Usage hints should offer the next narrow Loom call, not a generic "Read this file" reflex.
- **Prompt coercion as a substitute for capability.** Serena's hooks are useful, but Loom should earn trust through better compact answers first.

## Gaps / Open Questions

- I inspected source level only. I did not run each MCP server or validate runtime behavior against real projects.
- Serena's LSP backend and BumpyClock's codemap/semantic-search behavior deserve runtime dogfood against Loom itself.
- ViperJuice may have richer non-simple dispatchers, but the simple dispatcher and CLI path already reveal enough partial implementation risk to avoid treating it as a serious navigation competitor without further testing.
- Codebase Context's eval/result artifacts exist in the repo; I did not audit whether its claimed evaluation outcomes are reproducible.
- Aider and RepoMapper use tree-sitter tag queries, not LSP. Their ranking may be an excellent prior for Loom, but they cannot replace precise language-server references in typed languages.
- Need a follow-up teardown specifically for security/privacy defaults around indexed content, cache paths, secret exclusion, and logging across all repos.
