# Wave 1 — Foundation Rebuild

> The building is beautiful but it's sitting on sand. Time to pour concrete.

**Status:** PLANNED
**Date:** 2026-05-10
**Triggered by:** Benchmark LOOM-1 (webpack/lib — 587 files, 9262 symbols, 26050 edges)
**Goal:** Fix every foundational flaw that limits Loom's ceiling — before building anything else on top.

---

## Context: Why This Wave Exists

The LOOM-1 benchmark proved Loom's thesis: **vector search + structural awareness beats grep for AI code understanding.** Loom was 32% faster, found 3x more symbols, produced 72x less noise, and discovered semantic clusters grep literally cannot find.

But it also exposed that the foundation is rotten. Here's every flaw, traced to exact source code:

### Flaw 1: Name-Based Edges

The `Edge` dataclass in `src/loom/store/models.py:19-24`:
```python
@dataclass
class Edge:
    source_name: str       # "Compiler.compile"
    source_file: str       # "lib/Compiler.js"
    target_name: str       # "compile"  ← STRIPPED by parser.py:243
    target_file: str | None  # None = unresolved (most cross-file edges)
    relationship: str      # "calls"
```

The schema in `src/loom/store/db.py:29-39`:
```sql
CREATE TABLE IF NOT EXISTS edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_name TEXT NOT NULL,
    source_file TEXT NOT NULL,
    target_name TEXT NOT NULL,
    target_file TEXT,
    relationship TEXT NOT NULL
);
```

**Consequence:** Every edge lookup is a text search. In `engine.py:267-269`, resolving an edge target means:
```python
targets = self._db.get_symbol_by_name(edge.target_name, edge.target_file)
```
This returns 0-N symbols, requiring disambiguation. With ID-based edges, it would return exactly 1.

### Flaw 2: Single-Pass Indexing

In `src/loom/indexer/pipeline.py:59-121`, `_index_files` processes each file independently:
```python
for path in files:
    symbols, edges = parse_file(path, source)     # Parse one file
    for sym in symbols:
        sym_id = self._db.insert_symbol(sym)       # Store symbols
    _resolve_edge_targets(edges, rel_path, symbols) # Resolve against THIS file only
    for edge in edges:
        self._db.insert_edge(edge)                  # Store partially-resolved edges
```

The resolution function `_resolve_edge_targets` at `pipeline.py:129-148` has only two resolution sources:
1. **Import map** — built from import edges in the current file (line 134-139)
2. **Local names** — symbols defined in the current file (line 141)

```python
def _resolve_edge_targets(edges, source_file, symbols):
    import_map: dict[str, str] = {}
    for edge in edges:
        if edge.relationship == "imports" and edge.target_file:
            resolved_path = _resolve_import_path(edge.target_file, source_file)
            import_map[edge.source_name] = resolved_path
            edge.target_file = resolved_path
    local_names = {sym.name for sym in symbols}
    for edge in edges:
        if edge.relationship != "imports" and edge.target_file is None:
            if edge.target_name in import_map:
                edge.target_file = import_map[edge.target_name]
            elif edge.target_name in local_names:
                edge.target_file = source_file
```

