# Research: Database & Embedding Model Alternatives for Loom

**Date:** 2026-05-11
**Prompt:** Research better DB + embedding model alternatives. Current: SQLite + sqlite-vec + jina-embeddings-v2-base-code via fastembed (ONNX, CPU). Problem: 10K Go files = ~2 hours to index.
**Refined goal:** Find the combination of embedding model and vector storage that reduces Loom's indexing time from ~2 hours to <15 minutes for a 10K-file codebase, while maintaining code search quality. Must be embeddable, no server, Python bindings.

## Fan-out Plan

| Agent | Sub-question | Surface |
|-------|-------------|---------|
| 1 | Faster code-optimized embedding models | Internet |
| 2 | Embedded vector DB alternatives to sqlite-vec | Internet |
| 3 | Current Loom implementation constraints | Codebase |

---

## Scout / Pre-identification

Skipped — the topic was pre-structured into two clear dimensions (models + DBs) plus a codebase constraint check.

---

## Sub-question 1: Embedding Model Alternatives

### Baseline

jina-embeddings-v2-base-code: 161M params, 768 dims, 8K context, ONNX via fastembed, ~50ms/symbol on CPU. For 160K symbols = ~2.2 hours.

### Tier 1 — Drop-in, High Impact, Low Risk

**1. Enable fastembed parallel workers (`parallel=0`, `batch_size=256`)**
- Currently NOT configured in Loom — running single-threaded, batch_size=default
- Expected: 4-8x throughput on multi-core Macs (M2/M3 have 8-12 cores)
- Zero model change, zero quality impact
- **This alone could cut indexing from 2 hours to 15-30 minutes**

**2. INT8 quantization of jina-v2-base-code**
- Via sentence-transformers ONNX backend: `model_kwargs={"quantize": "int8"}`
- Benchmark: 3.08-3.3x speedup, <1% quality loss (Nixiesearch benchmark)
- jina-v2-base-code HuggingFace repo has 10 quantized ONNX variants
- Need to verify if fastembed already uses INT8 — check `TextEmbedding.list_supported_models()`
- Combined with parallel: could reach **10-20x speedup** → 2 hours → 6-12 minutes

### Tier 2 — Model Swap

| Model | Params | Dims | Code Quality | Est. Speed (CPU) | fastembed? | MRL? |
|-------|--------|------|-------------|-----------------|-----------|------|
| jina-v2-base-code (current) | 161M | 768 | Good | ~50ms | Yes | No |
| jina-v2-base-code INT8 | 161M | 768 | Good (-1%) | ~15ms | Via sbert | No |
| CodeRankEmbed | 137M | ~768 | Good | ~50ms | Unconfirmed | No |
| BGE-small-en-v1.5 INT8 | 45M | 384 | Poor (code) | ~5ms | Yes | No |
| CodeSage Small v2 | 130M | Flex | Good | ~30-50ms | No | Yes |
| EmbeddingGemma 300M | 308M | 768 | Unknown | ~30ms | Manual ONNX | 128-768 |
| Qwen3-Embed-0.6B | 0.6B | 1024 | Good | ~380ms bare | No | Yes |
| jina-code-embed-0.5b | 0.5B | 896 | Better | ~200-500ms | No | 64-896 |
| Nomic Embed Code | 7B | 768 | Excellent | GPU only | No | 256-768 |

### Tier 3 — Alternative Runtimes

**TEI + Metal (Apple Silicon):**
- HuggingFace's Text Embeddings Inference, Rust-based, Metal flag for GPU
- Supports jina-v2-base-code natively. Best latency on M-series.
- Adds process management (HTTP API), not embedded in Python process
- `cargo install text-embeddings-inference --features metal`

**MLX on Apple Silicon:**
- M2 Max: 38ms for BERT-base (vs 50ms fastembed). M1: 179ms (worse).
- No fastembed integration — requires custom code.

**Not recommended:**
- all-MiniLM-L6-v2: 256-token context limit kills code. Skip.
- Model2Vec: 500x faster but unacceptable quality drop for code.
- MPS/Metal via PyTorch: Actually slower than CPU for small-batch embeddings.

### Matryoshka (MRL) — Variable Dimensions

Models supporting dim truncation (e.g., 768→256): vector comparisons 3x faster, index 3x smaller. But: embedding generation time barely changes (model runs fully, output truncated). Helps search time, not index time.

---

## Sub-question 2: Vector Database Alternatives

### Baseline

sqlite-vec: brute-force KNN over `vec0` virtual table. No ANN. ~60-80ms per query at 160K × 768 dims. Single `.db` file. Native FTS5 hybrid search.

### Scoring Matrix

