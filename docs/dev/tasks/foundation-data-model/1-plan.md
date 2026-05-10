> Author: planner

# Plan — foundation-data-model

## Feature Context

Migrate Loom's edge model from name-based string matching to integer foreign keys, preserve full dotted call expressions in the parser, and implement two-phase indexing with global cross-file edge resolution — all together as the foundation for accurate structural coupling.

## Current State

### Key files and current shape

**`src/loom/store/models.py`**
`Edge` dataclass has five string fields: `source_name`, `source_file`, `target_name`, `target_file | None`, `relationship`. No `id`, no `confidence`. `Symbol` already has `id: int | None = None`.

**`src/loom/store/db.py`**
`SCHEMA` creates `edges` with `source_name TEXT NOT NULL`, `source_file TEXT NOT NULL`, `target_name TEXT NOT NULL`, `target_file TEXT`, `relationship TEXT NOT NULL` — pure name-based, no foreign keys, no confidence column.

`get_edges_from(name, file)` and `get_edges_to(name, file)` both query by name string. `insert_edge()` takes an `Edge` and inserts five string columns. `remove_file()` deletes all edges where `source_file = path OR target_file = path` — destroys incoming edges instead of nullifying them.

`get_symbol_by_id()` already exists. `get_colocated_symbols()` already exists.

Missing methods that the spec requires: `get_edges_from(symbol_id)` (ID variant), `get_edges_to(symbol_id)` (ID variant), `get_edges_to_by_name(target_name)`, `get_unresolved_edges()`, `update_edge_target(edge_id, target_id, confidence)`, `remove_edges_for_source(symbol_id)`, `_row_to_edge(row)`.

**`src/loom/indexer/parser.py`**
`_extract_calls()` at line 243: `clean = callee.split(".")[-1]` strips dotted method chains before storing as `target_name`. The `console.` filter is a prefix check on the raw `callee`, which is correct. The `new_expression` path already captures the class name verbatim. Import handling is separate and unaffected.

**`src/loom/indexer/pipeline.py`**
Single-phase `_index_files()` loop: for each file it parses, inserts symbols, inserts embeddings, calls `_resolve_edge_targets(edges, rel_path, symbols)`, then inserts edges and commits. `_resolve_edge_targets()` is a per-file function that builds a local import map and resolves target_file where possible — it has no visibility into other files' symbols. This is the cross-file resolution gap.

**`src/loom/search/engine.py`**
`_find_coupled()` calls `get_edges_from(target.name, target.file)` and `get_edges_to(target.name, target.file)`, then resolves each edge's target by calling `get_symbol_by_name(edge.target_name, edge.target_file)`. `impact()` calls `get_edges_to(target.name, target.file)` for incoming edges. Both methods will need to switch to ID-based lookups. The `_GENERIC_CALL_TARGETS` filter currently checks `edge.target_name in _GENERIC_CALL_TARGETS` — with full call expressions stored, this check must use the last dotted segment.

### Existing tests

`tests/conftest.py` — `sample_edge` fixture uses name-based `Edge`. `populated_db` builds four name-based edges directly.

`tests/test_db.py` — `TestEdgeCRUD` tests query by name. `TestRemoveFile` asserts that `get_edges_from("doStuff", file="remove_me.js") == []` — this will break under the new schema where the method signature changes. `test_remove_file_cleans_everything` also inserts name-based edges.

`tests/test_engine.py` — `TestBuiltinFiltering.test_generic_targets_filtered_from_coupled` inserts edges by name (`"push"`, `"map"`, `"realHelper"`). All engine tests using `populated_db` will be affected by the schema change.

`tests/test_parser.py` — `test_method_call_extracts_last_part` asserts `calls[0].target_name == "query"` for `db.query(...)` — this test will FAIL under Phase 3 and must be updated to assert `"db.query"`.

`tests/test_pipeline.py` — Imports and calls `_resolve_edge_targets` directly in `TestResolveEdgeTargets`. This function is removed in Phase 2. Those tests must be replaced with two-phase integration tests. Also imports `_resolve_edge_targets` at module level, so removal will break the import.

