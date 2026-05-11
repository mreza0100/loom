# OCTOCODE Competitor Teardown

## Executive Verdict

### Muvon/octocode: serious competitor

This is the real overlap. `Muvon/octocode` is a Rust code-intelligence system with tree-sitter indexing, LanceDB vector and FTS storage, semantic retrieval, optional reranking, optional GraphRAG, MCP tools, watch mode, branch deltas, LSP-backed navigation, and a retrieval benchmark suite. It competes with Loom directly on local code indexing and agent-facing retrieval.

The sharp edges are also real: defaults point at remote embedding models, GraphRAG precision depends on heuristics, the MCP surface includes file-mutating structural rewrite behavior, and the LSP provider contains an apparent temporary debug hack that substitutes fake content for `src/main.rs`. Serious system, serious caution.

Classification: **serious competitor**.

### bgauryy/octocode-mcp: adjacent tool, serious MCP UX competitor

This repo is not a local semantic-index competitor. It is an MCP/CLI/VScode-oriented code research orchestrator around provider APIs, local `rg`/`find`/file reads, clone caching, package search, LSP calls, credentials, safety checks, pagination, and agent workflow polish. There is no local vector index, no embedding pipeline, no persistent code graph, and no ranking model beyond upstream APIs, grep/find ordering, filtering, pagination, and LSP semantics.

It is still dangerous in the operational UX sense: the tool surface, output containment, path/security discipline, sparse clone workflow, and benchmark framing are better than many "semantic" systems that merely wave a vector at a README.

Classification: **adjacent tool**.

## Repo Manifest

### Muvon__octocode

- Local path: `/Users/reza/work/loom/tmp/competitors/Muvon__octocode`
- Inferred URL: `https://github.com/Muvon/octocode.git` from `.git/config`
- Primary language: Rust
- Package: `octocode` v0.14.1 in `Cargo.toml`
- Main stack: `rmcp`, `lancedb`, `lance`, `tree-sitter-*`, `ast-grep-core`, `notify`, `octolib`, `lsp-types`, optional `fastembed`/Hugging Face features
- Primary surfaces: CLI, MCP server, semantic search, indexing, GraphRAG, LSP tools, watch mode, benchmarks
- Verdict: **serious competitor**

### bgauryy__octocode-mcp

- Local path: `/Users/reza/work/loom/tmp/competitors/bgauryy__octocode-mcp`
- Inferred URL: `https://github.com/bgauryy/octocode-mcp.git` from `.git/config`
- Primary language: TypeScript
- Packages: `octocode-mcp`, `octocode-cli`, `octocode-shared`, `octocode-security`, `octocode-vscode`
- Main stack: MCP server, GitHub/GitLab/Bitbucket provider APIs, local shell search, clone cache, LSP orchestration, credential storage, CLI and skills
- Primary surfaces: MCP tools, CLI, VS Code extension, install/skills workflow
- Verdict: **adjacent tool**

## Source-Level Findings

## Muvon/octocode

### Architecture

`Muvon/octocode` is not a wrapper around grep. The CLI in `src/main.rs` exposes indexing, search, view, watch, config, grep, GraphRAG, MCP server/proxy, stats, explain, diff, clear, branch, commit, review, release, format, logs, model listing, and completion commands. The dependency graph in `Cargo.toml` confirms a broad local code-intelligence stack: Rust MCP via `rmcp`, LanceDB/Lance storage, tree-sitter parsers, AST grep, file watching, embeddings, LSP types, and optional model backends.

This is overbuilt in places, but the shape is coherent: ingest source into language-aware blocks, embed/store them, search with vector plus lexical fallback, expose compact MCP tools, then add GraphRAG/LSP/watch/branch deltas around the core.

### Indexing and Parsing Flow

The differential indexer parses files with tree-sitter, extracts imports and symbols into a contextual `FileContext`, hashes chunks by content/path/range, reuses existing block IDs when possible, and removes stale hashes after processing. See `src/indexer/differential_processor.rs:43`, `src/indexer/differential_processor.rs:79`, `src/indexer/differential_processor.rs:93`, `src/indexer/differential_processor.rs:106`, and `src/indexer/differential_processor.rs:152`.

The region extractor deliberately indexes meaningful AST regions rather than arbitrary token windows. It does a two-pass extraction and merge process, collects comments, symbols, and fallback regions, then smart-merges small regions under bounded line limits. See `src/indexer/code_region_extractor.rs:41`, `src/indexer/code_region_extractor.rs:63`, and `src/indexer/code_region_extractor.rs:116`.

