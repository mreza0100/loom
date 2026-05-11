# Canonical Code Search / Sourcegraph-Lineage Competitor Teardown

## Executive verdict

Zoekt is the canonical exact-search competitor. Sourcegraph's fork is the live baseline Loom should benchmark against: trigram posting lists, query simplification, regex candidate pruning, fielded boolean syntax, shard-level partial evaluation, branch masks, ctags symbols, and pragmatic ranking. Google Zoekt in this clone is only a historical pointer to that fork.

Livegrep is the other serious exact baseline, especially for interactive regex: suffix arrays, RE2 query planning, hard timeouts, match caps, and tag-first search for symbol-looking queries. OpenGrok is a serious navigation/index appliance: heavier and less agent-native, but strong on Lucene fields, xrefs, definitions, history, REST, multi-project search, and incremental per-project indexing.

Sourcebot is a serious Sourcegraph-lineage platform wrapper around vendored Zoekt. Its useful contribution is not a new index algorithm; it is repo sync, permission scoping, streaming search, MCP tool design, and agent-facing structured output. Cody's public snapshot is adjacent but important for retrieval: it shows how to merge remote indexed context with local dirty-file context and how to de-duplicate context items.

The semantic/MCP repos are mostly adjacent, not canonical code search. `osgrep` has a real hybrid semantic architecture but its MCP server exposes no tools. `codebase-index-cli`, `semantic-search`, `github-semantic-search-mcp`, and `semantic-code-mcp` contain useful vector/indexing patterns but are not exact search, regex search, or Sourcegraph-class navigation. `Code2MCP` is a generator workflow, not search. `MCP-Github-Agent` has moved and contains no implementation here.

## Repo manifest

| Local repo | URL inferred from `.git/config` | Classification | Verdict |
|---|---|---|---|
| `tmp/competitors/sourcegraph__zoekt` | `https://github.com/sourcegraph/zoekt.git` | serious competitor | Primary exact-search baseline. |
| `tmp/competitors/google__zoekt` | `https://github.com/google/zoekt.git` | adjacent / historical | README-only pointer to Sourcegraph Zoekt in this clone. |
| `tmp/competitors/oracle__opengrok` | `https://github.com/oracle/opengrok.git` | serious competitor | Heavy Lucene/xref/navigation appliance. |
| `tmp/competitors/livegrep__livegrep` | `https://github.com/livegrep/livegrep.git` | serious competitor | Exact regex search baseline with suffix-array planning. |
| `tmp/competitors/sourcebot-dev__sourcebot` | `https://github.com/sourcebot-dev/sourcebot.git` | serious adjacent platform | Sourcegraph-style repo sync, Zoekt wrapper, MCP and AI surfaces. |
| `tmp/competitors/sourcegraph__cody-public-snapshot` | `https://github.com/sourcegraph/cody-public-snapshot.git` | adjacent retrieval system | Strong context-retrieval patterns, not a search engine. |
| `tmp/competitors/DEFENSE-SEU__Code2MCP` | `https://github.com/DEFENSE-SEU/Code2MCP.git` | thin wrapper / adjacent generator | LLM-generated MCP service pipeline, no search index. |
| `tmp/competitors/DEFENSE-SEU__MCP-Github-Agent` | `https://github.com/DEFENSE-SEU/MCP-Github-Agent.git` | no-op theater in this clone | README says repository moved to Code2MCP; no source implementation. |
| `tmp/competitors/Ryandonofrio3__osgrep` | `https://github.com/Ryandonofrio3/osgrep.git` | serious adjacent semantic tool, MCP no-op | Real hybrid semantic search, but MCP exposes zero tools. |
| `tmp/competitors/edelauna__github-semantic-search-mcp` | `https://github.com/edelauna/github-semantic-search-mcp.git` | adjacent tool | GitHub-backed cloud semantic search, not exact code search. |
| `tmp/competitors/dudufcb1__codebase-index-cli` | `https://github.com/dudufcb1/codebase-index-cli.git` | adjacent tool | Local vector indexing CLI with tree-sitter and sqlite/Qdrant storage. |
| `tmp/competitors/dudufcb1__semantic-search` | `https://github.com/dudufcb1/semantic-search.git` | adjacent / thin semantic MCP | FastMCP vector-search wrapper over sqlite-vec/Qdrant. |
| `tmp/competitors/vrppaul__semantic-code-mcp` | `https://github.com/vrppaul/semantic-code-mcp.git` | adjacent semantic MCP | Small LanceDB hybrid semantic MCP, mostly Python-oriented. |

## Source-level findings

### `sourcegraph__zoekt`

Serious competitor. This is the exact-search architecture Loom must beat or wrap as a baseline.

