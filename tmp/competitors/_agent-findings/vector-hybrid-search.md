# Vector/Hybrid Search Competitor Teardown

## Executive Verdict

The serious threat is not "vector search." Everyone can bolt embeddings onto chunks now. The threat is the full loop: code-shaped chunks, local or controllable storage, dense+sparse fusion, incremental sync, and evidence-rich responses that keep an agent out of shell. By that standard, the strongest competitors in this slice are `MinishLab__semble`, `BeaconBay__ck`, `elastic__semantic-code-search-*`, `zilliztech__claude-context`, `bobmatnyc__mcp-vector-search`, `mhalder__qdrant-mcp-server`, and parts of `m1rl0k__Context-Engine`.

Loom's clean advantage should remain local-first, bounded, structured, symbol-aware neighborhoods. Several repos return either raw chunks without enough relationships or pretty metadata without trustworthy retrieval. The best things to steal are boring and powerful: chunk/location separation, RRF hybrid defaults, tokenizer/index version stamps, path-safe evidence reconstruction, deletion-correct incremental sync, and response contracts with component scores and "why this result" fields. Boring wins. The abstraction parade can go stand in traffic.

## Repo Manifest

| Repo | Inferred URL | Classification |
|---|---|---|
| `tmp/competitors/zilliztech__claude-context` | `https://github.com/zilliztech/claude-context.git` | serious competitor |
| `tmp/competitors/FarhanAliRaza__claude-context-local` | `https://github.com/FarhanAliRaza/claude-context-local.git` | adjacent tool |
| `tmp/competitors/elastic__semantic-code-search-indexer` | `https://github.com/elastic/semantic-code-search-indexer.git` | serious competitor |
| `tmp/competitors/elastic__semantic-code-search-mcp-server` | `https://github.com/elastic/semantic-code-search-mcp-server.git` | serious competitor |
| `tmp/competitors/mixedbread-ai__mgrep` | `https://github.com/mixedbread-ai/mgrep.git` | adjacent tool; MCP side is no-op theater |
| `tmp/competitors/yoanbernabeu__grepai` | `https://github.com/yoanbernabeu/grepai.git` | serious competitor |
| `tmp/competitors/bobmatnyc__mcp-vector-search` | `https://github.com/bobmatnyc/mcp-vector-search.git` | serious competitor |
| `tmp/competitors/BeaconBay__ck` | `https://github.com/BeaconBay/ck.git` | serious competitor |
| `tmp/competitors/MinishLab__semble` | `https://github.com/MinishLab/semble.git` | serious competitor |
| `tmp/competitors/yichuan-w__LEANN` | `https://github.com/yichuan-w/LEANN.git` | adjacent retrieval infra |
| `tmp/competitors/tecnomanu__pampa` | `https://github.com/tecnomanu/pampa.git` | serious-ish competitor |
| `tmp/competitors/m1rl0k__Context-Engine` | `https://github.com/m1rl0k/Context-Engine.git` | serious but overgrown |
| `tmp/competitors/Wildcard-Official__deepcontext-mcp` | `https://github.com/Wildcard-Official/deepcontext-mcp.git` | adjacent cloud-backed tool |
| `tmp/competitors/jghiringhelli__codeseeker` | `https://github.com/jghiringhelli/codeseeker.git` | thin/noisy wrapper with some real pieces |
| `tmp/competitors/st3v3nmw__sourcerer-mcp` | `https://github.com/st3v3nmw/sourcerer-mcp.git` | adjacent tool |
| `tmp/competitors/mhalder__qdrant-mcp-server` | `https://github.com/mhalder/qdrant-mcp-server.git` | serious competitor |
| `tmp/competitors/steiner385__qdrant-mcp-server` | `https://github.com/steiner385/qdrant-mcp-server.git` | thin wrapper |

## Source-Level Findings

### `zilliztech__claude-context` - Serious Competitor

Milvus is the retrieval engine, and the repo has actual hybrid mode rather than a search checkbox. It creates dense vector fields, sparse vector fields, BM25 analyzer functions, and separate indexes for hybrid collections in `packages/core/src/vectordb/milvus-restful-vectordb.ts:525-616`. Hybrid search sends dense and sparse requests together to Milvus's hybrid endpoint, using sparse `metricType: "BM25"` and a configurable rerank strategy in `packages/core/src/vectordb/milvus-restful-vectordb.ts:697-802`.