Language support is broad and explicitly modeled in `src/indexer/languages/mod.rs`. The language trait includes tree-sitter language, file extensions, meaningful node kinds, symbols, imports/exports, function calls, and import resolution. The repo has implementations for Rust, JavaScript, TypeScript, Python, Go, Java, C++, PHP, Bash, Ruby, Lua, JSON, Svelte, CSS, and Markdown.

Loom-relevant read: their chunking is closer to "semantic code regions with symbols and comments" than naive chunks. That is table stakes for Loom's north-star metric.

### Storage

Storage is LanceDB-backed. `Store` owns a LanceDB connection, code/text vector dimensions, and a table cache in `src/store/mod.rs:146`. Store construction resolves model dimensions from config before connecting to LanceDB in `src/store/mod.rs:194` and `src/store/mod.rs:211`.

The dimension guard is worth stealing: startup checks existing embedding field dimensions and drops mismatched tables so changed embedding models do not poison searches with stale vector schemas. See `src/store/mod.rs:217`.

Physical storage is local under an Octocode data directory. `src/storage.rs` builds a project identifier from normalized Git remote or path hash, stores project data under a `storage` directory, and keeps branch deltas separately under `branches`.

### Embeddings and Vector Strategy

The default embedding config is privacy-relevant: `src/embedding/mod.rs:42` defaults code embeddings to `voyage:voyage-code-3` and text embeddings to `voyage:voyage-3.5-lite`, with batch size 16 and max input tokens 100,000. It has retry/backoff paths for query and batch embedding generation in `src/embedding/mod.rs:76` and `src/embedding/mod.rs:166`.

This is powerful, but it is not Loom's local-only posture unless configured that way. The crate has local-ish feature flags (`fastembed`, Hugging Face support), but source defaults point to remote models.

### Retrieval, Ranking, and Token Containment

Search uses a hybrid path: vector search plus FTS/BM25, Reciprocal Rank Fusion, and FTS fallback. The comment at `src/store/mod.rs:1048` says exactly that; the implementation opens native LanceDB vector and full-text search paths at `src/store/mod.rs:1069`, filters/optimizes/sorts/truncates around `src/store/mod.rs:1090`, and has an FTS-only fallback around `src/store/mod.rs:1128`.

The query path in `src/indexer/search.rs:611` initializes store and branch context, generates mode-specific embeddings, and chooses result limits. If reranking is enabled, it over-fetches candidates and defers thresholding until after rerank in `src/indexer/search.rs:630`. The mode switch handles code, text, docs, commits, and all-mode splitting around `src/indexer/search.rs:652`; reranking and final truncation happen around `src/indexer/search.rs:837`.

The MCP `semantic_search` schema has bounded `max_results` from 1 to 20 and detail levels `signatures`, `partial`, and `full`, plus mode and threshold controls. See `src/mcp/server.rs:58` and `src/mcp/server.rs:223`. That detail-level idea is good agent ergonomics: default cheap, expand only when needed.

### Graph and Relationship Strategy

GraphRAG is integrated, not bolted only at the README level. `src/indexer/graphrag/builder.rs:37` defines a `GraphBuilder` with config, in-memory graph, embedding provider, store, project root, and optional AI enhancements. It loads project DB state, optionally enables AI enhancements, groups code blocks by file, skips unchanged content by hash, removes stale graph nodes, and builds symbols/imports/exports/functions from code blocks. See `src/indexer/graphrag/builder.rs:84`, `src/indexer/graphrag/builder.rs:120`, `src/indexer/graphrag/builder.rs:161`, `src/indexer/graphrag/builder.rs:181`, and `src/indexer/graphrag/builder.rs:220`.

Relationship discovery uses indexed symbols/paths, import/export edges, hierarchical parent relationships, language-specific patterns, and function-call relationships. See `src/indexer/graphrag/relationships.rs:25`, `src/indexer/graphrag/relationships.rs:33`, `src/indexer/graphrag/relationships.rs:48`, `src/indexer/graphrag/relationships.rs:67`, `src/indexer/graphrag/relationships.rs:91`, and `src/indexer/graphrag/relationships.rs:98`.

This is credible but likely noisy. The precision of import resolution and call extraction across languages is the obvious place to attack it.

### MCP/API Surface

The MCP server exposes `semantic_search`, `view_signatures`, `graphrag`, LSP-style tools, and structural search. `SemanticSearchParams` are defined at `src/mcp/server.rs:58`; `GraphRagParams` at `src/mcp/server.rs:90`; `StructuralSearchParams` at `src/mcp/server.rs:117`.