- The design is trigram-first. Files are indexed by positional trigrams; string search chooses trigram pairs and checks offsets before confirming matches (`tmp/competitors/sourcegraph__zoekt/doc/design.md:26`, `tmp/competitors/sourcegraph__zoekt/doc/design.md:33`).
- Regex search is planned by extracting normal strings into a boolean substring query, then running the full regexp only on candidates (`tmp/competitors/sourcegraph__zoekt/doc/design.md:41`).
- Query execution is explicitly about posting-list selectivity: intersect a small number of candidate lists and choose selective trigram pairs (`tmp/competitors/sourcegraph__zoekt/doc/design.md:56`).
- The memory/index economics are concrete: index size around several times corpus, only bounded ngram structures in memory, and mmap shard files (`tmp/competitors/sourcegraph__zoekt/doc/design.md:68`, `tmp/competitors/sourcegraph__zoekt/doc/design.md:132`).
- Multi-branch storage uses branch bitmasks so mostly-identical branches do not duplicate whole indexes (`tmp/competitors/sourcegraph__zoekt/doc/design.md:115`).
- The shard format is explicit: file contents, filenames, posting lists, branch masks, and metadata, with practical limits due to uint32 offsets (`tmp/competitors/sourcegraph__zoekt/doc/design.md:132`).
- Concurrency is shard-driven: one goroutine per shard, so shard splitting controls query parallelism (`tmp/competitors/sourcegraph__zoekt/doc/design.md:152`).
- Ranking is pragmatic rather than mystical: atom count, closeness, word boundaries, update time, filename length, tokenizer/symbol ranking, and ctags all contribute (`tmp/competitors/sourcegraph__zoekt/doc/design.md:167`).
- Query semantics are Sourcegraph-like: substrings and regexps over file/content, positive atoms required, negations only prune, and filters like `file:` and `branch:` (`tmp/competitors/sourcegraph__zoekt/doc/design.md:211`, `tmp/competitors/sourcegraph__zoekt/doc/design.md:230`).
- Builder options include shard max, trigram limits, ctags, delta build, language maps, shard merging, and large-file knobs (`tmp/competitors/sourcegraph__zoekt/index/builder.go:52`).
- Index-time document ordering deprioritizes skipped/generated/vendor/test files and boosts short names/content, symbol-rich files, many branches, and original order; query-time limits then benefit from this ordering (`tmp/competitors/sourcegraph__zoekt/index/builder.go:880`).
- Search simplifies a multi-repo query into true/false per shard, expands file/content terms, prunes the match tree, and skips shards when the pruned tree is nil (`tmp/competitors/sourcegraph__zoekt/index/eval.go:34`, `tmp/competitors/sourcegraph__zoekt/index/eval.go:138`).
- Scoring rewards word boundaries, basename matches, exact/edge/overlap symbol matches, symbol kind score, and custom weights (`tmp/competitors/sourcegraph__zoekt/index/score.go:93`).
- Optional BM25 exists at line and file levels, but it skips IDF and uses Lucene-style defaults (`tmp/competitors/sourcegraph__zoekt/index/score.go:200`, `tmp/competitors/sourcegraph__zoekt/index/score.go:355`).
- Term frequency boosts filename and symbol matches, while low-priority files are penalized (`tmp/competitors/sourcegraph__zoekt/index/score.go:249`).
- Ctags integration stores symbols and byte ranges, including Universal/SCIP ctags support (`tmp/competitors/sourcegraph__zoekt/index/ctags.go:44`, `tmp/competitors/sourcegraph__zoekt/index/ctags.go:117`).
- Sourcegraph's fork adds a scheduler that downgrades slow interactive queries to batch work after a configured duration (`tmp/competitors/sourcegraph__zoekt/search/sched.go:59`, `tmp/competitors/sourcegraph__zoekt/search/sched.go:101`).
- Shards are watched and hot-loaded/dropped from `.zoekt` as files change (`tmp/competitors/sourcegraph__zoekt/search/watcher.go:120`).

### `google__zoekt`

Adjacent / historical. The clone is not a source competitor.

- The README says the active main repository is `github.com/sourcegraph/zoekt` (`tmp/competitors/google__zoekt/README.md:1`).
- The README still documents indexing git branches, `zoekt-indexserver` mirroring/fetching/cleaning repos, and Universal ctags for better ranking (`tmp/competitors/google__zoekt/README.md:13`, `tmp/competitors/google__zoekt/README.md:59`, `tmp/competitors/google__zoekt/README.md:84`).
- No source-level architecture exists in this checkout beyond README guidance.

### `oracle__opengrok`

Serious competitor. Strong on source navigation and history, less on agent-token efficiency.

