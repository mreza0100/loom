# Research: Optimized Rust Techniques for Loom Rewrite

**Date:** 2026-05-11
**Goal:** Identify the most optimized, production-proven Rust patterns and libraries for rewriting Loom's code intelligence stack (indexer, embedder, search, MCP server) from Python to Rust.

---

## (1) Prompt

Research the most optimized and elegant Rust techniques for building a high-performance code intelligence tool like Loom. Current stack: Python 3.12+, FastMCP, tree-sitter, fastembed (ONNX), SQLite + sqlite-vec, NetworkX. Planning full Rust rewrite.

## (2) Fan-Out Plan

| Agent | Sub-question |
|-------|-------------|
| 1 | Zero-copy patterns (arena allocators, mmap, Cow), SIMD vector ops, serialization |
| 2 | Rust ML inference: candle vs ort vs burn for embedding generation |
| 3 | Rust SQLite bindings (rusqlite), sqlite-vec, graph representations (petgraph) |
| 4 | Async architecture (tokio + rayon), tree-sitter Rust bindings, pipeline patterns |
| 5 | MCP server in Rust (rmcp), cross-platform binary distribution |

## (3) Findings

### A. Zero-Copy, SIMD, Serialization

**Arena Allocators (bumpalo):**
- 2-5x faster than std allocator for batch AST allocation patterns
- Pattern: arena-allocate per-file AST, extract owned Strings for symbols, drop arena
- `bumpalo-herd` for concurrent allocation across rayon threads
- Caveat: `'arena` lifetime propagates through entire data model

**Memory-Mapped Files (memmap2):**
- ripgrep switched AWAY from mmap as default — bad for many small files (10K source files)
- Page fault overhead dominates for small files; buffered `fs::read()` wins
- mmap only for rare large files (auto-generated code, minified bundles)
- Recommendation: `fs::read()` for source files, no mmap

**Cow<str>:**
- Symbol names are almost always direct substrings of source — `Cow<'src, str>` avoids String allocation
- Convert to owned only at persistence boundary (DB insert)

**SIMD Vector Operations:**
- `std::simd` still nightly-only (2025-2026), no confirmed stable date
- **SimSIMD** — 350+ SIMD kernels, runtime dispatch across AVX2/AVX-512/NEON, 4.7x faster than NumPy
- **LanceDB's approach** — manual `std::arch` intrinsics, zero allocation in hot path, AVX-2 = 3.85x over naive
- **sqlite-vec already has SIMD** (AVX + NEON) — benchmark before adding custom SIMD
- Cross-platform: `#[cfg(target_arch)]` dispatch, NEON mandatory on aarch64, AVX-2 needs runtime detection
- For stable Rust: use `pulp` (powers faer) or SimSIMD crate

**Serialization:**
- Vector data: raw bytes in SQLite BLOBs is optimal (768 floats * 4 bytes = 3,072 bytes). Zero deserialization cost.
- Symbol metadata: SQLite handles mixed types natively, no additional serialization needed
- rkyv (zero-copy, 1.2ns access) has no schema migration — risky for evolving data model
- bincode: pragmatic choice for any binary cache layer

**Bloop Reference:**
- Cold index of 1.3M-line, 9GB monorepo: ~4m20s CPU-only
- Uses Tantivy (FTS) + Qdrant (vectors) + MiniLM embeddings
- Performance comes from Rust toolchain efficiency, not manual memory tricks

### B. ML Inference: candle vs ort vs burn

**Candle (HuggingFace, Rust-native):**
- Powers HuggingFace Text Embeddings Inference (TEI) in production
- **First-class JinaBERT support** — ALiBi positional encoding, loads safetensors natively
- Metal backend confirmed (`candle-metal-kernels`), CUDA full support
- Memory: mmap for safetensors (zero-copy weight loading), no arena pre-allocation
- ~800MB-1.5GB working set for 161M param model (vs 60GB with ONNX arena)
- Quantization: GGUF INT8/INT4 supported (primarily exercised on LLMs, BERT less documented)
- `tokenizers` crate v0.23.1 — production-grade, loads jina's tokenizer.json natively