GraphRAG operations include `search`, `get-node`, `get-relationships`, `find-path`, and `overview`. See `src/mcp/server.rs:90`.

The structural search tool is both interesting and dangerous: it supports AST pattern matching, optional rewrite, and `update_all`. That makes it a mutation-capable MCP tool, not just a retrieval surface. If Loom keeps MCP code-intelligence tools read-only by default, this is a clear avoid.

### LSP Integration

Muvon exposes LSP navigation tools through the MCP surface: `goto_definition`, `hover`, `find_references`, `document_symbols`, `workspace_symbols`, and completion-related plumbing are visible around `src/mcp/server.rs:296`.

There is a bad wart in `src/mcp/lsp/provider.rs`: the single-file open path contains a "TEMPORARY DEBUG" branch that replaces `src/main.rs` content with `fn main() { println!("Hello, world!"); }`. If that path is live, it can corrupt LSP evidence. Even if it is harmless in practice, it is exactly the kind of debug residue a code-intelligence tool cannot afford.

### Evidence/Proof Model

Evidence is mostly file path, language, content/range-bearing code blocks, signatures, graph node IDs, and LSP ranges. The retrieval benchmark uses line-range overlap as ground truth, which is the right proof direction. The system is not just returning prose summaries.

The weak point is generated/contextual/AI-enriched descriptions. If enabled, those can become a second-order evidence layer. Loom should keep source spans primary and generated labels secondary.

### Security and Privacy

Good: storage is local; walker behavior excludes common ignored/no-index areas; MCP output appears structured and bounded; stdout cleanliness matters in the server implementation.

Bad: default models in `src/embedding/mod.rs:42` are Voyage remote models. Optional contextual descriptions, reranking, and GraphRAG AI enhancements can send source or source-derived content out of process depending on config. That is not compatible with Loom's "private code can live inside indexes; local-only handling is sacred ground" principle unless made explicit and opt-in.

Also bad: an MCP tool that can rewrite files via structural search is a trust-boundary smell.

### Benchmarks

The `benchmark/README.md` is one of the best parts. It defines retrieval-quality evaluation for the full retrieval pipeline: chunking, embedding, vector search, and reranking. It uses pinned ground truth CSVs for code/docs queries, line overlap scoring, Hit@k, MRR, NDCG@10, Recall@k, and hard natural-language queries that intentionally avoid symbol names. It fails if Hit@5 drops below a threshold.

This is directly useful for Loom. Most competitors hand-wave benchmarks; this one at least tries to measure whether the system found the needed code.

### Operational UX

The CLI surface is large and ambitious. Search/view/watch/config/GraphRAG/MCP/stats/explain/diff/branch/review/release/logs/models/completion make it feel like a full workbench rather than a narrow MCP server. The cost is complexity and policy sprawl. Loom should copy the ergonomic wins, not the everything-menu.

## bgauryy/octocode-mcp

### Architecture

`bgauryy/octocode-mcp` is a TypeScript multi-package repo: MCP server, CLI, shared utilities, security package, and VS Code extension. The tool catalog imports GitHub search/content/repo/PR tools, package search, clone repo, local ripgrep/view/find/fetch, and LSP tools in `packages/octocode-mcp/src/tools/toolConfig.ts:1`.

The catalog registration is explicit and metadata-rich. Tool definitions include provider support, auth requirements, command names, and registration functions around `packages/octocode-mcp/src/tools/toolConfig.ts:97`. `ToolsManager` registers tools with filters, metadata gateway support, and output sanitization in `packages/octocode-mcp/src/tools/toolsManager.ts:35`.

This is a polished MCP tool suite, not a local indexer.

### Indexing and Retrieval Flow

There is no source-level evidence of local semantic indexing, local embeddings, vector DB, or persistent graph retrieval. Local retrieval is shell-backed:

