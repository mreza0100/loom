# Static Analysis / CPG / Tree-sitter Competitor Teardown

## Executive Verdict

The serious static-analysis competitors are Joern, ShiftLeft's CPG schema, code-pathfinder, CodeGraph, and codegraph-rust. They are not all competitors in the same way:

- Joern and ShiftLeft are the full CPG/SAST world. They prove the power of explicit node/edge schemas, dataflow overlays, and query DSLs, but they are too heavy for Loom's north star unless Loom wants to become a vulnerability scanner with a JVM in the backpack.
- colbymchenry CodeGraph is the most directly Loom-shaped competitor: Tree-sitter extraction, SQLite graph, unresolved references, framework-aware resolution, MCP tools, bounded output, watcher sync.
- Jakedismo codegraph-rust is a serious but overbuilt Rust/SurrealDB/vector graph attempt. Strong ideas are visible, but the storage/query surface is much heavier than Loom should copy.
- code-pathfinder is serious SAST-adjacent: real Tree-sitter extraction, call graphs, CFGs, variable dependency graphs, taint/dataflow, and MCP tools. Useful lessons, but the center of gravity is security analysis.
- Most Tree-sitter MCP repos are AST workbenches or symbol extractors. Useful as UX references, not graph competitors.

Best direction for Loom: keep a compact local graph with typed, confidence-scored edges and bounded neighborhoods. Steal the source-faithful bits: unresolved refs plus resolution provenance, caller/callee/context tools, Tree-sitter query diagnostics, changed-file invalidation, and progressive payloads. Avoid Joern-scale DDG/CDG/taint-by-default. That road is paved with good intentions and benchmark graphs no one wants to boot.

## Repo Manifest

| Repo | Inferred URL | Classification |
|---|---|---|
| `tmp/competitors/joernio__joern` | `https://github.com/joernio/joern.git` | serious competitor |
| `tmp/competitors/ShiftLeftSecurity__codepropertygraph` | `https://github.com/ShiftLeftSecurity/codepropertygraph.git` | serious competitor, schema substrate |
| `tmp/competitors/shivasurya__code-pathfinder` | `https://github.com/shivasurya/code-pathfinder.git` | serious competitor, SAST-adjacent |
| `tmp/competitors/lekssays__codebadger` | `https://github.com/lekssays/codebadger.git` | thin wrapper around Joern |
| `tmp/competitors/nendotools__tree-sitter-mcp` | `https://github.com/nendotools/tree-sitter-mcp.git` | thin wrapper / adjacent AST tool |
| `tmp/competitors/wrale__mcp-server-tree-sitter` | `https://github.com/wrale/mcp-server-tree-sitter.git` | adjacent Tree-sitter query tool |
| `tmp/competitors/ctoth__mcp_server_code_extractor` | `https://github.com/ctoth/mcp_server_code_extractor.git` | thin wrapper |
| `tmp/competitors/aimasteracc__tree-sitter-analyzer` | `https://github.com/aimasteracc/tree-sitter-analyzer.git` | adjacent tool, graph claims thin |
| `tmp/competitors/Jakedismo__codegraph-rust` | `https://github.com/Jakedismo/codegraph-rust.git` | serious competitor, overbuilt |
| `tmp/competitors/vymalo__code-graph` | `https://github.com/vymalo/code-graph.git` | adjacent graph analyzer |
| `tmp/competitors/colbymchenry__CodeGraph` | `https://github.com/colbymchenry/CodeGraph.git` | serious competitor |
| `tmp/competitors/er77__code-graph-rag-mcp` | `https://github.com/er77/code-graph-rag-mcp.git` | adjacent tool, noisy/overclaim-prone |
| `tmp/competitors/glommer__codemogger` | `https://github.com/glommer/codemogger.git` | adjacent semantic code index |

## Source-Level Findings

### `joernio__joern`

Classification: serious competitor.

Joern is the heavyweight baseline for CPG and dataflow. `console/src/main/scala/io/joern/console/cpgcreation/CpgGeneratorFactory.scala:12-31` registers a broad language set including C, C#, Go, Java, JavaScript, Python, PHP, Kotlin, Swift, Rust, LLVM, Ghidra, and ABAP. The same generator path converts proto CPG output to flatgraph and deletes the proto zip in `CpgGeneratorFactory.scala:59-93`, so it is an import/build pipeline, not a lightweight always-on index.