| Candidate | Query Latency | Hybrid FTS+Vec | Portability | Python API | Verdict |
|-----------|--------------|----------------|-------------|------------|---------|
| **sqlite-vec (current)** | ~60-80ms brute | Native FTS5 ✅ | Single file ✅ | Good | Baseline |
| **LanceDB** | <5ms (HNSW) | Native BM25+vec ✅ | Directory ⚠️ | Excellent | **Best overall** |
| **vectorlite** | 5-10x faster than sqlite-vec | SQLite FTS5 ✅ | Single file ✅ | SQL | Hidden gem (ARM penalty) |
| **USearch** | <1ms | None ❌ | Single .usearch ✅ | Simple | Split-store required |
| **Faiss** | <1ms (flat) | None ❌ | Single file ✅ | Untyped | Dominated by USearch |
| **DuckDB + vss** | 7x slower brute | Not combined ❌ | Single file ✅ | Excellent | Not ready (persistence bugs) |
| **ChromaDB** | ~4-8ms (HNSW) | No FTS ❌ | Directory ⚠️ | Good | Blocked — no FTS |
| **Qdrant local** | Brute (20k limit) | Sparse only ❌ | Poor | Good | **Disqualified** |
| **Turbopuffer** | N/A | N/A | Cloud ❌ | Cloud | **Not applicable** |

### Top Recommendations

**1. LanceDB** — Best overall. Native hybrid search (BM25 + vector + RRF reranking), ANN indexing (IVF-HNSW-PQ), disk-based indexes. Trade-off: directory format, not single file. Would require restructuring store layer.

**2. vectorlite** — Drop-in for sqlite-vec. SQLite extension that adds HNSW to sqlite-vec's brute force. 8-100x faster queries. Keeps single-file portability and FTS5. Concern: 3-4x slower on macOS ARM (Apple Silicon penalty). Maintenance status unclear.

**3. USearch + SQLite (split-store)** — Fastest raw ANN (<1ms). But no FTS — requires SQLite for symbols/edges/FTS5, USearch for vectors only. ID synchronization complexity.

### Disqualified

- **Qdrant local**: 20k point cap
- **DuckDB + vss**: Experimental persistence, crash corruption risk
- **ChromaDB**: No FTS
- **Turbopuffer**: Cloud-only
- **Faiss**: Dominated by USearch; Apple Silicon perf issues without custom build

---

## Sub-question 3: Current Implementation Constraints

### Embedder — Clean Interface, Easy to Swap

```python
class Embedder:
    def embed(self, texts: list[str]) -> list[list[float]]
    def embed_single(self, text: str) -> list[float]
    def build_symbol_text(self, name: str, kind: str, context: str) -> str
```

- Only 2 public methods used externally. No abstract Protocol exists.
- fastembed coupled in `_load_model()` and `embed()` via isinstance check and `.tolist()` numpy assumption
- `parallel` and `batch_size` NOT configured — **single-threaded by default**
- Hardcoded `providers=["CPUExecutionProvider"]` — no GPU option

### Vector Store — Tightly Coupled to sqlite-vec

- `_serialize_vec()` uses `struct.pack("...f", ...)` — sqlite-vec-specific binary format
- `search_vec()` uses `WHERE embedding MATCH ? AND k = ?` — sqlite-vec-specific SQL
- `vec0` virtual table creation in schema
- L2 distance assumed in `compute_semantic()`: `1.0 - distance` formula
- Semantic threshold of 0.3 calibrated to jina L2 distances
- `vec_symbols` table name hardcoded in 5+ methods

### Configuration

- `embedding_model` and `embedding_dimensions` configurable but NOT auto-synced
- DB schema is immutable once created (must delete + reindex to change dims)
- RRF_K=60, semantic threshold 0.3 hardcoded (not in config)

### Tests

- 768 hardcoded in 20+ test locations across 9 files
- `test_embedder.py` imports `fastembed.TextEmbedding` for mock specs
- DB tests exercise `search_vec`, `insert_embedding` directly

### Swap Complexity Summary

| Change | Complexity | What Breaks |
|--------|-----------|-------------|
| Enable fastembed parallel/batch | **Trivial** | Nothing |
| Swap fastembed model (same interface) | Low | Config + delete .loom.db + 20 test fixtures |
| Swap to non-fastembed embedding | Medium | Embedder class + test_embedder.py |
| Swap sqlite-vec for another vector DB | **High** | LoomDB.connect/insert/search/remove, _serialize_vec, vec SQL, compute_semantic, all vec tests |
| Change distance metric (L2→cosine) | Medium | compute_semantic formula, threshold, tests |

---

## Verdict

**The 2-hour indexing bottleneck has a trivial fix that doesn't require any DB or model swap.**

Loom's `Embedder.embed()` calls `self._model.embed(texts)` without configuring `parallel` or `batch_size`. fastembed supports `parallel=0` (auto-detect CPU cores) and `batch_size=256` for throughput-optimized bulk embedding. On an 8-core M2/M3 Mac, this alone could provide **4-8x throughput improvement** — cutting indexing from ~2 hours to **15-30 minutes**.