- REST APIs expose definitions with symbol type, signature, symbol, and line ranges (`tmp/competitors/oracle__opengrok/apiary.apib:155`).
- `/search` accepts `full`, `def`, `symbol`, `path`, `hist`, `type`, `projects`, pagination, max hits per file, and returns Lucene `relevancy` plus path and last-modified metadata (`tmp/competitors/oracle__opengrok/apiary.apib:653`).
- Docker/source layout expects project source under `/opengrok/src` and index/history/blame data under `/opengrok/data` (`tmp/competitors/oracle__opengrok/docker/README.md:81`).
- Repo sync/reindex is operationalized through `SYNC_PERIOD_MINUTES`, `INDEXER_OPT`, worker counts, REST `/reindex`, and mirror config (`tmp/competitors/oracle__opengrok/docker/README.md:102`, `tmp/competitors/oracle__opengrok/docker/README.md:121`).
- IndexDatabase creates/updates one Lucene database per project and stores index, xref, and suggester directories (`tmp/competitors/oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/index/IndexDatabase.java:124`, `tmp/competitors/oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/index/IndexDatabase.java:195`).
- Incremental indexing opens readers, collects UID terms, identifies files to index/remove, and parallelizes indexing (`tmp/competitors/oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/index/IndexDatabase.java:620`).
- QueryBuilder has dedicated Lucene fields for full text, definitions, references, path, history, type, scopes, project, date, and internal tags (`tmp/competitors/oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/search/QueryBuilder.java:52`).
- SearchEngine builds Lucene queries from those fields, uses MultiReader for multi-project search, and can sort by last modified, path, or score (`tmp/competitors/oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/search/SearchEngine.java:90`, `tmp/competitors/oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/search/SearchEngine.java:210`, `tmp/competitors/oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/search/SearchEngine.java:224`).
- Definitions stores per-line symbol tags, symbol-to-line maps, and tag lists for xref lookup (`tmp/competitors/oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/analysis/Definitions.java:64`, `tmp/competitors/oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/analysis/Definitions.java:130`).

### `livegrep__livegrep`

Serious competitor for exact regex. It is narrower than Zoekt but very real.

- Index files are self-contained after build, usually 3-5x indexed text, mmap-backed, and optimized for SSD (`tmp/competitors/livegrep__livegrep/README.md:190`).
- Regex uses RE2 (`tmp/competitors/livegrep__livegrep/README.md:201`).
- Query objects carry RE2 line patterns, file/repo/tag filters and negations, filename-only mode, and context lines (`tmp/competitors/livegrep__livegrep/src/codesearch.h:109`).
- Runtime has explicit timeout, thread, line-limit, and index/compression flags (`tmp/competitors/livegrep__livegrep/src/codesearch.cc:50`).
- SearchLimiter enforces timeouts and max matches (`tmp/competitors/livegrep__livegrep/src/codesearch.cc:98`).
- File/tree positive and negative filters are applied before matching (`tmp/competitors/livegrep__livegrep/src/codesearch.cc:155`).
- Chunks build suffix arrays with `divsufsort`; filenames get a suffix array too (`tmp/competitors/livegrep__livegrep/src/chunk.cc:68`, `tmp/competitors/livegrep__livegrep/src/codesearch.cc:488`).
- Query planning walks RE2 programs with selectivity constants and limits on recursion/program width (`tmp/competitors/livegrep__livegrep/src/query_planner.cc:23`, `tmp/competitors/livegrep__livegrep/src/query_planner.cc:181`).
- Chunk search chooses filtered suffix-array search when a plan exists, otherwise full search (`tmp/competitors/livegrep__livegrep/src/codesearch.cc:652`).
- Suffix search walks the query plan over the suffix array and caps candidate explosions (`tmp/competitors/livegrep__livegrep/src/codesearch.cc:692`).
- gRPC search rejects large regex programs/width and does tags-first exact/prefix search before corpus search when the query looks symbol-like (`tmp/competitors/livegrep__livegrep/src/tools/grpc_server.cc:296`, `tmp/competitors/livegrep__livegrep/src/tools/grpc_server.cc:329`, `tmp/competitors/livegrep__livegrep/src/tools/grpc_server.cc:386`).

### `sourcebot-dev__sourcebot`

Serious adjacent platform. Sourcebot mostly proves how to package Zoekt for agents and teams.