- `local_ripgrep` validates query/path, estimates large directories, builds an `rg` command, runs it through safe execution, falls back to grep, parses output, and returns matches. See `packages/octocode-mcp/src/tools/local_ripgrep/ripgrepExecutor.ts:30`, `packages/octocode-mcp/src/tools/local_ripgrep/ripgrepExecutor.ts:45`, `packages/octocode-mcp/src/tools/local_ripgrep/ripgrepExecutor.ts:69`, `packages/octocode-mcp/src/tools/local_ripgrep/ripgrepExecutor.ts:79`, and `packages/octocode-mcp/src/tools/local_ripgrep/ripgrepExecutor.ts:132`.
- `RipgrepCommandBuilder` maps structured query options to `rg` flags and command arguments in `packages/octocode-mcp/src/commands/RipgrepCommandBuilder.ts:5` and `packages/octocode-mcp/src/commands/RipgrepCommandBuilder.ts:105`.
- `local_find_files` checks `find`, validates paths, excludes bulky directories by default, runs `find`, caps output, and paginates/sorts. See `packages/octocode-mcp/src/tools/local_find_files/findFiles.ts:28`, `packages/octocode-mcp/src/tools/local_find_files/findFiles.ts:46`, `packages/octocode-mcp/src/tools/local_find_files/findFiles.ts:56`, `packages/octocode-mcp/src/tools/local_find_files/findFiles.ts:95`, and `packages/octocode-mcp/src/tools/local_find_files/findFiles.ts:116`.
- `local_fetch_content` performs bounded file reading, match extraction, truncation, and pagination. See `packages/octocode-mcp/src/tools/local_fetch_content/fetchContent.ts:24`, `packages/octocode-mcp/src/tools/local_fetch_content/fetchContent.ts:61`, `packages/octocode-mcp/src/tools/local_fetch_content/fetchContent.ts:109`, `packages/octocode-mcp/src/tools/local_fetch_content/fetchContent.ts:286`, `packages/octocode-mcp/src/tools/local_fetch_content/fetchContent.ts:324`, and `packages/octocode-mcp/src/tools/local_fetch_content/fetchContent.ts:350`.

Remote retrieval is provider-backed GitHub/GitLab/Bitbucket/package API search, not local semantic search. `packages/octocode-mcp/src/providers/factory.ts` dynamically initializes provider clients and caches them. `packages/octocode-mcp/src/providers/types.ts` defines provider capabilities such as code search, file content, repo search, pull request search, repo structure, and default branch resolution.

### Storage

No code index storage. Persistent-ish storage is operational:

- Clone cache under `~/.octocode/repos`, documented in `packages/octocode-mcp/src/tools/github_clone_repo/cloneRepo.ts:1`.
- Clone cache TTL/size/count/GC are controlled in `packages/octocode-mcp/src/tools/github_clone_repo/cache.ts`.
- Credentials are stored by the shared credential package, with encrypted file storage under `~/.octocode/credentials.json` and key material under `~/.octocode/.key` in `packages/octocode-shared/src/credentials/storage.ts` and `packages/octocode-shared/src/credentials/credentialEncryption.ts`.

This is storage for operational state, not retrieval state.

### Embeddings and Vector Strategy

None found. Despite package metadata using terms like semantic search, the local source inspected here does not implement embeddings, vector search, or model-backed retrieval. It delegates search to APIs and local text tools.

Classification pressure: this is why it is adjacent, not a serious Loom-index competitor.

### Graph and Relationship Strategy

No persistent code graph found. Relationship-like behavior comes from LSP references, goto definition, and call hierarchy. That can be semantically valuable for a live workspace, but it is not an indexed graph that survives across repos or supports graph ranking.

### LSP Integration

This is one of the repo's stronger pieces. `packages/octocode-mcp/src/lsp/manager.ts:20` creates an LSP client per call and handles start/stop failure behavior. It checks command availability and resolves user-configured or built-in language server configs around `packages/octocode-mcp/src/lsp/manager.ts:72` and `packages/octocode-mcp/src/lsp/manager.ts:112`.

`lsp_goto_definition` validates paths, reads the file, resolves symbols with line-search radius/order hints, resolves workspace, checks LSP support, and marks semantic LSP mode when successful. See `packages/octocode-mcp/src/tools/lsp_goto_definition/execution.ts:72`, `packages/octocode-mcp/src/tools/lsp_goto_definition/execution.ts:82`, `packages/octocode-mcp/src/tools/lsp_goto_definition/execution.ts:96`, and `packages/octocode-mcp/src/tools/lsp_goto_definition/execution.ts:132`.

`lsp_find_references` creates a client, warms TypeScript-ish project graphs via `prepareCallHierarchy`, collects raw locations, post-filters declarations, applies include/exclude globs, and paginates/enhances visible results. See `packages/octocode-mcp/src/tools/lsp_find_references/lspReferencesCore.ts:80`, `packages/octocode-mcp/src/tools/lsp_find_references/lspReferencesCore.ts:90`, `packages/octocode-mcp/src/tools/lsp_find_references/lspReferencesCore.ts:100`, `packages/octocode-mcp/src/tools/lsp_find_references/lspReferencesCore.ts:119`, `packages/octocode-mcp/src/tools/lsp_find_references/lspReferencesCore.ts:143`, and `packages/octocode-mcp/src/tools/lsp_find_references/lspReferencesCore.ts:172`.

