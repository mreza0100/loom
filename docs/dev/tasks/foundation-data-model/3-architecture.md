> Author: architect

# Architecture — foundation-data-model

## Overview

Three sequential phases that together replace the name-string edge model with an integer-FK edge model, preserve full dotted call expressions in the parser, and split the single-phase indexer into a parse-all / resolve-all two-phase pipeline. Each phase builds on the last — strict build order.

Build order: Phase 1 (ID-Based Edges) → Phase 3 (Full Call Expressions) → Phase 2 (Two-Phase Indexing).

---

## File Structure Changes

No new files or modules. All changes are in-place edits to existing files:

```
src/loom/store/models.py         — Edge dataclass replace
src/loom/store/db.py             — Schema, 7 new/updated methods
src/loom/indexer/parser.py       — Remove callee.split(".")[-1], one line
src/loom/indexer/pipeline.py     — Split _index_files, add resolution logic
src/loom/search/engine.py        — ID-based edge queries, filter fix
tests/conftest.py                — sample_edge fixture, populated_db edges
tests/test_db.py                 — TestEdgeCRUD rewrite + 5 new tests
tests/test_engine.py             — TestBuiltinFiltering updates + 2 new tests
tests/test_parser.py             — 1 test rename+update + 5 new tests
tests/test_pipeline.py           — TestResolveEdgeTargets removed, TestTwoPhaseIndexing added
```

---

## Phase 1: ID-Based Edge Model

### `src/loom/store/models.py`

Replace the `Edge` dataclass entirely. The new shape:

- `source_id: int` — FK to `symbols.id`. Always resolved at insertion time (source symbol is always in the same file as the edge producer).
- `target_name: str` — preserved always. Diagnostic field and fallback for unresolved edges.
- `relationship: str` — unchanged.
- `confidence: float = 0.0` — resolution confidence; 0.0 means unresolved or parser-stage default.
- `target_id: int | None = None` — FK to `symbols.id`. None until Phase 2 resolves it.
- `target_file: str | None = None` — resolution hint carried from parser; not a FK, not authoritative after resolution.
- `id: int | None = None` — DB row id; needed by Phase 2's `update_edge_target(edge.id, ...)` call.

The parser's intermediate edge objects still carry `source_name` and `source_file` (string fields) until the pipeline converts them. This is resolved via the `ParsedEdge` intermediate type described in the Pipeline section below.

### Critical decision — parser intermediate type

The parser currently constructs `Edge` objects with `source_name`/`source_file`. After Phase 1 removes those fields from `Edge`, the parser can no longer produce `Edge` directly. Two options:

**Option A:** Keep `source_name: str | None` and `source_file: str | None` as optional fields on `Edge` for parser-stage use, cleared after pipeline conversion.

**Option B:** Introduce a `ParsedEdge` NamedTuple in `parser.py` with `(source_name, target_name, target_file, relationship)`. Pipeline converts to `Edge` by looking up `source_id`.

**Decision: Option B.** `ParsedEdge` as a `NamedTuple` in `src/loom/store/models.py` (or `src/loom/indexer/parser.py`). Keeps `Edge` clean — no optional fields polluting the final model. `parse_file()` returns `list[ParsedEdge]` instead of `list[Edge]`. Pipeline converts to `Edge` after symbol insertion. This is a localized type boundary; only `parser.py` and `pipeline.py` touch `ParsedEdge`.

Place `ParsedEdge` in `src/loom/store/models.py` alongside `Edge` so both modules can import from one place.

### `src/loom/store/db.py`

**Schema change — `edges` table:**

```sql
CREATE TABLE IF NOT EXISTS edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    target_id INTEGER REFERENCES symbols(id) ON DELETE SET NULL,
    target_name TEXT NOT NULL,
    target_file TEXT,
    relationship TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 0.0
);
```

Foreign key constraints require `PRAGMA foreign_keys = ON` at connection time. This pragma is currently missing from `connect()` and must be added before `executescript(SCHEMA)`.

**Schema migration strategy:** Drop-and-recreate. Change `CREATE TABLE IF NOT EXISTS edges` to `DROP TABLE IF EXISTS edges` followed by `CREATE TABLE edges` in `SCHEMA`. This runs atomically on every `connect()`. Since `.loom.db` is gitignored and Loom is pre-1.0, wiping the edges table on schema change is acceptable. Symbols table is untouched; only the edges table is dropped.