## Gaps and Needed Changes

### Phase 1: ID-Based Edge Model

**`src/loom/store/models.py`**
Replace `Edge`:
```python
@dataclass
class Edge:
    source_id: int
    target_name: str           # always preserved for diagnostics / unresolved fallback
    relationship: str
    confidence: float = 0.0
    target_id: int | None = None
    target_file: str | None = None   # hint for resolution
    id: int | None = None            # DB row id
```
The parser still produces edges without IDs — `source_id` is populated by the pipeline after symbol insertion, `target_id` is NULL until Phase 2 resolves it.

**`src/loom/store/db.py`**

Schema changes:
- Drop name-based columns from `edges`. New schema:
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
- New indexes: `idx_edges_source(source_id)`, `idx_edges_target(target_id)`, `idx_edges_target_name(target_name)`, partial `idx_edges_unresolved ON edges(target_id) WHERE target_id IS NULL`.

New and updated methods:
- `insert_edge(edge: Edge) -> int` — inserts by source_id/target_id, returns row id
- `get_edges_from(symbol_id: int) -> list[Edge]` — replaces name-based variant
- `get_edges_to(symbol_id: int) -> list[Edge]` — replaces name-based variant
- `get_edges_to_by_name(target_name: str) -> list[Edge]` — for unresolved impact traversal
- `get_unresolved_edges() -> list[Edge]` — WHERE target_id IS NULL
- `update_edge_target(edge_id: int, target_id: int, confidence: float) -> None`
- `remove_edges_for_source(symbol_id: int) -> None` — used when re-indexing a file
- `_row_to_edge(row: tuple) -> Edge` — static helper
- `remove_file()` — change from `DELETE FROM edges WHERE ... target_file = path` to `UPDATE edges SET target_id = NULL, confidence = 0.0 WHERE target_id IN (SELECT id FROM symbols WHERE file = ?)` before deleting symbols, then delete source edges via CASCADE.

Note: the two existing `get_edges_from(name, file)` and `get_edges_to(name, file)` signatures must be completely replaced — there are no callers outside of `engine.py` and tests.

**`src/loom/indexer/pipeline.py`**
In `_index_files()`, after inserting symbols and collecting `symbol_ids`, build a local name-to-id map: `local_symbol_map: dict[str, int]`. For each raw edge from the parser:
- Resolve `source_id` from `local_symbol_map[edge.source_name]` (guaranteed present — source is always a symbol in the same file)
- Set `target_id = None` for now
- Insert edge with source_id resolved, target_id=NULL

Remove `_resolve_edge_targets` function entirely (it becomes dead code after Phase 2 replaces its logic). The call at line 94 is removed.

**`src/loom/search/engine.py`**
- `_find_coupled()`: replace `get_edges_from(target.name, target.file)` with `get_edges_from(target.id)`. For each outgoing edge, resolve via `get_symbol_by_id(edge.target_id)` if `edge.target_id` is not None, skip if unresolved. Replace `get_edges_to(target.name, target.file)` with `get_edges_to(target.id)`. Caller lookup becomes `get_symbol_by_id(edge.source_id)`.
- `impact()`: use `get_edges_to(target.id)` for resolved incoming edges, plus `get_edges_to_by_name(target.name)` for unresolved callers (those where `target_id IS NULL` but `target_name` matches). Caller lookup from resolved edge uses `get_symbol_by_id(edge.source_id)`.
- `_GENERIC_CALL_TARGETS` filter: change `edge.target_name in _GENERIC_CALL_TARGETS` to `edge.target_name.split(".")[-1] in _GENERIC_CALL_TARGETS` in both `_find_coupled` and `impact`. This is a Phase 1 change to pre-empt Phase 3's full expressions landing.

**`tests/conftest.py`**
`sample_edge` fixture: must insert two symbols first, then create `Edge(source_id=..., target_name=..., relationship=...)`. Since fixtures run in isolation, the simplest approach is to remove `sample_edge` as a standalone fixture and instead have `TestEdgeCRUD` create edges after inserting the needed symbols inline.

