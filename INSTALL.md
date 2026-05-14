# Installing Loom MCP Server

Loom is a Rust MCP server that gives AI coding tools vector search over a pre-indexed codebase.

## Requirements

- Rust stable toolchain
- Cargo

## Build

```bash
git clone https://github.com/mreza0100/loom.git
cd loom
cargo build --workspace
```

Use `target/debug/loom-mcp` during development or `target/release/loom-mcp` after a release build.

## Claude Code

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

## Claude Desktop

Open **Settings > Developer > Edit Config** and add:

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

## Cursor

Add a server with:

- Name: `loom`
- Command: `/path/to/loom/target/debug/loom-mcp --target /path/to/your/project`

## VS Code

Add to `.vscode/mcp.json`:

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

## First Run

1. Loom walks supported files in the target project.
2. Tree-sitter extracts symbols and relationships.
3. The embedder generates local vectors.
4. SQLite stores symbols, edges, vectors, file hashes, and co-change data in `.loom/loom.db`.
5. Later runs use content hashes for incremental updates.

The active runtime contract is documented in [docs/dev/runtime-contract.md](docs/dev/runtime-contract.md), including MCP JSON, `status()` fields, scoring semantics, schema version, storage path, and benchmark metrics.

## Tools

| Tool | What it does |
|------|-------------|
| `search(query)` | Hybrid retrieval with coupled-symbol expansion |
| `symbols(query)` | Exact/suffix symbol enumeration for known names and repeated methods |
| `related(symbol)` | Related symbols and relationship scores |
| `impact(symbol)` | Ranked blast radius |
| `neighborhood(file, line)` | Related code around a location |
| `inspect(handle)` | Bounded source inspection for a selected Loom handle |
| `evidence_pack(query)` | Citable evidence bundle for final answers |
| `reindex()` | Re-index changed files |
| `status()` | Index health |

## CLI

The MCP binary also exposes the same runtime as a CLI:

```bash
/path/to/loom/target/debug/loom-mcp --target /path/to/your/project reindex
/path/to/loom/target/debug/loom-mcp --target /path/to/your/project search "findProjectSpec" --limit 5
/path/to/loom/target/debug/loom-mcp --target /path/to/your/project symbols execute --file-prefix sources/commands --kind method
/path/to/loom/target/debug/loom-mcp --target /path/to/your/project evidence-pack "package manager integrity"
/path/to/loom/target/debug/loom-mcp --target /path/to/your/project inspect 'symbol:idx-...:42' --line-budget 24
```

JSON is the default output. Add `--format text` for a compact terminal view.

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```