Chunking is AST-aware with subchunking and overlap in `packages/core/src/splitter/ast-splitter.ts:184-242`. Sync is not hand-wavy: `packages/mcp/src/sync.ts:159-263` wraps indexing in a global lock, `packages/mcp/src/sync.ts:276-310` schedules periodic sync, and `packages/mcp/src/sync.ts:332-398` watches a trigger file. Hybrid mode is documented as an environment-level default in `docs/getting-started/environment-variables.md:62`.

The catch: privacy depends on where Milvus/Zilliz lives, and the vector DB layer logs query substrings around failures in `packages/core/src/vectordb/milvus-restful-vectordb.ts:744-754`. For Loom, this is the benchmark for "hybrid really means hybrid," not for local-only handling.

### `FarhanAliRaza__claude-context-local` - Adjacent Tool

This is a local FAISS fork, useful as a baseline but not a hybrid competitor. `search/indexer.py:17-35` wires FAISS plus `sqlitedict`; `search/indexer.py:73-99` chooses `IndexFlatIP` or IVF; `search/indexer.py:101-134` normalizes vectors and stores metadata. `search/searcher.py:67-91` explicitly routes exact-name work to grep instead of solving it with hybrid retrieval, and `search/searcher.py:108-135` does embedding search with oversampling.

Incremental indexing exists via Merkle snapshots in `search/incremental_indexer.py:81-155`, but deletion is suspect: `search/indexer.py:271-309` removes chunk metadata without actually compacting/removing vectors from FAISS. Chunking has tree-sitter wrappers in `chunking/base_chunker.py:114-177` and language dispatch in `chunking/multi_language_chunker.py:97-138`, but metadata quality is sparse. The MCP server returns JSON with file, line, kind, score, chunk id, and snippet in `mcp_server/code_search_server.py:155-250`.

### `elastic__semantic-code-search-indexer` - Serious Competitor

Elastic's indexer has the best storage model in the set: content chunks are deduplicated separately from file-location occurrences. The main index maps `semantic_text`, `code_vector`, nested imports/symbols/exports, and code metadata in `src/utils/elasticsearch.ts:190-256`; the companion location index stores file path, line numbers, repo, directory, and git metadata in `src/utils/elasticsearch.ts:286-312`. Bulk indexing upserts one chunk doc plus one or more location docs in `src/utils/elasticsearch.ts:499-760`.

Incremental indexing is production-shaped. `src/utils/sqlite_queue.ts:71-120` sets up WAL-backed queue tables; `src/utils/sqlite_queue.ts:208-241` atomically claims work with `UPDATE ... RETURNING`; `src/utils/indexer_worker.ts:52-72` requeues stale jobs and claims batches; `src/commands/incremental_index_command.ts:44-80` derives changed files from last indexed commit/git diff; `src/commands/incremental_index_command.ts:86-121` handles renames, copies, deletes, adds, and modifies.

Weakness for Loom's north star: it is enterprise Elastic-dependent and heavier than a local SQLite index. Strength to steal: chunk/location split and queue semantics.

### `elastic__semantic-code-search-mcp-server` - Serious Competitor

The MCP layer is evidence-oriented. `src/mcp_server/server.ts:70-135` registers semantic search, symbol mapping, symbol analysis, read-file reconstruction, and document-symbol tools. `tools/semantic_code_search.ts:44-73` validates query/KQL routing, `tools/semantic_code_search.ts:112-154` pages semantic candidates with bounded search, and `tools/semantic_code_search.ts:156-183` joins location evidence into each result.

The strongest agent feature is file reconstruction from indexed chunks: `tools/read_file.ts:56-94` pages locations, `tools/read_file.ts:97-117` fetches chunks, and `tools/read_file.ts:119-173` sorts/dedupes spans and returns gap/missing warnings. Symbol tools expose imports/exports/calls rather than naked text matches in `tools/map_symbols_by_query.ts:100-223` and `tools/symbol_analysis.ts:118-223`.

### `mixedbread-ai__mgrep` - Adjacent Tool; MCP No-Op Theater

