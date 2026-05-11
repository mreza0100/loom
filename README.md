# Loom

**AI code intelligence via vector search.** Loom is an MCP server that replaces broad `grep` loops with ranked code neighborhoods for AI coding tools.

> grep finds what you asked for. Loom finds what you need.

## Why

AI coding agents understand local context, but they stumble when the relevant code is related by structure, behavior, or concept instead of exact text. Loom indexes a repo ahead of time and lets the agent ask for the neighborhood around a symbol, behavior, or file location.

The metric that matters: **useful symbols discovered per token spent.**

## Quick Start

```bash
git clone https://github.com/mreza0100/loom.git
cd loom
cargo build --workspace
```

## Connect to Claude Code

Add this to the target project's `.mcp.json`:

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

## MCP Tools

| Tool | What it does |
|------|-------------|
| `search(query)` | Hybrid keyword plus semantic search, expanded with coupled symbols |
| `related(symbol)` | Structurally and semantically related symbols with scores |
| `impact(symbol)` | Blast radius for a symbol change |
| `neighborhood(file, line)` | Related code around a specific location |
| `reindex()` | Incremental re-index for changed files |
| `status()` | Index health, freshness, and counts |

## How It Works

```text
Codebase
  |
  v
Indexer
  - tree-sitter symbols and edges
  - local embeddings
  - git co-change analysis
  |
  v
SQLite store in .loom/
  |
  v
Rust MCP server via rmcp
```

Loom fuses three signals:

```text
coupling(A, B) = structural + semantic + evolutionary
```

Search returns exact hits plus related code that exact text search would miss.

## Tech Stack

| Component | Choice |
|-----------|--------|
| Language | Rust |
| MCP Framework | `rmcp` |
| AST Parser | `tree-sitter` |
| Graph Engine | `petgraph` |
| Embeddings | Jina code embeddings via Candle, hashing fallback when configured |
| Vector Store | SQLite blob backend now, sqlite-vec backend supported |
| Persistence | `.loom/loom.db` per target project |

## Runtime Contract

The active implementation is the Rust `loom-mcp` binary. Python runtime paths are historical only. See [docs/dev/runtime-contract.md](docs/dev/runtime-contract.md) for the MCP JSON contract, `status()` fields, scoring semantics, schema version, storage path, and benchmark metric requirements.

## Supported Languages

Currently: **JavaScript, TypeScript, Go, Java, Rust, and C#**.

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
cargo run -p loom-mcp -- status --target .
cargo run -p loom-mcp -- reindex --target /path/to/project
```

## License

MIT