- README advertises multi-repo/branch search, regex, filters, boolean syntax, and IDE-level code navigation (`tmp/competitors/sourcebot-dev__sourcebot/README.md:42`).
- Its AGENTS doc identifies vendored Go Zoekt under `vendor/zoekt`, a zoekt service, web app, backend worker, MCP watcher, and universal-ctags symbol generation (`tmp/competitors/sourcebot-dev__sourcebot/AGENTS.md:9`, `tmp/competitors/sourcebot-dev__sourcebot/AGENTS.md:23`, `tmp/competitors/sourcebot-dev__sourcebot/AGENTS.md:45`).
- Query grammar includes boolean syntax and prefixes such as `rev`, `content`, `context`, `file`, `repo`, `lang`, `sym`, and `reposet` (`tmp/competitors/sourcebot-dev__sourcebot/packages/queryLanguage/src/query.grammar:17`, `tmp/competitors/sourcebot-dev__sourcebot/packages/queryLanguage/src/query.grammar:41`).
- RepoIndexManager owns git working copies, shards, async BullMQ jobs, re-index intervals, distributed locks, and garbage collection (`tmp/competitors/sourcebot-dev__sourcebot/packages/backend/src/repoIndexManager.ts:37`, `tmp/competitors/sourcebot-dev__sourcebot/packages/backend/src/repoIndexManager.ts:63`, `tmp/competitors/sourcebot-dev__sourcebot/packages/backend/src/repoIndexManager.ts:99`).
- It schedules indexing by staleness, active/failed jobs, and thresholds, and cleans disconnected repos after a grace period (`tmp/competitors/sourcebot-dev__sourcebot/packages/backend/src/repoIndexManager.ts:109`, `tmp/competitors/sourcebot-dev__sourcebot/packages/backend/src/repoIndexManager.ts:174`).
- Zoekt indexing shells out to `zoekt-git-index` with branches, index dir, max trigram count, file limits, tenant/repo IDs, shard prefixes, and large-file settings (`tmp/competitors/sourcebot-dev__sourcebot/packages/backend/src/zoekt.ts:11`).
- Search API scopes accessible repos, parses the string query into IR, creates Zoekt requests, runs search or stream search, and audits calls (`tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/search/searchApi.ts:31`, `tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/search/searchApi.ts:62`).
- Parser keeps regex syntax distinct from plain terms and expands search contexts from DB (`tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/search/parser.ts:33`, `tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/search/parser.ts:77`, `tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/search/parser.ts:203`).
- ZoektSearcher defaults to HEAD branch when no `rev:` is supplied, supports `repo_set`, chunk matches, context lines, whole-file mode, wall-time controls, and `total_max_match_count = display + 1` to detect truncation (`tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/search/zoektSearcher.ts:28`, `tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/search/zoektSearcher.ts:68`).
- Streaming search transforms Zoekt gRPC streams into SSE chunks while accumulating stats and repository info (`tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/search/zoektSearcher.ts:150`).
- MCP server registers grep, glob, diff, commits, repos, readFile, listTree, symbol definitions, and symbol references; `ask_codebase` is enabled only when models are configured and returns an answer plus research session link (`tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/mcp/server.ts:29`, `tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/mcp/server.ts:72`).
- MCP grep wraps Sourcebot search by building query strings from regex, path, repo, ref, and reposet inputs, and returns metadata with files/query/match counts/repo counts (`tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/tools/grep.ts:77`, `tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/tools/grep.ts:161`).
- Symbol definition/reference tools are search-based wrappers (`tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/tools/findSymbolDefinitions.ts:28`, `tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/tools/findSymbolReferences.ts:38`).

### `sourcegraph__cody-public-snapshot`

Adjacent retrieval system. It is valuable for context assembly, not index design.

- ContextRetriever resolves codebase roots, can rewrite queries through an LLM, and retrieves local/live and indexed/remote context in parallel (`tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/chat/chat-view/ContextRetriever.ts:171`, `tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/chat/chat-view/ContextRetriever.ts:199`, `tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/chat/chat-view/ContextRetriever.ts:217`).
- Local modified files are searched live with symf and then used to filter stale indexed/remote context for the same files (`tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/chat/chat-view/ContextRetriever.ts:247`, `tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/chat/chat-view/ContextRetriever.ts:256`).
- Remote context comes from `graphqlClient.contextSearch` and carries source/repo/revision/range metadata (`tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/chat/chat-view/ContextRetriever.ts:371`, `tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/chat/chat-view/ContextRetriever.ts:417`).
- Local indexed context uses `symf`, index locks, retry-on-index-not-found, scopes, JSON output, and boosted keywords (`tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/local-context/symf.ts:130`, `tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/local-context/symf.ts:175`, `tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/local-context/symf.ts:300`).
- Chat context composes explicit mentions, OpenCtx, and retrieved context in a fixed order (`tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/chat/chat-view/handlers/ChatHandler.ts:281`, `tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/chat/chat-view/handlers/ChatHandler.ts:336`).
- The `code_search` tool is a wrapper over ContextRetriever and can skip query rewrite (`tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/chat/chat-view/tools/search.ts:21`).
- Context de-duplication compares path, ranges, content, and whether the item was user-added (`tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/prompt-builder/unique-context.ts:14`, `tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/prompt-builder/unique-context.ts:53`).

### `DEFENSE-SEU__Code2MCP`

Thin wrapper / adjacent generator. Not a code-search system.

- README describes transforming code repos into MCP services with generation, environment simulation, smoke tests, and deployment (`tmp/competitors/DEFENSE-SEU__Code2MCP/README.md:28`, `tmp/competitors/DEFENSE-SEU__Code2MCP/README.md:89`).
- Analysis node uses gitingest, LLM, DeepWiki, and a shallow AST scan of Python symbols/signatures (`tmp/competitors/DEFENSE-SEU__Code2MCP/src/nodes/analysis_node.py:1`, `tmp/competitors/DEFENSE-SEU__Code2MCP/src/nodes/analysis_node.py:32`, `tmp/competitors/DEFENSE-SEU__Code2MCP/src/nodes/analysis_node.py:149`, `tmp/competitors/DEFENSE-SEU__Code2MCP/src/nodes/analysis_node.py:318`).
- Generate node prompts an LLM to produce FastMCP service code and has fallback/direct wrapper generation (`tmp/competitors/DEFENSE-SEU__Code2MCP/src/nodes/generate_node.py:149`, `tmp/competitors/DEFENSE-SEU__Code2MCP/src/nodes/generate_node.py:295`).
- There is no persistent code-search index, regex query engine, symbol ranking, xref store, or multi-repo search architecture.

### `DEFENSE-SEU__MCP-Github-Agent`

No-op theater in this clone.

