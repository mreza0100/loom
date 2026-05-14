# Loom

## Status: Cancelled

Loom is cancelled.

The benchmark work ended with the result Loom was supposed to avoid: plain
grep/no-MCP Codex was not beaten by any Loom-like MCP competitor on the strict
reproduction task. One competitor, Serena, completed the task with real MCP
tool use, but it was slower than the no-MCP control. Several others produced
the right answer only after their MCP calls failed and the agent fell back to
shell search. That is not a win. That is grep wearing a fake mustache.

The north-star metric was **useful symbols discovered per token spent**. On the
strict benchmark below, no MCP tool beat the no-MCP baseline, so this project is
not worth continuing in its current direction.

**AI code intelligence via vector search.** Loom is an MCP server that replaces broad `grep` loops with ranked code neighborhoods for AI coding tools.

> grep finds what you asked for. Loom finds what you need.

## Final Benchmark: Grep Won

The final benchmark compared a no-MCP Codex control against the Loom-like MCP
systems collected under `tmp/loom-like/`.

The first matrix run was flawed because it counted attempted MCP calls as if
they had succeeded. The strict rerun fixed that: a competitor only counts as an
MCP-assisted pass when the MCP server is set up, Codex makes successful MCP tool
calls, and the answer is correct. If MCP calls failed or were cancelled and the
agent solved the task via shell, the run is marked failed.

### Target

```text
Repository: https://github.com/nodejs/corepack.git
Commit:     964d8cfaba59641128f8147668abca70bdfbac2b
Task:       enumerate every concrete async execute() command method under
            sources/commands, including sources/commands/deprecated
```

Expected answer:

| Class | Path | Line |
|---|---|---:|
| `CacheCommand` | `sources/commands/Cache.ts` | 20 |
| `DisableCommand` | `sources/commands/Disable.ts` | 41 |
| `EnableCommand` | `sources/commands/Enable.ts` | 41 |
| `InstallGlobalCommand` | `sources/commands/InstallGlobal.ts` | 42 |
| `InstallLocalCommand` | `sources/commands/InstallLocal.ts` | 21 |
| `PackCommand` | `sources/commands/Pack.ts` | 38 |
| `UpCommand` | `sources/commands/Up.ts` | 33 |
| `UseCommand` | `sources/commands/Use.ts` | 25 |
| `HydrateCommand` | `sources/commands/deprecated/Hydrate.ts` | 20 |
| `PrepareCommand` | `sources/commands/deprecated/Prepare.ts` | 30 |

### Final Strict Results

Result files:

```text
tmp/benchmark/loom-like-matrix-strict/SUMMARY.md
tmp/benchmark/loom-like-matrix-strict/SUMMARY.jsonl
tmp/benchmark/loom-like-matrix-strict/runs/<slug>/
```

| Tool | Status | Time | Successful MCP | Failed MCP | Shell Calls | Notes |
|---|---:|---:|---|---|---:|---|
| `no-mcp` | passed | 97s | - | - | 40 | Baseline. Correct answer with no MCP server. |
| `oraios__serena` | passed | 149s | `initial_instructions`, `check_onboarding_performed`, `onboarding`, `list_dir`, `read_file`, `search_for_pattern`, `get_symbols_overview` | `create_text_file`, `write_memory` | 12 | Only true MCP-assisted pass, but slower than no-MCP. |
| `GitNexus_npx_1_6_3` | passed by artifact correctness | 97s | `read_mcp_resource` | `list_repos`, `query` | 42 | The actual code query failed, so this is not a real code-search MCP win. |
| `Muvon__octocode` | failed | 76s | - | `semantic_search`, `structural_search` | 34 | Correct answer came after failed MCP calls. |
| `DeusData__codebase-memory-mcp` | failed | 82s | - | `list_projects` | 42 | Correct answer came after failed MCP calls. |
| `MinishLab__semble` | failed | 85s | - | `search` | 42 | Correct answer came after failed MCP call. |
| `BumpyClock__lsp-mcp` | failed | 89s | - | `findSymbol`, `initialSetup` | 36 | Correct answer came after failed MCP calls. |
| `jgravelle__jcodemunch-mcp` | failed | 70s | - | `list_repos` | 40 | Correct answer came after failed MCP call. |
| `sdsrss__code-graph-mcp` | failed | 103s | - | `project_map`, `semantic_code_search` | 36 | Correct answer came after failed MCP calls. |
| `tirth8205__code-review-graph` | failed | 71s | - | `get_minimal_context_tool` | 36 | Correct answer came after failed MCP call. |
| `BeaconBay__ck` | failed | 27s | - | - | 0 | Server launched poorly, no result artifacts. |
| `bartolli__codanna` | failed | 0s | - | - | 0 | No successful MCP calls; no result artifacts. |
| `srclight__srclight` | failed | 90s | - | - | 0 | No successful MCP calls; no result artifacts. |
| `Jakedismo__codegraph-rust` | setup failed | 0s | - | - | 0 | Requires local SurrealDB service for indexing/MCP state. |
| `SimplyLiz__CodeMCP` | setup failed | 0s | - | - | 0 | `go` was not available on `PATH`. |
| `abhigyanpatwari__GitNexus` | setup failed | 0s | - | - | 0 | Local checkout setup/analyze failed. |
| `colbymchenry__CodeGraph` | setup failed | 0s | - | - | 0 | Setup failed. |
| `kuberstar__qartez-mcp` | setup failed | 0s | - | - | 0 | Setup failed. |
| `elastic__semantic-code-search-indexer` | setup failed | 0s | - | - | 0 | Indexer repo, not an MCP server checkout. |
| `elastic__semantic-code-search-mcp-server` | setup failed | 0s | - | - | 0 | Requires Elasticsearch endpoint/index or cloud credentials. |
| `sourcebot-dev__sourcebot` | setup failed | 0s | - | - | 0 | Docker/web service deployment, not local stdio MCP. |
| `zilliztech__claude-context` | setup failed | 0s | - | - | 0 | Requires OpenAI API key plus Milvus/Zilliz/vector service. |