The dataflow engine is real. `dataflowengineoss/src/main/scala/io/joern/dataflowengineoss/DefaultSemantics.scala:9-17` composes default semantics; `DefaultSemantics.scala:24-71` includes assignment, pointer, field, index, and indirection flow rules; `DefaultSemantics.scala:79-145` adds external call summaries. Slicing runs over reaching definitions: `dataflowengineoss/src/main/scala/io/joern/dataflowengineoss/slicing/DataFlowSlicing.scala:44-55` traverses `ddgIn` and `EdgeTypes.REACHING_DEF`, and `DataFlowSlicing.scala:19-39` parallelizes sink processing.

The console path wires overlays into load/create flows. `console/src/main/scala/io/joern/console/BridgeBase.scala:391-419` loads or creates CPGs, imports code, runs `run.ossdataflow` or `run.dataflow`, and persists results.

Loom translation: steal explicit overlays and query affordances, not the whole CPG machine.

### `ShiftLeftSecurity__codepropertygraph`

Classification: serious competitor as schema, not product.

This is the schema lesson. `schema/src/main/scala/io/shiftleft/codepropertygraph/schema/Ast.scala:13-29` defines the AST layer as the frontend-created typed AST basis for later CFG creation. `Ast.scala:70-91` standardizes `AST_NODE` properties such as `CODE`, `ORDER`, line/column, and offset. `Ast.scala:725-749` defines `CALL` with `METHOD_FULL_NAME`, `TYPE_FULL_NAME`, name, and signature; `Ast.scala:751-785` adds expression argument index/name semantics and CALL AST out-edges.

Call graph modeling is explicit. `schema/src/main/scala/io/shiftleft/codepropertygraph/schema/CallGraph.scala:25-82` defines argument and method-full-name properties. `CallGraph.scala:128-153` defines dispatch type and the `CALL` edge between call sites and methods, auto-created on load when `METHOD_FULL_NAME` exists.

The PDG is also explicit: `schema/src/main/scala/io/shiftleft/codepropertygraph/schema/Pdg.scala:7-17` defines PDG as DDG plus CDG through `REACHING_DEF` and `CDG`; `Pdg.scala:27-53` defines variable, control-dependence, and reaching-definition edges. `codepropertygraph/src/main/scala/io/shiftleft/codepropertygraph/cpgloading/CpgLoader.scala:24-75` handles proto, OverflowDB, and flatgraph loading/conversion.

Loom translation: a small stable edge vocabulary with source spans, confidence, and provenance is more valuable than full CPG parity.

### `shivasurya__code-pathfinder`

Classification: serious competitor, SAST-adjacent.

This repo has a real custom engine. `sast-engine/graph/parser.go:8-18` dispatches Tree-sitter nodes by language; `parser.go:20-39` handles C/C++/Python functions/classes/calls; `parser.go:110-169` adds Java, Go, C, and C++ method/call handling.

The graph model is not cosmetic. `sast-engine/graph/callgraph/core/types.go:73-158` defines `CallGraph` with forward/reverse edges, call sites, functions, parameters, summaries, statements, CFGs, CFG block statements, type engines, and registries. `types.go:160-193` initializes those maps and maintains both edge directions. CFG support is explicit in `sast-engine/graph/callgraph/cfg/cfg.go:9-51` with entry/exit/normal/conditional/loop/switch/try/catch/finally blocks, and `cfg.go:53-135` defines basic blocks and CFG structure.

Taint/dataflow is real but fallback-heavy. `sast-engine/dsl/dataflow_executor.go:15-44` wraps CallGraph for local/global dataflow. `dataflow_executor.go:46-103` tries CFG-aware variable dependency graph, then flat VDG, then line proximity. `dataflow_executor.go:159-172` attaches confidence by method. `sast-engine/graph/callgraph/analysis/taint/var_dep_graph.go:33-38` defines VDG nodes/edges/latest definitions; `var_dep_graph.go:53-105` builds VDG from statements; `var_dep_graph.go:116-168` BFSes from sources to sinks with sanitizer filtering. `interprocedural.go:8-48` defines taint transfer summaries and `interprocedural.go:50-122` builds summaries with synthetic params and callee summaries.