- README says the repository moved to `https://github.com/DEFENSE-SEU/Code2MCP` (`tmp/competitors/DEFENSE-SEU__MCP-Github-Agent/README.md:1`, `tmp/competitors/DEFENSE-SEU__MCP-Github-Agent/README.md:8`).
- No implementation is present to inspect here.

### `Ryandonofrio3__osgrep`

Serious adjacent semantic search engine, with an explicitly no-op MCP surface.

- Language support is broad and tree-sitter WASM-based, with per-language definition types (`tmp/competitors/Ryandonofrio3__osgrep/src/lib/core/languages.ts:11`).
- LanceDB schema stores dense vectors, ColBERT blobs/scales, pooled ColBERT vectors, token IDs, defined/referenced symbols, imports/exports, role, parent symbol, and skeleton (`tmp/competitors/Ryandonofrio3__osgrep/src/lib/store/vector-db.ts:21`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/store/vector-db.ts:86`).
- Graph queries are symbol-array based: callers are chunks whose referenced_symbols contain a symbol; callees resolve referenced symbols to definition rows (`tmp/competitors/Ryandonofrio3__osgrep/src/lib/graph/graph-builder.ts:21`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/graph/graph-builder.ts:41`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/graph/graph-builder.ts:61`).
- Search embeds query to dense, ColBERT, and pooled vectors; applies path/definition/reference/intent filters; overfetches vector candidates; falls back to FTS; fuses vector and FTS with reciprocal rank fusion; then does pooled cosine and ColBERT reranking with structure boosts and diversification (`tmp/competitors/Ryandonofrio3__osgrep/src/lib/search/searcher.ts:283`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/search/searcher.ts:323`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/search/searcher.ts:363`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/search/searcher.ts:391`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/search/searcher.ts:409`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/search/searcher.ts:433`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/search/searcher.ts:483`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/search/searcher.ts:526`).
- MCP server returns an empty tools list and `Not implemented` for tool calls, despite a background sync loop (`tmp/competitors/Ryandonofrio3__osgrep/src/commands/mcp.ts:66`, `tmp/competitors/Ryandonofrio3__osgrep/src/commands/mcp.ts:81`).

### `edelauna__github-semantic-search-mcp`

Adjacent GitHub-backed semantic search. Useful hosted-sync ideas, not exact search.

- Cloudflare Worker exposes `/mcp` GET/POST and workflows for embedding, indexing, and scanning (`tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/index.ts:4`, `tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/index.ts:45`).
- GitHub access uses batched GraphQL tree queries, tokens from workflow state, retry/backoff, and blob content fetch by oid (`tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/steps/github.step.ts:11`, `tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/steps/github.step.ts:24`, `tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/steps/github.step.ts:60`, `tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/steps/github.step.ts:107`).
- Embedding workflow queues chunks, reads/writes chunk text through R2, runs Cloudflare AI embeddings, stores vectors in Vectorize, and marks chunks processed (`tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/steps/embed.step.ts:9`, `tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/steps/embed.step.ts:31`, `tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/steps/embed.step.ts:84`).
- Vector service mirrors vector IDs in D1 and Vectorize (`tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/services/vector.service.ts:4`, `tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/services/vector.service.ts:23`).
- Tool checks token length and repo access, triggers initial/reindex workflows if needed, embeds the query, runs Vectorize `topK: 5`, filters by owner/repo/branch, reads chunk content from R2, generates GitHub URLs, and sorts by score (`tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/handlers/tools/github-semantic-search/github-semantic-search.tool.ts:12`, `tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/handlers/tools/github-semantic-search/github-semantic-search.tool.ts:32`, `tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/handlers/tools/github-semantic-search/github-semantic-search.tool.ts:44`, `tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/handlers/tools/github-semantic-search/github-semantic-search.tool.ts:64`, `tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/handlers/tools/github-semantic-search/github-semantic-search.tool.ts:78`).

### `dudufcb1__codebase-index-cli`

Adjacent vector indexer. Useful local storage/chunking ideas, not canonical exact search.