`populated_db` fixture: must insert symbols first (it already does), capture their IDs from `insert_symbol()` return values, then build edges using those IDs. The four existing edges become:
- `Edge(source_id=ids[0], target_name="validateCart", target_id=ids[1], relationship="calls", confidence=1.0)`
- `Edge(source_id=ids[0], target_name="Cart", target_id=ids[2], relationship="instantiates", confidence=1.0)`
- `Edge(source_id=ids[0], target_name="getProduct", target_id=ids[4], relationship="calls", confidence=1.0)`
- `Edge(source_id=ids[2], target_name="EventEmitter", target_id=None, relationship="extends", confidence=0.0)`

**`tests/test_db.py`** — `TestEdgeCRUD`
All tests must be rewritten to insert symbols first, then create `Edge` with `source_id`. Add five new tests:
- `test_get_unresolved_edges` — insert edge with target_id=None, assert returned by `get_unresolved_edges()`
- `test_update_edge_target` — insert unresolved edge, call `update_edge_target`, assert target_id and confidence updated
- `test_get_edges_to_by_name` — insert edge with target_name="foo", assert `get_edges_to_by_name("foo")` returns it
- `test_edge_confidence_roundtrip` — insert with confidence=0.85, retrieve via `get_edges_from`, assert confidence preserved
- `test_remove_file_nullifies_target_edges` — insert file A's symbol as target, insert edge from file B pointing to it, remove file A, assert edge's target_id is now NULL and source edge (from A) is deleted

`TestRemoveFile.test_remove_file_cleans_everything` needs full rewrite: insert two symbols (one per file), create edges with IDs, verify cascade deletes source edges and nullifies target edges.

**`tests/test_engine.py`**
`TestBuiltinFiltering.test_generic_targets_filtered_from_coupled`: insert symbols, get their IDs, create `Edge(source_id=myFunc_id, target_name="push", ...)` with no target_id. The filter will check `"push".split(".")[-1] in _GENERIC_CALL_TARGETS` — still works.

Add two new test classes:
- `test_impact_includes_unresolved_name_matches` — create edge with target_id=NULL, target_name matching the queried symbol name; assert impact() finds the unresolved caller
- `test_related_excludes_unresolved` — create edge with target_id=NULL; assert _find_coupled() skips it (only follows resolved edges)

### Phase 3: Full Call Expressions

**`src/loom/indexer/parser.py`**
Line 243: remove `clean = callee.split(".")[-1]`. Change the `edges.append()` call to use `target_name=callee` directly (the raw dotted expression).

The `console.` prefix filter on line 242 already operates on `callee` (the raw string) before the split, so it remains correct.

The `new_expression` path at line 253 captures a bare `identifier` node — no dotted expression possible, unchanged.

**`src/loom/search/engine.py`**
The `_GENERIC_CALL_TARGETS` filter change (check last segment) is already handled in Phase 1. No additional changes needed here for Phase 3.

**`tests/test_parser.py`**
`test_method_call_extracts_last_part` currently asserts `calls[0].target_name == "query"` for `db.query(...)`. This must be updated to assert `calls[0].target_name == "db.query"`. The test name should be updated to `test_method_call_preserves_full_expression`.

Add five new tests in `TestCallEdges`:
- `test_full_call_expression_stored` — `this.hooks.make.callAsync()` → target_name == `"this.hooks.make.callAsync"`
- `test_simple_call_unchanged` — `compile()` → target_name == `"compile"` (no dot, unchanged)
- `test_method_call_on_import` — `fs.readFileSync()` → target_name == `"fs.readFileSync"`
- `test_console_still_filtered` — `console.log()` → no call edge produced
- `test_callee_recursion_guard` — `function foo() { foo() }` → no self-edge (already tested in `test_no_self_call_edge` but worth confirming with a dotted variant: `function Foo() { Foo.init() }` where source_name is `"Foo"` and callee is `"Foo.init"` — since `callee != caller_name` this WILL produce an edge, which is correct behavior)