MCP surface is useful. `sast-engine/mcp/tools.go:119-180` exposes `get_index_info` and `find_symbol`; the tool set includes `list_modules`, `get_callers`, `get_callees`, `get_call_details`, `resolve_import`, and `status`.

Loom translation: confidence-tag fallback is worth stealing. The line-proximity fallback should only ever be labeled as heuristic, not dataflow.

### `lekssays__codebadger`

Classification: thin wrapper around Joern.

Codebadger delegates CPG construction and dataflow to Joern. `src/services/cpg_generator.py:21-39` maps languages to Joern CLI commands such as `javasrc2cpg`, `c2cpg`, `jssrc2cpg`, `pysrc2cpg`, and `gosrc2cpg`. `cpg_generator.py:52-78` validates language and picks the command; `cpg_generator.py:80-99` adds repo size checks; `cpg_generator.py:107-136` builds the Joern command with `JAVA_OPTS` and exclusions; `cpg_generator.py:148-176` validates the CPG, starts a Joern server, and loads it.

Operationally, `src/services/joern_server_manager.py:22-57` manages Docker-backed Joern server state and an LRU pool. `joern_server_manager.py:87-150` spawns a containerized server, and `joern_server_manager.py:212-259` calls `importCpg` with expensive overlays and kills the JVM on timeout.

The SAST layer is a Joern query wrapper. `src/tools/taint_analysis_tools.py:19-204` defines default sources, sinks, and sanitizers. `taint_analysis_tools.py:293-310` runs an auto taint query; `taint_analysis_tools.py:344-377` builds regex query inputs. `src/tools/queries/taint_flows_auto.scala:38-95` selects sources/sinks and calls `reachableByFlows`.

Loom translation: steal timeout, cache, and source/sink profile ideas only if security packs become optional. Do not steal Docker/JVM dependency shape.

### `nendotools__tree-sitter-mcp`

Classification: thin wrapper / adjacent AST tool.

Language coverage is decent: `src/core/languages.ts:23-115` configures JS, TS, Python, Go, Rust, Java, C/C++, Ruby, C#, PHP, HTML, Kotlin; `languages.ts:117-166` maps grammars and parser initialization.

The parser extracts only file/function/class-ish nodes. `src/core/parser.ts:17-58` parses files and returns a raw file node for unsupported types. `parser.ts:64-101` parses content and calls `extractElements`; `parser.ts:104-128` recursively extracts function/class nodes; `parser.ts:130-177` builds random IDs, line ranges, and content.

Search is in-memory node search. `src/core/search.ts:13-76` does fuzzy/name/content/path matching. `search.ts:104-184` has a useful payload policy: many results get metadata only, while one to three results may include content. Incremental behavior is basic but real: `src/project/manager.ts:42-87` parses projects into file/node maps, `manager.ts:94-122` reparses changed/deleted files, and `src/core/watcher.ts:18-80` uses debounced chokidar changes. MCP dispatch lives in `src/mcp/handlers.ts:70-218` and `src/mcp/server.ts:25-100`.

Loom translation: progressive content inclusion and simple changed-file invalidation are useful. There is no real call graph or reference graph.

### `wrale__mcp-server-tree-sitter`

Classification: adjacent Tree-sitter query tool.

This is a Tree-sitter workbench, not a CPG. `src/mcp_server_tree_sitter/language/registry.py:23-69` maps a broad set of extensions: Python, JS/TS, Ruby, Rust, Go, Java, C/C++, C#, PHP, Scala, Swift, Dart, Kotlin, Lua, Haskell, OCaml, shell, YAML, JSON, Markdown, HTML, CSS, SQL, proto, Elm, Clojure, Elixir. `registry.py:170-222` loads languages/parsers from `tree_sitter_language_pack`.

Caching is practical: `src/mcp_server_tree_sitter/cache/parser_cache.py:22-37` keys cache entries by language/path/mtime, `parser_cache.py:93-134` handles TTL reads, and `parser_cache.py:136-191` enforces max size.

The MCP surface is broad: `src/mcp_server_tree_sitter/tools/registration.py:88-174` registers project and language tools; `registration.py:230-256` exposes `get_ast`; the same registration file wires `find_text`, `run_query`, query templates, `build_query`, `adapt_query`, `analyze_project`, dependency/complexity tools, similar-code, usage, cache, and prompts. `src/mcp_server_tree_sitter/tools/search.py:139-299` runs Tree-sitter queries over project files. `search.py:401-474` implements AST fingerprint similar-code.