The per-call client lifetime is simple but may be slow. The warm-up trick is worth stealing.

### MCP/API Surface

The MCP surface is broad and practical:

- Provider search/content/repo/PR tools
- Package search
- Clone repository
- Local ripgrep
- Local file view/fetch
- Local file find
- LSP goto definition
- LSP find references
- LSP call hierarchy

Tool metadata and gating are centralized in `packages/octocode-mcp/src/tools/toolConfig.ts:26` and `packages/octocode-mcp/src/tools/toolConfig.ts:97`. Tool registration and sanitization flow through `packages/octocode-mcp/src/tools/toolsManager.ts:35`.

The weakness: some schemas/metadata live in the external `@octocodeai/octocode-core` package, so the local repo does not fully reveal the contract without inspecting installed package artifacts.

### Ranking and Token Containment

Ranking is not model-based. GitHub/GitLab/Bitbucket/package APIs rank provider results; local grep/find order and filters drive local results; LSP returns protocol locations.

Token containment is strong. `local_fetch_content` defaults to 8,000 output chars and 50 max match lines in `packages/octocode-mcp/src/tools/local_fetch_content/fetchContent.ts:24`. Large files require explicit `charLength`, `matchString`, or `startLine`, with actionable refusal hints around `packages/octocode-mcp/src/tools/local_fetch_content/fetchContent.ts:109`. Match output is capped and truncated around `packages/octocode-mcp/src/tools/local_fetch_content/fetchContent.ts:324`, and automatic pagination kicks in around `packages/octocode-mcp/src/tools/local_fetch_content/fetchContent.ts:350`.

`local_find_files` caps default output at 1,000 files and paginates/sorts around `packages/octocode-mcp/src/tools/local_find_files/findFiles.ts:116`. `local_ripgrep` estimates large directories and emits chunking warnings around `packages/octocode-mcp/src/tools/local_ripgrep/ripgrepExecutor.ts:69`.

This is probably the best part for Loom to copy: make bounded outputs and continuation paths feel native, not punitive.

### Evidence/Proof Model

Evidence is mostly raw provider snippets, file content ranges, ripgrep matches, find results, and LSP locations. It is concrete enough for agents to cite, but it has no local index-level provenance model.

Compared with Loom's intended compact ranked neighborhoods, this is more "tools with bounded excerpts" than "retrieval engine with proof graph."

### Security and Privacy

The security package is unusually serious for an MCP wrapper. `packages/octocode-security/src/pathValidator.ts:30` defines allowed roots, including workspace, home directory defaults, environment-configured paths, and registered paths. It resolves symlinks before validation around `packages/octocode-security/src/pathValidator.ts:101`, rejects paths outside allowed directories around `packages/octocode-security/src/pathValidator.ts:126`, and validates symlink realpaths around `packages/octocode-security/src/pathValidator.ts:150`.

Content sanitization includes size limits and secret redaction. See `packages/octocode-security/src/contentSanitizer.ts:156` and `packages/octocode-security/src/contentSanitizer.ts:209`.

Clone auth avoids putting tokens in remote URLs by using `http.extraHeader`, scrubs tokens, and runs git with constrained output/timeout/env behavior. See `packages/octocode-mcp/src/tools/github_clone_repo/cloneRepo.ts:214`, `packages/octocode-mcp/src/tools/github_clone_repo/cloneRepo.ts:253`, and `packages/octocode-mcp/src/tools/github_clone_repo/cloneRepo.ts:278`.

This is a good standard for Loom's shell-adjacent tools.

### Benchmarks

`packages/octocode-cli/docs/BENCHMARK.md` frames a realistic agent harness using `claude -p`, comparing MCP-only mode against CLI-plus-skill shell mode. Tasks include package investigation, library usage examples, repo orientation, PR archaeology, and comparative research. It measures wall-clock, tokens, turns, effective cost, and scoring against pinned ground truth.

This is not a retrieval-quality benchmark like Muvon's. It is an operational agent benchmark. Still useful: it tests whether the tool helps an agent finish realistic tasks, not whether a demo query looks clever.

### Operational UX