`mgrep` is a Mixedbread SaaS client, not a local code-intelligence engine. Store creation and search route through `https://api.mixedbread.com` in `src/lib/context.ts:12-37`, with upload/search/ask delegated to the SDK in `src/lib/store.ts:150-260`. File collection uses git-aware traversal and ignore files in `src/lib/file.ts:79-207`, and sync does metadata comparison plus high-concurrency upload in `src/lib/utils.ts:278-452`.

The CLI search formatting is polished: citation extraction and path/line/content output live in `src/commands/search.ts:29-128`, while options include rerank, web, dry-run, sync, and agentic mode in `src/commands/search.ts:193-346`. But the MCP watcher is theater: `src/commands/watch_mcp.ts:50-72` returns `tools: []`, and `src/commands/watch_mcp.ts:76-93` says calls are not implemented.

### `yoanbernabeu__grepai` - Serious Competitor

`grepai` is a compact Go implementation with local or Qdrant storage. Chunking is line/token-based with context prefixes in `indexer/chunker.go:11-15`, `indexer/chunker.go:59-130`, and `indexer/chunker.go:159-170`. Incremental indexing uses mtime/hash skips, deletes removed files, embeds batches, and stores chunk hash/content hash in `indexer/indexer.go:98-215` and `indexer/indexer.go:230-292`.

Hybrid search is RRF over vector results plus a text scan in `search/search.go:29-85` and `search/hybrid.go:11-89`. It is not BM25: text search is token/contains logic, with a fixed RRF `k=60`. Qdrant storage creates cosine collections and payload indexes in `store/qdrant.go:45-106`, upserts UUID-stable chunk IDs in `store/qdrant.go:122-151`, and supports path-prefix post-filtering in `store/qdrant.go:223-275`. MCP output supports JSON/TOON and trace-ish tools in `mcp/server.go:39-58` and `mcp/server.go:166-260`.

Good competitor, rough hybrid. It will miss exact code identifiers that proper BM25/sparse vectors catch.

### `bobmatnyc__mcp-vector-search` - Serious Competitor

This is the "everything bagel" competitor. LanceDB stores a rich schema including vector, content, file, line range, language, chunk type, symbol metadata, imports/calls/inherits, complexity, subproject, git, and quality fields in `src/mcp_vector_search/core/lancedb_backend.py:38-100`. Search supports vector, BM25, hybrid alpha, reranking, MMR, expansion, path/test filters, and timeouts in `src/mcp_vector_search/core/search.py:67-159`; identifier queries lower hybrid alpha to favor BM25 in `src/mcp_vector_search/core/search.py:258-283`.

The BM25 layer has a code-ish tokenizer that preserves dotted, hyphenated, and slashed tokens and splits snake/camel case in `src/mcp_vector_search/core/bm25_backend.py:29-40`, builds weighted docs from content/name/path/chunk type in `src/mcp_vector_search/core/bm25_backend.py:66-154`, and version-stamps tokenizer compatibility in `src/mcp_vector_search/core/bm25_backend.py:215-300`. MMR is implemented in `src/mcp_vector_search/core/mmr.py:98-158`. Multiprocess chunking and incremental indexing are in `src/mcp_vector_search/core/chunk_processor.py:38-240` and `src/mcp_vector_search/core/indexer.py:2327-2635`.

The dedicated hybrid handler runs semantic, text, and graph strategies concurrently, fuses with RRF, and returns per-strategy scores/counts/warnings in `src/mcp_vector_search/mcp/hybrid_search_handler.py:102-228`. Strong ideas, but the product surface is swollen.

### `BeaconBay__ck` - Serious Competitor

`ck` is a strong local Rust competitor. Semantic search loads sidecar embeddings, embeds queries, brute-force ranks cosine over chunks, extracts source previews from file spans, skips stale files, and optionally reranks in `ck-engine/src/semantic_v3.rs:20-132` and `ck-engine/src/semantic_v3.rs:215-277`. Lexical search uses Tantivy in `ck-engine/src/lib.rs:729-845`, builds a whole-file Tantivy index in `ck-engine/src/lib.rs:847-985`, and hybrid combines regex plus semantic using RRF keyed by `file:line_start` in `ck-engine/src/lib.rs:992-1059`.