Verdict:

```text
No MCP competitor beat no-MCP/grep on the strict task.
Serena was the only real MCP-assisted pass, and it was slower.
Project cancelled.
```

## Reproduce The Final Benchmark

The strict runner is intentionally local and file-backed. It clones Corepack,
creates an isolated run directory per tool, tries to set up the MCP server,
runs headless Codex with isolated config, and appends one row per run.

Prerequisites:

```bash
codex --version
python3 --version
cargo --version
npm --version
uv --version
```

Some competitors also need optional tooling or external services. Missing
requirements are recorded as `setup_failed`; do not paper over them with global
configuration.

From the repo root:

```bash
cd /Users/reza/work/loom
python3 tmp/benchmark/loom-like-matrix-strict-runner.py setup
python3 tmp/benchmark/loom-like-matrix-strict-runner.py all
```

To rerun a single target:

```bash
python3 tmp/benchmark/loom-like-matrix-strict-runner.py no-mcp
python3 tmp/benchmark/loom-like-matrix-strict-runner.py oraios__serena
python3 tmp/benchmark/loom-like-matrix-strict-runner.py GitNexus_npx_1_6_3
```

To continue only missing rows after an interrupted run:

```bash
python3 tmp/benchmark/loom-like-matrix-strict-runner.py continue remaining
```

The runner writes:

```text
tmp/benchmark/loom-like-matrix-strict/ROSTER.txt
tmp/benchmark/loom-like-matrix-strict/SUMMARY.md
tmp/benchmark/loom-like-matrix-strict/SUMMARY.jsonl
tmp/benchmark/loom-like-matrix-strict/runs/<slug>/events.jsonl
tmp/benchmark/loom-like-matrix-strict/runs/<slug>/stderr-time.log
tmp/benchmark/loom-like-matrix-strict/runs/<slug>/final.txt
tmp/benchmark/loom-like-matrix-strict/runs/<slug>/run-meta.txt
tmp/benchmark/loom-like-matrix-strict/runs/<slug>/result.md
tmp/benchmark/loom-like-matrix-strict/runs/<slug>/result.json
```

Scoring rules:

- `passed`: exact 10 expected rows, no extra concrete `execute` false positives,
  and, for MCP competitors, at least one successful MCP tool call involved in
  the run.
- `failed`: Codex ran but the answer was wrong, artifacts were missing, or MCP
  calls failed and the answer was only produced by shell fallback.
- `setup_failed`: the MCP server could not be built, started, or reasonably
  configured from the local checkout without external services or missing
  system tools.

This is the result that cancelled the project.

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
| `symbols(query)` | Exact/suffix symbol enumeration for known names and repeated methods |
| `related(symbol)` | Structurally and semantically related symbols with scores |
| `impact(symbol)` | Blast radius for a symbol change |
| `neighborhood(file, line)` | Related code around a specific location |
| `inspect(handle)` | Bounded source inspection for selected Loom handles |
| `evidence_pack(query)` | Compact citable proof bundle before a final answer |
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

## CLI

The same binary is also a local CLI over the active Rust runtime:

```bash
cargo run -p loom-mcp -- --target /path/to/project reindex
cargo run -p loom-mcp -- --target /path/to/project search "findProjectSpec" --limit 5
cargo run -p loom-mcp -- --target /path/to/project symbols execute --file-prefix sources/commands --kind method
cargo run -p loom-mcp -- --target /path/to/project evidence-pack "signature integrity verification"
cargo run -p loom-mcp -- --target /path/to/project inspect 'symbol:idx-...:42' --line-budget 24
```

Use `--format text` for a compact terminal view. JSON remains the default so scripts and benchmark harnesses get the same contract shape as MCP.

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
cargo run -p loom-mcp -- --target /path/to/project search "query"
```

## License

MIT