### Phase 2: Two-Phase Indexing

**`src/loom/indexer/pipeline.py`**

Split `_index_files()` into two methods:

`_parse_all_files(files)`:
- For each file (skip if hash unchanged):
  - `remove_file(rel_path)` → CASCADE deletes source edges, nullifies stale target references
  - parse file → symbols, raw_edges
  - insert symbols → get IDs
  - insert embeddings
  - build `local_name_to_id: dict[str, int]`
  - for each raw edge: set `source_id = local_name_to_id[edge.source_name]`, `target_id = None`, insert raw edge
  - set file hash
- Commit once after all files

`_resolve_all_edges()`:
1. `_build_import_map() -> dict[tuple[str, str], str]` — queries all `imports` edges from DB, maps `(source_file, local_name) → resolved_target_file`
2. `get_unresolved_edges()` — all edges with `target_id IS NULL`
3. For each unresolved edge, call `_resolve_single_edge(edge, import_map) -> tuple[int, float] | None`
4. Call `update_edge_target(edge.id, target_id, confidence)` for each resolved result

`_resolve_single_edge(edge, import_map)` — tries strategies in order, returns `(target_id, confidence)` or None:

1. **Exact file match** (confidence 1.0): if `edge.target_file` is set and a symbol with `edge.target_name` exists in that file
2. **Import-resolved** (confidence 0.95): look up `(source_file, base_of_target_name)` in import_map → get target file → look up symbol there. For dotted `obj.method`, `base_of_target_name = obj` (first segment).
3. **File suffix match** (confidence 0.9): if `edge.target_file` is a relative path without resolution, find symbols whose file ends with the normalized path
4. **Qualified name match** (confidence 0.8): look up `*.{target_name}` in symbols — if exactly one result, use it. E.g., `compile` → `Compiler.compile`.
5. **Unique name match** (confidence 0.6): `get_symbol_by_name(target_name)` — if exactly one result globally, use it.

For full dotted expressions (Phase 3 input):
- `this.method` → treat as method lookup on the enclosing class (needs `enclosing_class` context from edge metadata, or fall through to unique name match on last segment)
- `Class.method` (uppercase first char) → exact symbol lookup for `Class.method` (confidence 1.0 if found)
- `import_alias.method` → look in import_map for `(source_file, import_alias)` → target file → look for method there (confidence 0.85)
- Fallback: try last segment as simple name through strategies 4 and 5

**Incremental re-resolution** in `incremental_index()`: after `_parse_all_files([changed_files])`, call `_resolve_all_edges()` to pick up edges that may now resolve against newly-indexed symbols. Deleted files handled by `remove_file()` CASCADE/SET NULL.

**`src/loom/store/db.py`**
`get_colocated_symbols(file)` already exists — no change needed. `_build_import_map` in the pipeline will query the edges table directly via `conn` for import-relationship edges with resolved target files.

**`tests/test_pipeline.py`**
`TestResolveEdgeTargets` class and the `_resolve_edge_targets` import are removed entirely — the function no longer exists.

Add new test class `TestTwoPhaseIndexing`:
- `test_two_phase_basic` — two files A and B. A has `function a() { b(); }`, B has `function b() {}`. After `full_index()`, verify edge from a to b has `target_id` = b's symbol id with confidence > 0.
- `test_two_phase_import_resolution` — A imports b from B, A calls b(). After indexing, edge resolves via import map.
- `test_two_phase_qualified_name` — file has `compile()` call, another file has `class Compiler { compile() {} }`. After Phase 2, edge resolves to `Compiler.compile` at confidence 0.8.
- `test_two_phase_unique_name` — `_makePathsRelative()` called, defined uniquely. Confidence 0.6.
- `test_two_phase_ambiguous_name` — `create()` defined in multiple files, no import chain. Edge stays unresolved (target_id=NULL).
- `test_two_phase_confidence_ordering` — assert strategy 1 > strategy 2 > strategy 5 confidence values.
- `test_incremental_re_resolution` — index file A (b() unresolved), then index file B (defines b). After second index, edge resolves.
- `test_incremental_delete_nullifies` — index A and B with resolved edge. Delete B. Re-index. Assert edge target_id becomes NULL.