Indexing reuses embeddings by chunk hash and only embeds misses in `ck-index/src/lib.rs:1088-1227` and `ck-index/src/lib.rs:1275-1318`. MCP tools are structured and include semantic, regex, lexical, and hybrid search with spans/content/scores in `ck-cli/src/mcp_server.rs:511-560` and `ck-cli/src/mcp_server.rs:715-875`.

Weakness: semantic search is still sidecar brute force, and hybrid fuses regex with semantic rather than chunk-level BM25. Still, the local/offline ergonomics are dangerous.

### `MinishLab__semble` - Serious Competitor

`semble` is the cleanest direct threat to Loom's pitch. It has compact AST chunking in `src/semble/chunking/core.py:29-140`, exact CPU embeddings via a static Model2Vec model in `src/semble/index/dense.py:11-79`, and BM25 over enriched code text in `src/semble/index/sparse.py:18-29`. Index creation wires file scanning, chunking, embedding, BM25, and a basic backend in `src/semble/index/create.py:17-60`.

Search is exactly the pattern Loom should benchmark against: RRF helper at `src/semble/search.py:11-19`, semantic and BM25 searches at `src/semble/search.py:22-67`, and hybrid overfetch/fusion/boosts/path penalties at `src/semble/search.py:70-122`. Ranking adapts alpha for symbol-like queries in `src/semble/ranking/weighting.py:3-11`, applies symbol/path/query boosts in `src/semble/ranking/boosting.py:9-120`, and penalizes tests/docs/compat/d.ts paths in `src/semble/ranking/penalties.py:6-120`.

MCP instructions explicitly tell agents to prefer tools over grep/read in `src/semble/mcp.py:49-60`; tools expose `search` and `find_related` in `src/semble/mcp.py:62-115`. Weakness: indexes are in-memory/cache-oriented, exact scan is fine until it is not, and watcher-loop errors are swallowed in `src/semble/mcp.py:132-202`.

### `yichuan-w__LEANN` - Adjacent Retrieval Infra

LEANN is serious retrieval infrastructure, not primarily an agent code-search product. It has compact/pruned vector indexes, passage offset maps, Python BM25, and mutable indexes in `packages/leann-core/src/leann/api.py:120-224`, `packages/leann-core/src/leann/api.py:280-354`, `packages/leann-core/src/leann/api.py:356-582`, and `packages/leann-core/src/leann/api.py:762-1078`. Backends include IVF with add/remove support in `packages/leann-backend-ivf/leann_backend_ivf/ivf_backend.py:85-291` and compact HNSW/recompute search in `packages/leann-backend-hnsw/leann_backend_hnsw/hnsw_backend.py:49-260`.

The code app chunks files for RAG in `apps/code_rag.py:81-178`, but the MCP layer is a handwritten shell wrapper around CLI commands in `packages/leann-core/src/leann/mcp.py:1-160`. Hybrid uses direct weighted raw vector/BM25 scores, not rank fusion, in `packages/leann-core/src/leann/api.py:1155-1382`. Also watch the logs: result text snippets and queries are logged in the search path.

### `tecnomanu__pampa` - Serious-ish Competitor

`pampa` has more substance than its size suggests. It implements RRF over vector and BM25 in `src/search/hybrid.js:1-57`, uses `wink-bm25` in `src/search/bm25Index.js:1-63`, and has a single service that handles SQLite schema, embedding provider setup, chunk deletion, AST chunking, fallback chunks, Merkle skip, stale delete, exact vector scan, BM25 build over scoped chunks, metadata/symbol boosts, optional intention expansion, optional cross-encoder rerank, and result formatting in `src/service.js:334-423`, `src/service.js:761-891`, `src/service.js:908-1235`, and `src/service.js:1246-1635`.

There are practical touches: file Merkle state in `src/indexer/merkle.js:24-98`, chokidar debounce/update flow in `src/indexer/watch.js:20-239`, optional Xenova cross-encoder reranking in `src/ranking/crossEncoderReranker.js:75-220`, and AES-256-GCM encrypted chunk storage in `src/storage/encryptedChunks.js:54-240`.

Weakness: vector search is brute-force in JS/SQLite, MCP `search_code` returns path/symbol/similarity/SHA but no snippet in `src/mcp-server.js:331-419`, so agents need a second call to inspect code. Search can also mutate learned-intention state. Clever, but a little too jazz hands.

### `m1rl0k__Context-Engine` - Serious but Overgrown