Loom translation: expose a raw Tree-sitter query/debug tool for diagnostics. Do not mistake query captures for a resolved reference graph.

### `ctoth__mcp_server_code_extractor`

Classification: thin wrapper.

This is source extraction. `code_extractor/languages.py:11-31` supports py/js/jsx/ts/tsx/go/rs/java/c/cpp/cs/rb/php/swift/kotlin/scala, and `languages.py:74-92` returns a `tree_sitter_languages` parser or `None`.

`code_extractor/extractor.py:22-48` loads parser/language/query; `extractor.py:49-61` loads `queries/<language>.scm`; `extractor.py:63-99` parses and processes captures; `extractor.py:101-133` extracts a function/class by name; `extractor.py:135-208` processes captures; `extractor.py:210-278` builds symbol hierarchy and line ranges.

The search engine is shallow. `code_extractor/search_engine.py:20-30` claims AST/query cache, but `search_file` reparses every file. `search_engine.py:99-192` has function-call queries only for Python/JS/TS and matches target by string containment. `mcp_server_code_extractor_new.py:324-475` exposes `get_function`, `get_class`, `get_symbols`, `get_lines`, and `get_signature`.

Loom translation: exact symbol extraction patterns are useful. This is not a graph competitor.

### `aimasteracc__tree-sitter-analyzer`

Classification: adjacent tool, graph claims thin.

The parser is well wrapped. `tree_sitter_analyzer/core/parser.py:46-61` has a class-level LRU cache; `parser.py:63-140` caches by path/mtime/size/language and handles reads; `parser.py:153-214` parses code with language detection and safe parser creation. `core/analysis_engine.py:39-99` lazy-initializes CacheService, PluginManager, PerformanceMonitor, LanguageDetector, SecurityValidator, Parser, and QueryExecutor; `analysis_engine.py:127-182` validates, detects, caches, parses, runs plugins, and optional queries.

The project index is persistent but not a deep graph. `mcp/utils/project_index.py:23-89` maps many extensions, and `project_index.py:176-204` persists `.tree-sitter-cache/project-index.json`, summary, hashes, and critical nodes.

The edge layer is mostly lightweight. `mcp/utils/edge_extractors/base.py:8-37` defines an abstract `EdgeExtractor` returning `(source_class, target_class)` tuples. `edge_extractors/python.py:39-70` uses regex-based Python import/class edges with first-party filtering. `mcp/tools/build_project_index_tool.py:18-115` exposes project indexing. `mcp/tools/trace_impact_tool.py:1-10` explicitly calls itself lightweight impact analysis via ripgrep, and `trace_impact_tool.py:74-80` uses ripgrep usage tracing. MCP registration in `mcp/server.py:99-147` wires many tools, including query/read/structure/scale/list/search/grep/outline/trace/batch/project summary/index.

Loom translation: persistent summary plus cheap impact fallback is valuable if labeled. Regex edges are not a semantic graph.

### `Jakedismo__codegraph-rust`

Classification: serious competitor, overbuilt.

Parser/language coverage is serious. `crates/codegraph-parser/src/language.rs:16-127` supports Rust, TS, JS, Python, Go, Java, C/C++, Swift, C#, Ruby, PHP; Kotlin/Dart are disabled for Tree-sitter version conflicts. `language.rs:129-149` detects language by extension and creates parsers. `crates/codegraph-parser/src/parser.rs:44-60` tracks registry, concurrency, chunk size, parsed cache, and parser pool; `parser.rs:74-172` collects files, filters supported, sorts by size descending, uses semaphore plus `buffer_unordered`, and reports files/sec and lines/sec.

The edge model is simple at the Rust type layer: `crates/codegraph-parser/src/edge.rs:1-24` defines `CodeEdge { from, to, edge_type, metadata }`. Storage is not simple. `crates/codegraph-graph/src/surrealdb_storage.rs:19-77` configures SurrealDB, schema version, and cache toggles. `surrealdb_storage.rs:269-327` runs HNSW vector search through SurrealDB; `surrealdb_storage.rs:472-537` maps nodes into multiple embedding dimension columns and compressed content/metadata; `surrealdb_storage.rs:539-605` batches node and edge upserts.

