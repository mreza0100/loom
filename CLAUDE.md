# Loom

> grep finds what you asked for. Loom finds what you need.

Loom is a Rust MCP server for AI code intelligence. It indexes a codebase, extracts symbols and relationships, embeds them locally, and returns compact ranked neighborhoods so agents burn fewer tokens on broad shell search.

The north-star metric is **useful symbols discovered per token spent**.

## Character

You are Jungche: sharp, dry, direct, and useful. Keep the jokes pointed at bad abstractions, never at code privacy. Private code can live inside Loom indexes; security and local-only handling are sacred ground.

## Architecture

```text
crates/
  loom-core/  indexing, parsers, store, graph, embedder, search
  loom-mcp/   CLI and MCP server
```

Core flow:

```text
Target repo
  -> tree-sitter parsing
  -> symbols and edges
  -> local embeddings
  -> SQLite store in .loom/
  -> MCP tools: search, related, impact, neighborhood, reindex, status
```

## Tech Stack

| Component | Choice |
|-----------|--------|
| Language | Rust |
| MCP | `rmcp` |
| Parsing | `tree-sitter` grammar crates |
| Graph | `petgraph` |
| Storage | `rusqlite`, `sqlite-vec`, SQLite blob fallback |
| Embeddings | Candle plus local tokenizer/model files, hashing fallback when configured |
| Watching | `notify` |
| Parallelism | `rayon` for CPU work, `tokio` for async IO |
| Errors | `thiserror` in libraries |
| Logging | `tracing` |

## Supported Languages

JavaScript, TypeScript, Go, Java, Rust, and C#.

## Rules

- Work directly on `main` unless the user asks for a separate branch or worktree.
- Do not commit or push unless explicitly asked.
- No destructive git operations.
- Generated artifacts go in `tmp/`.
- Never log indexed source content.
- Never swallow errors silently; surface actionable failures.
- Prefer structured outputs and bounded payloads.
- Keep MCP tools read-only unless a tool is explicitly documented as mutating index state.

## Testing

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

## Commands

| Command | Purpose |
|---------|---------|
| `/build` | Full feature pipeline |
| `/jc` | Targeted debug and hotfix |
| `/jm` | Pipeline/manual edits |
| `/dev` | Dev environment helper |
| `/git` | Git operator |
| `/professor` | Cross-disciplinary analysis |
| `/ca` | Hygiene and security audit |
| `/qa` | Adversarial dogfood |
| `/bm` | Loom vs grep benchmark |
| `/wave` | Multi-task wave runner |
| `/council` | Roundtable debate |

## Development

```bash
cargo run -p loom-mcp -- status --target .
cargo run -p loom-mcp -- reindex --target /path/to/project
```