- WorkspaceIndexer coordinates embedder, vector store, cache, ignore rules, scanner, file watcher, git watcher, and optional commit LLM (`tmp/competitors/dudufcb1__codebase-index-cli/src/indexer.ts:20`).
- It initializes sqlite or Qdrant vector stores, performs initial full scan, starts file watching, and can index/analyze git commits (`tmp/competitors/dudufcb1__codebase-index-cli/src/indexer.ts:35`, `tmp/competitors/dudufcb1__codebase-index-cli/src/indexer.ts:124`, `tmp/competitors/dudufcb1__codebase-index-cli/src/indexer.ts:137`, `tmp/competitors/dudufcb1__codebase-index-cli/src/indexer.ts:181`).
- Tree-sitter support uses WASM grammars by file path, parser caching, reset every 100 files, and many language query packs (`tmp/competitors/dudufcb1__codebase-index-cli/src/services/tree-sitter/languageParser.ts:48`, `tmp/competitors/dudufcb1__codebase-index-cli/src/services/tree-sitter/languageParser.ts:67`, `tmp/competitors/dudufcb1__codebase-index-cli/src/services/tree-sitter/languageParser.ts:94`, `tmp/competitors/dudufcb1__codebase-index-cli/src/services/tree-sitter/languageParser.ts:133`).
- sqlite-vec store opens `.codebase/vectors.db`, enables WAL, creates a `vec0` virtual table, upserts chunks, and searches with `embedding MATCH ?` (`tmp/competitors/dudufcb1__codebase-index-cli/src/vectorStore/sqliteVecClient.ts:11`, `tmp/competitors/dudufcb1__codebase-index-cli/src/vectorStore/sqliteVecClient.ts:25`, `tmp/competitors/dudufcb1__codebase-index-cli/src/vectorStore/sqliteVecClient.ts:76`, `tmp/competitors/dudufcb1__codebase-index-cli/src/vectorStore/sqliteVecClient.ts:139`, `tmp/competitors/dudufcb1__codebase-index-cli/src/vectorStore/sqliteVecClient.ts:189`).
- Qdrant collection uses cosine distance, on-disk storage, HNSW tuning, and recreates on dimension mismatch (`tmp/competitors/dudufcb1__codebase-index-cli/src/vectorStore/qdrantVectorStore.ts:119`, `tmp/competitors/dudufcb1__codebase-index-cli/src/vectorStore/qdrantVectorStore.ts:178`).
- CLI `semantic-search` requires Qdrant, even though sqlite storage exists elsewhere (`tmp/competitors/dudufcb1__codebase-index-cli/src/index.ts:133`).

### `dudufcb1__semantic-search`

Adjacent / thin semantic MCP. Real enough to study for output formatting, but not exact search.

- SQLite server lazily initializes FastMCP, embedder, Qdrant store, and storage resolver (`tmp/competitors/dudufcb1__semantic-search/src/server_sqlite.py:40`, `tmp/competitors/dudufcb1__semantic-search/src/server_sqlite.py:51`).
- SQLite search loads `sqlite-vec`, runs `embedding MATCH ?`, orders by distance, and converts distance to score (`tmp/competitors/dudufcb1__semantic-search/src/server_sqlite.py:358`, `tmp/competitors/dudufcb1__semantic-search/src/server_sqlite.py:384`, `tmp/competitors/dudufcb1__semantic-search/src/server_sqlite.py:403`, `tmp/competitors/dudufcb1__semantic-search/src/server_sqlite.py:425`).
- SQLite MCP `semantic_search` validates workspace and `.codebase/vectors.db`, embeds the query, searches, merges chunks by file, and optionally generates an LLM brief (`tmp/competitors/dudufcb1__semantic-search/src/server_sqlite.py:446`, `tmp/competitors/dudufcb1__semantic-search/src/server_sqlite.py:498`, `tmp/competitors/dudufcb1__semantic-search/src/server_sqlite.py:522`, `tmp/competitors/dudufcb1__semantic-search/src/server_sqlite.py:542`, `tmp/competitors/dudufcb1__semantic-search/src/server_sqlite.py:561`, `tmp/competitors/dudufcb1__semantic-search/src/server_sqlite.py:573`).
- Qdrant server loads collection name from state and has optional Voyage reranker setup (`tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:102`, `tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:79`).
- Qdrant MCP `semantic_search` embeds the query, searches Qdrant, optionally reranks with Voyage, merges chunks by file, and paginates output (`tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:490`, `tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:577`, `tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:591`, `tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:620`, `tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:652`, `tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:676`).
- `semantic_parallel_search` accepts agent-supplied query variants, embeds them concurrently, runs Qdrant searches in parallel, deduplicates by file/range, tracks which query returned each file, and can rerank (`tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:745`, `tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:826`, `tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:842`, `tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:862`, `tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:902`, `tmp/competitors/dudufcb1__semantic-search/src/server_qdrant.py:956`).
- Chunk merger validates chunks against the live file, can show full files vs fragments, skips likely compiled/minified artifacts, and prints omitted-line gaps (`tmp/competitors/dudufcb1__semantic-search/src/chunk_merger.py:7`, `tmp/competitors/dudufcb1__semantic-search/src/chunk_merger.py:33`, `tmp/competitors/dudufcb1__semantic-search/src/chunk_merger.py:77`, `tmp/competitors/dudufcb1__semantic-search/src/chunk_merger.py:177`).
- Voyage reranker replaces vector scores with relevance scores and filters empty chunks defensively (`tmp/competitors/dudufcb1__semantic-search/src/voyage_reranker.py:17`, `tmp/competitors/dudufcb1__semantic-search/src/voyage_reranker.py:58`, `tmp/competitors/dudufcb1__semantic-search/src/voyage_reranker.py:80`, `tmp/competitors/dudufcb1__semantic-search/src/voyage_reranker.py:120`).

### `vrppaul__semantic-code-mcp`

Adjacent semantic MCP. Cleaner than many wrappers, but small and not Sourcegraph-lineage.