The schema is broad. `schema/codegraph.surql:24-25` uses edge types including calls, defines, imports, uses, extends, implements, references, depends_on, exports, reexports, enables, generates, flows_to, returns, captures, and mutates. `codegraph.surql:159` exposes `fn::edge_types()` including contains, belongs_to, violates_boundary, documents, and specifies. `codegraph.surql:900-951` defines node fields and BM25/HNSW indexes across multiple embedding dimensions. `codegraph.surql:997-1044` repeats that complexity for cached symbol embeddings.

The MCP/tool layer is agentic and output-aware. `crates/codegraph-mcp-tools/src/graph_tool_schemas.rs:19-39` starts a set of graph analysis tools, and `graph_tool_schemas.rs:196-261` describes hybrid search and tool names. `crates/codegraph-mcp-tools/src/graph_tool_executor.rs:49-74` has an executor with LRU result cache; `graph_tool_executor.rs:201-267` truncates oversized tool results; `graph_tool_executor.rs:280-325` validates and executes tool calls.

Loom translation: good ideas are batch parsing, output budgets, hybrid graph/search tools, and graph metrics. Avoid SurrealDB, multi-dimension HNSW schema, and agentic tool sprawl as defaults.

### `vymalo__code-graph`

Classification: adjacent graph analyzer.

The graph data model is understandable. `packages/code-analyzer/src/analyzer/types.ts:12-44` defines `AstNode` with instance ID, global entityId, kind, name, file path, source span, language, visibility, return type, docs, and parent. `types.ts:50-59` defines `RelationshipInfo` with entityId, type, sourceId, targetId, properties, weight, and timestamp.

Parsing is multi-path. `packages/code-analyzer/src/analyzer/parser.ts:61-138` delegates Python, C/C++, Java, Go, C#, and TS/JS; TS/JS goes through `ts-morph`, while other languages generate temp JSON. `parser.ts:147-260` collects temp JSON plus TS results, deduping by entityId. The repo also has language-specific Tree-sitter parsers such as `parsers/go-parser.ts` and `parsers/sql-parser.ts`; SQL is visible but disabled in `parser.ts:13` and `parser.ts:91-93`.

Storage is Neo4j. `packages/code-analyzer/src/analyzer/storage-manager.ts:29-77` saves nodes in batches with `UNWIND` and `MERGE`. `storage-manager.ts:85-132` saves relationships with dynamic Cypher relationship types and `MERGE`s missing source/target nodes, which can create placeholder nodes if relationship resolution outruns node creation. `storage-manager.ts:137-172` flattens node/relationship properties.

Resolution is incomplete. `packages/code-analyzer/src/analyzer/relationship-resolver.ts:38-107` runs Pass 2; `relationship-resolver.ts:69-80` resolves TS/JS modules, inheritance, cross-file interactions, and component usage; `relationship-resolver.ts:82-93` attempts C/C++ include resolution through `ts-morph`; `relationship-resolver.ts:95-101` leaves Python, Java, Go, C#, SQL, etc. as TODOs.

Loom translation: two-pass graph resolution is useful. Neo4j and placeholder MERGE behavior are not.

### `colbymchenry__CodeGraph`

Classification: serious competitor.

This is the closest direct comparator. The SQLite schema is compact and useful. `src/db/schema.sql:19-41` defines symbol nodes with kind, name, qualified name, file path, language, spans, docstring, signature, visibility, flags, and timestamps. `schema.sql:43-55` defines edges with source, target, kind, metadata, line/col, and provenance. `schema.sql:57-81` tracks files and unresolved references. `schema.sql:87-144` adds practical indexes, FTS, composite edge indexes, unresolved-ref indexes, and provenance index.

The Tree-sitter extraction interface is well designed. `src/extraction/tree-sitter-types.ts:45-71` gives language hooks a controlled context to create nodes, visit children/bodies, add unresolved references, and manage scope. `tree-sitter-types.ts:80-208` defines language extractor hooks for functions/classes/methods/interfaces/imports/calls/variables, signatures, visibility, exported/static/async flags, receivers, misparse handling, and bare calls.