This is a dense Qdrant architecture: named dense/lexical/sparse vectors, query DSL filters, caching, expansion, RRF/adaptive/MMR/microspan ranking, and subprocess fallbacks are visible in `scripts/hybrid_search.py:47-157`, `scripts/hybrid_search.py:186-259`, and `scripts/hybrid_search.py:539-980`. Ingestion composes dense text from info/code/pseudo/tags in `scripts/ingest/pipeline.py:116-227`, chunks AST/symbol/micro spans in `scripts/ingest/chunking.py:30-283`, and creates Qdrant schema for dense lexical mini-pattern vectors plus optional sparse vectors in `scripts/ingest/qdrant.py:111-340`.

The MCP implementation returns useful agent fields: path, symbol, line, why, component scores, doc ids, relations, pseudo, tags, optional snippets, and used-rerank/code-signal metadata in `scripts/mcp_impl/search.py:1171-1525`. It also has mode-aware reordering and path-safe snippet reads in `scripts/mcp_impl/search.py:1220-1412`.

The warning is architectural sprawl: too many env knobs, subprocess fallback paths, remote Qdrant assumptions, unrelated memory/web/auth machinery, and debug-style output in hot paths. Steal the result contract, not the bazaar.

### `Wildcard-Official__deepcontext-mcp` - Adjacent Cloud-Backed Tool

DeepContext outsources the hard retrieval parts to Jina and Turbopuffer. Hybrid search generates a Jina embedding, then calls Turbopuffer hybrid search with vector and BM25 weights in `src/services/SearchCoordinationService.ts:39-85`. BM25 can be reranked with Jina in `src/services/SearchCoordinationService.ts:158-225`, while vector search is a direct embedding/query path in `src/services/SearchCoordinationService.ts:258-269`.

Indexing batches chunks, sends chunk content to Jina for embeddings, and upserts vector/content/file/line/language/symbol attributes to Turbopuffer in `src/core/indexing/IndexingOrchestrator.ts:408-486`. The fallback chunker uses 100-line chunks in `src/core/indexing/IndexingOrchestrator.ts:496-531`. The backend proxy sends hybrid requests as two Turbopuffer rankers, ANN vector plus BM25 content, in `backend/src/routes/vectordb.routes.ts:129-172`; Jina defaults are `jina-embeddings-v3` and `jina-reranker-v2-base-multilingual` in `backend/src/routes/helpers/embeddings.ts:15-30`.

This is not local-first. It logs queries in normal search paths at `src/services/SearchCoordinationService.ts:67` and `src/services/SearchCoordinationService.ts:180`, and uploads code content to cloud services.

### `jghiringhelli__codeseeker` - Thin/Noisy Wrapper With Some Real Pieces

Codeseeker contains many schemas and services, but the obvious CLI path is weak. It indexes only the first 50 changed files, chunks by fixed 20-line windows, embeds with Xenova, and stores to embedded SQLite or server DB in `src/cli/commands/handlers/search-command-handler.ts:187-268`. Search in embedded mode calls `searchByText`, not vector similarity, in `src/cli/commands/handlers/search-command-handler.ts:356-379`; server mode fetches embeddings and then does a simple text-similarity sort in `src/cli/commands/handlers/search-command-handler.ts:381-408`.

There are more credible pieces elsewhere: the storage interface defines vector, FTS, and RRF hybrid contracts in `src/storage/interfaces.ts:20-74`; PostgreSQL hybrid SQL creates weighted tsvector fields and a `hybrid_search` function in `src/database/hybrid-search-schema.sql:2-133`; MCP indexing service writes chunks and embeddings in `src/mcp/indexing-service.ts:857-933`; RAPTOR-like directory nodes are in `src/cli/services/search/raptor-indexing-service.ts:82-181`. But the repo reads like several generations of plans piled onto one another.

### `st3v3nmw__sourcerer-mcp` - Adjacent Tool

Sourcerer is a small, coherent vector-only MCP. It persists chromem-go vectors under `.sourcerer/db`, creates a `code-chunks` collection, stores chunk source and metadata, and maintains an mtime cache in `internal/index/index.go:43-90` and `internal/index/index.go:109-157`. Search queries chromem, overfetches for file-type filtering, sorts by similarity, and returns `id | summary [lines]` in `internal/index/index.go:180-268`; `get_chunk_code` retrieves exact source by id in `internal/index/index.go:271-299`.

