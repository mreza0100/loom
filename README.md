# Loom

**AI code intelligence via vector search.** An MCP server that replaces `grep` as the code understanding layer for AI coding tools.

When Claude Code, Cursor, or Copilot needs to understand code, they grep — burning tokens on thousands of lexical matches, most of them noise. Loom gives them **vector search** over a pre-indexed codebase: fewer calls, fewer tokens, more relevant symbols.

> grep finds what you asked for. Loom finds what you need.

## Why

AI coding tools are blind in a specific way: they understand code *locally* but not *relationally*. When an AI agent greps for `resolveSession`, it gets 14 hits — exact lexical matches. But it doesn't see:

- `SessionValidator` — structurally coupled, defined 3 lines below
- `SESSION_TIMEOUT_MS` — the constant that governs its behavior, in a different file
- `refreshToken` — the sibling function that breaks when you change the session shape

An AI agent can't grep for what it doesn't know to look for. Loom inverts this: **one vector search returns the neighborhood of related code, ranked by relevance.**

## Benchmark: Loom vs Grep

Benchmarked on **webpack/lib** (587 JS files, 9,654 symbols) across 5 real code understanding tasks:

| Metric | Loom | Grep | Winner |
|--------|------|------|--------|
| **Symbols discovered** | **309** | ~65 | Loom — **4.8x more** |
| **Tool calls needed** | **21** | 52 | Loom — **2.5x fewer** |
| **False positives** | **0** | ~860 | Loom — **860x less noise** |
| **Engine time** | **27.9s** | 176s | Loom — **6.3x faster** |
| **Blast radius recall** | 82% | 100% | Grep — 18% gap |

**Highlight — concept search:** Loom found the complete webpack tree-shaking plugin ecosystem (10 related plugins) through 2 semantic searches + 2 related expansions. Grep required 9 different search terms and still missed 4. You can't grep for what you don't know exists.

<details>
<summary>Full benchmark breakdown</summary>

### Per-Task Results

| Task | Loom | Grep | Loom advantage |
|------|------|------|----------------|
| **Call chain** — trace who calls `compile()` | 87 symbols, 6 calls, 1.2s | 18 symbols, 22 calls, 64s | 4.8x symbols, 3.7x fewer calls |
| **Blast radius** — what breaks if `makePathsRelative` changes | 77 symbols, 82% recall | 22 symbols, 100% recall | Grep wins on recall |
| **Concept search** — find "tree shaking" code | 120 symbols, 10 semantic wins | ~8 symbols, 0 semantic | Impossible for grep |
| **Modify & verify** — add function, check index | Found + impact verified | Found | Tie |
| **Ambiguity** — disambiguate 909 `create` hits | 25 ranked results | 909 noise | Loom: signal. Grep: noise |

### Three Signals

| Signal | How it works |
|--------|-------------|
| **Structural** (AST graph) | Tree-sitter → symbols + call/import/type edges → NetworkX graph |
| **Semantic** (embeddings) | Local embedding model → vector similarity → conceptual matches |
| **Evolutionary** (git co-change) | Git log mining → symbols that always change together |

Each signal alone produces false positives. Together, they triangulate.

</details>

## Quick Start

```bash
# Clone and install
git clone https://github.com/mreza0100/loom.git
cd loom
uv sync
cargo build --workspace
```

### Connect to Claude Code

Add to `.mcp.json` in any project you want to index:

```json
{
  "mcpServers": {
    "loom": {
      "command": "/path/to/loom/target/debug/loom-mcp",
      "args": [
        "--target", "/path/to/your/project"
      ]
    }
  }
}
```

### Connect to Other Tools

<details>
<summary>Claude Desktop, Cursor, VS Code</summary>

**Claude Desktop** — Settings → Developer → Edit Config:
```json
{
  "mcpServers": {
    "loom": {
      "command": "/path/to/loom/target/debug/loom-mcp",
      "args": ["--target", "/path/to/your/project"]
    }
  }
}
```

**Cursor** — Settings → MCP → Add server:
- Command: `/path/to/loom/target/debug/loom-mcp --target /path/to/your/project`

**VS Code** — `.vscode/mcp.json`:
```json
{
  "servers": {
    "loom": {
      "command": "/path/to/loom/target/debug/loom-mcp",
      "args": ["--target", "${workspaceFolder}"]
    }
  }
}
```

</details>

## MCP Tools

| Tool | What it does |
|------|-------------|
| `search(query)` | Hybrid keyword + semantic search, results expanded with coupled symbols |
| `related(symbol)` | All structurally and semantically coupled symbols with scores |
| `impact(symbol)` | Blast radius — what breaks if this symbol changes |
| `neighborhood(file, line)` | Everything related to the code at a specific location |
| `reindex()` | Trigger incremental re-index (only changed files) |
| `status()` | Index health, symbol count, freshness |

## How It Works

```
Codebase (files on disk)
    │
    ▼
Indexer Pipeline
    ├── Tree-sitter ──► AST ──► symbols + edges ──► Graph (NetworkX)
    ├── Embedding Model (local) ──► vectors ──► Vector Store (sqlite-vec)
    └── Git Analyzer ──► co-change matrix ──► Coupling Store
    │
    ▼
Unified SQLite Database (.loom.db)
    │
    ▼
MCP Server (FastMCP) ──► search / related / impact / neighborhood
```

Loom computes a **coupling score** per relationship:

```
coupling(A, B) = w1 * structural(A, B) + w2 * semantic(A, B) + w3 * evolutionary(A, B)
```

When you search for A, Loom returns A's results **plus** every symbol whose coupling score exceeds the threshold — ranked, with scores and reasons.

## Tech Stack

| Component | Choice |
|-----------|--------|
| Language | Rust core + legacy Python reference |
| MCP Framework | [rmcp](https://docs.rs/rmcp/latest/rmcp/) |
| AST Parser | [tree-sitter](https://tree-sitter.github.io/) |
| Graph Engine | petgraph |
| Embeddings | jina-embeddings-v2-base-code via Candle |
| Vector Store | SQLite blob vector store now; sqlite-vec backend planned |
| Persistence | `.loom/loom.db` SQLite database per project |

## What Happens on First Run

1. Run `loom-mcp reindex --target /path/to/project`.
2. Parses supported source files using tree-sitter → extracts symbols and relationships
3. Generates embeddings using a local model (downloads on first reindex)
4. Builds the structural graph (call, import, type, co-location edges)
5. Mines git history for evolutionary coupling
6. Stores everything in `.loom/loom.db` — subsequent starts are instant (content-hash based incremental updates)

## Supported Languages

Currently: **Python, JavaScript, TypeScript, Go, Java, Rust, and C#**.

The architecture supports any language with a tree-sitter grammar. More languages are planned.

## Development

```bash
uv sync                              # install Python dev deps
cargo build --workspace              # build Rust crates
cargo run -p loom-mcp -- status --target .
cargo run -p loom-mcp -- reindex --target /path/to/project
cargo test --workspace               # Rust tests
uv run pytest                        # Python reference tests
uv run ruff check && uv run mypy      # Python lint + type check
```

## License

MIT