**Indexes:**
- `idx_edges_source ON edges(source_id)` — outgoing traversal
- `idx_edges_target ON edges(target_id)` — incoming traversal
- `idx_edges_target_name ON edges(target_name)` — unresolved fallback in `impact()`
- `idx_edges_unresolved ON edges(target_id) WHERE target_id IS NULL` — Phase 2 resolution batch

**Updated method signatures:**

`insert_edge(edge: Edge) -> int` — inserts `source_id`, `target_id`, `target_name`, `target_file`, `relationship`, `confidence`. Returns `lastrowid` (the edge's DB id). Callers that previously passed name-based `Edge` objects must pass ID-based ones.

`get_edges_from(symbol_id: int) -> list[Edge]` — replaces the `(name, file)` signature entirely. Queries `WHERE source_id = ?`. No file parameter.

`get_edges_to(symbol_id: int) -> list[Edge]` — replaces the `(name, file)` signature. Queries `WHERE target_id = ?`. No file parameter.

**New methods:**

`get_edges_to_by_name(target_name: str) -> list[Edge]` — queries `WHERE target_name = ?`. Used by `impact()` to find unresolved callers whose `target_id IS NULL` but `target_name` matches the queried symbol. Returns all rows, resolved and unresolved.

`get_unresolved_edges() -> list[Edge]` — queries `WHERE target_id IS NULL`. Returns all edges pending Phase 2 resolution.

`update_edge_target(edge_id: int, target_id: int, confidence: float) -> None` — `UPDATE edges SET target_id = ?, confidence = ? WHERE id = ?`. Used by Phase 2 resolution loop.

`remove_edges_for_source(symbol_id: int) -> None` — `DELETE FROM edges WHERE source_id = ?`. Used when re-indexing a file (after `remove_file()` CASCADE would have handled it, but this method is explicit for clarity in the pipeline).

`_row_to_edge(row: tuple) -> Edge` — static helper that constructs `Edge` from a DB row. Row column order: `(id, source_id, target_id, target_name, target_file, relationship, confidence)`. All existing query methods use this helper.

**Updated `remove_file(path)`:**

Current behavior: `DELETE FROM edges WHERE source_file = ? OR target_file = ?` — destroys incoming edges.

New behavior:
1. Before deleting symbols, nullify edges that point TO this file's symbols: `UPDATE edges SET target_id = NULL, confidence = 0.0 WHERE target_id IN (SELECT id FROM symbols WHERE file = ?)`. This converts resolved edges into unresolved ones rather than deleting them — the caller still exists, we just don't know who they call anymore.
2. Delete the symbols: `DELETE FROM symbols WHERE file = ?`. The `ON DELETE CASCADE` on `source_id` automatically deletes all edges FROM this file's symbols.
3. Remove from `index_meta`.

The FK CASCADE handles source-side cleanup. The explicit UPDATE handles target-side cleanup. This is correct because a caller in file B pointing to a symbol in file A should not disappear when file A is deleted — it should become unresolved.

**`get_symbol_by_id(symbol_id: int) -> Symbol | None`** already exists, no change.

### `src/loom/indexer/pipeline.py`

In `_index_files()`, after inserting all symbols for a file and collecting their IDs, build a local name-to-id map:

```
local_name_to_id: dict[str, int] = {sym.name: sym_id for sym, sym_id in zip(symbols, symbol_ids)}
```

For each `ParsedEdge` from the parser, convert to `Edge`:
- `source_id = local_name_to_id[parsed_edge.source_name]` — guaranteed present, same file
- `target_id = None` — unresolved until Phase 2
- Copy `target_name`, `target_file`, `relationship` from `ParsedEdge`
- `confidence = 0.0`

Remove `_resolve_edge_targets()` call entirely. Remove the function itself. It becomes dead code after Phase 2.

### `src/loom/search/engine.py`

`_find_coupled(target: Symbol)`:
- Outgoing: `get_edges_from(target.id)` (requires `target.id is not None`). For each edge, if `edge.target_id` is not None, resolve via `get_symbol_by_id(edge.target_id)`. If `edge.target_id is None`, skip — unresolved edges are not traversed in coupled results.
- Filter: `edge.target_name.split(".")[-1] in _GENERIC_CALL_TARGETS` (pre-empts Phase 3 full expressions).
- Incoming: `get_edges_to(target.id)`. For each edge, resolve source via `get_symbol_by_id(edge.source_id)`. Source is always resolved (source_id is always set).
- Filter on source: resolve `get_symbol_by_id(edge.source_id)` and check its `name.split(".")[-1] in _GENERIC_CALL_TARGETS`. This replaces the old `edge.source_name` check which no longer exists on the model.

`impact(target: Symbol)`:
- Resolved incoming: `get_edges_to(target.id)` — same as above.
- Unresolved incoming: `get_edges_to_by_name(target.name)` — returns all edges (resolved or not) where `target_name = target.name`. Filter out any that are already resolved to a different symbol (where `edge.target_id is not None and edge.target_id != target.id`). The remainder are edges that nominally target this symbol by name but haven't been resolved via Phase 2 yet.
- Source filter on unresolved: fetch source symbol via `get_symbol_by_id(edge.source_id)` and check its name against `_GENERIC_CALL_TARGETS`.

`get_symbol_by_name_fuzzy()` usage contract: only for MCP tool input parsing (the initial symbol lookup in `related()`, `impact()`, `neighborhood()`). Never for internal edge traversal after Phase 1.

### `tests/conftest.py`

**`sample_edge` fixture:** Remove as a standalone fixture. The new `Edge` model requires `source_id` which is only available after symbol insertion. `TestEdgeCRUD` will insert its own symbols inline.

**`populated_db` fixture:** Capture symbol IDs from `insert_symbol()` returns. Build the four edges using those IDs:

```
ids[0] = processOrder id
ids[1] = validateCart id
ids[2] = Cart id
ids[3] = Cart.addItem id
ids[4] = getProduct (product.js) id
ids[5] = getProduct (inventory.js) id
ids[6] = MAX_ITEMS id
```

Edges become:
- `Edge(source_id=ids[0], target_name="validateCart", target_id=ids[1], relationship="calls", confidence=1.0)`
- `Edge(source_id=ids[0], target_name="Cart", target_id=ids[2], relationship="instantiates", confidence=1.0)`
- `Edge(source_id=ids[0], target_name="getProduct", target_id=ids[4], relationship="calls", confidence=1.0)`
- `Edge(source_id=ids[2], target_name="EventEmitter", target_id=None, relationship="extends", confidence=0.0)`

### `tests/test_db.py` — TestEdgeCRUD

All existing tests must be rewritten. Each test inserts symbols first via `db.insert_symbol()`, captures IDs, constructs `Edge` with `source_id`.

Tests that previously called `get_edges_from("processOrder")` or `get_edges_to("validateCart")` with name strings now call `get_edges_from(source_symbol_id)` and `get_edges_to(target_symbol_id)`.

The two tests `test_get_edges_from_with_file` and `test_get_edges_to_with_file` — the `file` parameter no longer exists on these methods. These tests must be rewritten without the file-filtering variant.

`test_null_target_file_edges_excluded_with_file` in `TestBuiltinFiltering` — this tests `get_edges_to("create", file="factory.js")`. After Phase 1, `get_edges_to` takes `symbol_id`, not `(name, file)`. This test must be migrated to use `get_edges_to_by_name("create")` and assert the returned edge count (3 edges), not filtered by file.

**Five new tests in `TestEdgeCRUD`:**

1. `test_get_unresolved_edges` — insert edge with `target_id=None`, call `get_unresolved_edges()`, assert it appears in results.
2. `test_update_edge_target` — insert unresolved edge, call `update_edge_target(edge.id, target_id, 0.95)`, retrieve via `get_edges_from`, assert `target_id` and `confidence` updated.
3. `test_get_edges_to_by_name` — insert edge with `target_name="fooBar"`, call `get_edges_to_by_name("fooBar")`, assert returned.
4. `test_edge_confidence_roundtrip` — insert edge with `confidence=0.85`, retrieve via `get_edges_from(source_id)`, assert `edge.confidence == pytest.approx(0.85)`.
5. `test_remove_file_nullifies_target_edges` — insert symbol in file A as target, insert edge from file B with `target_id=A_symbol_id`, call `remove_file("a.js")`, assert edge's `target_id` is now None (retrieve via `get_unresolved_edges()`) and the source edge from A's symbol is deleted.

**`TestRemoveFile.test_remove_file_cleans_everything`:** Full rewrite. Insert two symbols (one per file), create edge from symbol-in-A pointing to symbol-in-B (to test nullification). Create edge from symbol-in-A pointing somewhere (to test CASCADE delete). Remove file A. Assert: symbol-in-A gone, source-edge gone, target-edge to A's symbol has `target_id=NULL`.

### `tests/test_engine.py` — TestBuiltinFiltering

`test_generic_targets_filtered_from_coupled`: Insert symbols, capture IDs, build `Edge(source_id=myFunc_id, target_name="push", target_id=None, relationship="calls")`. Filter checks `"push".split(".")[-1] in _GENERIC_CALL_TARGETS` — passes.

`test_generic_sources_filtered_from_impact`: This test inserts an edge with `source_name="callback"` — a generic name. After Phase 1, there's no `source_name` on `Edge`. The test must insert a real symbol named "callback", get its ID, create `Edge(source_id=callback_id, target_name="targetFunc", target_id=targetFunc_id, ...)`. The `impact()` filter fetches the source symbol by id and checks its name.

`test_null_target_file_edges_excluded_with_file`: This directly calls `db.get_edges_to("create", file="factory.js")` — the file parameter no longer exists. The test behavior it covered (filtering by file) is now handled differently: either move this to a doctest note or rewrite as `get_edges_to_by_name("create")` asserting 3 results total.

**Two new test functions:**
- `test_impact_includes_unresolved_name_matches` — insert symbol "targetSym", insert edge with `target_id=None, target_name="targetSym"`, call `engine.impact("targetSym")`, assert the unresolved caller appears.
- `test_related_excludes_unresolved` — insert symbol "targetSym", insert edge with `target_id=None, target_name="targetSym"`, call `engine.related("targetSym")`, assert the unresolved edge does NOT yield a coupled result (only resolved edges are followed in `_find_coupled`).

---

## Phase 3: Preserve Full Call Expressions

### `src/loom/indexer/parser.py`

In `_extract_calls`, around line 243:

**Remove:** `clean = callee.split(".")[-1]`

**Change:** The `edges.append()` (now `ParsedEdge` construction after Phase 1) uses `target_name=callee` directly.

The `console.` filter (`not callee.startswith("console.")`) already operates on the raw `callee` before the removed line. It remains correct and unchanged.

The `new_expression` path captures bare `identifier` nodes from the AST — these cannot contain dots by definition. No change needed there.

### `src/loom/search/engine.py`

The `_GENERIC_CALL_TARGETS` filter change (`split(".")[-1]`) is already made in Phase 1 for both `_find_coupled` and `impact`. No additional engine changes required for Phase 3.

### `tests/test_parser.py`

`test_method_call_extracts_last_part` — rename to `test_method_call_preserves_full_expression`. Change assertion from `calls[0].target_name == "query"` to `calls[0].target_name == "db.query"`.

**Five new tests in `TestCallEdges`:**

1. `test_full_call_expression_stored` — input `this.hooks.make.callAsync()` inside a function, assert `target_name == "this.hooks.make.callAsync"`.
2. `test_simple_call_unchanged` — input `compile()` inside a function (no dot), assert `target_name == "compile"`.
3. `test_method_call_on_import` — input `fs.readFileSync()`, assert `target_name == "fs.readFileSync"`.
4. `test_console_still_filtered` — input `console.log("x")`, assert zero call edges produced.
5. `test_callee_recursion_guard` — `function foo() { foo(); }` produces zero edges (self-call guard). Note: `function Foo() { Foo.init(); }` where `callee="Foo.init"` != `caller_name="Foo"` WILL produce an edge — this is correct and expected behavior. Do not assert zero for that case.

---

## Phase 2: Two-Phase Indexing

### `src/loom/indexer/pipeline.py`

**Split `_index_files(files)` into two methods:**

`_parse_all_files(files: list[Path]) -> dict[str, int]`:
- For each file (same hash check, remove_file, parse, insert symbols, insert embeddings as before).
- Build `local_name_to_id` from inserted symbol IDs.
- Convert `ParsedEdge` list to `Edge` list (source_id resolved, target_id=None).
- Insert raw edges.
- Set file hash.
- Single commit after all files.
- Returns `{indexed, symbols, edges}` counts.

`_resolve_all_edges() -> int`:
- Calls `_build_import_map()` → `dict[tuple[str, str], str]`.
- Gets all unresolved edges via `db.get_unresolved_edges()`.
- For each edge, calls `_resolve_single_edge(edge, import_map)` → `tuple[int, float] | None`.
- Calls `db.update_edge_target(edge.id, target_id, confidence)` for resolved results.
- Commits once after all updates.
- Returns count of resolved edges.

`full_index()` becomes:
1. `result = self._parse_all_files(all_files)`
2. `resolved = self._resolve_all_edges()`
3. Return merged dict.

`incremental_index()` becomes:
1. Handle deletions (remove_file, CASCADE/nullify).
2. `result = self._parse_all_files(changed_files)`
3. `resolved = self._resolve_all_edges()` — re-runs on ALL unresolved edges, not just new ones. This is correct: newly indexed symbols may resolve edges from older files.
4. Return merged dict.

**`_build_import_map() -> dict[tuple[str, str], str]`:**

Queries the edges table for all `relationship = 'imports'` edges where `target_file IS NOT NULL`:
```sql
SELECT e.target_name, s.file, e.target_file
FROM edges e
JOIN symbols s ON s.id = e.source_id
WHERE e.relationship = 'imports' AND e.target_file IS NOT NULL
```

Returns `{(source_file, local_name): resolved_target_file}`. Note: import edges store `target_name=original_name` and the symbol whose ID is `source_id` is the importing symbol in `source_file`. The `target_file` on import edges is already a resolved path (from Phase 1's conversion of `./relative` paths via `_resolve_import_path`).

Wait — Phase 1 removes `_resolve_edge_targets` which was previously resolving import paths. Phase 2's `_parse_all_files` stores raw import edges with `target_file` = whatever the parser emitted (e.g., `"./utils/validate.js"`). The import path normalization (`_resolve_import_path`) must be applied during `_parse_all_files` when building the edge — specifically for `relationship == "imports"` edges, normalize `target_file` from the relative `./` path to an absolute-relative path using `_resolve_import_path(parsed_edge.target_file, rel_path)`. This keeps `_resolve_import_path` in use; only `_resolve_edge_targets` is removed.

**`_resolve_single_edge(edge: Edge, import_map: dict[tuple[str, str], str]) -> tuple[int, float] | None`:**

Needs access to DB methods. Should be a method on `IndexPipeline`, not a module-level function.

Tries strategies in order, returns on first match:

1. **Exact file match** (confidence 1.0): if `edge.target_file` is set, call `db.get_symbol_by_name(edge.target_name, edge.target_file)`. If exactly one result, return `(result.id, 1.0)`.

2. **Import-resolved** (confidence 0.95): for full dotted expressions (`"obj.method"`), the base is the first segment (`"obj"`). For simple names, the base is the name itself. Look up `(source_file, base)` in `import_map` to get `target_file`. Then look up `db.get_symbol_by_name(target_name, target_file)`. The `source_file` for an edge is obtained by fetching the source symbol: `db.get_symbol_by_id(edge.source_id).file`.
   - For dotted expressions: look up base segment in import map → get target_file → look up `method_name` (last segment) in that file.
   - Cache the source symbol lookup to avoid N+1 queries per edge.

3. **File suffix match** (confidence 0.9): if `edge.target_file` is a relative path that doesn't resolve cleanly, find symbols where `symbols.file LIKE '%' || edge.target_file`. Use `db.get_symbol_by_name(edge.target_name)` and filter results by `sym.file.endswith(normalized_suffix)`.

4. **Qualified name match** (confidence 0.8): query `db.get_symbol_by_name_fuzzy(edge.target_name)` using the LIKE `%.{name}` fallback. If exactly one result, use it. If multiple results, no resolution (ambiguous).

5. **Unique name match** (confidence 0.6): `db.get_symbol_by_name(edge.target_name)` (exact, global). If exactly one result codebase-wide, use it.

For full dotted expressions from Phase 3:
- `this.method` → try last segment (`method`) through strategies 4 and 5 only. `this` is not meaningful as an import alias.
- `ClassName.method` (uppercase first char of first segment) → treat as qualified name, try `db.get_symbol_by_name("ClassName.method")` directly (confidence 1.0 if found).
- `import_alias.method` → strategy 2 handles this via `import_map[(source_file, "import_alias")]` → look for `method` in target file.
- Fallback for any dotted expression: try last segment through strategies 4 and 5.

**Incremental re-resolution:** `_resolve_all_edges()` fetches ALL unresolved edges each time, not just edges from changed files. This is correct because:
- A newly indexed file B may define symbol `b()` that was previously unresolved in older file A's edge.
- Running the full unresolved set is safe: already-resolved edges are not in `get_unresolved_edges()` output.

**`_resolve_import_path` function:** Retained. Used in `_parse_all_files` to normalize import edge `target_file` values.

**`_resolve_edge_targets` function:** Removed entirely.

### `src/loom/store/db.py`

`get_colocated_symbols(file)` already exists — no change.

`_build_import_map` queries the connection directly via `self._db.conn`. No new DB method needed.

An optional helper `get_symbols_in_file(file: str) -> list[Symbol]` — this is already `get_colocated_symbols`. Use it.

### `tests/test_pipeline.py`

**Remove `TestResolveEdgeTargets`** class entirely. Remove `_resolve_edge_targets` from the import line at the top of the file.

`_resolve_import_path` import and `TestResolveImportPath` are retained — that function still exists.

**Existing `TestIndexPipeline`:** The `test_full_index` result assertions `result["symbols"] == 2` and `result["edges"] >= 1` remain valid. No edge-content assertions exist in this class, so no changes needed there beyond removing the dead import.

**Add `TestTwoPhaseIndexing`** class with 8 tests. These are integration tests that write real JS files to `tmp_dir` and run `pipeline.full_index()` or `pipeline.incremental_index()`:

1. `test_two_phase_basic` — File A: `function a() { b(); }`. File B: `function b() {}`. After `full_index()`, find edge from `a` to `b`, assert `target_id == b_symbol_id` and `confidence > 0`.

2. `test_two_phase_import_resolution` — File A: `import { b } from './b.js'; function a() { b(); }`. File B: `function b() {}`. After indexing, edge from `a` to `b` resolved via import map at confidence 0.95.

3. `test_two_phase_qualified_name` — File A: `function compile() {}` inside `class Compiler`. File B: `function run() { compile(); }`. After indexing, edge from `run` to `Compiler.compile` at confidence 0.8.

4. `test_two_phase_unique_name` — `_makePathsRelative()` called in A, defined uniquely in B. Confidence 0.6.

5. `test_two_phase_ambiguous_name` — `create()` defined in B and C, no import chain in A. Edge stays unresolved (`target_id=None`).

6. `test_two_phase_confidence_ordering` — Three scenarios; assert strategy-1-resolved confidence > strategy-2-resolved confidence > strategy-5-resolved confidence.

7. `test_incremental_re_resolution` — Index file A (calls `b()`, unresolved). Then add file B defining `b`. Run `incremental_index([B])`. Assert edge now resolved.

8. `test_incremental_delete_nullifies` — Index A and B with resolved edge A→B. Delete B. Run `incremental_index([B])`. Assert edge `target_id=None`.

---

## Data Flow

### Phase 1 (after changes)

```
parse_file(path) -> (list[Symbol], list[ParsedEdge])
    |
    v
pipeline._parse_all_files():
    for each file:
        insert symbols -> symbol_ids
        build local_name_to_id map
        for each ParsedEdge:
            if relationship == "imports": normalize target_file via _resolve_import_path
            convert to Edge(source_id=local_name_to_id[source_name], target_id=None, ...)
            insert_edge(edge) -> edge_id
        set_file_hash()
    commit()
```

### Phase 2 (resolution)

```
_resolve_all_edges():
    _build_import_map() -> {(source_file, local_name): target_file}
    get_unresolved_edges() -> list[Edge]
    for each edge:
        _resolve_single_edge(edge, import_map) -> (target_id, confidence) | None
        if resolved:
            update_edge_target(edge.id, target_id, confidence)
    commit()
```

### Engine queries (after Phase 1)

```
related(symbol_name):
    get_symbol_by_name_fuzzy(symbol_name) -> target Symbol
    _find_coupled(target):
        get_edges_from(target.id) -> outgoing resolved edges
        for each edge where target_id is not None:
            get_symbol_by_id(edge.target_id)
        get_edges_to(target.id) -> incoming edges
        for each edge:
            get_symbol_by_id(edge.source_id)

impact(symbol_name):
    get_symbol_by_name_fuzzy(symbol_name) -> target Symbol
    get_edges_to(target.id) -> resolved incoming edges
    get_edges_to_by_name(target.name) -> resolved+unresolved by name
    for unresolved: get_symbol_by_id(edge.source_id) to check filter
```

---

## Trade-off Decisions

### ParsedEdge intermediate type vs optional fields on Edge

Chose `ParsedEdge` NamedTuple over making `source_name`/`source_file` optional on `Edge`. The cost is a new type. The benefit is `Edge` stays clean — it always represents a stored DB row, never a pre-storage intermediate. The type boundary is explicit and narrow: only `parser.py` produces `ParsedEdge`, only `pipeline.py` converts them.

### Drop-and-recreate vs versioned schema migration

Chose `DROP TABLE IF EXISTS edges` before `CREATE TABLE edges` in `SCHEMA`. The alternative (a `_migrate_schema()` with ALTER TABLE statements) would be needed for production but Loom is pre-1.0 with gitignored per-project DBs. Drop-recreate runs on every `connect()`, meaning the developer never has to manually wipe `.loom.db`. The downside: every connect call drops and recreates the table. This is acceptable because `connect()` is called once at startup.

Alternative: add a `schema_version` table and only drop when version mismatches. This is cleaner for long-term but premature at pre-1.0. Recommend revisiting when Loom approaches 1.0.

### `_resolve_all_edges()` runs on ALL unresolved edges, not just new ones

Correct behavior for incremental indexing: newly indexed symbols can resolve previously unresolved edges from older files. Running the full unresolved set is O(unresolved_edges * strategies) which is acceptable. The partial index on `target_id IS NULL` makes the `get_unresolved_edges()` query fast.

Alternative: track which files changed and only re-resolve edges FROM those files. More complex bookkeeping, marginal perf gain. Not worth it at this scale.

### `get_edges_to_by_name` for unresolved impact traversal

`impact()` must catch both resolved callers (via `get_edges_to(symbol_id)`) and unresolved callers that nominally target this symbol by name (via `get_edges_to_by_name(symbol.name)`). The hybrid approach is necessary: if Phase 2 hasn't resolved an edge yet (or can't resolve it), `impact()` should still surface the caller. The tradeoff: `get_edges_to_by_name` may return false positives (edges targeting a different symbol with the same name). Filter: skip any edge where `target_id IS NOT NULL AND target_id != symbol.id`.

### `PRAGMA foreign_keys = ON`

Must be added to `connect()`. SQLite foreign key enforcement is opt-in per connection. Without it, `ON DELETE CASCADE` and `ON DELETE SET NULL` are silently ignored. The `executescript()` call resets pragma state, so the pragma must be re-applied as a separate `execute()` call after `executescript()`.

---

## Research Notes

No new libraries introduced in this pipeline. All capabilities are native to the existing stack.

### SQLite partial indexes

Supported since SQLite 3.8.9 (2014). The Python `sqlite3` module ships with SQLite 3.35+ on modern systems. `CREATE INDEX ... WHERE target_id IS NULL` is standard syntax. No compatibility concern.

### `ON DELETE CASCADE` / `ON DELETE SET NULL`

Standard SQLite behavior, but requires `PRAGMA foreign_keys = ON` per connection. This pragma is not persistent across connections in SQLite — it must be set on every new connection. The current `connect()` method does not set it. Adding `self.conn.execute("PRAGMA foreign_keys = ON")` after `sqlite_vec.load()` and before `executescript(SCHEMA)` is the correct fix.

### `executescript()` and pragma interaction

`sqlite3.Connection.executescript()` issues an implicit `COMMIT` before executing the script. It does not reset pragmas. However, to be safe, set the pragma as a separate statement after `executescript()` completes, not inside the script string. The current SCHEMA string does not include pragma statements, which is correct.