**ort (Rust ONNX Runtime, v2.0.0-rc.12):**
- Same C++ ONNX Runtime underneath — same 60GB arena problem
- `disable_cpu_mem_arena()` available on SessionBuilder
- Used by Bloop, SurrealDB, Google Magika
- fastembed-rs wraps ort, supports jina-v2-base-code explicitly

**Burn:**
- Skip. Compile-time ONNX-to-Rust codegen, no runtime model swapping, 15-20% higher memory than candle, no JinaBERT examples

**TEI as sidecar (fastest path):**
- `brew install text-embeddings-inference` on macOS
- HTTP/gRPC API, handles JinaBERT + Metal + batching + memory
- Memory ~1-2GB for 161M model
- Adds network hop but throughput dominates for batch indexing

**Verdict: Candle for embedded inference, TEI for quick wins.**

### C. SQLite + Graph

**rusqlite (v0.39.0):**
- Production-mature, idiomatic Rust API
- WAL mode + r2d2 connection pool for concurrent read/write
- `bundled` feature for zero-dep distribution
- Custom scalar functions via `functions` feature (for vector similarity)
- Pattern: 1 writer (Mutex) + N readers (r2d2 pool)

**sqlite-vec from Rust:**
- First-class Rust support via `sqlite-vec` crate (static compilation)
- **Brute-force only** (no ANN) — ~100ms at 140K vectors, acceptable for now
- ANN (HNSW/IVF) tracked in issue #25, not shipped yet

**Alternative vector stores:**
- **USearch** — C++ HNSW with Rust FFI, 10x faster than FAISS, sub-ms at 140K. Best performance.
- **hnsw_rs** — pure Rust HNSW, 62K req/s on 784d, parallel insert/search. Best pure-Rust option.
- **LanceDB** — overkill for 140K scale, good for future growth to millions
- hora — abandoned, skip

**petgraph (v0.8.3):**
- CSR format: ~8MB for 1M edges (vs ~200-400MB in NetworkX) — 10-20x memory reduction
- BFS/DFS: sub-millisecond for depth 3-4 on 1M-edge graph
- codemem validates this at Loom's exact scale (<1ms BFS, <2ms HNSW)

**Architecture recommendation:**
- Short term: unified SQLite + sqlite-vec (single file, acceptable latency)
- Medium term: SQLite (relational + FTS5) + hnsw_rs (vector ANN, in-memory) + petgraph CSR (graph, in-memory), rebuilt from SQLite on startup

### D. Async Architecture + Tree-sitter

**Pipeline architecture (the key design):**
```
ignore::WalkParallel (num_cpu threads, gitignore-aware)
    ──bounded mpsc(32)──>
rayon worker pool (one Parser per thread, Cow<str> symbols)
    ──bounded mpsc(batch_size*2)──>
Embedder (single Arc<OnnxSession> or candle model, large batches)
    ──bounded mpsc(20)──>
SQLite writer (tokio + spawn_blocking, batched transactions)
```

**Critical rules:**
- Never call `par_iter()` on tokio worker threads (PostHog disaster: 2.5s p99 spikes)
- Bridge rayon → tokio via oneshot channel
- One `Parser` per rayon thread (`parse()` takes `&mut self`)
- Grammars (`Language`) are shareable via `Arc<Language>`
- ONNX Session thread-safe for `run()` but better to maximize batch size than concurrency
- Bound EVERY channel — unbounded = OOM when embedder lags
- Budget: `tokio(num_cpu/2)` + `rayon(num_cpu)`

**Tree-sitter Rust:**
- `tree-sitter` crate, Parser is Send+Sync but parse() needs &mut self
- tree-sitter-language-pack: 305+ parsers, on-demand download + caching
- Incremental parsing: pass old_tree to parse() for sub-second re-index
- Each grammar adds ~50-200KB binary size; 15 languages = ~2-3MB