- FastMCP exposes `search_code`, `index_codebase`, and `index_status`; search auto-indexes stale files and returns structured debug timings/stats/status (`tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/server.py:30`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/server.py:76`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/server.py:113`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/server.py:159`).
- SearchService checks index status, auto-indexes missing/stale files, embeds query, does hybrid vector+FTS search, filters by min score, applies a small recency boost, and groups by file (`tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/search_service.py:54`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/search_service.py:83`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/search_service.py:120`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/search_service.py:125`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/search_service.py:135`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/search_service.py:159`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/search_service.py:181`).
- IndexService scans with `git ls-files` where possible, falls back to directory walk with ignores, detects changes, chunks, embeds, stores, and updates file-change cache after successful store (`tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/index_service.py:48`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/index_service.py:70`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/index_service.py:87`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/index_service.py:107`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/index_service.py:135`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/index_service.py:155`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/index_service.py:221`).
- Python chunker extracts module docstrings, functions, decorated definitions, classes, and methods with tree-sitter (`tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/chunkers/python.py:25`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/chunkers/python.py:31`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/chunkers/python.py:73`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/chunkers/python.py:100`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/chunkers/python.py:126`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/chunkers/python.py:178`).
- LanceDB storage supports vector search, FTS search, and weighted hybrid merging by file/line key (`tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/storage/lancedb.py:151`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/storage/lancedb.py:189`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/storage/lancedb.py:231`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/storage/lancedb.py:262`).

## Strong ideas Loom should steal

### P0

- Add or benchmark against a Zoekt-class exact search path: positional trigrams, boolean query AST, fielded filters, regex-to-substring candidate extraction, positive atom requirement, shard partial evaluation, and bounded stats. Evidence: `tmp/competitors/sourcegraph__zoekt/doc/design.md:26`, `tmp/competitors/sourcegraph__zoekt/doc/design.md:41`, `tmp/competitors/sourcegraph__zoekt/doc/design.md:211`, `tmp/competitors/sourcegraph__zoekt/index/eval.go:34`.
- Make ranking transparent and tunable: word boundary, basename, symbol exact/edge/overlap, symbol kind, file category penalties, repo rank, and optional BM25-style scoring. Evidence: `tmp/competitors/sourcegraph__zoekt/index/score.go:93`, `tmp/competitors/sourcegraph__zoekt/index/score.go:249`, `tmp/competitors/sourcegraph__zoekt/index/score.go:300`.
- Return bounded, structured results with explicit truncation proof. Sourcebot's `display + 1` max-count trick is simple and agent-friendly. Evidence: `tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/search/zoektSearcher.ts:68`.
- Design MCP tools as first-class products: grep/read/tree/symbol refs/defs with metadata, query string, match counts, repo counts, and sources. Evidence: `tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/mcp/server.ts:29`, `tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/tools/grep.ts:161`.
- Overlay local dirty files over indexed context, and remove stale indexed hits for files modified locally. Evidence: `tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/chat/chat-view/ContextRetriever.ts:247`, `tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/chat/chat-view/ContextRetriever.ts:256`.

### P1

- Use shard/file watchers with versioned hot reload, not restart-only indexing. Evidence: `tmp/competitors/sourcegraph__zoekt/search/watcher.go:120`.
- Add scheduler fairness: interactive queries get capacity, slow queries downgrade to batch. Evidence: `tmp/competitors/sourcegraph__zoekt/search/sched.go:59`.
- Support multi-branch and multi-repo cheaply. Branch bitmasks and repo-set filters beat duplicating near-identical branch indexes. Evidence: `tmp/competitors/sourcegraph__zoekt/doc/design.md:115`, `tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/search/zoektSearcher.ts:28`.
- Add tags/symbol-first path for symbol-looking queries before broad corpus search. Evidence: `tmp/competitors/livegrep__livegrep/src/tools/grpc_server.cc:296`, `tmp/competitors/livegrep__livegrep/src/tools/grpc_server.cc:386`.
- Keep an explicit regex safety envelope: timeout, max matches, query plan limits, and regex program rejection. Evidence: `tmp/competitors/livegrep__livegrep/src/codesearch.cc:98`, `tmp/competitors/livegrep__livegrep/src/query_planner.cc:23`, `tmp/competitors/livegrep__livegrep/src/tools/grpc_server.cc:329`.
- For beyond-grep mode, borrow osgrep's hybrid search ladder: vector + FTS, reciprocal rank fusion, pooled cosine filter, ColBERT rerank, structure boost, dedup/diversification. Evidence: `tmp/competitors/Ryandonofrio3__osgrep/src/lib/search/searcher.ts:391`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/search/searcher.ts:409`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/search/searcher.ts:433`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/search/searcher.ts:483`, `tmp/competitors/Ryandonofrio3__osgrep/src/lib/search/searcher.ts:526`.
- Offer a portable sqlite-vec storage mode for local/dev, even if a faster backend exists later. Evidence: `tmp/competitors/dudufcb1__codebase-index-cli/src/vectorStore/sqliteVecClient.ts:76`, `tmp/competitors/dudufcb1__semantic-search/src/server_sqlite.py:403`.

### P2

- Use tree-sitter WASM parser caching and memory reset patterns if Loom wants broad language support with fewer native dependency headaches. Evidence: `tmp/competitors/dudufcb1__codebase-index-cli/src/services/tree-sitter/languageParser.ts:67`, `tmp/competitors/dudufcb1__codebase-index-cli/src/services/tree-sitter/languageParser.ts:94`.
- Keep source metadata on every context result: source, repo, revision, ranges, URI. Evidence: `tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/chat/chat-view/ContextRetriever.ts:417`.
- De-duplicate context by file/range/content/user-added status before feeding agents. Evidence: `tmp/competitors/sourcegraph__cody-public-snapshot/vscode/src/prompt-builder/unique-context.ts:53`.
- Use `git ls-files` as a fast scan path with a fallback walker and ignore handling. Evidence: `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/index_service.py:107`, `tmp/competitors/vrppaul__semantic-code-mcp/src/semantic_code_mcp/services/index_service.py:135`.
- For hosted GitHub mode only, study batched GraphQL tree/blob fetch plus async embedding workflows. Evidence: `tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/steps/github.step.ts:24`, `tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/steps/embed.step.ts:84`.

## Weak/no-op repos and why

- `DEFENSE-SEU__MCP-Github-Agent`: no-op in this clone. The README is only a move notice to Code2MCP, with no implementation to inspect (`tmp/competitors/DEFENSE-SEU__MCP-Github-Agent/README.md:1`).
- `DEFENSE-SEU__Code2MCP`: thin adjacent generator. It analyzes a repo and asks an LLM to produce MCP wrappers, but it has no persistent search index, regex engine, ranking model, xrefs, or multi-repo search (`tmp/competitors/DEFENSE-SEU__Code2MCP/src/nodes/generate_node.py:149`).
- `Ryandonofrio3__osgrep` MCP mode: no-op wrapper. The underlying search engine is real, but MCP `tools/list` returns `[]` and `tools/call` returns `Not implemented` (`tmp/competitors/Ryandonofrio3__osgrep/src/commands/mcp.ts:66`).
- `google__zoekt`: historical pointer in this checkout, not a current source competitor. README directs readers to Sourcegraph Zoekt (`tmp/competitors/google__zoekt/README.md:1`).
- `edelauna__github-semantic-search-mcp`: adjacent cloud semantic toy compared to Sourcegraph-lineage search. `topK: 5`, branch-scoped Vectorize search is not a canonical exact/beyond-grep baseline (`tmp/competitors/edelauna__github-semantic-search-mcp/workflow/src/handlers/tools/github-semantic-search/github-semantic-search.tool.ts:64`).

## What Loom should avoid

- Do not ship MCP endpoints that expose zero tools. `osgrep` proves this reads as theater even when the underlying engine is interesting (`tmp/competitors/Ryandonofrio3__osgrep/src/commands/mcp.ts:66`).
- Do not confuse LLM wrapper generation with code search. Code2MCP is useful for demos, not for indexed retrieval (`tmp/competitors/DEFENSE-SEU__Code2MCP/src/nodes/analysis_node.py:149`).
- Do not make semantic/vector search the only path. Exact string, regex, path, symbol, and branch filters are table stakes for this competitor class.
- Do not hide truncation. Sourcebot's explicit over-limit detection is better than returning a pretty incomplete answer (`tmp/competitors/sourcebot-dev__sourcebot/packages/web/src/features/search/zoektSearcher.ts:68`).
- Do not make ctags/symbol extraction all-or-nothing. Sourcebot continues indexing without ctags symbol data; Loom should degrade similarly (`tmp/competitors/sourcebot-dev__sourcebot/AGENTS.md:45`).
- Do not build an OpenGrok-sized appliance if Loom's north-star remains useful symbols per token. OpenGrok's xref/history surface is strong, but its operational model is heavier than an agent-native local MCP (`tmp/competitors/oracle__opengrok/docker/README.md:81`).
- Do not log indexed source content. Many competitors print operational logs freely; Loom's local-only/security posture should be stricter.
- Do not expose storage-mode contradictions. `codebase-index-cli` supports sqlite-vec but its CLI semantic search requires Qdrant, which is exactly the sort of papercut that makes tools feel unserious (`tmp/competitors/dudufcb1__codebase-index-cli/src/index.ts:133`).

## Gaps/open questions

- Need real benchmark numbers on Loom vs Zoekt/livegrep/OpenGrok for exact text, regex, path filters, symbol-looking queries, and multi-repo scoped queries.
- Need decide whether Loom should embed a Zoekt-like trigram index directly, call out to Zoekt as a baseline tool, or implement only enough exact search to route into Loom neighborhoods.
- Need define Loom's query language. Sourcebot's grammar is a good compatibility target, but Loom may want explicit modes for exact, regex, symbol, related, impact, and neighborhood.
- Need decide how much ctags vs tree-sitter symbol extraction to keep. Zoekt and Sourcebot lean on ctags; Loom already has parser ambitions and should not inherit ctags' unevenness without a fallback.
- Need test branch-aware storage on large monorepos. Zoekt's branch bitmasks are attractive, but Loom's graph/symbol store may need a separate revision model.
- Need define evidence/citation payloads for MCP results: local file refs, line ranges, query used, index revision, truncated/exhaustive flag, and source backend.
- Need decide whether semantic rerankers belong in core. osgrep's ladder is strong, but hosted/API rerankers from the smaller repos are optional, not a local-first default.