The MCP instructions are agent-aware: they explain semantic vs exact search, chunk ids, batching, and when to use file tools in `internal/mcp/server.go:31-85`. Tools expose `semantic_search`, `find_similar_chunks`, `get_chunk_code`, `index_workspace`, and `get_index_status` in `internal/mcp/server.go:88-139`.

Weakness: no hybrid/BM25, no score in the search response, and exact matches are punted to grep. Useful UX, not a Loom replacement.

### `mhalder__qdrant-mcp-server` - Serious Competitor

This is a real Qdrant-backed code indexer, not just a generic collection wrapper. It registers semantic and hybrid tools in `src/tools/search.ts:20-127`. Hybrid validates collection capability, embeds the query, generates a sparse vector, and calls Qdrant hybrid search in `src/tools/search.ts:67-125`. Qdrant collections use named dense vectors plus sparse `text` vectors when hybrid is enabled in `src/qdrant/client.ts:52-83`, and hybrid search uses Qdrant prefetch over dense and sparse vectors with `fusion: "rrf"` in `src/qdrant/client.ts:255-319`.

Code indexing scans files, creates/clears collections, chunks with tree-sitter, skips detected secrets, batches embeddings, and stores rich payloads in `src/code/indexer.ts:68-218` and `src/code/indexer.ts:218-299`. AST chunking covers TS/JS/Python/Go/Rust/Java/Bash with character fallback in `src/code/chunker/tree-sitter-chunker.ts:37-185`. Incremental/search behavior includes post-filtering path globs and hybrid fallback in `src/code/indexer.ts:365-430`.

The sparse generator is weaker than the labels suggest: `src/embeddings/sparse.ts:1-10` admits it is BM25-like hash sparse vectors, not production BM25, and `src/embeddings/sparse.ts:103-137` defaults IDF to `1.0` unless trained. Also, tool logging includes query substrings in `src/tools/search.ts:32-40` and `src/tools/search.ts:76-77`.

### `steiner385__qdrant-mcp-server` - Thin Wrapper

This is a minimal OpenAI+Qdrant wrapper. The indexer embeds per-file semantic descriptions plus the first 20 lines, not real code chunks, in `src/qdrant-openai-indexer.py:146-186`. It batches OpenAI embeddings and upserts one point per file to Qdrant in `src/qdrant-openai-indexer.py:210-280`. The MCP server creates a single 1536-dim collection, exposes `search`, `store`, and `collection_info`, embeds the query, and returns only score plus metadata in `src/mcp-qdrant-openai-wrapper.py:42-93` and `src/mcp-qdrant-openai-wrapper.py:131-266`.

The background indexer watches files and spawns the Python indexer per file in `src/qdrant-background-indexer.cjs:257-443`, but deletion only removes local state, not Qdrant points, in `src/qdrant-background-indexer.cjs:320-332`. The docs themselves list "Real MCP server integration," "Embedding model integration," and "Incremental updates" as future work in `docs/background-indexer.md:223-228`. That is unusually kind of the evidence to walk into the courtroom by itself.

## Strong Ideas Loom Should Steal

### P0

- **Chunk/location split** from Elastic: store deduplicated chunk content separately from per-repo/per-file occurrences, so search returns reusable chunks plus exact line/file evidence (`elastic__semantic-code-search-indexer/src/utils/elasticsearch.ts:190-312`, `:499-760`).
- **Default hybrid = dense + lexical/sparse + RRF**, not "vector with a keyword filter." Good implementations: Semble's exact hybrid (`src/semble/search.py:70-122`), Qdrant prefetch+RRF (`mhalder.../src/qdrant/client.ts:288-308`), and Milvus dense+sparse BM25 (`zilliztech...milvus-restful-vectordb.ts:697-802`).
- **Evidence-rich MCP contracts**: return file, span, content/snippet, chunk id, score, component scores, strategy, and enough relationship metadata that agents do not immediately shell out. Elastic MCP, Context-Engine, and bobmatnyc are strongest here.
- **Deletion-correct incremental indexing**: content hash + file snapshot + explicit stale delete/reindex. Avoid metadata-only deletes. Use queue/claim semantics like Elastic's WAL queue for larger repos.
- **Code-aware BM25 tokenization**: preserve `foo.bar`, `foo-bar`, path fragments, snake/camel splits, and symbol names. bobmatnyc's tokenizer is worth stealing (`mcp_vector_search/core/bm25_backend.py:29-40`).