**File I/O:**
- `ignore` crate (from ripgrep) — parallel walk, gitignore-aware, work-stealing
- `fs::read()` for source files (NOT mmap — ripgrep's own conclusion)
- `notify` crate (v7.x) for file watching — used by rust-analyzer, Deno, Zed

### E. MCP Server + Distribution

**rmcp (official Rust MCP SDK, v1.6.0):**
- Official Anthropic-maintained SDK, 3.4K stars, 76 releases
- stdio transport for local tools (what Claude Code/Cursor use)
- `#[tool_handler]` macro: auto-generates ServerHandler, tool discovery, schema registration
- Working tool implementation: ~60-70 lines total
- Streamable HTTP transport for future cloud hosting

**Distribution stack:**
- **cargo-dist** — generates CI workflows, builds for 5+ platforms, auto-generates Homebrew formulas
- **cargo-binstall** — `cargo binstall loom-mcp` downloads pre-built binary
- **Maturin + PyPI** — `pip install loom-mcp` installs Rust binary as platform wheel (ruff's approach)
- **Homebrew tap** — `brew install loom-mcp`
- Binary size: ~8-20MB (Rust + SQLite + tree-sitter), ONNX Runtime ~13-18MB, model ~300-547MB downloaded on first run

**The ONNX distribution problem:**
- ONNX Runtime shared lib must be distributed alongside binary OR downloaded at first run
- Embedding model (~547MB) downloaded on first run to `~/.loom/models/`
- Same UX as Ollama — users accept this pattern
- fastembed-rs default cache is `/tmp` (bad!) — must configure persistent path

---

## (5) Verdict

The Rust rewrite is architecturally sound. The stack is mature enough: `rmcp` for MCP, `candle` for embeddings (no ONNX arena problem), `rusqlite` + `sqlite-vec` for storage, `petgraph` CSR for graphs, `rayon` + `tokio` for parallel pipeline, `ignore` + `notify` for file operations. The key insight: **candle eliminates the 60GB arena problem entirely** by using mmap'd safetensors instead of ONNX Runtime's aggressive arena allocator.

## (6) Plan — Recommended Rust Stack

| Component | Crate | Why |
|-----------|-------|-----|
| MCP Server | `rmcp` (stdio) | Official SDK, macro-driven tools |
| Async Runtime | `tokio` | IO coordination, MCP transport |
| CPU Parallelism | `rayon` | File parsing, batch processing |
| AST Parser | `tree-sitter` + language-pack | 305+ languages, incremental parsing |
| ML Inference | `candle` + `tokenizers` | No arena, Metal support, JinaBERT native |
| SQLite | `rusqlite` (bundled) | WAL, FTS5, custom functions |
| Vector Store | `sqlite-vec` (now), `hnsw_rs` (later) | Brute-force acceptable at 140K, ANN when needed |
| Graph | `petgraph` (CSR) | 10-20x less memory than NetworkX |
| File Walking | `ignore` | ripgrep's gitignore-aware parallel walker |
| File Watching | `notify` | rust-analyzer, Zed, Deno all use it |
| Vector SIMD | SimSIMD or `std::arch` | Runtime dispatch, AVX2+NEON |
| Serialization | `serde` + `bincode` | Vectors as raw BLOBs, metadata via serde |
| Distribution | cargo-dist + Maturin | GitHub Releases + PyPI + Homebrew |
| Error Handling | `anyhow` + `thiserror` | anyhow at boundaries, thiserror for library |
| Logging | `tracing` | Structured, async-safe, to stderr |

## (7) Open Questions

1. **Candle INT8 quantization for JinaBERT** — infrastructure exists for LLMs but BERT-class quantization via GGUF less documented. Needs prototype.
2. **sqlite-vec ANN timeline** — if HNSW ships in sqlite-vec before Loom needs it, unified architecture wins by default.
3. **bundled SQLite perf regression** — issue #1621 reports 3,500x slower on NixOS. Test on macOS before committing.
4. **rmcp API stability** — v1.x had breaking changes. Pin version, upgrade deliberately.
5. **Windows ONNX distribution** — hardest platform. `onnxruntime.dll` + MSVC runtime deps.
6. **macOS code signing** — unsigned binaries get Gatekeeper-blocked. cargo-dist handles ad-hoc signing.