The extractor is production-minded. `src/extraction/tree-sitter.ts:117-135` stores file/language/source/tree/nodes/edges/unresolved refs and language extractor. `tree-sitter.ts:140-238` parses, creates a file node, visits AST, captures errors, deletes the WASM tree immediately, and releases source text. `tree-sitter.ts:263-385` dispatches node types into function/class/method/interface/struct/enum/type/import/call/instantiation/Rust impl handling. `tree-sitter.ts:390-433` creates stable nodes and containment edges. `tree-sitter.ts:548-580` extracts type annotations, decorators, and function-body calls.

Resolution has confidence/provenance. `src/resolution/types.ts:12-29` defines unresolved refs; `types.ts:34-43` defines resolved refs with confidence and `resolvedBy` values such as exact-match, import, qualified-name, framework, fuzzy, instance-method, and file-path. `types.ts:65-101` exposes indexed resolution context APIs including name, qualified name, lower-name, imports, project aliases, and re-exports. `types.ts:116-134` defines framework resolvers and framework-specific extraction.

Language coverage is broad. `src/types.ts:63-86` lists TS, JS, JSX, Python, Go, Rust, Java, C/C++, C#, PHP, Ruby, Swift, Dart, Svelte, Vue, Liquid, Pascal, and unknown. `src/extraction/grammars.ts:20-36` maps WASM grammars; `grammars.ts:44-78` maps extensions; `grammars.ts:106-174` lazy-loads grammars and parsers; `grammars.ts:203-226` marks Svelte/Vue/Liquid as custom extractors.

The MCP surface is intentionally context-bounded. `src/mcp/tools.ts:16-30` defines max output and project-size explore budgets. `tools.ts:93-203` exposes `codegraph_search`, `codegraph_context`, callers, callees, impact, and node detail. `tools.ts:205-257` adds explore/status/files. The watcher is practical: `src/sync/watcher.ts:49-75` configures native watch with debounce, `watcher.ts:85-127` filters changes and ignores `.codegraph`, and `watcher.ts:173-195` reruns sync and reschedules when changes arrive during sync.

Loom translation: this repo is the one to study hardest. Steal unresolved refs, confidence/provenance, bounded MCP design, watcher sync, and framework-aware resolution only where it improves discovery per token.

### `er77__code-graph-rag-mcp`

Classification: adjacent tool, noisy/overclaim-prone.

This repo is not a pure no-op. It has substantial code, but it is loaded with task-banner comments, agent layers, and broad claims. The Tree-sitter parser supports JS/TS/TSX/JSX/Markdown/Python/C/C++/C#/Rust/Go/Java/Kotlin/VBA in `src/types/parser.ts:23-39`. `src/parsers/tree-sitter-parser.ts:38-87` defines language loaders for JS, TS, Python, C, C++, Rust, C#, Go, Java, Kotlin. `tree-sitter-parser.ts:113-167` detects languages by extension, defaulting unknown files to JavaScript, which is risky. `tree-sitter-parser.ts:177-212` uses an LRU cache; `tree-sitter-parser.ts:251-423` parses content, calls language-specific analyzers for Python/C#/Rust/C/C++/Go/Java/Kotlin, and returns entities plus optional relationships. `tree-sitter-parser.ts:425-450` supports incremental parsing by editing cached old trees.

The parser entity model is rich, perhaps too rich. `src/types/parser.ts:94-141` includes entity types from function/class/method/interface through magic_method, dataclass, protocol, macro, enum_variant, impl_block, union, and crate. `types/parser.ts:158-180` stores references, modifiers, method type, decorators, and inheritance.

Storage is SQLite plus FTS plus vector side tables. `src/storage/schema-migrations.ts:47-145` creates entities, relationships, files, query cache, and `entities_fts`. `schema-migrations.ts:157-255` adds complexity/language/size, relationship weight/created_at, embeddings, performance metrics, and cache stats. `src/storage/graph-storage.ts:49-69` prepares graph storage; `graph-storage.ts:103-167` prepares insert/update/get statements; `graph-storage.ts:173-275` dedupes and batch-inserts entities.

Semantic/vector support is real but separate and elaborate. `src/semantic/vector-store.ts:75-100` defines a guarded vector store with sqlite-vec state; `vector-store.ts:147-226` initializes DB, loads sqlite-vec, sets WAL/cache/temp/synchronous pragmas, creates `vec_doc_embeddings` when possible or BLOB fallback otherwise.