Combined with INT8 quantization (3x speedup, <1% quality loss), the total improvement could be **10-20x** — bringing 10K file indexing to **6-12 minutes**.

For the vector DB, sqlite-vec is adequate at Loom's current scale (160K vectors). The brute-force KNN at ~60-80ms is fine for interactive queries. If query latency becomes a problem at larger scales, **vectorlite** is the lowest-risk upgrade (drop-in SQLite extension with HNSW, keeps single-file + FTS5). **LanceDB** is the long-term answer for scale but requires a store layer rewrite.

---

## Plan

### Immediate (this sprint)

1. **Enable `parallel=0` and `batch_size=256` in `Embedder.embed()`** — 1-line change, 4-8x throughput
2. **Investigate if fastembed already loads INT8 ONNX for jina-v2-base-code** — run `TextEmbedding.list_supported_models()`, check model config
3. **If not INT8**: try `sentence-transformers` ONNX backend with `model_kwargs={"quantize": "int8"}` — benchmark vs fastembed

### Short-term (next 2 sprints)

4. **Introduce `EmbedderProtocol`** — typing.Protocol with `embed()`, `embed_single()`, `build_symbol_text()`. Decouple tests from fastembed.
5. **Extract vector store interface** — `VectorStore` Protocol with `insert`, `search`, `delete`. Isolate sqlite-vec coupling.
6. **Parameterize 768 in tests** — use `config.embedding_dimensions` fixture

### Long-term (when scale demands)

7. **Evaluate vectorlite** — benchmark on Apple Silicon for the ARM penalty
8. **Evaluate LanceDB** — prototype split-store or full migration
9. **Consider TEI + Metal** — if pure Python embedding remains too slow even with parallelism

---

## Open Questions

1. Does fastembed already use INT8 ONNX for jina-v2-base-code? If yes, the 50ms is the floor for this model.
2. What batch size does fastembed use by default? If it's already 256, the parallel workers are the key lever.
3. vectorlite ARM performance on M1/M2 — the 3-4x penalty needs direct measurement.
4. Is Alex Garcia planning ANN support for sqlite-vec? If so, vectorlite becomes unnecessary.
5. LanceDB BM25 vs SQLite FTS5 for code tokenization (camelCase, snake_case handling)?

---

## Sources

### Embedding Models
- [Modal: 6 Best Code Embedding Models Compared](https://modal.com/blog/6-best-code-embedding-models-compared)
- [Nixiesearch: LLM Embeddings 3X Faster with Quantization](https://medium.com/nixiesearch/how-to-compute-llm-embeddings-3x-faster-with-model-quantization-25523d9b4ce5)
- [HuggingFace: CPU Optimized Embeddings with Optimum Intel](https://huggingface.co/blog/intel-fast-embedding)
- [Sentence Transformers: Speeding up Inference](https://sbert.net/docs/sentence_transformer/usage/efficiency.html)
- [Qdrant: FastEmbed Optimize Throughput](https://qdrant.tech/documentation/fastembed/fastembed-optimize/)
- [MLX Benchmarks: arxiv 2510.18921](https://arxiv.org/abs/2510.18921)
- [ONNX Runtime: CoreML Execution Provider](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html)
- [HuggingFace TEI GitHub](https://github.com/huggingface/text-embeddings-inference)
- [jinaai/jina-embeddings-v2-base-code HuggingFace](https://huggingface.co/jinaai/jina-embeddings-v2-base-code)
- [jinaai/jina-code-embeddings-0.5b HuggingFace](https://huggingface.co/jinaai/jina-code-embeddings-0.5b)
- [nomic-ai/nomic-embed-code HuggingFace](https://huggingface.co/nomic-ai/nomic-embed-code)
- [codesage/codesage-large-v2 HuggingFace](https://huggingface.co/codesage/codesage-large-v2)
- [Qwen3 Embedding Blog](https://qwenlm.github.io/blog/qwen3-embedding/)

### Vector Databases
- [Alex Garcia: sqlite-vec Stable Release (benchmarks)](https://alexgarcia.xyz/blog/2024/sqlite-vec-stable-release/index.html)
- [LanceDB Docs: Hybrid Search](https://docs.lancedb.com/search/hybrid-search)
- [DuckDB: What's New in VSS Extension](https://duckdb.org/2024/10/23/whats-new-in-the-vss-extension)
- [USearch Benchmarks](https://github.com/unum-cloud/usearch/blob/main/BENCHMARKS.md)
- [vectorlite GitHub](https://github.com/1yefuwang1/vectorlite)
- [Qdrant Edge](https://qdrant.tech/edge/)
- [ChromaDB Performance Docs](https://docs.trychroma.com/guides/deploy/performance)
- [sqlite-vec Issue #186 (performance at scale)](https://github.com/asg017/sqlite-vec/issues/186)
