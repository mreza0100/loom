# Installing Loom MCP Server

Loom is an MCP server that gives AI coding tools vector search over your codebase. One search call returns the relevant neighborhood of code — ranked, with coupling scores — instead of thousands of grep matches.

## Requirements

- Python 3.12+
- [uv](https://docs.astral.sh/uv/) (recommended) or pip

## Quick Start

```bash
# Clone the repo
git clone https://github.com/yourusername/loom.git
cd loom

# Install dependencies
uv sync
```

## Connecting to Your AI Tool

### Claude Code (CLI / Desktop)

Add to your project's `.mcp.json` in the repo you want to index:

```json
{
  "mcpServers": {
    "loom": {
      "command": "uv",
      "args": [
        "run",
        "--directory", "/path/to/loom",
        "python", "-m", "loom",
        "/path/to/your/project"
      ]
    }
  }
}
```

Replace `/path/to/loom` with the absolute path to your Loom clone, and `/path/to/your/project` with the codebase you want indexed.

### Claude Desktop

Open **Settings > Developer > Edit Config** and add to `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "loom": {
      "command": "uv",
      "args": [
        "run",
        "--directory", "/path/to/loom",
        "python", "-m", "loom",
        "/path/to/your/project"
      ]
    }
  }
}
```

### Cursor

Open **Settings > MCP** and add a new server:

- **Name:** loom
- **Command:** `uv run --directory /path/to/loom python -m loom /path/to/your/project`

### VS Code (Copilot / Continue)

Add to your `.vscode/mcp.json`:

```json
{
  "servers": {
    "loom": {
      "command": "uv",
      "args": [
        "run",
        "--directory", "/path/to/loom",
        "python", "-m", "loom",
        "${workspaceFolder}"
      ]
    }
  }
}
```

### Any MCP-Compatible Tool

Loom uses [FastMCP](https://github.com/jlowin/fastmcp) and speaks standard MCP over stdio. The server command is:

```bash
uv run --directory /path/to/loom python -m loom /path/to/your/project
```

## What Happens on First Run

1. Loom parses every JS/TS file in the target directory using tree-sitter
2. Extracts symbols (functions, classes, methods, variables) and their relationships
3. Generates embeddings for each symbol using a local model (jina-embeddings-v2-base-code)
4. Stores everything in a `.loom.db` SQLite file in the target directory
5. Starts a file watcher for incremental updates

First index takes **1-5 minutes** depending on codebase size (the embedding model downloads ~270MB on first run). Subsequent starts are instant — only changed files are re-indexed.

## Available Tools

Once connected, your AI tool gets these MCP tools:

| Tool | What it does |
|------|-------------|
| `search(query)` | Hybrid keyword + semantic search, results expanded with coupled symbols |
| `related(symbol)` | All structurally and semantically coupled symbols |
| `impact(symbol)` | Blast radius — what breaks if this symbol changes |
| `neighborhood(file, line)` | Everything related to the code at a specific location |
| `reindex()` | Force a full re-index |
| `status()` | Index health and stats |

## Configuration

Loom works with zero configuration. The defaults handle most codebases:

| Setting | Default | What it controls |
|---------|---------|-----------------|
| Watch extensions | `.js .jsx .ts .tsx .mjs .cjs` | File types to index |
| Max file size | 1MB | Skip files larger than this |
| Excluded dirs | `node_modules`, `.git`, `dist`, `build` | Directories to skip |
| Embedding model | `jinaai/jina-embeddings-v2-base-code` | Local embedding model |
| DB location | `<target>/.loom.db` | Where the index lives |

## Supported Languages

Currently: **JavaScript and TypeScript** (including JSX/TSX).

The architecture supports any language with a tree-sitter grammar. More languages are planned.

## Verifying It Works

After connecting, ask your AI tool:

> "Use Loom to search for the main entry point of this project"

You should see it call `search()` and return ranked results with coupling scores. If you see `structural=0.XX` in the coupled results, structural coupling is working.

## Troubleshooting

**"Loom not initialized"** — The target directory path is wrong or doesn't exist. Check the path in your MCP config.

**First run is slow** — The embedding model (~270MB) downloads on first use. Subsequent runs skip this.

**`.loom.db` is large** — The database includes embeddings for every symbol. For a 500-file project, expect ~50-100MB. Add `.loom.db` to your `.gitignore`.

**No results for a file** — Check that the file extension is in the watch list and the file isn't in an excluded directory.

## Development

```bash
# Run tests
uv run pytest

# Lint
uv run ruff check

# Type check
uv run mypy

# Format
uv run ruff format
```
