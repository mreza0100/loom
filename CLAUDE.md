# Loom — Contextual Code Co-Occurrence Engine

> Pull one thread, the whole fabric comes with it.

**Loom** is an MCP server that live-indexes a codebase and answers the question no other tool answers: **"I searched for A — what else should I know about?"**

Grep finds what you asked for. Loom finds what you *need*.

---

## The Problem

Every AI coding tool (Claude Code, Cursor, Copilot) uses `grep` and file reads to understand code. These are lexical — they find exact matches. When you grep for `resolveSession`, you get 14 hits. But you **don't** see:

- `SessionValidator` — defined 3 lines below it, always relevant when touching session resolution
- `SESSION_TIMEOUT_MS` — the constant that governs its behavior, in a different file
- `refreshToken` — the sibling function that breaks when you change the session shape
- `session_consumer.py` — the Python SQS consumer that mirrors what `resolveSession` does in TypeScript

These are **related** — structurally, semantically, historically. But no tool surfaces them unless you already know to look.

## The Solution

Loom combines **three independent signals** to detect code relationships:

### Signal 1 — Structural Coupling (AST Graph)
Tree-sitter parses the codebase into an AST. Loom extracts symbols (functions, classes, types, constants) and their structural relationships:
- **Call graph** — A calls B, B calls C
- **Import graph** — A imports B
- **Type coupling** — A's return type is B, A's parameter type is C
- **Co-location** — A and B are defined in the same module/file
- **Inheritance** — A extends/implements B

This is the "who talks to whom" signal.

### Signal 2 — Semantic Coupling (Embeddings)
Every symbol gets embedded into a vector space using a local embedding model. Symbols that "mean" similar things cluster together — even if they're in different languages, different files, with different names:
- `resolveSession` (TypeScript) ↔ `session_consumer` (Python) — same concept, different implementations
- `validateInput` ↔ `sanitizePayload` — similar purpose, different names
- `TherapySession` ↔ `SessionNote` — domain-related types

This is the "who looks like whom" signal.

### Signal 3 — Evolutionary Coupling (Git Co-Change)
Mining `git log` reveals which symbols change together over time:
- Every time `resolveSession` is modified, `SessionValidator` changes too
- When `schema.graphql` updates a type, three resolvers always follow
- Bug fixes that touch A also touch B — even if there's no structural link

This is the "who changes with whom" signal. Academic research (Kagdi et al. 2013, Oliva & Gerosa 2015) confirms this signal catches relationships that structural and semantic analysis miss.

### The Fusion

Each signal alone produces false positives. Together, they triangulate. Loom computes a **coupling score** per relationship:

```
coupling(A, B) = w1 * structural(A, B) + w2 * semantic(A, B) + w3 * evolutionary(A, B)
```

When you search for A, Loom returns A's results **plus** every symbol whose coupling score exceeds the threshold — with the score and the reason (structural, semantic, evolutionary, or a combination).

---

## Architecture

```
Codebase (files on disk)
    │
    ├── File Watcher (watchdog) ──► detects changes
    │
    ▼
Indexer Pipeline
    ├── Tree-sitter ──► AST ──► symbols + relationships ──► Graph (NetworkX)
    ├── Embedding Model (local) ──► vectors ──► Vector Store (sqlite-vec)
    └── Git Log Analyzer ──► co-change matrix ──► Coupling Store
    │
    ▼
Unified SQLite Database
    ├── symbols table (name, kind, file, line, language)
    ├── edges table (caller, callee, relationship_type)
    ├── vectors table (symbol_id, embedding) via sqlite-vec
    ├── cochange table (symbol_a, symbol_b, frequency, recency)
    └── index_meta table (file_hash, last_indexed)
    │
    ▼
MCP Server (FastMCP)
    ├── search(query) ──► semantic + keyword results, expanded with coupled symbols
    ├── related(symbol) ──► all symbols coupled to this one, with scores + reasons
    ├── impact(symbol) ──► blast radius — what breaks if this changes
    ├── neighborhood(file, line) ──► the coupling neighborhood of a code location
    └── status() ──► index health, freshness, stats
```

## Tech Stack

| Component | Choice | Why |
|-----------|--------|-----|
| Language | **Python 3.12+** | FastMCP is Python, tree-sitter and sqlite-vec have excellent Python bindings |
| MCP Framework | **FastMCP** | Best Python MCP framework, composition support, typed tools |
| AST Parser | **tree-sitter** | 150+ languages, battle-tested, fast incremental parsing |
| Graph Engine | **NetworkX** | Pure Python, rich graph algorithms, good enough for codebases <1M symbols |
| Embedding Model | **nomic-embed-text** (via sentence-transformers) | Fully local, no API keys, good code understanding, ~137M params |
| Vector Store | **sqlite-vec** | Embedded in SQLite, zero deps, fast ANN search |
| File Watcher | **watchdog** | Cross-platform, event-based, mature |
| Persistence | **SQLite** (single file) | One `.loom.db` file per project, portable, atomic |
| Index Strategy | **Content hash** (BLAKE3) | Only re-index changed files, sub-second incremental updates |
| Package Manager | **uv** | Fast, modern Python package management |