The MCP/index surface is huge. `src/index.ts:540-581` contains entity resolution with regex fallback and first-entity fallback. `index.ts:748-860` defines batch index sessions under tmp logs. Tests exercise many tools in `test-mcp-methods.js:181-395`, including graph health/stats/get_graph/query/semantic/cross-language/related/clean/reset.

Loom translation: the useful parts are incremental parser cache, SQLite graph basics, and vector fallback. Avoid defaulting unknown to JavaScript, task-banner architecture, and tool sprawl. Noisy abstractions are still abstractions, just wearing a fake mustache.

### `glommer__codemogger`

Classification: adjacent semantic code index.

Codemogger is not a static graph competitor; it is a chunked semantic/keyword index. `src/index.ts:90-127` scans a directory, computes hashes, chunks changed files, embeds stale chunks, cleans deleted files, and rebuilds FTS. `index.ts:144-214` batches chunking and DB writes; `index.ts:214-242` embeds stale chunks in batches; `index.ts:244-267` cleans and reports timings. Search has clear modes: `index.ts:276-318` routes semantic to vector search, keyword to FTS, and hybrid to reciprocal-rank fusion.

Chunking is Tree-sitter based. `src/chunk/treesitter.ts:12-31` initializes parser and lazy-loads WASM languages. `treesitter.ts:33-141` has a broad name extractor for exports, decorators, templates, Ruby singleton methods, C declarators, Go receivers, Scala, Zig, Rust impls, and JS/TS declarations. `treesitter.ts:160-198` creates chunks with file path, language, kind, name, signature, snippet, line range, and file hash. `treesitter.ts:228-293` walks top-level nodes and splits large nodes above 150 lines.

Language coverage is focused but solid. `src/chunk/languages.ts:15-212` configures Rust, JS, TS, TSX, C, Python, Go, Zig, Java, Scala, C++, PHP, C#, and Ruby top-level/split nodes.

Storage is compact. `src/db/schema.ts:3-42` defines codebases, chunks, and indexed files. `schema.ts:44-85` creates per-codebase FTS tables/indexes over name/signature. `src/db/store.ts:125-180` batch-upserts chunks and indexed files, resetting embeddings when chunks change. `store.ts:224-320` handles embedding upserts, vector search, and FTS search.

The MCP surface is intentionally tiny. `src/mcp.ts:66-104` exposes `codemogger_search` with semantic/keyword modes, limit, and snippet option. `mcp.ts:106-159` exposes `codemogger_index` and `codemogger_reindex`, advertising incremental reindexing.

Loom translation: excellent reference for small, honest API surface and incremental chunk indexing. It does not solve call/reference edges.

## Strong Ideas Loom Should Steal

### P0

- Minimal typed edge vocabulary with provenance and confidence. Use `contains`, `defines`, `imports`, `calls`, `references`, maybe `instantiates` and `implements`. CodeGraph's edge provenance and unresolved-ref schema are the cleanest local model: `colbymchenry__CodeGraph/src/db/schema.sql:43-81`.
- Store unresolved references during parsing, resolve later, and record `resolvedBy` plus confidence. CodeGraph's `ResolvedRef` contract is exactly the right shape: `src/resolution/types.ts:34-43`.
- Keep MCP payloads bounded by design. CodeGraph's output max and explore budgets at `src/mcp/tools.ts:16-30`, plus nendotools progressive metadata/content behavior in `src/core/search.ts:104-184`, are directly useful.
- Incremental changed-file invalidation. CodeGraph's watcher in `src/sync/watcher.ts:85-195`, nendotools' update path in `src/project/manager.ts:94-122`, and codemogger's hash-based skip/re-embed path in `src/index.ts:127-242` all point the same way.
- Confidence-tagged fallback for imperfect analysis. code-pathfinder's CFG VDG to flat VDG to line-proximity fallback at `sast-engine/dsl/dataflow_executor.go:46-103` is useful if Loom labels degradation honestly.

### P1