### P1

- **Adaptive weighting for symbol-like queries**: Semble lowers alpha for symbol queries; bobmatnyc favors BM25 for identifiers. This is cheap quality.
- **Path/role penalties and boosts**: tests/docs/compat/generated penalties; definition/symbol/path/file-coherence boosts. Semble has the cleanest version.
- **Rerank as bounded optional stage**: rerank top-N only, return whether rerank was used, and fail open with a warning rather than hiding the miss.
- **Path-safe read/reconstruction tool**: Elastic's `read_file_from_chunks` pattern and Context-Engine's snippet safety are good models.
- **Tokenizer/index version stamps**: if BM25 tokenizer or embedding model changes, invalidate or refuse stale indexes explicitly.
- **MMR/diversity mode** for broad conceptual searches, especially when many top chunks come from the same file.

### P2

- **Compact output formats** like TOON/structured minimal JSON for high-volume result sets.
- **Watcher cache eviction** for MCP-hosted indexes, but never swallow watcher errors.
- **Query DSL/filters** for path, language, test-only, symbol kind, and repo, with bounded payloads.
- **Benchmark harness around useful symbols per token**, comparing grep, vector-only, hybrid, and symbol-neighborhood outputs.

## Weak/No-Op Repos and Why

- `mixedbread-ai__mgrep`: the CLI may be useful, but the MCP server lists zero tools and returns "Not implemented" for calls (`src/commands/watch_mcp.ts:50-93`). It also routes indexing/search through Mixedbread cloud.
- `steiner385__qdrant-mcp-server`: one point per file, OpenAI-only embeddings, no hybrid, no snippets in search result, deletion does not remove Qdrant points, and docs list core features as future work.
- `jghiringhelli__codeseeker`: lots of architecture, but the visible CLI path limits indexing to 50 files, chunks by 20-line windows, and searches text rather than vectors in the embedded path. Some real subsystems exist, but the execution path is not competitive.
- `FarhanAliRaza__claude-context-local`: useful local FAISS baseline, but no hybrid/BM25 and deletion appears metadata-only, leaving stale vectors.
- `st3v3nmw__sourcerer-mcp`: coherent and useful, but vector-only and explicitly tells agents to use grep for exact identifiers.

## What Loom Should Avoid

- **Cloud by default** for private code. Jina, Turbopuffer, Mixedbread, OpenAI, Zilliz Cloud, Elastic Cloud, and remote Qdrant are all adoption friction for security-sensitive codebases.
- **Logging queries or source snippets**. Several repos log query substrings in normal paths. Loom should keep local-only handling sacred.
- **Search tools that return only IDs or metadata**. That forces a second shell/read hop and loses the north-star metric.
- **"BM25-like" sparse vectors with untrained IDF marketed as BM25**. It may help, but the label is doing unpaid labor.
- **Deletion that removes local state but leaves vector points**. Stale results poison trust faster than slow indexing does.
- **Feature soup**: memory systems, web dashboards, subprocess bridges, half-built graph layers, and model-rerank-provider matrices before the core retrieval contract is excellent.
- **Silent watcher/indexing failures**. Background indexing must expose actionable failures, not just limp onward heroically.

## Gaps/Open Questions

- Which competitors have public relevance benchmarks with reproducible corpora? Source inspection found claims and tests, but not enough apples-to-apples evidence.
- How do Semble and ck behave past medium repos when exact scan/sidecar brute force meets large monorepos?
- Can Loom support a local sparse index with real BM25 and code-aware tokenization while keeping storage simple in `.loom/`?
- Should Loom expose component scores (`dense`, `bm25`, `symbol`, `graph`) in every result, or only in debug/verbose mode?
- What is the minimum evidence contract that actually keeps agents out of shell: snippet plus span, full chunk content, relationship neighborhood, or a reconstruct/read tool?
- Should Loom add optional Qdrant/Elastic adapters later, or treat local SQLite/sqlite-vec/blob fallback as the core identity and make external stores explicitly secondary?