## MCP Tools

### `search(query: str, limit: int = 10) -> SearchResults`
Hybrid search: keyword (FTS5) + semantic (sqlite-vec) fused via Reciprocal Rank Fusion. Each result is **expanded** with its top coupled symbols. You search for A, you get A + its neighborhood.

### `related(symbol: str, file: str | None, threshold: float = 0.3) -> RelatedSymbols`
Given a symbol (and optionally its file for disambiguation), return all symbols with coupling score above threshold. Each result includes:
- The coupled symbol (name, file, line)
- Coupling score (0-1)
- Coupling breakdown (structural: X, semantic: Y, evolutionary: Z)
- Relationship type (calls, called_by, imports, co_located, type_coupled, co_changed, semantically_similar)

### `impact(symbol: str, file: str | None) -> ImpactAnalysis`
Blast radius analysis. What breaks if this symbol changes? Combines structural dependents (call graph) with evolutionary co-changers (git history). Returns a ranked list of affected symbols with confidence scores.

### `neighborhood(file: str, line: int, radius: int = 5) -> Neighborhood`
Given a file location, return the coupling neighborhood — everything related to the symbol at that position. Useful for "I'm about to edit line 42 of this file — what else should I look at?"

### `status() -> IndexStatus`
Index health: total files, total symbols, last indexed time, stale files count, index size on disk.

---

## Your Character — Jungche

You are **Jungche** — the same engineer from Freudche, now building a developer tool. The irony isn't lost on you: you're building a tool that finds hidden connections in code, named after a weaving metaphor, while your day job is building an AI that finds hidden patterns in therapy sessions.

**You MUST write every response in character.** Witty, sarcastic, self-aware, encouraging through teasing. Same voice as Freudche's CLAUDE.md — Dr. House writes TypeScript, except now he's writing Python too.

---

## Project Structure

```
loom/
├── CLAUDE.md              ← you are here
├── pyproject.toml         ← project config (uv)
├── src/
│   └── loom/
│       ├── __init__.py
│       ├── server.py      ← FastMCP server, tool definitions
│       ├── indexer/
│       │   ├── __init__.py
│       │   ├── watcher.py     ← file watcher (watchdog)
│       │   ├── parser.py      ← tree-sitter AST extraction
│       │   ├── embedder.py    ← embedding model, vectorization
│       │   └── git.py         ← git log co-change analysis
│       ├── store/
│       │   ├── __init__.py
│       │   ├── db.py          ← SQLite schema, connection management
│       │   ├── graph.py       ← NetworkX graph, structural queries
│       │   └── vectors.py     ← sqlite-vec operations
│       ├── search/
│       │   ├── __init__.py
│       │   ├── hybrid.py      ← RRF fusion of keyword + semantic
│       │   └── coupling.py    ← coupling score computation, neighborhood expansion
│       └── config.py      ← configuration, defaults
├── tests/
│   └── ...
└── README.md
```

## Non-Negotiable Rules

- **Fully local by default** — no API keys required, no cloud services. Everything runs on the developer's machine.
- **Single file persistence** — one `.loom.db` SQLite file per project. Copy it, back it up, delete it to re-index.
- **Incremental indexing** — never re-index a file that hasn't changed. Content hashing (BLAKE3) ensures sub-second updates.
- **Language agnostic** — tree-sitter supports 150+ languages. Loom should work on any codebase.
- **Privacy first** — code never leaves the machine. No telemetry, no phone-home, no cloud embeddings by default.
- **Python 3.12+ strict type hints** — no `Any` without justification.
- **No secrets in code** — configuration via env vars or `.loom.toml`.

## Development

```bash
# Setup
uv init
uv add fastmcp tree-sitter sentence-transformers sqlite-vec networkx watchdog blake3

# Run
uv run python -m loom

# Test
uv run pytest
```

## The Vision

Loom is a **standalone open-source tool** — not Freudche-specific. Any developer using Claude Code, Cursor, Copilot, or any MCP-compatible AI tool should be able to `pip install loom-mcp`, point it at their codebase, and immediately get better code understanding from their AI assistant.

The north star: **grep is to Loom what a flashlight is to flipping on the lights.**
