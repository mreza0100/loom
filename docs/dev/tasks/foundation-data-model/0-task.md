# Pipeline: foundation-data-model

**Wave:** foundation-rebuild
**Phases:** 1 (ID-Based Edges) + 3 (Full Call Expressions) + 2 (Two-Phase Indexing)
**Build order within pipeline:** Phase 1 first, then Phase 3, then Phase 2

---

## Phase 1: ID-Based Edge Model

**Priority:** CRITICAL — everything else depends on this

### What to change

**`src/loom/store/models.py`** — Replace `Edge` dataclass:
- `source_name: str` + `source_file: str` → `source_id: int` (FK to symbols.id, always resolved)
- `target_name: str` + `target_file: str | None` → `target_id: int | None` (FK to symbols.id, None = unresolved) + `target_name: str` (diagnostic) + `target_file: str | None` (hint)
- Add `confidence: float` (0.0-1.0, feeds Phase 5 coupling scores)
- Add `id: int | None = None` (DB row ID, for Phase 2 UPDATE by ID)

**`src/loom/store/db.py`** — New schema + methods:
- Schema: `edges` table with `source_id INTEGER NOT NULL`, `target_id INTEGER`, `target_name TEXT NOT NULL`, `target_file TEXT`, `relationship TEXT NOT NULL`, `confidence REAL NOT NULL DEFAULT 0.0`
- Foreign keys: `source_id REFERENCES symbols(id) ON DELETE CASCADE`, `target_id REFERENCES symbols(id) ON DELETE SET NULL`
- Indexes: `idx_edges_source(source_id)`, `idx_edges_target(target_id)`, `idx_edges_target_name(target_name)`, partial index `idx_edges_unresolved ON edges(target_id) WHERE target_id IS NULL`
- New methods: `get_edges_from(symbol_id)`, `get_edges_to(symbol_id)`, `get_edges_to_by_name(target_name)`, `get_unresolved_edges()`, `update_edge_target(edge_id, target_id, confidence)`, `remove_edges_for_source(symbol_id)`, `_row_to_edge(row)`
- Updated `remove_file()`: edges FROM deleted file are deleted; edges TO deleted file's symbols become unresolved (SET target_id=NULL, confidence=0.0) — not deleted
- Updated `insert_edge()`: takes new Edge format, returns edge ID
- Add `get_symbol_by_id(symbol_id)` if not already present