The existing `TestIndexPipeline` tests (`test_full_index`, `test_skip_unchanged_files`, etc.) need updates to reflect that edges now store `source_id` not `source_name`. The `result["edges"]` count assertions remain valid. Tests that check edge content (there are none in the current `TestIndexPipeline`) are unaffected.

## Risks and Dependencies

**Build order is strict:** Phase 1 changes the `Edge` dataclass and schema. Phase 3 changes what string gets stored as `target_name`. Phase 2 builds on both — it needs integer edge IDs (Phase 1) and full dotted expressions (Phase 3) to run its resolution strategies correctly. Do not reorder.

**Schema has no migration path:** The task spec explicitly states: drop old table, full re-index. Since the SCHEMA uses `CREATE TABLE IF NOT EXISTS`, the developer must drop the `edges` table manually or wipe `.loom.db` before running. Add a `_migrate_schema()` method or a `DROP TABLE IF EXISTS edges` before `CREATE TABLE IF NOT EXISTS edges` in `SCHEMA` to make this automatic. The developer should decide which approach — explicit DROP or a schema version check.

**`source_name` on raw parser edges:** The parser returns `Edge` objects with `source_name` (the parser's current field). After Phase 1 changes the dataclass, the parser must be updated too — it no longer constructs `Edge` with `source_name`/`source_file` but with a placeholder. The pipeline then populates `source_id`. One clean approach: keep the parser producing an intermediate `ParsedEdge` namedtuple with `(source_name, target_name, target_file, relationship)` and have the pipeline convert to `Edge` by looking up source_id. Alternatively, keep `source_name` as an optional field on `Edge` for parser-stage use only. The developer should decide — the spec implies the pipeline does the conversion.

**`_extract_calls` self-edge guard:** Currently checks `callee != caller_name`. With full expressions, `caller_name = "processOrder"` and `callee = "processOrder.init"` — the guard would NOT fire even though it's arguably a self-reference. The spec says keep the guard as-is; the task description's recursion guard test (`function foo() { foo() }`) still works because `callee == "foo" == caller_name`.

**Coverage floor:** `pyproject.toml` enforces 85% coverage. Removing `_resolve_edge_targets` (currently tested in `TestResolveEdgeTargets`) and adding new code means the new two-phase tests must cover the replacement code paths adequately.

**Engine `get_edges_from` / `get_edges_to` signature change:** Any callers that pass `(name, file)` string arguments must be updated. The only callers are in `engine.py` and tests — confirmed above.

**`test_generic_sources_filtered_from_impact`** in `test_engine.py`: currently inserts an edge with `source_name="callback"` — a name in `_GENERIC_CALL_TARGETS`. After Phase 1, this edge has a `source_id` pointing to a real symbol. The filter in `impact()` must filter on the source symbol's name, or on the edge's `source_name`... but the new `Edge` model doesn't carry `source_name` anymore. The developer must decide: either keep `source_name` as a diagnostic field on `Edge` (like `target_name` is kept for diagnostics), or look up the source symbol's name via `get_symbol_by_id(edge.source_id).name` to check against `_GENERIC_CALL_TARGETS`. The spec says `get_symbol_by_name_fuzzy` should only be used for MCP input parsing — the diagnostic lookup here is `get_symbol_by_id`, which is fine. Recommend: fetch source symbol by id and check its name against the filter set.

## Research Needed

No new libraries required. All needed capabilities are present:
- `sqlite3` partial indexes — supported in SQLite 3.8.9+, available in the environment
- `ON DELETE CASCADE` and `ON DELETE SET NULL` — standard SQLite foreign key behavior; requires `PRAGMA foreign_keys = ON` at connection time (currently not set — this must be added to `connect()`)
- NetworkX not yet used in this pipeline (planned for later phases)