**Consequence:** Any call to a symbol that isn't imported or defined locally gets `target_file=None`. In webpack's 26,050 edges, the vast majority of cross-file call edges are unresolved. The `impact()` function can only find callers whose edges happen to have been resolved — explaining the 8% recall (3/38 references found vs grep's 38/38).

### Flaw 3: Destroyed Call Expressions

In `src/loom/indexer/parser.py:231-252`, `_extract_calls` strips call expressions:
```python
def _extract_calls(node, source, caller_name, file_path, edges):
    if node.type == "call_expression":
        func_node = node.children[0] if node.children else None
        if func_node:
            callee = source[func_node.start_byte : func_node.end_byte].decode()
            if callee != caller_name and not callee.startswith("console."):
                clean = callee.split(".")[-1]    # ← LINE 243: THE CRITICAL LINE
                edges.append(Edge(
                    source_name=caller_name,
                    source_file=file_path,
                    target_name=clean,            # Only the last segment
                    target_file=None,
                    relationship="calls",
                ))
```

**What gets destroyed** — concrete webpack examples:
| Full expression | What we store | What we lose |
|----------------|--------------|-------------|
| `this.hooks.make.callAsync()` | `callAsync` | Hook name (`make`), receiver (`this.hooks`) |
| `compilation.seal()` | `seal` | Receiver (`compilation`) — can't disambiguate from Array.seal |
| `compiler.outputFileSystem.mkdirp()` | `mkdirp` | File system context |
| `NormalModule.getCompilationHooks(compilation)` | `getCompilationHooks` | Class context (`NormalModule`) |
| `this.applyPluginsAsync("compile", params)` | `applyPluginsAsync` | The plugin event name |

**Consequence:** In Phase 2 resolution, `target_name="callAsync"` matches nothing in the symbol table (methods are stored as `SomeClass.callAsync`). The edge stays unresolved. Meanwhile, `target_name="this.hooks.make.callAsync"` could be resolved to the `make` hook, or at minimum used as semantic signal.

### Flaw 4: No Real Graph

`pyproject.toml` does NOT include `networkx` in its dependencies (lines 19-26):
```toml
dependencies = [
    "fastmcp>=2.0",
    "tree-sitter>=0.24",
    "tree-sitter-javascript>=0.23",
    "sqlite-vec>=0.1",
    "watchdog>=4.0",
    "fastembed>=0.4",
]
```

Despite CLAUDE.md claiming "Graph Engine: NetworkX" in the tech stack, NetworkX is never installed, never imported, never used. The "graph" is SQL queries that do one-hop lookups:

- `engine.py:262-281`: `_find_coupled` does `get_edges_from` (outgoing) + `get_edges_to` (incoming) — two flat SQL queries, no traversal
- `engine.py:186-202`: `impact()` does the same one-hop incoming query

There is no:
- Transitive dependency tracking (`A→B→C`, finding A when asking about C)
- Shortest path computation
- Graph centrality (which symbols are hubs?)
- Cycle detection
- Community detection (clusters of related symbols)

**Consequence:** `impact("_makePathsRelative")` only finds direct callers whose edges resolved. In webpack, `_makePathsRelative` is called by functions that are themselves called by other functions — those transitive dependents are invisible.

### Flaw 5: Hardcoded Coupling Scores

In `engine.py`, coupling scores are fixed constants, not computed:

```python
# _find_coupled (line 258-320):
score=0.7    # All outgoing structural edges (line 278)
score=0.6    # All incoming structural edges (line 296)
score=sim    # Semantic similarity, raw (line 317), if sim > 0.3

# impact (line 177-227):
score=0.8    # All structural dependents (line 199)
score=sim    # Semantic, if sim > 0.3 (line 218)
```

Every `calls` edge gets 0.7. Every `imports` edge gets 0.7. Every `extends` edge gets 0.7. A direct caller and a transitive dependent (if we had transitive tracking) would get the same score. There is no:
- Relationship type weighting (`calls` should score higher than `co_located`)
- Depth decay (direct caller > 2-hop caller)
- Edge confidence weighting (import-resolved edge > unresolved guess)
- Multi-signal fusion (structural + semantic + evolutionary combined)

**Consequence:** The `reason` field in MCP output is always one of three strings: `"calls (structural)"`, `"called_by (structural)"`, or `"semantically similar"`. No meaningful ranking — AI agents can't prioritize results.

### Flaw 6: No Evolutionary Coupling

Signal 3 (git co-change) is described in CLAUDE.md lines 46-52:
```
### Signal 3 — Evolutionary Coupling (Git Co-Change)
Mining `git log` reveals which symbols change together over time...
```

And in the architecture diagram (line 77):
```
└── Git Analyzer ──► co-change matrix ──► Coupling Store
```

And the described schema includes (line 84):
```
├── cochange table (symbol_a, symbol_b, frequency, recency)
```

**None of this exists.** There is no `cochange` table in the actual schema (`db.py:14-50`). There is no `git_analyzer.py` in `src/loom/indexer/`. There is no git log parsing anywhere. The coupling score is structural + semantic only.

**Consequence:** Loom misses relationships that have no structural or semantic link but always change together. Academic research consistently shows this signal catches 15-30% of relationships the other two miss.

### Flaw 7: Impact/Related Divergence Bug

The JC fix for Bug 2 changed `get_edges_to` at `db.py:306-319`:

**Before (broken for `related()`):**
```python
WHERE target_name = ? AND (target_file IS NULL OR target_file = ?)
```

**After (broken for `impact()`):**
```python
WHERE target_name = ? AND target_file = ?
```

This fixed `related("create")` (went from 80+ chaos to 7 coherent results) but broke `impact()` recall — legitimate cross-file callers with `target_file=NULL` are now excluded. The benchmark shows: `impact("_makePathsRelative")` recall dropped from 40% (v1) to 8% (v2).

**Root cause:** Both `related()` and `impact()` use the same edge query, but they need different behaviors:
- `related()` needs strict scoping — only show edges that resolve to this specific symbol
- `impact()` needs inclusive scoping — show ALL callers, even unresolved ones, because a caller with `target_file=NULL` might still be a real dependent

This is a **symptom** of the name-based edge model. With ID-based edges, this divergence disappears — resolved edges have `target_id`, unresolved edges have `target_id=NULL`. `related()` queries resolved edges; `impact()` queries both.

---

## The North Star

Loom exists to **replace grep as the code understanding layer for AI coding tools.** Not for humans reading code — for AI agents spending tokens. The metric that matters:

> **Useful symbols discovered per token spent.**

Current performance (LOOM-1 benchmark v2):
- **Grep:** ~789 tokens per useful symbol (176s, 52 commands, ~65 unique symbols)
- **Loom:** ~376 tokens per useful symbol (119s, 28 calls, ~200 unique symbols)
- **Ratio:** 2.1x more efficient

After this wave, the target is **5x better than grep** — ~150 tokens per useful symbol.

Every design decision in this wave optimizes for:
1. **Recall** — find ALL relevant symbols, not just the ones that share a name
2. **Precision** — return ranked results, not 909 undifferentiated hits
3. **Token efficiency** — one Loom call should replace 5-10 grep commands
4. **Latency** — sub-second queries on 10K+ symbol codebases

---

## Phase 1: ID-Based Edge Model

**Priority:** CRITICAL — everything else depends on this
**Estimated effort:** Medium
**Files changed:** `models.py`, `db.py`, `parser.py`, `pipeline.py`, `engine.py`, `conftest.py`, `test_db.py`, `test_engine.py`
**Files created:** None
**Dependencies removed:** `get_symbol_by_name_fuzzy()` from internal edge traversal (kept for MCP input parsing)

### Current State — Exact Code Trace

**1. Parser creates edges with names** (`parser.py:244-251`):
```python
edges.append(Edge(
    source_name=caller_name,    # e.g., "Compiler.compile"
    source_file=file_path,      # e.g., "lib/Compiler.js"
    target_name=clean,          # e.g., "compile" (STRIPPED)
    target_file=None,           # Always None at parse time for calls
    relationship="calls",
))
```

**2. Pipeline resolves some edges** (`pipeline.py:94`):
```python
_resolve_edge_targets(edges, rel_path, symbols)
```
This sets `target_file` for edges whose target is imported or locally defined. Most cross-file call edges stay `target_file=None`.

**3. Pipeline stores edges with names** (`pipeline.py:96-97`):
```python
for edge in edges:
    edge.source_file = rel_path
    self._db.insert_edge(edge)
```

**4. DB stores edge as 5 text columns** (`db.py:177-188`):
```python
def insert_edge(self, edge: Edge) -> None:
    self.conn.execute(
        "INSERT INTO edges (source_name, source_file, target_name, target_file, relationship) "
        "VALUES (?, ?, ?, ?, ?)",
        (edge.source_name, edge.source_file, edge.target_name, edge.target_file, edge.relationship),
    )
```

**5. Engine queries edges by name** (`engine.py:262`):
```python
outgoing = self._db.get_edges_from(target.name, target.file)
```

**6. Engine resolves edge targets to symbols by name** (`engine.py:267-269`):
```python
if edge.target_file:
    targets = self._db.get_symbol_by_name(edge.target_name, edge.target_file)
else:
    targets = self._db.get_symbol_by_name(edge.target_name)
```

This is where the name mismatch hits. The parser stored `target_name="compile"` but the symbol is stored as `name="Compiler.compile"`. The lookup fails. This is why `get_symbol_by_name_fuzzy()` was added as a band-aid — but it's only used in `related()` and `impact()` for the initial symbol lookup, not for edge target resolution.

### The Fix — New Edge Model

**`src/loom/store/models.py`** — new `Edge` dataclass:

```python
@dataclass
class Edge:
    source_id: int              # FK → symbols.id — always resolved (parser knows the enclosing symbol)
    target_id: int | None       # FK → symbols.id — None = unresolved, set in Phase 2
    target_name: str            # Original call expression — kept for diagnostics and re-resolution
    target_file: str | None     # File hint for resolution (from import map or local scope)
    relationship: str           # "calls", "imports", "extends", "extended_by", "instantiates"
    confidence: float           # 0.0-1.0 — how sure are we this edge is correct?
    id: int | None = None       # DB row ID, set after insert
```

Key differences:
- `source_name` + `source_file` → `source_id` (single integer, always known)
- `target_name` + `target_file` → `target_id` (nullable integer) + `target_name` (diagnostic) + `target_file` (hint)
- New `confidence` field (feeds into Phase 5 coupling scores)
- New `id` field (for Phase 2 re-resolution — need to UPDATE edges by ID)

**`src/loom/store/db.py`** — new schema:

```sql
CREATE TABLE IF NOT EXISTS edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id INTEGER NOT NULL,
    target_id INTEGER,
    target_name TEXT NOT NULL,
    target_file TEXT,
    relationship TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 0.0,
    FOREIGN KEY (source_id) REFERENCES symbols(id) ON DELETE CASCADE,
    FOREIGN KEY (target_id) REFERENCES symbols(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id);
CREATE INDEX IF NOT EXISTS idx_edges_target_name ON edges(target_name);
CREATE INDEX IF NOT EXISTS idx_edges_unresolved ON edges(target_id) WHERE target_id IS NULL;
```

Design notes:
- **`ON DELETE CASCADE` for source_id:** When a symbol is deleted (file re-indexed), all its outgoing edges are automatically cleaned up. Currently `remove_file` at `db.py:122-145` does this manually with explicit DELETEs.
- **`ON DELETE SET NULL` for target_id:** When a target symbol is deleted, edges pointing to it become unresolved rather than deleted. Phase 2's incremental re-resolution can then re-resolve them.
- **Partial index on `target_id IS NULL`:** Makes `get_unresolved_edges()` fast — Phase 2 needs to query all unresolved edges efficiently.
- **`confidence` defaults to 0.0** (unresolved). Phase 2 sets it during resolution.

**`src/loom/store/db.py`** — new edge methods:

```python
def insert_edge(self, edge: Edge) -> int:
    cursor = self.conn.execute(
        "INSERT INTO edges (source_id, target_id, target_name, target_file, "
        "relationship, confidence) VALUES (?, ?, ?, ?, ?, ?)",
        (edge.source_id, edge.target_id, edge.target_name, edge.target_file,
         edge.relationship, edge.confidence),
    )
    edge_id = cursor.lastrowid
    assert edge_id is not None
    return edge_id

def get_edges_from(self, symbol_id: int) -> list[Edge]:
    rows = self.conn.execute(
        "SELECT id, source_id, target_id, target_name, target_file, "
        "relationship, confidence FROM edges WHERE source_id = ?",
        (symbol_id,),
    ).fetchall()
    return [self._row_to_edge(r) for r in rows]

def get_edges_to(self, symbol_id: int) -> list[Edge]:
    rows = self.conn.execute(
        "SELECT id, source_id, target_id, target_name, target_file, "
        "relationship, confidence FROM edges WHERE target_id = ?",
        (symbol_id,),
    ).fetchall()
    return [self._row_to_edge(r) for r in rows]

def get_edges_to_by_name(self, target_name: str) -> list[Edge]:
    """For impact() — find ALL edges pointing at this name, even unresolved."""
    rows = self.conn.execute(
        "SELECT id, source_id, target_id, target_name, target_file, "
        "relationship, confidence FROM edges WHERE target_name = ?",
        (target_name,),
    ).fetchall()
    return [self._row_to_edge(r) for r in rows]

def get_unresolved_edges(self) -> list[Edge]:
    rows = self.conn.execute(
        "SELECT id, source_id, target_id, target_name, target_file, "
        "relationship, confidence FROM edges WHERE target_id IS NULL",
    ).fetchall()
    return [self._row_to_edge(r) for r in rows]

def update_edge_target(self, edge_id: int, target_id: int, confidence: float) -> None:
    self.conn.execute(
        "UPDATE edges SET target_id = ?, confidence = ? WHERE id = ?",
        (target_id, confidence, edge_id),
    )

def remove_edges_for_source(self, symbol_id: int) -> None:
    self.conn.execute("DELETE FROM edges WHERE source_id = ?", (symbol_id,))

@staticmethod
def _row_to_edge(row: tuple) -> Edge:
    return Edge(
        id=row[0],
        source_id=row[1],
        target_id=row[2],
        target_name=row[3],
        target_file=row[4],
        relationship=row[5],
        confidence=row[6],
    )
```

**`src/loom/store/db.py`** — updated `remove_file`:

```python
def remove_file(self, path: str) -> None:
    symbol_ids = [
        row[0] for row in self.conn.execute(
            "SELECT id FROM symbols WHERE file = ?", (path,)
        ).fetchall()
    ]
    if symbol_ids:
        placeholders = ",".join("?" * len(symbol_ids))
        self.conn.execute(
            f"DELETE FROM vec_symbols WHERE rowid IN ({placeholders})",
            symbol_ids,
        )
        self.conn.execute(
            f"DELETE FROM symbols_fts WHERE rowid IN ({placeholders})",
            symbol_ids,
        )
        # Edges with source in this file are deleted (CASCADE handles this if FK enabled)
        # But SQLite doesn't enforce FK by default, so be explicit:
        self.conn.execute(
            f"DELETE FROM edges WHERE source_id IN ({placeholders})",
            symbol_ids,
        )
        # Edges targeting symbols in this file become unresolved:
        self.conn.execute(
            f"UPDATE edges SET target_id = NULL, confidence = 0.0 "
            f"WHERE target_id IN ({placeholders})",
            symbol_ids,
        )
    self.conn.execute("DELETE FROM symbols WHERE file = ?", (path,))
    self.conn.execute("DELETE FROM index_meta WHERE file_path = ?", (path,))
```

Note the critical difference: edges FROM this file are deleted, but edges TO symbols in this file are **nullified, not deleted**. They become unresolved and can be re-resolved when the file is re-indexed (Phase 2).

**`src/loom/search/engine.py`** — updated queries:

```python
# _find_coupled — BEFORE (name-based):
outgoing = self._db.get_edges_from(target.name, target.file)
for edge in outgoing:
    targets = self._db.get_symbol_by_name(edge.target_name, edge.target_file)
    for sym in targets:
        ...

# _find_coupled — AFTER (ID-based):
outgoing = self._db.get_edges_from(target.id)
for edge in outgoing:
    if edge.target_id is not None:
        sym = self._db.get_symbol_by_id(edge.target_id)
        if sym:
            ...  # Single symbol, no ambiguity
```

This eliminates:
- `get_symbol_by_name_fuzzy()` from all internal edge traversal
- The disambiguation problem (0-N results → exactly 1)
- The name mismatch between parser output and symbol table

**`src/loom/search/engine.py`** — impact/related divergence fix:

```python
# impact() — AFTER (uses BOTH resolved and unresolved edges):
def impact(self, symbol_name, file=None, kind=None):
    symbols = self._db.get_symbol_by_name_fuzzy(symbol_name, file)
    target = symbols[0]
    
    # 1. Resolved edges — direct callers with known identity
    incoming = self._db.get_edges_to(target.id)
    for edge in incoming:
        source_sym = self._db.get_symbol_by_id(edge.source_id)
        if source_sym and source_sym.name not in _GENERIC_CALL_TARGETS:
            dependents.append(CoupledSymbol(
                symbol=source_sym,
                score=edge.confidence,
                reason=f"{edge.relationship} (structural, confidence={edge.confidence:.1f})",
            ))
    
    # 2. Unresolved edges — callers that name-match but weren't resolved
    #    This recovers the recall we lost with the NULL-edge fix
    unresolved = self._db.get_edges_to_by_name(target.name)
    for edge in unresolved:
        if edge.target_id is not None:
            continue  # Already handled above
        source_sym = self._db.get_symbol_by_id(edge.source_id)
        if source_sym and source_sym.id not in seen:
            dependents.append(CoupledSymbol(
                symbol=source_sym,
                score=0.3,  # Low confidence — unresolved
                reason=f"{edge.relationship} (unresolved, name-match)",
            ))
```

This is the hybrid approach that solves the impact/related divergence:
- `related()` uses `get_edges_from(symbol_id)` — only resolved edges, strict scoping
- `impact()` uses `get_edges_to(symbol_id)` + `get_edges_to_by_name(name)` — resolved + unresolved, inclusive

### Migration Strategy

- **No schema migration.** Loom is pre-1.0 (`version = "0.1.0"` in `pyproject.toml:3`). Drop old edges table, full re-index.
- Re-index is <5s for most codebases, ~280s for webpack (but that's a 587-file JS codebase).
- The `.loom.db` file is per-project and gitignored — no user data to preserve.
- `db.py` should detect old schema (check for `source_name` column) and log a warning telling user to re-index.

### Test Changes

**`tests/conftest.py`** — `populated_db` fixture edges change from:
```python
Edge(source_name="processOrder", source_file="src/services/order.js",
     target_name="validateCart", target_file="src/utils/validation.js", relationship="calls")
```
To:
```python
Edge(source_id=ids[0], target_id=ids[1], target_name="validateCart",
     target_file="src/utils/validation.js", relationship="calls", confidence=1.0)
```
Where `ids[0]` is the `processOrder` symbol ID and `ids[1]` is the `validateCart` symbol ID.

**`tests/test_db.py`** — `TestEdgeCRUD` class:
- `test_insert_and_query_from` — change to use symbol IDs
- `test_get_edges_from_with_file` — replaced by `test_get_edges_from_by_id`
- `test_get_edges_to_with_file` — replaced by `test_get_edges_to_by_id`
- New: `test_get_unresolved_edges` — queries `WHERE target_id IS NULL`
- New: `test_update_edge_target` — sets `target_id` and `confidence`
- New: `test_get_edges_to_by_name` — finds both resolved and unresolved edges by target name
- New: `test_edge_confidence_roundtrip` — confidence stored and retrieved correctly
- New: `test_remove_file_nullifies_target_edges` — edges TO deleted file become unresolved, not deleted

**`tests/test_engine.py`** — `TestBuiltinFiltering`:
- Update edge construction to use IDs instead of names
- New: `test_impact_includes_unresolved_name_matches` — impact() finds callers with NULL target_id
- New: `test_related_excludes_unresolved` — related() only returns resolved edges

### Done When

- [ ] `Edge` dataclass uses `source_id: int` and `target_id: int | None`
- [ ] Schema uses integer foreign keys with CASCADE/SET NULL
- [ ] All edge queries work by ID — `get_edges_from(symbol_id)`, `get_edges_to(symbol_id)`
- [ ] `get_edges_to_by_name()` exists for impact() inclusive queries
- [ ] `get_unresolved_edges()` exists for Phase 2 resolution
- [ ] `update_edge_target()` exists for Phase 2 resolution
- [ ] `remove_file()` nullifies target edges instead of deleting them
- [ ] `get_symbol_by_name_fuzzy()` is only used in MCP tool input parsing (server.py → engine.py first lookup), never in internal edge traversal
- [ ] All existing tests updated and passing
- [ ] New tests for ID-based edge operations
- [ ] Benchmark: edge query latency < 1ms for 10K-edge graph (integer index vs text LIKE)

---

## Phase 2: Two-Phase Indexing

**Priority:** CRITICAL — required for cross-file edge resolution
**Estimated effort:** Medium
**Files changed:** `pipeline.py`, `db.py`
**Files created:** None
**Depends on:** Phase 1 (ID-based edges, `update_edge_target()`, `get_unresolved_edges()`)

### Current State — Exact Code Trace

The current `_index_files` method at `pipeline.py:59-121` processes files sequentially, one at a time:

```python
def _index_files(self, files: list[Path]) -> dict[str, int]:
    for path in files:
        # 1. Check content hash — skip if unchanged
        content_hash = _hash_file(path)
        existing_hash = self._db.get_file_hash(rel_path)
        if existing_hash == content_hash:
            continue
        
        # 2. Clear old data for this file
        self._db.remove_file(rel_path)
        
        # 3. Parse → symbols + edges
        symbols, edges = parse_file(path, source)
        
        # 4. Store symbols, get IDs, generate embeddings
        for sym in symbols:
            sym_id = self._db.insert_symbol(sym)
            embed_texts.append(self._embedder.build_symbol_text(...))
        embeddings = self._embedder.embed(embed_texts)
        for sym_id, emb in zip(symbol_ids, embeddings):
            self._db.insert_embedding(sym_id, emb)
        
        # 5. Resolve edges within THIS file's scope
        _resolve_edge_targets(edges, rel_path, symbols)
        
        # 6. Store edges
        for edge in edges:
            self._db.insert_edge(edge)
        
        # 7. Commit per file
        self._db.commit()
```

The problem is step 5. `_resolve_edge_targets` at `pipeline.py:129-148` only knows about:
- **Import map:** Names imported in the current file → resolved file paths
- **Local names:** Symbols defined in the current file

When `Compiler.js` calls `compilation.seal()`, the parser stores `target_name="seal"` (stripped by `split(".")[-1]`). The import map might resolve `Compilation` to `./Compilation.js`, but `seal` isn't the import name — it's a method on the imported class. The edge stays `target_file=None`.

### The Fix — Two Phases

```python
def _index_files(self, files: list[Path]) -> dict[str, int]:
    # ═══════════════════════════════════════════
    # PHASE 1: PARSE ALL FILES → STORE SYMBOLS + RAW EDGES
    # ═══════════════════════════════════════════
    all_edges: list[tuple[str, list[Edge]]] = []  # (rel_path, edges)
    
    for path in files:
        content_hash = _hash_file(path)
        existing_hash = self._db.get_file_hash(rel_path)
        if existing_hash == content_hash:
            continue
        
        self._db.remove_file(rel_path)
        
        symbols, edges = parse_file(path, source)
        
        # Store symbols immediately — they get IDs
        file_symbol_ids: dict[str, int] = {}
        embed_texts: list[str] = []
        for sym in symbols:
            sym.file = rel_path
            sym_id = self._db.insert_symbol(sym)
            file_symbol_ids[sym.name] = sym_id
            embed_texts.append(self._embedder.build_symbol_text(...))
        
        # Generate and store embeddings
        if embed_texts:
            embeddings = self._embedder.embed(embed_texts)
            for sym_id, emb in zip(file_symbol_ids.values(), embeddings):
                self._db.insert_embedding(sym_id, emb)
        
        # Store RAW edges — source_id resolved, target_id=NULL
        for edge in edges:
            # Resolve source_id from local symbol map
            source_id = file_symbol_ids.get(edge.source_name)
            if source_id is None:
                continue  # orphaned edge — caller not in symbol table
            
            self._db.insert_edge(Edge(
                source_id=source_id,
                target_id=None,             # Resolved in Phase 2
                target_name=edge.target_name,  # Full expression (Phase 3)
                target_file=edge.target_file,  # Hint from import resolution
                relationship=edge.relationship,
                confidence=0.0,
            ))
        
        self._db.set_file_hash(rel_path, content_hash)
        total_symbols += len(symbols)
        total_edges += len(edges)
        indexed += 1
    
    self._db.commit()
    
    # ═══════════════════════════════════════════
    # PHASE 2: RESOLVE ALL EDGES AGAINST COMPLETE SYMBOL TABLE
    # ═══════════════════════════════════════════
    if indexed > 0:
        resolved = self._resolve_all_edges()
        log.info("Phase 2: resolved %d/%d edges", resolved, total_edges)
    
    return {"indexed": indexed, "symbols": total_symbols, "edges": total_edges}
```

### Phase 2 Resolution — Detailed Algorithm

```python
def _resolve_all_edges(self) -> int:
    """Resolve unresolved edges against the complete symbol table."""
    
    # Build global import map: (source_file, local_name) → resolved_file
    import_map = self._build_import_map()
    
    unresolved = self._db.get_unresolved_edges()
    resolved_count = 0
    
    for edge in unresolved:
        target_id, confidence = self._resolve_single_edge(edge, import_map)
        if target_id is not None:
            self._db.update_edge_target(edge.id, target_id, confidence)
            resolved_count += 1
    
    self._db.commit()
    return resolved_count

def _build_import_map(self) -> dict[tuple[str, str], str]:
    """Build global import map from ALL import edges in the database.
    
    Returns: {(source_file, local_name): resolved_target_file}
    
    Example: If lib/Compiler.js has `import { Compilation } from './Compilation'`
    then import_map[("lib/Compiler.js", "Compilation")] = "lib/Compilation.js"
    """
    import_edges = self._db.conn.execute(
        "SELECT source_id, target_name, target_file FROM edges "
        "WHERE relationship = 'imports' AND target_file IS NOT NULL",
    ).fetchall()
    
    result: dict[tuple[str, str], str] = {}
    for source_id, target_name, target_file in import_edges:
        source_sym = self._db.get_symbol_by_id(source_id)
        if source_sym:
            result[(source_sym.file, target_name)] = target_file
    
    return result

def _resolve_single_edge(
    self, edge: Edge, import_map: dict[tuple[str, str], str]
) -> tuple[int | None, float]:
    """Resolve a single unresolved edge. Returns (target_id, confidence).
    
    Resolution strategy — tried in order, highest confidence first:
    
    1. Import-resolved: target_name matches an import in the source file
       → look up symbol in the import target file (confidence: 0.95)
    
    2. Exact name + file: target_file hint is set (from local resolution)
       → look up symbol by exact name in exact file (confidence: 1.0)
    
    3. Qualified name match: target_name has no dot, might be a method
       → try "%.target_name" pattern match (confidence: 0.8 if unique, 0.7 if disambiguated)
    
    4. Unique name match: only one symbol in the whole codebase with this name
       → use it (confidence: 0.6)
    
    5. Unresolved — return (None, 0.0)
    """
    source_sym = self._db.get_symbol_by_id(edge.source_id)
    if source_sym is None:
        return None, 0.0
    source_file = source_sym.file
    
    # Strategy 1: Exact name + file (if target_file hint exists)
    if edge.target_file:
        symbols = self._db.get_symbol_by_name(edge.target_name, edge.target_file)
        if symbols:
            return symbols[0].id, 1.0
        # Try file suffix matching (e.g., "./Compilation" → "lib/Compilation.js")
        suffix_symbols = [
            s for s in self._db.get_symbol_by_name(edge.target_name)
            if s.file.endswith(edge.target_file) or s.file.endswith(f"/{edge.target_file}")
        ]
        if len(suffix_symbols) == 1:
            return suffix_symbols[0].id, 0.9
    
    # Strategy 2: Import-resolved
    resolved_file = import_map.get((source_file, edge.target_name))
    if resolved_file:
        symbols = self._db.get_symbol_by_name(edge.target_name, resolved_file)
        if symbols:
            return symbols[0].id, 0.95
        # Import might re-export — try file suffix
        for sym in self._db.get_symbol_by_name(edge.target_name):
            if sym.file.startswith(resolved_file.rstrip(".js").rstrip(".ts")):
                return sym.id, 0.85
    
    # Strategy 3: Qualified name match (target_name="compile" → "Compiler.compile")
    if "." not in edge.target_name:
        pattern = f"%.{edge.target_name}"
        rows = self._db.conn.execute(
            "SELECT id, name, file FROM symbols WHERE name LIKE ? LIMIT 20",
            (pattern,),
        ).fetchall()
        if len(rows) == 1:
            return rows[0][0], 0.8
        if len(rows) > 1:
            # Disambiguate: prefer symbol in same file, then imported file
            same_file = [r for r in rows if r[2] == source_file]
            if len(same_file) == 1:
                return same_file[0][0], 0.8
            imported_files = {v for (k, v) in import_map.items() if k[0] == source_file}
            imported_match = [r for r in rows if r[2] in imported_files]
            if len(imported_match) == 1:
                return imported_match[0][0], 0.7
    
    # Strategy 4: Unique name match
    symbols = self._db.get_symbol_by_name(edge.target_name)
    if len(symbols) == 1:
        return symbols[0].id, 0.6
    
    # Strategy 5: Unresolved
    return None, 0.0
```

### Resolution Confidence Ladder

| Strategy | Confidence | When it applies | Example |
|----------|-----------|-----------------|---------|
| Exact name + exact file | 1.0 | `target_file` set and matches | `processOrder` → `validateCart` in `validation.js` |
| Import-resolved | 0.95 | Target name matches an import, symbol found in import target | `Compilation` imported from `./Compilation`, method found there |
| File suffix match | 0.9 | `target_file` is a relative path that suffix-matches | `./utils/helper` → `lib/utils/helper.js` |
| Import + re-export | 0.85 | Import target re-exports from a submodule | `Compilation` imported from barrel file |
| Unique qualified match | 0.8 | `compile` → only one `X.compile` in codebase | `compile` → `Compiler.compile` (unique) |
| Same-file qualified | 0.8 | `compile` → `X.compile` where X is in the same file | Method call within same class file |
| Import-chain qualified | 0.7 | `compile` → `X.compile` where X's file is imported | `compilation.seal()` → `Compilation.seal` |
| Unique name (codebase-wide) | 0.6 | Only one symbol with this name anywhere | `_makePathsRelative` — unique name |
| Unresolved | 0.0 | No match found | `callAsync` — too generic |

### Incremental Re-Resolution

When `pipeline.incremental_index()` processes a changed file:

```python
def incremental_index(self, changed_paths: list[Path]) -> dict[str, int]:
    files = [p for p in changed_paths if p.exists() and _should_index(p, self._config)]
    deleted = [p for p in changed_paths if not p.exists()]
    
    for p in deleted:
        rel = str(p.relative_to(self._config.target_dir))
        self._db.remove_file(rel)  # Edges TO this file become unresolved (not deleted)
    
    result = self._index_files(files)  # Phase 1 + Phase 2 for these files
    
    # NEW: Re-resolve edges that MIGHT now resolve to symbols in changed files
    # This handles the case where file B is changed, and file A had an unresolved
    # edge that should now point to a symbol in B
    if files:
        re_resolved = self._re_resolve_for_changed_files(
            [str(p.relative_to(self._config.target_dir)) for p in files]
        )
        log.info("Re-resolved %d edges targeting changed files", re_resolved)
    
    return result

def _re_resolve_for_changed_files(self, changed_files: list[str]) -> int:
    """Re-resolve unresolved edges that might target symbols in changed files."""
    import_map = self._build_import_map()
    resolved_count = 0
    
    # Get all symbol names in changed files
    target_names: set[str] = set()
    for file in changed_files:
        for sym in self._db.get_colocated_symbols(file):
            target_names.add(sym.name)
            # Also add the base name for qualified names
            if "." in sym.name:
                target_names.add(sym.name.split(".")[-1])
    
    # Find unresolved edges whose target_name matches
    unresolved = self._db.get_unresolved_edges()
    for edge in unresolved:
        if edge.target_name in target_names:
            target_id, confidence = self._resolve_single_edge(edge, import_map)
            if target_id is not None:
                self._db.update_edge_target(edge.id, target_id, confidence)
                resolved_count += 1
    
    self._db.commit()
    return resolved_count
```

### Benchmark Predictions

For webpack (587 files, 9262 symbols, 26050 edges):

| Metric | Current | After Phase 2 | Reasoning |
|--------|---------|--------------|-----------|
| Edges with `target_id` resolved | ~30% (local + imported) | ~70-80% | Global symbol table resolves most cross-file calls |
| Edges with `target_file` resolved | ~40% | same (this is Phase 1 data) | File resolution doesn't change |
| `impact()` recall on `_makePathsRelative` | 8% (3/38) | ~60% (23/38) | Resolved edges make callers visible |
| Full index time | ~280s | ~300s (+7%) | Phase 2 resolution is O(edges × resolution strategies) but uses indexed lookups |
| Incremental index time | 0.9s | ~1.2s | Re-resolution adds ~0.3s |

The remaining ~20-30% unresolved edges will be:
- Dynamic dispatch (`this[method]()`)
- Hook architecture (`this.hooks.X.call()`) — even with full expressions, hook listeners can't be statically resolved
- Chained calls where no intermediate name is a symbol (`a.b.c.d()`)

### Tests

- `test_two_phase_basic` — Parse 2 files, file A calls symbol in file B. After Phase 2, edge from A→B has target_id resolved.
- `test_two_phase_import_resolution` — File A imports `foo` from `./bar`. Call to `foo()` in A resolves to symbol in `bar.js`.
- `test_two_phase_qualified_name` — File A calls `compile()`. Symbol table has `Compiler.compile`. Phase 2 resolves with confidence 0.8.
- `test_two_phase_unique_name` — File A calls `_makePathsRelative()`. Only one symbol with that name. Phase 2 resolves with confidence 0.6.
- `test_two_phase_ambiguous_name` — File A calls `create()`. Multiple `create` symbols exist. Phase 2 uses import chain to disambiguate, or leaves unresolved.
- `test_two_phase_confidence_ordering` — Exact match gets 1.0, import-resolved gets 0.95, unique name gets 0.6.
- `test_incremental_re_resolution` — Change file B (add new symbol). Unresolved edge from A that target_name-matches the new symbol gets resolved.
- `test_incremental_delete_nullifies` — Delete file B. Edges with target_id pointing to B's symbols become unresolved (target_id=NULL).
- `test_phase2_performance` — Resolution of 1000 edges completes in <1s.

### Done When

- [ ] `_index_files` runs in two phases: parse-all → resolve-all
- [ ] `_resolve_single_edge` implements 5-strategy resolution with confidence levels
- [ ] `_build_import_map` constructs global import map from all import edges
- [ ] `incremental_index` re-resolves edges targeting changed files
- [ ] Resolution confidence is stored per edge
- [ ] `impact()` recall on webpack `_makePathsRelative` goes from 8% to >60%
- [ ] Full index time increases <20% vs current
- [ ] Incremental index time increases <50% vs current

---

## Phase 3: Preserve Full Call Expressions

**Priority:** HIGH — required for accurate structural coupling
**Estimated effort:** Small
**Files changed:** `parser.py`
**Files created:** None
**Depends on:** Phase 1 (new Edge model stores `target_name` as full expression)

### Current State — Exact Code Trace

The parser extracts call expressions at `parser.py:231-252`:

```python
def _extract_calls(node, source, caller_name, file_path, edges):
    if node.type == "call_expression":
        func_node = node.children[0] if node.children else None
        if func_node:
            callee = source[func_node.start_byte : func_node.end_byte].decode()
            if callee != caller_name and not callee.startswith("console."):
                clean = callee.split(".")[-1]    # ← DESTROYS RECEIVER INFO
                edges.append(Edge(
                    source_name=caller_name,
                    source_file=file_path,
                    target_name=clean,            # ONLY the last segment
                    target_file=None,
                    relationship="calls",
                ))
```

The tree-sitter AST for `this.hooks.make.callAsync(params, callback)`:
```
call_expression
  member_expression                   ← func_node (children[0])
    member_expression
      member_expression
        this
        property_identifier: "hooks"
      property_identifier: "make"
    property_identifier: "callAsync"
  arguments
    identifier: "params"
    identifier: "callback"
```

`source[func_node.start_byte : func_node.end_byte]` extracts `"this.hooks.make.callAsync"` — the full expression. Then `split(".")[-1]` throws away everything except `"callAsync"`.

### The Fix

Remove `split(".")[-1]`. Store the full expression.

```python
def _extract_calls(node, source, caller_name, file_path, edges):
    if node.type == "call_expression":
        func_node = node.children[0] if node.children else None
        if func_node:
            callee = source[func_node.start_byte : func_node.end_byte].decode()
            if callee != caller_name and not callee.startswith("console."):
                edges.append(Edge(
                    source_name=caller_name,
                    source_file=file_path,
                    target_name=callee,     # FULL expression
                    target_file=None,
                    relationship="calls",
                ))
```

That's it. One line deleted. The resolution of full expressions happens in Phase 2's `_resolve_single_edge`.

### Phase 2 Resolution Updates for Full Expressions

`_resolve_single_edge` needs new strategies to handle dotted expressions:

```python
def _resolve_single_edge(self, edge, import_map):
    target = edge.target_name
    parts = target.split(".")
    
    # Simple name — existing strategies apply
    if len(parts) == 1:
        return self._resolve_simple_name(edge, import_map)
    
    # "this.method" — resolve as "EnclosingClass.method"
    if parts[0] == "this":
        enclosing_class = self._get_enclosing_class(edge.source_id)
        if enclosing_class:
            qualified = f"{enclosing_class}.{parts[-1]}"
            symbols = self._db.get_symbol_by_name(qualified)
            if symbols:
                return symbols[0].id, 0.9
    
    # "Foo.bar" where Foo starts uppercase — already qualified
    if parts[0][0].isupper():
        symbols = self._db.get_symbol_by_name(target)
        if symbols:
            return symbols[0].id, 1.0
        # Try just "Foo.bar" without any prefix
        short = f"{parts[0]}.{parts[-1]}"
        symbols = self._db.get_symbol_by_name(short)
        if symbols:
            return symbols[0].id, 0.9
    
    # "obj.method" where obj is an imported name
    if parts[0] in {name for (file, name) in import_map if file == source_file}:
        target_file = import_map.get((source_file, parts[0]))
        if target_file:
            # Look for "method" or "ImportedClass.method" in target file
            for suffix in [parts[-1], f"{parts[0]}.{parts[-1]}"]:
                symbols = self._db.get_symbol_by_name(suffix, target_file)
                if symbols:
                    return symbols[0].id, 0.85
    
    # Fallback: try the last segment as a simple name
    return self._resolve_simple_name(
        Edge(..., target_name=parts[-1], ...), import_map
    )

def _get_enclosing_class(self, symbol_id: int) -> str | None:
    """Find the class that contains this symbol (if it's a method)."""
    sym = self._db.get_symbol_by_id(symbol_id)
    if sym and "." in sym.name:
        return sym.name.split(".")[0]
    # If symbol is in a class file, find the class
    if sym:
        classes = [s for s in self._db.get_colocated_symbols(sym.file) if s.kind == "class"]
        if len(classes) == 1:
            return classes[0].name
    return None
```

### What This Unlocks — Concrete Webpack Examples

| Current (`split(".")[-1]`) | After (full expression) | Resolution | Confidence |
|----------------------------|------------------------|------------|------------|
| `callAsync` (from `this.hooks.make.callAsync`) | `this.hooks.make.callAsync` | Unresolved, but hook name `make` preserved for diagnostics | 0.0 (but semantically valuable) |
| `seal` (from `compilation.seal()`) | `compilation.seal` | → `Compilation.seal` via import chain | 0.85 |
| `compile` (from `this.compile()`) | `this.compile` | → `Compiler.compile` via enclosing class | 0.9 |
| `getCompilationHooks` (from `NormalModule.getCompilationHooks()`) | `NormalModule.getCompilationHooks` | → exact symbol match (already qualified) | 1.0 |
| `mkdirp` (from `compiler.outputFileSystem.mkdirp()`) | `compiler.outputFileSystem.mkdirp` | Unresolved (intermediate object, not a symbol) | 0.0 |

### Edge Count Impact

Currently, many call edges are invisible because stripped names match builtins in `_GENERIC_CALL_TARGETS`:
- `this.hooks.make.call()` → `call` → filtered as builtin
- `arr.items.push(item)` → `push` → filtered as builtin
- `promise.then(cb)` → `then` → filtered as builtin

With full expressions, these become:
- `this.hooks.make.call` → NOT `call`, not filtered, hook info preserved
- `arr.items.push` → still a builtin pattern, but we can detect `push` at the end and filter
- `promise.then` → same

The builtin filter in `engine.py:13-75` needs updating: instead of `if edge.target_name in _GENERIC_CALL_TARGETS`, check `if edge.target_name.split(".")[-1] in _GENERIC_CALL_TARGETS and not edge.target_name.startswith("this.hooks")`. This preserves hook info while still filtering `arr.push`.

### Tests

- `test_full_call_expression_stored` — Parse `this.hooks.make.callAsync()`, verify `target_name == "this.hooks.make.callAsync"`
- `test_simple_call_unchanged` — Parse `compile()`, verify `target_name == "compile"` (no dots, no change)
- `test_method_call_on_import` — Parse `fs.readFileSync()`, verify `target_name == "fs.readFileSync"`
- `test_this_method_call` — Parse `this.compile()`, verify `target_name == "this.compile"`
- `test_chained_call` — Parse `a.b.c()`, verify `target_name == "a.b.c"`
- `test_console_still_filtered` — Parse `console.log()`, verify no edge created
- `test_new_expression_unchanged` — Parse `new Compilation()`, verify `target_name == "Compilation"` (new expressions don't use member access)
- `test_callee_recursion_guard` — Parse `function foo() { foo() }`, verify no self-edge

### Done When

- [ ] `_extract_calls` stores full call expression in `target_name`
- [ ] `callee.split(".")[-1]` removed from parser.py line 243
- [ ] Builtin filter updated to check last segment of dotted expressions
- [ ] Phase 2 resolution handles `this.method`, `Class.method`, `import.method` patterns
- [ ] `_get_enclosing_class` utility method implemented
- [ ] All parser tests updated and passing
- [ ] Benchmark: webpack index stores receiver info for >80% of method calls (vs 0% currently)

---

## Phase 4: Build the Actual Graph

**Priority:** HIGH — required for real coupling scores and transitive queries
**Estimated effort:** Medium
**Files changed:** `engine.py`, `pipeline.py`, `server.py`
**Files created:** `src/loom/store/graph.py`
**New dependency:** `networkx>=3.0` added to `pyproject.toml`
**Depends on:** Phase 1 (ID-based edges), Phase 2 (resolved edges)

### Current State

`pyproject.toml` line 20-26 — dependencies do NOT include networkx:
```toml
dependencies = [
    "fastmcp>=2.0",
    "tree-sitter>=0.24",
    "tree-sitter-javascript>=0.23",
    "sqlite-vec>=0.1",
    "watchdog>=4.0",
    "fastembed>=0.4",
]
```

CLAUDE.md line 103 claims "Graph Engine: NetworkX" — this is documentation that describes intent, not reality.

The current "graph" is two SQL queries in `engine.py`:
1. `_find_coupled` at line 258: `get_edges_from(target.name, target.file)` + `get_edges_to(target.name, target.file)` — one hop out, one hop in
2. `impact` at line 177: `get_edges_to(target.name, target.file)` — one hop in only

Both are flat adjacency lookups. There is no transitive traversal.

### The Fix — `src/loom/store/graph.py`

```python
"""In-memory symbol graph for transitive traversal and centrality analysis."""

import logging
from collections import deque

import networkx as nx

from loom.store.db import LoomDB
from loom.store.models import Edge

log = logging.getLogger(__name__)


class SymbolGraph:
    """Directed graph of resolved symbol relationships.
    
    Built from the edges table after Phase 2 resolution. Lives in memory
    alongside the SQLite store. Rebuilt on full index, incrementally
    updated on file changes.
    
    Nodes are symbol IDs (integers). Edge attributes:
    - relationship: str ("calls", "imports", "extends", etc.)
    - confidence: float (0.0-1.0, from Phase 2 resolution)
    """
    
    def __init__(self) -> None:
        self._g: nx.DiGraph = nx.DiGraph()
    
    @property
    def node_count(self) -> int:
        return self._g.number_of_nodes()
    
    @property
    def edge_count(self) -> int:
        return self._g.number_of_edges()
    
    def build_from_db(self, db: LoomDB) -> None:
        """Load all resolved edges into the graph. Replaces any existing graph."""
        self._g.clear()
        
        rows = db.conn.execute(
            "SELECT source_id, target_id, relationship, confidence "
            "FROM edges WHERE target_id IS NOT NULL",
        ).fetchall()
        
        for source_id, target_id, relationship, confidence in rows:
            # NetworkX DiGraph allows only one edge per (u, v) pair.
            # If multiple edges exist (e.g., A calls B AND A imports B),
            # keep the one with highest confidence.
            if self._g.has_edge(source_id, target_id):
                existing = self._g[source_id][target_id]
                if confidence > existing.get("confidence", 0.0):
                    self._g[source_id][target_id].update(
                        relationship=relationship, confidence=confidence
                    )
            else:
                self._g.add_edge(
                    source_id, target_id,
                    relationship=relationship, confidence=confidence
                )
        
        log.info(
            "Graph built: %d nodes, %d edges",
            self._g.number_of_nodes(),
            self._g.number_of_edges(),
        )
    
    def add_edge(self, source_id: int, target_id: int,
                 relationship: str, confidence: float) -> None:
        """Add or update a single edge. For incremental updates."""
        if self._g.has_edge(source_id, target_id):
            existing = self._g[source_id][target_id]
            if confidence > existing.get("confidence", 0.0):
                existing.update(relationship=relationship, confidence=confidence)
        else:
            self._g.add_edge(
                source_id, target_id,
                relationship=relationship, confidence=confidence
            )
    
    def remove_node(self, symbol_id: int) -> None:
        """Remove a node and all its edges. For incremental updates."""
        if symbol_id in self._g:
            self._g.remove_node(symbol_id)
    
    def dependents(self, symbol_id: int, max_depth: int = 3) -> list[tuple[int, int, str, float]]:
        """All symbols that depend on this one (transitively).
        
        Returns: [(symbol_id, depth, relationship, confidence)]
        Uses reverse BFS — follows edges backwards from the target.
        """
        if symbol_id not in self._g:
            return []
        
        result: list[tuple[int, int, str, float]] = []
        visited: set[int] = {symbol_id}
        queue: deque[tuple[int, int]] = deque()  # (node, depth)
        
        # Seed with direct predecessors
        for pred in self._g.predecessors(symbol_id):
            if pred not in visited:
                queue.append((pred, 1))
                visited.add(pred)
        
        while queue:
            node, depth = queue.popleft()
            edge_data = self._g[node][symbol_id] if self._g.has_edge(node, symbol_id) else {}
            # For transitive deps, get the edge to the intermediate node
            if depth > 1:
                # Find the edge from this node to any visited node
                for succ in self._g.successors(node):
                    if succ in visited and succ != node:
                        edge_data = self._g[node][succ]
                        break
            
            result.append((
                node,
                depth,
                edge_data.get("relationship", "transitive"),
                edge_data.get("confidence", 0.5),
            ))
            
            if depth < max_depth:
                for pred in self._g.predecessors(node):
                    if pred not in visited:
                        queue.append((pred, depth + 1))
                        visited.add(pred)
        
        return sorted(result, key=lambda x: x[1])
    
    def dependencies(self, symbol_id: int, max_depth: int = 3) -> list[tuple[int, int, str, float]]:
        """All symbols this one depends on (transitively).
        
        Returns: [(symbol_id, depth, relationship, confidence)]
        Uses forward BFS — follows edges forward from the source.
        """
        if symbol_id not in self._g:
            return []
        
        result: list[tuple[int, int, str, float]] = []
        visited: set[int] = {symbol_id}
        queue: deque[tuple[int, int]] = deque()
        
        for succ in self._g.successors(symbol_id):
            if succ not in visited:
                edge_data = self._g[symbol_id][succ]
                queue.append((succ, 1))
                visited.add(succ)
        
        while queue:
            node, depth = queue.popleft()
            # Get the edge from the nearest visited predecessor
            edge_data = {}
            for pred in self._g.predecessors(node):
                if pred in visited:
                    edge_data = self._g[pred][node]
                    break
            
            result.append((
                node,
                depth,
                edge_data.get("relationship", "transitive"),
                edge_data.get("confidence", 0.5),
            ))
            
            if depth < max_depth:
                for succ in self._g.successors(node):
                    if succ not in visited:
                        queue.append((succ, depth + 1))
                        visited.add(succ)
        
        return sorted(result, key=lambda x: x[1])
    
    def shortest_path(self, source_id: int, target_id: int) -> list[int] | None:
        """How does source reach target? Returns the symbol ID chain, or None."""
        try:
            return list(nx.shortest_path(self._g, source_id, target_id))
        except (nx.NetworkXNoPath, nx.NodeNotFound):
            return None
    
    def impact_radius(self, symbol_id: int, max_depth: int = 3) -> dict[int, float]:
        """Blast radius with exponential decay.
        
        Depth 1 = 1.0, depth 2 = 0.5, depth 3 = 0.25.
        Score is further weighted by edge confidence.
        """
        result: dict[int, float] = {}
        for dep_id, depth, relationship, confidence in self.dependents(symbol_id, max_depth):
            decay = 1.0 / (2 ** (depth - 1))
            result[dep_id] = decay * confidence
        return result
    
    def centrality(self, top_n: int = 20) -> list[tuple[int, float]]:
        """Most connected/important symbols by PageRank."""
        if self._g.number_of_nodes() == 0:
            return []
        scores = nx.pagerank(self._g)
        return sorted(scores.items(), key=lambda x: x[1], reverse=True)[:top_n]
    
    def neighbors_with_metadata(
        self, symbol_id: int, max_depth: int = 2
    ) -> list[tuple[int, int, str, float]]:
        """Both dependents and dependencies, merged.
        
        Returns: [(symbol_id, depth, relationship, confidence)]
        """
        deps = self.dependents(symbol_id, max_depth)
        depencies = self.dependencies(symbol_id, max_depth)
        
        seen: set[int] = set()
        merged: list[tuple[int, int, str, float]] = []
        for item in deps + depencies:
            if item[0] not in seen:
                seen.add(item[0])
                merged.append(item)
        
        return sorted(merged, key=lambda x: (x[1], -x[3]))
```

### Memory Budget — Real Numbers

NetworkX stores nodes as dict keys, edges as nested dicts. Per-edge overhead: ~200 bytes (two dict entries + attribute dict).

| Codebase | Symbols | Resolved Edges | Graph Memory | Acceptable? |
|----------|---------|---------------|--------------|-------------|
| Small project (50 files) | ~500 | ~1,500 | ~300 KB | Trivial |
| Medium project (500 files) | ~5,000 | ~15,000 | ~3 MB | Fine |
| webpack (587 files) | 9,262 | ~18,000 (est. 70% resolved) | ~4 MB | Fine |
| Large monorepo (5K files) | ~50,000 | ~150,000 | ~30 MB | Acceptable |
| Very large (50K files) | ~500,000 | ~1,500,000 | ~300 MB | Borderline — Wave 2 concern |

For the MCP server long-running process, 4-30 MB is negligible. The graph stays in memory alongside the SQLite connection.

### Integration Points

**`server.py`** — Initialize graph alongside engine:
```python
from loom.store.graph import SymbolGraph

def initialize(target_dir: Path) -> None:
    ...
    _graph = SymbolGraph()
    _engine = SearchEngine(_db, _embedder, _graph)
    result = _pipeline.full_index()
    _graph.build_from_db(_db)  # Build graph after indexing
```

**`pipeline.py`** — Rebuild graph after indexing:
```python
class IndexPipeline:
    def __init__(self, config, db, embedder, graph: SymbolGraph) -> None:
        ...
        self._graph = graph
    
    def _index_files(self, files):
        ...
        if indexed > 0:
            resolved = self._resolve_all_edges()
            self._graph.build_from_db(self._db)  # Rebuild after resolution
```

**`engine.py`** — Use graph for impact and coupled:
```python
class SearchEngine:
    def __init__(self, db, embedder, graph: SymbolGraph) -> None:
        ...
        self._graph = graph
    
    def impact(self, symbol_name, file=None, kind=None):
        ...
        # Use graph traversal instead of one-hop SQL
        radius = self._graph.impact_radius(target.id, max_depth=3)
        for sym_id, decay_score in radius.items():
            sym = self._db.get_symbol_by_id(sym_id)
            if sym and sym.name not in _GENERIC_CALL_TARGETS:
                dependents.append(CoupledSymbol(
                    symbol=sym, score=decay_score,
                    reason=f"transitive dependent",
                ))
```

### Tests

- `test_graph_build_from_resolved_edges` — Build graph from 5 resolved edges, verify node/edge count
- `test_graph_ignores_unresolved_edges` — Unresolved edges (target_id=NULL) not in graph
- `test_transitive_dependents` — A→B→C, `dependents(C)` returns [B at depth 1, A at depth 2]
- `test_transitive_dependencies` — A→B→C, `dependencies(A)` returns [B at depth 1, C at depth 2]
- `test_dependents_max_depth` — A→B→C→D, `dependents(D, max_depth=2)` excludes A (depth 3)
- `test_shortest_path` — A→B→C, `shortest_path(A, C)` returns [A, B, C]
- `test_shortest_path_no_path` — Disconnected nodes return None
- `test_impact_radius_decay` — depth 1 = 1.0 × confidence, depth 2 = 0.5 × confidence
- `test_centrality_ranking` — Hub node (many in/out edges) ranks higher
- `test_neighbors_with_metadata` — Returns both dependents and dependencies, deduplicated
- `test_incremental_add_edge` — `add_edge` updates graph without full rebuild
- `test_incremental_remove_node` — `remove_node` cleans up all connected edges
- `test_empty_graph` — All methods return empty results, no crashes
- `test_self_loop_handling` — A→A edge doesn't cause infinite traversal

### Done When

- [ ] `networkx>=3.0` added to `pyproject.toml` dependencies
- [ ] `SymbolGraph` class in `src/loom/store/graph.py`
- [ ] Graph built from resolved edges after Phase 2 indexing
- [ ] Graph rebuilt on full index, incrementally updated on file changes
- [ ] `impact()` uses `graph.impact_radius()` for transitive traversal
- [ ] `_find_coupled()` uses `graph.neighbors_with_metadata()` for multi-hop discovery
- [ ] Benchmark: `impact("_makePathsRelative")` recall > 70% (was 8%)
- [ ] Benchmark: graph build < 1s for 10K-symbol / 20K-edge graph
- [ ] Benchmark: traversal operations < 10ms per query

---

## Phase 5: Real Coupling Scores

**Priority:** HIGH — the entire value proposition
**Estimated effort:** Medium
**Files changed:** `engine.py`
**Files created:** `src/loom/search/scoring.py`
**Depends on:** Phase 1 (edge confidence), Phase 2 (resolved edges), Phase 4 (graph traversal)

### Current State — Exact Code Trace

All coupling scores in `engine.py` are hardcoded:

**`_find_coupled`** (line 258-320):
- Line 278: `score=0.7` — all outgoing structural edges, regardless of type or confidence
- Line 296: `score=0.6` — all incoming structural edges
- Line 317: `score=sim` where `sim = max(0.0, 1.0 - distance)` — raw vector distance, no combination with structural

**`impact`** (line 177-227):
- Line 199: `score=0.8` — all structural dependents
- Line 218: `score=sim` — semantic neighbors

The `reason` field is always one of:
- `"{relationship} (structural)"` — e.g., `"calls (structural)"`
- `"called_by (structural)"`
- `"semantically similar"`

There is no:
- Relationship type weighting (`calls` vs `imports` vs `co_located`)
- Depth decay (direct caller vs transitive dependent)
- Edge confidence weighting (import-resolved vs unresolved)
- Multi-signal fusion (combining structural proximity WITH semantic similarity WITH co-change)

### The Fix — `src/loom/search/scoring.py`

```python
"""Coupling score computation — fuses structural, semantic, and evolutionary signals."""

from dataclasses import dataclass


@dataclass(frozen=True)
class CouplingScore:
    structural: float     # 0.0-1.0 — graph proximity × edge confidence × relationship weight
    semantic: float       # 0.0-1.0 — embedding cosine similarity
    evolutionary: float   # 0.0-1.0 — git co-change frequency (Phase 6, 0.0 until then)
    combined: float       # weighted fusion of all three

    def breakdown(self) -> str:
        parts = []
        if self.structural > 0.01:
            parts.append(f"structural={self.structural:.2f}")
        if self.semantic > 0.01:
            parts.append(f"semantic={self.semantic:.2f}")
        if self.evolutionary > 0.01:
            parts.append(f"evolutionary={self.evolutionary:.2f}")
        return " + ".join(parts) if parts else "unknown"


# Signal weights — TUNABLE. Start with Professor's recommendation.
# These should be benchmarked against webpack and adjusted.
W_STRUCTURAL = 0.45
W_SEMANTIC = 0.35
W_EVOLUTIONARY = 0.20  # Phase 6. Until then, structural and semantic get proportionally more.

# Relationship type base weights.
# calls/extends are the strongest structural signals.
# imports are weaker (you import many things you don't tightly couple with).
# co_located is weakest (same file doesn't mean related).
RELATIONSHIP_WEIGHT: dict[str, float] = {
    "calls": 1.0,
    "called_by": 0.9,
    "extends": 1.0,
    "extended_by": 0.9,
    "instantiates": 0.85,
    "imports": 0.5,
    "imported_by": 0.4,
    "co_located": 0.2,
}


def compute_structural(
    relationship: str,
    confidence: float,
    depth: int,
) -> float:
    """Structural coupling from graph proximity.
    
    Formula: base_weight × confidence × depth_decay
    
    - base_weight: from RELATIONSHIP_WEIGHT (1.0 for calls, 0.5 for imports)
    - confidence: from Phase 2 resolution (1.0 exact, 0.6 unique name)
    - depth_decay: 1/(2^(depth-1)) — 1.0 at depth 1, 0.5 at depth 2, 0.25 at depth 3
    """
    base = RELATIONSHIP_WEIGHT.get(relationship, 0.3)
    decay = 1.0 / (2 ** (depth - 1))
    return base * confidence * decay


def compute_semantic(distance: float) -> float:
    """Semantic coupling from embedding distance.
    
    sqlite-vec returns L2 distance. Convert to similarity:
    similarity = max(0, 1 - distance)
    
    jina-embeddings-v2-base-code produces normalized vectors,
    so distance ranges from 0 (identical) to ~2.0 (opposite).
    In practice, code symbols cluster in 0.3-1.5 range.
    """
    return max(0.0, 1.0 - distance)


def compute_evolutionary(frequency: int, max_frequency: int = 10) -> float:
    """Evolutionary coupling from git co-change frequency.
    
    Normalized: frequency / max_frequency, capped at 1.0.
    A pair that co-changes 10+ times gets 1.0.
    A pair that co-changes 2 times gets 0.2.
    
    Phase 6 implementation. Returns 0.0 until then.
    """
    if frequency <= 0:
        return 0.0
    return min(1.0, frequency / max_frequency)


def fuse_signals(
    structural: float,
    semantic: float,
    evolutionary: float,
) -> CouplingScore:
    """Fuse three signals into a single coupling score.
    
    If evolutionary coupling isn't available (Phase 6 not yet implemented),
    redistribute its weight proportionally:
    - structural gets: W_STRUCTURAL + W_EVOLUTIONARY * (W_STRUCTURAL / (W_STRUCTURAL + W_SEMANTIC))
    - semantic gets:   W_SEMANTIC + W_EVOLUTIONARY * (W_SEMANTIC / (W_STRUCTURAL + W_SEMANTIC))
    """
    if evolutionary == 0.0:
        # Redistribute evolutionary weight proportionally
        total_active = W_STRUCTURAL + W_SEMANTIC
        w_s = W_STRUCTURAL / total_active
        w_e = W_SEMANTIC / total_active
        combined = w_s * structural + w_e * semantic
    else:
        combined = (
            W_STRUCTURAL * structural
            + W_SEMANTIC * semantic
            + W_EVOLUTIONARY * evolutionary
        )
    
    return CouplingScore(
        structural=structural,
        semantic=semantic,
        evolutionary=evolutionary,
        combined=min(1.0, combined),
    )
```

### Updated `_find_coupled` in `engine.py`

```python
from loom.search.scoring import compute_structural, compute_semantic, fuse_signals

def _find_coupled(self, target: Symbol) -> list[CoupledSymbol]:
    coupled: list[CoupledSymbol] = []
    seen: set[int] = {target.id}
    
    # 1. Graph neighbors — structural signal
    neighbors = self._graph.neighbors_with_metadata(target.id, max_depth=2)
    for sym_id, depth, relationship, confidence in neighbors:
        if sym_id in seen:
            continue
        seen.add(sym_id)
        
        s_structural = compute_structural(relationship, confidence, depth)
        
        # Compute semantic similarity for this pair
        sym = self._db.get_symbol_by_id(sym_id)
        if sym is None:
            continue
        target_text = self._embedder.build_symbol_text(target.name, target.kind, target.context)
        sym_text = self._embedder.build_symbol_text(sym.name, sym.kind, sym.context)
        target_emb = self._embedder.embed_single(target_text)
        # For efficiency, we could cache embeddings. For now, use vec search.
        # TODO: Phase 5 optimization — use pre-computed embeddings from sqlite-vec
        s_semantic = 0.0  # computed below if we have vec data
        
        score = fuse_signals(s_structural, s_semantic, 0.0)
        if score.combined > 0.15:
            coupled.append(CoupledSymbol(
                symbol=sym,
                score=score.combined,
                reason=score.breakdown(),
            ))
    
    # 2. Semantic neighbors — symbols with no structural edge but high embedding similarity
    sym_text = self._embedder.build_symbol_text(target.name, target.kind, target.context)
    embedding = self._embedder.embed_single(sym_text)
    vec_hits = self._db.search_vec(embedding, limit=20)
    for sym_id, distance in vec_hits:
        if sym_id in seen:
            continue
        seen.add(sym_id)
        
        s_semantic = compute_semantic(distance)
        if s_semantic < 0.3:
            continue
        
        # Check if there's also a structural path (boost if so)
        path = self._graph.shortest_path(target.id, sym_id)
        s_structural = 0.0
        if path and len(path) <= 4:
            s_structural = 0.2 / len(path)  # Weak structural signal from indirect path
        
        score = fuse_signals(s_structural, s_semantic, 0.0)
        sym = self._db.get_symbol_by_id(sym_id)
        if sym and score.combined > 0.2:
            coupled.append(CoupledSymbol(
                symbol=sym,
                score=score.combined,
                reason=score.breakdown(),
            ))
    
    coupled.sort(key=lambda c: c.score, reverse=True)
    return coupled[:30]
```

### Expected Score Distribution

Before (hardcoded):
```
All structural outgoing: 0.7
All structural incoming: 0.6
All semantic: varies (0.3-0.9)
```

After (computed):
```
Direct call, confidence 1.0:      structural=1.0, semantic=varies → combined ~0.56-0.80
Direct call, confidence 0.6:      structural=0.6, semantic=varies → combined ~0.34-0.65
Import, confidence 0.95:          structural=0.47, semantic=varies → combined ~0.27-0.55
2-hop transitive, confidence 0.8: structural=0.40, semantic=varies → combined ~0.23-0.50
Co-located (same file):           structural=0.20, semantic=varies → combined ~0.12-0.40
Semantic-only (no edge):          structural=0.0, semantic=0.7 → combined ~0.31
```

This gives meaningful ranking: a direct caller with high confidence (0.80) clearly outranks a co-located symbol (0.40) or a 2-hop transitive dependent (0.50).

### Tests

- `test_structural_score_calls_vs_imports` — `calls` (1.0) > `imports` (0.5) for same confidence/depth
- `test_structural_score_depth_decay` — depth 1 (1.0) > depth 2 (0.5) > depth 3 (0.25) for same relationship/confidence
- `test_structural_score_confidence_weighting` — confidence 1.0 > confidence 0.6 for same relationship/depth
- `test_semantic_score_from_distance` — distance 0 → 1.0, distance 0.5 → 0.5, distance 1.0 → 0.0
- `test_fuse_signals_structural_only` — structural=0.8, semantic=0.0 → combined = 0.8 × (0.45/0.80) ≈ 0.45
- `test_fuse_signals_semantic_only` — structural=0.0, semantic=0.7 → combined = 0.7 × (0.35/0.80) ≈ 0.31
- `test_fuse_signals_both` — structural=0.8, semantic=0.7 → combined = 0.45×0.8 + 0.35×0.7 ≈ 0.61
- `test_fuse_signals_with_evolutionary` — all three signals present, proper weighting
- `test_score_capped_at_one` — extreme values don't exceed 1.0
- `test_coupling_score_breakdown_string` — `breakdown()` returns readable signal decomposition
- `test_relationship_weight_coverage` — all relationship types in RELATIONSHIP_WEIGHT have sensible values
- `test_evolutionary_zero_redistributes_weight` — when evolutionary=0, structural+semantic weights sum to 1.0

### Done When

- [ ] `CouplingScore` dataclass with structural/semantic/evolutionary breakdown
- [ ] `scoring.py` module with `compute_structural`, `compute_semantic`, `compute_evolutionary`, `fuse_signals`
- [ ] `_find_coupled` uses real coupling computation instead of hardcoded 0.7/0.6
- [ ] `impact()` uses coupling scores from graph traversal
- [ ] MCP tool output `reason` field shows score breakdown (e.g., `"structural=0.85 + semantic=0.42"`)
- [ ] Score distribution is continuous (not flat 0.6/0.7)
- [ ] Weights configurable via `LoomConfig` (add `structural_weight`, `semantic_weight`, `evolutionary_weight`)
- [ ] Benchmark: results ranked meaningfully — direct callers score higher than imports

---

## Phase 6: Evolutionary Coupling (Git Co-Change)

**Priority:** MEDIUM — the third signal, completes the trifecta
**Estimated effort:** Medium-Large
**Files changed:** `db.py` (new table), `pipeline.py`, `config.py`
**Files created:** `src/loom/indexer/git_analyzer.py`
**Depends on:** Phase 1 (ID-based edges for symbol references), Phase 5 (scoring integration)

### Current State

No implementation exists. The entire git analysis pipeline is vaporware:
- No `cochange` table in `db.py:14-50` schema
- No `git_analyzer.py` in `src/loom/indexer/`
- No git subprocess calls anywhere in the codebase
- `pyproject.toml` has no git-related dependencies (none needed — `subprocess` + `git` CLI)
- The architecture diagram in CLAUDE.md shows `Git Log Analyzer` but it doesn't exist

### The Fix — `src/loom/indexer/git_analyzer.py`

```python
"""Git log analysis for evolutionary coupling (co-change detection)."""

import logging
import subprocess
from collections import defaultdict
from pathlib import Path

log = logging.getLogger(__name__)


class GitAnalyzer:
    """Mines git history for file-level co-change patterns.
    
    Design decisions:
    - Uses `git log --name-only` — no diff parsing, fast
    - Filters commits with >max_files_per_commit files (merges, bulk refactors = noise)
    - Filters commits with <2 files (no co-change possible)
    - Only considers files matching the configured extensions
    - Time-decays: recent co-changes score higher than old ones
    """
    
    def __init__(self, repo_root: Path, extensions: frozenset[str]) -> None:
        self._root = repo_root
        self._extensions = extensions
    
    def is_git_repo(self) -> bool:
        result = subprocess.run(
            ["git", "rev-parse", "--is-inside-work-tree"],
            cwd=self._root, capture_output=True, text=True,
        )
        return result.returncode == 0
    
    def analyze_cochanges(
        self,
        max_commits: int = 500,
        max_files_per_commit: int = 20,
    ) -> dict[tuple[str, str], int]:
        """Mine git log for file-level co-changes.
        
        Returns: {(file_a, file_b): frequency} where file_a < file_b (sorted pair).
        
        Algorithm:
        1. Parse `git log --name-only` for the last N commits
        2. For each commit, collect the list of changed files
        3. Skip commits with >max_files or <2 files
        4. For each pair of files in the commit, increment co-change count
        
        Complexity: O(commits × files_per_commit²) — bounded by max_files_per_commit.
        For 500 commits × 20 files max: 500 × 190 = 95,000 pair checks. Fast.
        """
        if not self.is_git_repo():
            log.warning("Not a git repository: %s", self._root)
            return {}
        
        result = subprocess.run(
            ["git", "log", f"--max-count={max_commits}",
             "--name-only", "--pretty=format:---COMMIT---"],
            cwd=self._root, capture_output=True, text=True, timeout=30,
        )
        
        if result.returncode != 0:
            log.error("git log failed: %s", result.stderr)
            return {}
        
        cochanges: dict[tuple[str, str], int] = defaultdict(int)
        current_files: list[str] = []
        
        for line in result.stdout.splitlines():
            if line == "---COMMIT---":
                self._process_commit(current_files, max_files_per_commit, cochanges)
                current_files = []
            elif line.strip():
                file_path = line.strip()
                # Only track files matching configured extensions
                if any(file_path.endswith(ext) for ext in self._extensions):
                    current_files.append(file_path)
        
        # Process last commit
        self._process_commit(current_files, max_files_per_commit, cochanges)
        
        log.info(
            "Git analysis: %d commits → %d co-change pairs",
            max_commits, len(cochanges),
        )
        return dict(cochanges)
    
    def _process_commit(
        self,
        files: list[str],
        max_files: int,
        cochanges: dict[tuple[str, str], int],
    ) -> None:
        if len(files) < 2 or len(files) > max_files:
            return
        for i, file_a in enumerate(files):
            for file_b in files[i + 1:]:
                pair = (min(file_a, file_b), max(file_a, file_b))
                cochanges[pair] += 1
```

### Schema Addition in `db.py`

Add to `SCHEMA` string:

```sql
CREATE TABLE IF NOT EXISTS cochange (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_a TEXT NOT NULL,
    file_b TEXT NOT NULL,
    frequency INTEGER NOT NULL DEFAULT 1,
    UNIQUE(file_a, file_b)
);

CREATE INDEX IF NOT EXISTS idx_cochange_a ON cochange(file_a);
CREATE INDEX IF NOT EXISTS idx_cochange_b ON cochange(file_b);
```

Note: This is **file-level** co-change, not symbol-level. Symbol-level co-change (every symbol in file A paired with every symbol in file B) creates a combinatorial explosion. Instead, we store file pairs and compute symbol-level scores on demand:

```python
# In scoring.py:
def compute_evolutionary(
    db: LoomDB,
    source_file: str,
    target_file: str,
) -> float:
    """Evolutionary coupling between two symbols based on their files' co-change frequency."""
    if source_file == target_file:
        return 0.0  # Same-file co-change is meaningless
    pair = (min(source_file, target_file), max(source_file, target_file))
    row = db.conn.execute(
        "SELECT frequency FROM cochange WHERE file_a = ? AND file_b = ?",
        pair,
    ).fetchone()
    if row is None:
        return 0.0
    return min(1.0, row[0] / 10.0)  # 10+ co-changes = max score
```

### New DB Methods

```python
def upsert_cochange(self, file_a: str, file_b: str, frequency: int) -> None:
    pair = (min(file_a, file_b), max(file_a, file_b))
    self.conn.execute(
        "INSERT INTO cochange (file_a, file_b, frequency) VALUES (?, ?, ?) "
        "ON CONFLICT(file_a, file_b) DO UPDATE SET frequency = ?",
        (*pair, frequency, frequency),
    )

def get_cochange_frequency(self, file_a: str, file_b: str) -> int:
    pair = (min(file_a, file_b), max(file_a, file_b))
    row = self.conn.execute(
        "SELECT frequency FROM cochange WHERE file_a = ? AND file_b = ?",
        pair,
    ).fetchone()
    return row[0] if row else 0

def get_top_cochanges(self, file: str, limit: int = 20) -> list[tuple[str, int]]:
    rows = self.conn.execute(
        "SELECT CASE WHEN file_a = ? THEN file_b ELSE file_a END, frequency "
        "FROM cochange WHERE file_a = ? OR file_b = ? "
        "ORDER BY frequency DESC LIMIT ?",
        (file, file, file, limit),
    ).fetchall()
    return [(row[0], row[1]) for row in rows]
```

### Pipeline Integration

```python
# In pipeline.py — after Phase 2 resolution:
def full_index(self) -> dict[str, int]:
    ...
    result = self._index_files(files)
    
    # Git co-change analysis (if enabled and in a git repo)
    if self._config.enable_git_analysis:
        git = GitAnalyzer(self._config.target_dir, self._config.watch_extensions)
        if git.is_git_repo():
            cochanges = git.analyze_cochanges(
                max_commits=self._config.git_max_commits,
            )
            for (file_a, file_b), freq in cochanges.items():
                self._db.upsert_cochange(file_a, file_b, freq)
            self._db.commit()
            log.info("Git analysis: stored %d co-change pairs", len(cochanges))
    
    return result
```

### Config Addition

```python
@dataclass(frozen=True)
class LoomConfig:
    ...
    enable_git_analysis: bool = True
    git_max_commits: int = 500
    git_max_files_per_commit: int = 20
```

### Performance

For webpack (6,338 commits as of benchmark):
- `git log --max-count=500 --name-only` completes in ~0.5s
- Processing 500 commits with up to 20 files each: ~0.1s
- Storing ~1,000-5,000 co-change pairs: ~0.1s
- **Total: ~0.7s** — negligible compared to embedding generation (~200s)

### Tests

- `test_git_cochange_extraction` — Mock `subprocess.run` with sample git log output, verify co-change pairs
- `test_large_commit_filtered` — Commits with >20 files excluded from analysis
- `test_single_file_commit_filtered` — Commits with <2 files excluded
- `test_extension_filtering` — Only .js/.ts files counted, .md/.json ignored
- `test_cochange_pair_ordering` — Pairs are always (min, max) for consistent dedup
- `test_not_a_git_repo` — Gracefully returns empty dict
- `test_git_timeout` — subprocess timeout doesn't crash the indexer
- `test_upsert_cochange` — Inserting same pair twice updates frequency
- `test_get_cochange_frequency` — Correct frequency returned
- `test_get_top_cochanges` — Returns top N co-changed files for a given file
- `test_evolutionary_score_computation` — freq 10 → 1.0, freq 5 → 0.5, freq 0 → 0.0
- `test_same_file_evolutionary_zero` — Same file pair returns 0.0 (meaningless)
- `test_scoring_with_evolutionary` — Full fusion with all three signals present

### Done When

- [ ] `GitAnalyzer` class in `src/loom/indexer/git_analyzer.py`
- [ ] `cochange` table in database schema
- [ ] `upsert_cochange`, `get_cochange_frequency`, `get_top_cochanges` in `db.py`
- [ ] Git analysis runs during `full_index()` when enabled
- [ ] `compute_evolutionary()` in `scoring.py` queries cochange table
- [ ] `fuse_signals()` properly weights evolutionary signal when present
- [ ] Config flags: `enable_git_analysis`, `git_max_commits`, `git_max_files_per_commit`
- [ ] Git analysis completes in <2s for 500 commits
- [ ] Benchmark: at least 3 relationships discovered that structural + semantic miss
- [ ] All tests pass with git analysis enabled and disabled

---

## Build Order & Dependencies

```
Phase 1 ─── ID-Based Edges ──────────────────────┐
    │                                              │
    ▼                                              │
Phase 3 ─── Full Call Expressions (small, quick)   │
    │                                              │
    ▼                                              │
Phase 2 ─── Two-Phase Indexing ────────────────────┤
    │                                              │
    ▼                                              │
Phase 4 ─── Build Graph (NetworkX) ◄──────────────┘
    │
    ▼
Phase 5 ─── Real Coupling Scores
    │
    ▼
Phase 6 ─── Evolutionary Coupling (Git)
```

**Phase 1 is the keystone.** Nothing else works without ID-based edges. Start here, no exceptions.

**Phase 3 before Phase 2.** Phase 3 is a small parser change (remove one line). Phase 2's resolution logic needs to handle the richer data that Phase 3 produces. Do Phase 3 first so Phase 2 can resolve full call expressions from day one.

**Phase 4 depends on resolved edges.** The graph is only useful if edges have `target_id` set. Without Phase 1+2, you're building a graph of unresolved names — just a more expensive version of the current SQL bag.

**Phase 5 depends on the graph.** Real coupling scores need graph traversal for structural scoring. Without Phase 4, you're computing hardcoded values with extra steps.

**Phase 6 is independent in schema but depends on Phase 5 for scoring integration.** The git analyzer can be built and tested in isolation, but it only feeds into coupling scores once Phase 5's `fuse_signals()` exists.

### Implementation Cadence

Each phase gets its own `/build` pipeline:
- `build/wave1-phase1-id-edges`
- `build/wave1-phase3-call-expressions` (Phase 3 before 2!)
- `build/wave1-phase2-two-phase-index`
- `build/wave1-phase4-graph`
- `build/wave1-phase5-coupling-scores`
- `build/wave1-phase6-evolutionary`

No mega-PRs. Each phase is independently testable and deployable.

**Benchmark after every merge.** Re-run `/bm` against webpack/lib after each phase lands. Track the metrics incrementally:

| After Phase | Expected Impact |
|-------------|----------------|
| Phase 1 (edges) | Cleaner queries, no functional change yet (edges still unresolved) |
| Phase 3 (expressions) | More edge data stored, no resolution change yet |
| Phase 2 (resolution) | `impact()` recall jumps from 8% to ~60%. F-grade calls drop. |
| Phase 4 (graph) | Transitive dependencies surface. `impact()` recall → >70%. |
| Phase 5 (scores) | Score distribution becomes continuous. Rankings become meaningful. |
| Phase 6 (git) | New relationships appear. Token efficiency approaches 5x target. |

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Full re-index required | Certain | Low | `.loom.db` is per-project, gitignored. Re-index is <5s for most codebases. Pre-1.0, no backwards compat promise. |
| Two-phase indexing increases index time | High | Medium | Phase 2 resolution is O(unresolved edges × strategies). Each strategy is an indexed SQL lookup. Budget 20% overhead. Profile after Phase 2 lands — optimize if >30%. |
| Graph memory at scale | Low | Medium | 10K symbols = ~4MB. 100K symbols = ~30MB. Only a concern at 500K+ — revisit in Wave 2 if someone indexes a 50K-file monorepo. |
| Git analysis slow on large repos | Medium | Low | Capped at 500 commits. Commits with >20 files excluded. `timeout=30` on subprocess. Config flag to disable entirely. |
| Coupling weight tuning wrong | High | Medium | Start with Professor's recommendation (0.45/0.35/0.20). Expose as config for experimentation. Benchmark against webpack. Adjust based on data, not intuition. |
| Resolution ambiguity creates false edges | Medium | Medium | Confidence field captures uncertainty. Low-confidence edges (0.6) score lower than high-confidence (1.0). The scoring system makes this a ranking problem, not a correctness problem. |
| Semantic signal regresses | Low | High | The embedding-based search is Loom's strongest feature. Every phase tested against Task 3 (concept search) and Task 5 (disambiguation). No phase should change embedding generation or vector search. |
| Phase ordering violation | Low | High | Build order is enforced by `/build` pipeline names. Each phase's tests validate its dependencies exist. Phase 2 tests import Phase 1 models. Phase 4 tests import Phase 2 resolution. |

---

## Success Criteria

After Wave 1 is complete, re-run the LOOM-1 benchmark (webpack/lib). The targets:

| Metric | Current (v2) | Target | Improvement |
|--------|-------------|--------|-------------|
| `impact()` recall on `_makePathsRelative` | 8% (3/38) | >70% (27/38) | 9x |
| F-grade call rate | 11% (3/28) | <5% (1/20) | 2x fewer failures |
| Total calls needed for 5 tasks | 28 | <20 | 30% reduction |
| Tokens per useful symbol | 376 | <150 | 2.5x more efficient |
| Coupling score distribution | flat (0.6/0.7) | continuous 0.15-1.0 | meaningful ranking |
| Score breakdown in output | single label | multi-signal | `structural=0.85 + semantic=0.42` |
| Semantic cluster discovery (Task 3) | works (4 plugins found) | still works | no regression |
| Loom recall vs grep | 44% (8/18 grep symbols also found) | >80% | structural improvements close the gap |
| Time vs grep | -32% (119s vs 176s) | -50% | transitive graph queries replace multi-hop grep chains |
| Edge resolution rate | ~30% (est.) | >70% | Phase 2 resolution against complete symbol table |

The **existential metric**: Loom should find >90% of what grep finds, plus >3x more that grep can't find. Currently it's ~44% recall vs grep — the foundation rebuild should push this above 80%.

---

## What This Wave Does NOT Cover

These are explicitly Wave 2 or later:

- **Multi-language support** — currently JS/TS only via `tree-sitter-javascript`. Adding Python (`tree-sitter-python`), Go, Rust parsers requires per-language `_walk_node` implementations. Wave 2.
- **Plugin/hook architecture detection** — webpack's `this.hooks.X.call()` pattern is partially addressed by Phase 3 (we store the full expression), but detecting hook registration/subscription (`hooks.X.tap("PluginName", fn)`) requires webpack-specific AST patterns. Wave 2.
- **Embedding model upgrade** — `jina-embeddings-v2-base-code` (768 dims, via fastembed) works. Evaluating alternatives (`nomic-embed-text`, `codebert`, `unixcoder`) is a Wave 2 experiment. The `build_symbol_text` function in `embedder.py:43-44` is too simple (`f"{kind} {name}\n{context}"`) but doesn't block the foundation.
- **MCP tool API changes** — the tool surface (`search`, `related`, `impact`, `neighborhood`, `status`, `reindex`) stays identical. Only internal quality improves. Wave 2 might add `path(from, to)` or `hub()`.
- **Performance optimization** — batch embedding, connection pooling, query caching, incremental graph updates. Wave 2.
- **Better embedding text** — `embedder.py:43-44` conflates purpose with implementation. `build_symbol_text` should include file path, relationships, and docstrings. Wave 2.
- **TypeScript-specific parsing** — currently uses `tree-sitter-javascript` for .ts files too. Type annotations, interfaces, generics are not parsed. Wave 2.

This wave is about the foundation. Get the data model right, get the graph right, get the scores right. Everything else builds on this.

---

## Appendix A: Benchmark Data Reference

From `tmp/bench-results-2026-05-10-v2.md`:

### Per-Task Results

| Task | Loom Time | Grep Time | Loom Calls | Grep Cmds | Winner |
|------|-----------|-----------|------------|-----------|--------|
| T1: Call chain (`compile` in `Compiler.js`) | 19s | 64s | 8 | 22 | **Loom** (3.4x faster) |
| T2: Blast radius (`_makePathsRelative`) | 7s | 21s | 4 | 3 | **Grep** (100% recall vs 8%) |
| T3: Concept search (tree shaking) | 13s | 21s | 5 | 12 | **Loom** (4 plugins grep missed) |
| T4: Modify (`NullFactory`) | 31s | 14s | 4 | 4 | **Tie** |
| T5: Disambiguation (`create`) | 12s | 28s | 5 | 7 | **Loom** (10 ranked vs 909 noise) |

### Aggregate

| Metric | Loom | Grep |
|--------|------|------|
| Total time | 119s | 176s |
| Total calls | 28 | 52 |
| Total symbols discovered | ~200 | ~65 |
| Total false positives | 12 | ~860 |
| Total false negatives | ~43 | ~5 |
| F-grade calls | 3/28 (11%) | N/A |
| Call grade A | 11/28 (39%) | N/A |

### Loom v1 → v2 Improvements (JC fixes)

| Metric | v1 | v2 | Change |
|--------|----|----|--------|
| Total time | 191s | 119s | -38% |
| Total calls | 41 | 28 | -32% |
| F-grade rate | 24% | 11% | -54% relative |
| `related("compile","Compiler.js")` | 0 results | 2 results | Fixed (fuzzy matching) |
| `related("create")` no file | 80+ chaos | 7 coherent | Fixed (NULL exclusion) |
| `impact()` recall (T2) | 40% | 8% | Regressed (tradeoff) |

---

## Appendix B: File Change Summary

### Files Modified (Phases 1-6)

| File | Phase(s) | Nature of Change |
|------|----------|-----------------|
| `src/loom/store/models.py` | 1 | `Edge` dataclass: name fields → ID fields + confidence |
| `src/loom/store/db.py` | 1, 2, 6 | Schema, edge methods, cochange table |
| `src/loom/indexer/parser.py` | 3 | Remove `callee.split(".")[-1]`, keep full expression |
| `src/loom/indexer/pipeline.py` | 2, 4, 6 | Two-phase indexing, graph rebuild, git analysis |
| `src/loom/search/engine.py` | 1, 4, 5 | ID-based queries, graph traversal, real coupling scores |
| `src/loom/config.py` | 5, 6 | Coupling weights, git analysis config |
| `src/loom/server.py` | 4 | Pass graph to SearchEngine |
| `pyproject.toml` | 4 | Add `networkx>=3.0` |
| `tests/conftest.py` | 1 | Edge fixtures use IDs |
| `tests/test_db.py` | 1, 6 | Edge tests, cochange tests |
| `tests/test_engine.py` | 1, 4, 5 | Updated for ID edges, graph, scoring |

### Files Created (Phases 1-6)

| File | Phase | Purpose |
|------|-------|---------|
| `src/loom/store/graph.py` | 4 | NetworkX-backed symbol graph |
| `src/loom/search/scoring.py` | 5 | Coupling score computation |
| `src/loom/indexer/git_analyzer.py` | 6 | Git co-change analysis |
| `tests/test_graph.py` | 4 | Graph tests |
| `tests/test_scoring.py` | 5 | Scoring tests |
| `tests/test_git_analyzer.py` | 6 | Git analyzer tests |

---

*The irony of building a tool that finds hidden connections while manually tracing connections through your own codebase isn't lost on anyone. But that's exactly why this wave matters — Loom should be the one doing this work, not us.* 🧵