This repo is ahead on packaging and agent ergonomics: installable CLI, MCP server, VS Code integration, skills, provider tokens, clone cache, sparse clone, path guards, pagination, large-file hints, and bulk-ish structured tool calls.

It is also externally coupled. Provider APIs, `@octocodeai/octocode-core`, installed language servers, local shell commands, credentials, and rate limits all matter. The UX hides a lot, but the dependency surface is wide.

## What Loom Should Steal

## P0

- Retrieval benchmark discipline from `Muvon/octocode`: pinned ground truth, line-overlap scoring, Hit@k, MRR, NDCG, Recall, hard natural-language queries, and CI failure thresholds. This directly serves Loom's "useful symbols discovered per token spent" metric.
- Bounded output and continuation UX from `bgauryy/octocode-mcp`: default char limits, max matches, pagination, large-file refusal with next actions, result IDs, and explicit offsets.
- Security hygiene from `bgauryy/octocode-mcp`: realpath/symlink path validation, safe command builders, token scrubbing, secret redaction, max content sizes, and command timeouts.
- Detail-level retrieval from `Muvon/octocode`: `signatures`, `partial`, and `full` modes are a clean way to contain tokens while giving agents an upgrade path.

## P1

- Muvon's schema-dimension guard for embedding table compatibility. Wrong-dimension vectors should fail/rebuild loudly, not degrade quietly.
- Muvon's branch-delta overlay idea. Agent workflows often need "main plus my working branch" retrieval without corrupting the base index.
- Muvon's semantic AST region extraction with symbol/comment context. Loom should beat it, but the direction is right.
- Muvon's graph builder from existing code blocks. Graph construction should reuse indexed artifacts rather than reparsing everything through a separate universe.
- bgauryy's sparse clone/cache workflow for remote repo exploration. Useful if Loom grows first-class remote indexing.
- bgauryy's LSP mode labeling and warm-up behavior. Tell the agent whether a result is semantic LSP output or fallback text logic.

## P2

- Muvon's optional contextual descriptions, but only as secondary labels behind source spans and only with explicit privacy controls.
- Muvon's GraphRAG operations: `get-node`, `get-relationships`, `find-path`, and `overview` are useful MCP shapes.
- Muvon's AST structural search, but as read-only preview unless Loom explicitly ships a mutating refactor tool.
- bgauryy's real-agent benchmark framing: compare MCP, CLI, and shell-assisted workflows on actual tasks, not just retrieval microbenchmarks.
- bgauryy's skills/CLI packaging if Loom moves beyond MCP server usage.

## What Loom Should Avoid

- Do not default private code embeddings or summaries to remote providers. If remote models exist, make them loudly opt-in.
- Do not put file mutation behind a normal retrieval MCP tool. Muvon's structural `rewrite` plus `update_all` is exactly the wrong trust boundary for read-mostly code intelligence.
- Do not advertise "semantic search" when the implementation is provider APIs plus grep/find. bgauryy is useful, but the label is sloppy.
- Do not let generated/contextual descriptions outrank source spans as evidence.
- Do not ship with debug-path behavior like Muvon's `src/main.rs` fake-content substitution in the LSP provider.
- Do not rely on per-call LSP startup if low latency is a goal. It is simple, but it will bite repeated agent loops.
- Do not make the local tool contract depend too much on remote metadata packages or gateways. Local robustness matters for MCP.
- Do not grow an everything-menu CLI before the core retrieval proof is excellent.

## Gaps/Open Questions

- I did not run either repo's benchmark suite; this is a source-level teardown only.
- Muvon's actual privacy posture depends on user config, templates, and `octolib` provider behavior. The source defaults are enough to flag risk, but a runtime config audit would be needed for a final privacy verdict.
- Muvon's GraphRAG relationship precision is unknown. The source is credible, but import/call heuristics across many languages can easily produce confident junk.
- Muvon's LSP debug substitution needs runtime confirmation. If live, it is a correctness bug; if dead, it is still bad hygiene.
- bgauryy's exact MCP schemas are partly imported from `@octocodeai/octocode-core`, so the local source does not fully expose schema constraints.
- bgauryy's local LSP quality depends on installed language servers and startup overhead. The code has fallback behavior, but fallback quality is not equivalent.
- bgauryy's provider ranking, auth behavior, and rate limits are inherited from GitHub/GitLab/Bitbucket/package APIs.
- Neither repo appears to have Loom's exact target shape: compact local symbol neighborhoods optimized for useful symbols per token. Muvon is closest technically; bgauryy is closer on operational polish.