- Add a Tree-sitter query/debug MCP tool. wrale's `run_query`/templates/build/adapt surface and `query_code` implementation in `tools/search.py:139-299` would help agents inspect grammar reality without burning shell tokens.
- Caller/callee/detail tools as first-class read-only MCP surface. code-pathfinder and CodeGraph both expose this cleanly; Loom should make it core, not an afterthought.
- Project summary and file index cache. aimasteracc's `.tree-sitter-cache/project-index.json` path at `mcp/utils/project_index.py:176-204` is a good cheap layer above raw symbols.
- AST fingerprint or structural similarity as a diagnostic, not a primary search result. wrale's `find_similar_code` at `tools/search.py:401-474` is a useful P1 tool.
- Query timeouts and result caches for expensive graph traversals. Codebadger's Joern server timeout behavior and codegraph-rust's result cache/truncation are useful operational patterns without their heavy dependencies.

### P2

- Optional security pack with source/sink/sanitizer profiles. Borrow from codebadger/code-pathfinder only as an opt-in, not core Loom behavior.
- Export formats for graph inspection. Jakedismo's graph tooling and SurrealDB functions show user appetite for architecture/coupling/cycle reports, but Loom should export rather than require a graph DB.
- Tiny declarative neighborhood query language. Use CPG query languages as inspiration, not implementation scope. A small selector like `symbol -> callers depth=2 -> files` is enough.

## Weak / Thin / No-Op Repos

- `lekssays__codebadger`: thin wrapper around Joern. It has useful productization around Docker, timeouts, and profiles, but Joern does the static-analysis work.
- `nendotools__tree-sitter-mcp`: thin AST/search wrapper. It extracts functions/classes and runs in-memory search; no resolved calls, references, dataflow, or graph store.
- `ctoth__mcp_server_code_extractor`: thin extractor. Useful `get_function`/`get_class`/`get_symbols`, but the search path reparses and has narrow language-specific call queries.
- `aimasteracc__tree-sitter-analyzer`: adjacent tool with thin graph claims. Its impact analysis is ripgrep, and Python edges are regex import/class tuples.
- `wrale__mcp-server-tree-sitter`: not weak as a Tree-sitter workbench, but weak as a graph competitor. It is query/capture tooling, not semantic graph storage.
- `er77__code-graph-rag-mcp`: not no-op, but noisy and overclaim-prone. It has real parser/storage/vector code, yet the surface sprawls into agents, metrics, semantic layers, clone detection, and batch sessions. Treat as a warning about tool inflation.

I did not find a pure no-op repo in this slice. The weakest repos still parse or extract something; the issue is overclaiming graph intelligence from AST captures or regex usage search.

## What Loom Should Avoid

- Full Joern parity: DDG/CDG, `REACHING_DEF`, full taint semantics, and vulnerability query packs in core. That is a different product.
- Required JVM, Docker, Neo4j, or SurrealDB. Loom's local SQLite/petgraph shape is a strategic advantage.
- Claiming line proximity or regex import edges as dataflow/reference resolution. Label heuristics as heuristics.
- Dynamic graph schemas with relationship type strings directly interpolated into storage queries. vymalo's dynamic Neo4j relationship writes are flexible, but also easy to make messy.
- Random or unstable node IDs. Stable IDs should include path, symbol kind/name, and span or semantic identity.
- Unbounded source in MCP responses. Default to locations/signatures/neighborhood summaries; include source only when asked or when result count is small.
- Logging indexed source content. Some repos log first failing batches or parser details aggressively. Loom's privacy posture should stay stricter.
- Multi-vector-dimension schema by default. Jakedismo's 384/768/1024/1536/2048/2560/3072/3584/4096 columns are operational drag unless a user explicitly wants that lab.
- Unknown-language fallback to JavaScript. er77's default is convenient and wrong in exactly the way that creates phantom symbols.

## Gaps / Open Questions

- How much cross-language reference resolution should Loom own? Direct imports/calls are P0, framework magic should be opt-in and language-scoped.
- Should Loom expose raw Tree-sitter query execution? I think yes as an expert/debug tool with strict output bounds.
- Should dataflow exist at all? If yes, start with local variable dependency hints and confidence labels, not taint/SAST claims.
- What is the minimal edge schema that survives all supported languages without turning into CPG cosplay?
- Should Loom maintain unresolved references permanently for transparency, or only as a transient indexing artifact? CodeGraph suggests permanent unresolved refs are useful for diagnostics.
- Do we want optional graph exports for Neo4j/Graphviz rather than running a graph DB? That seems like the right compromise.