**`src/loom/indexer/pipeline.py`** — Update edge insertion:
- `_index_files`: When inserting edges, resolve `source_id` from local symbol ID map. Set `target_id=None` for now (Phase 2 resolves).
- Remove old `_resolve_edge_targets` function (replaced by Phase 2's global resolution)

**`src/loom/search/engine.py`** — Update edge queries:
- `_find_coupled`: Use `get_edges_from(symbol_id)` instead of `get_edges_from(name, file)`. Resolve targets via `get_symbol_by_id(edge.target_id)` instead of `get_symbol_by_name(edge.target_name, edge.target_file)`.
- `impact()`: Use `get_edges_to(symbol_id)` for resolved callers + `get_edges_to_by_name(target.name)` for unresolved callers. This hybrid approach solves the impact/related divergence.
- `get_symbol_by_name_fuzzy()` should ONLY be used for MCP tool input parsing (first symbol lookup), never for internal edge traversal.

**`tests/conftest.py`** — Update `populated_db` fixture edges to use symbol IDs.

**`tests/test_db.py`** — Update `TestEdgeCRUD`:
- Change all edge tests to use symbol IDs
- Add: `test_get_unresolved_edges`, `test_update_edge_target`, `test_get_edges_to_by_name`, `test_edge_confidence_roundtrip`, `test_remove_file_nullifies_target_edges`

**`tests/test_engine.py`** — Update edge construction to use IDs. Add: `test_impact_includes_unresolved_name_matches`, `test_related_excludes_unresolved`.

### Migration strategy
No schema migration. Drop old table, full re-index. Loom is pre-1.0. `.loom.db` is per-project and gitignored.

### Done when
- Edge dataclass uses source_id/target_id
- Schema uses integer foreign keys with CASCADE/SET NULL
- All edge queries work by ID
- get_edges_to_by_name() exists for impact()
- get_unresolved_edges() + update_edge_target() exist for Phase 2
- remove_file() nullifies target edges instead of deleting them
- All existing tests updated and passing
- New tests for ID-based edge operations

---

## Phase 3: Preserve Full Call Expressions

**Priority:** HIGH — required for accurate structural coupling
**Do this AFTER Phase 1, BEFORE Phase 2**

### What to change

**`src/loom/indexer/parser.py`** — In `_extract_calls` (around line 243):
- REMOVE: `clean = callee.split(".")[-1]`
- Store the full `callee` string as `target_name`
- Keep the `console.` filter

**`src/loom/search/engine.py`** — Update builtin filter:
- Instead of `if edge.target_name in _GENERIC_CALL_TARGETS`, check `if edge.target_name.split(".")[-1] in _GENERIC_CALL_TARGETS`
- Exception: don't filter `this.hooks.*` patterns (preserve hook info)

### Tests
- `test_full_call_expression_stored` — `this.hooks.make.callAsync()` → target_name == "this.hooks.make.callAsync"
- `test_simple_call_unchanged` — `compile()` → target_name == "compile"
- `test_method_call_on_import` — `fs.readFileSync()` → target_name == "fs.readFileSync"
- `test_console_still_filtered` — `console.log()` → no edge
- `test_callee_recursion_guard` — `function foo() { foo() }` → no self-edge

### Done when
- Full call expression stored in target_name
- `callee.split(".")[-1]` removed
- Builtin filter checks last segment of dotted expressions
- All parser tests updated and passing

---

## Phase 2: Two-Phase Indexing

**Priority:** CRITICAL — required for cross-file edge resolution
**Do this AFTER Phase 1 and Phase 3**

### What to change

**`src/loom/indexer/pipeline.py`** — Split `_index_files` into two phases:

**Phase 1 (parse all):** For each file: parse → store symbols (get IDs) → store embeddings → store RAW edges (source_id resolved, target_id=NULL) → store file hash. Commit after all files.

**Phase 2 (resolve all):** Call `_resolve_all_edges()` which:
1. Builds global import map from ALL import edges: `{(source_file, local_name): resolved_target_file}`
2. Gets all unresolved edges via `get_unresolved_edges()`
3. For each unresolved edge, runs `_resolve_single_edge(edge, import_map)` which tries 5 strategies in order:
   - Strategy 1: Exact name + exact file (confidence 1.0)
   - Strategy 2: Import-resolved (confidence 0.95)
   - Strategy 3: File suffix match (confidence 0.9)
   - Strategy 4: Qualified name match — `compile` → `Compiler.compile` (confidence 0.8 if unique)
   - Strategy 5: Unique name match — only one symbol with this name in codebase (confidence 0.6)
4. Updates resolved edges via `update_edge_target(edge_id, target_id, confidence)`

**Full expressions (from Phase 3) require additional resolution strategies:**
- `this.method` → resolve as `EnclosingClass.method` (confidence 0.9)
- `Class.method` (uppercase first) → exact symbol lookup (confidence 1.0)
- `import.method` → look in import target file (confidence 0.85)
- Fallback: try last segment as simple name

**Incremental re-resolution:** When files change, re-resolve unresolved edges that might now target symbols in changed files.

**`src/loom/store/db.py`** — May need `get_colocated_symbols(file)` if not present.

### Tests
- `test_two_phase_basic` — 2 files, A calls B. After Phase 2, edge resolved.
- `test_two_phase_import_resolution` — Import chain resolves cross-file call.
- `test_two_phase_qualified_name` — `compile()` → `Compiler.compile` at confidence 0.8.
- `test_two_phase_unique_name` — `_makePathsRelative()` → unique match at confidence 0.6.
- `test_two_phase_ambiguous_name` — `create()` with multiple matches → import-chain disambiguated or unresolved.
- `test_two_phase_confidence_ordering` — Exact > import > unique.
- `test_incremental_re_resolution` — New symbol in changed file resolves previously-unresolved edge.
- `test_incremental_delete_nullifies` — Deleted file's target edges become unresolved.

### Done when
- `_index_files` runs in two phases: parse-all → resolve-all
- `_resolve_single_edge` implements 5+ strategy resolution with confidence levels
- `_build_import_map` constructs global import map
- Incremental re-resolution works
- Resolution confidence stored per edge
- All tests pass
