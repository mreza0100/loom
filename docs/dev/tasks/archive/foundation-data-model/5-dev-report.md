# Dev Report — foundation-data-model

## Implementation Summary

Three phases implemented in strict build order across 8 source files.

### Phase 1 — ID-Based Edge Model

**`src/loom/store/models.py`**
- Added `ParsedEdge` NamedTuple as the parser-stage intermediate type `(source_name, target_name, relationship, target_file)`.
- Replaced `Edge` string-based fields with `source_id: int` (FK, always set), `target_id: int | None` (FK, set after Phase 2), `target_name: str` (kept for diagnostics/fallback).

**`src/loom/store/db.py`**
- Schema: `DROP TABLE IF EXISTS edges` + `CREATE TABLE edges` with `source_id REFERENCES symbols(id) ON DELETE CASCADE` and `target_id REFERENCES symbols(id) ON DELETE SET NULL`.
- `PRAGMA foreign_keys = ON` applied after `executescript()` (which implicitly commits and may reset connection state).
- New methods: `insert_edge`, `get_edges_from`, `get_edges_to`, `get_edges_to_by_name`, `get_unresolved_edges`, `update_edge_target`, `remove_edges_for_source`.
- `remove_file()`: nullifies incoming edges before deleting symbols (FK cascade handles outgoing).

### Phase 3 — Full Call Expressions

**`src/loom/indexer/parser.py`**
- Removed `callee.split(".")[-1]` — now stores full dotted expressions as `target_name` (e.g., `"db.query"` instead of `"query"`).
- Returns `tuple[list[Symbol], list[ParsedEdge]]`.
- Filtering moved to engine layer: `edge.target_name.split(".")[-1] in _GENERIC_CALL_TARGETS`.

### Phase 2 — Two-Phase Indexing

**`src/loom/indexer/pipeline.py`**
- `_parse_all_files()`: Phase 1 — for each file, inserts symbols, generates embeddings, then converts `ParsedEdge` → `Edge`. Import edges use `file_anchor_id` as `source_id` and store `target_name=local_binding_name` for import map key construction.
- `_resolve_all_edges()`: Phase 2 — queries all `target_id IS NULL` edges, runs 5-strategy resolution, bulk-updates with `(target_id, confidence)`.
- `_build_import_map()`: builds `{(source_file, local_name): target_file}` from import edges in DB.
- `_resolve_single_edge()`: 5 strategies in descending confidence: exact file+name (1.0), import-resolved (0.95), file suffix (0.9), qualified name (0.8), unique global (0.6).

**`src/loom/search/engine.py`**
- `_find_coupled()`: ID-based edge traversal, skips unresolved outgoing edges (`target_id is None`).
- `impact()`: combines resolved callers via `get_edges_to(id)` and unresolved callers via `get_edges_to_by_name(name)`.
- All generic-call filtering uses last segment: `name.split(".")[-1] in _GENERIC_CALL_TARGETS`.

### Key Design Decision: Import Edge Anchoring

The trickiest architectural problem was import edges. Local bindings like `importedFunc` (from `import { importedFunc } from './util.js'`) are not declared symbols — they have no `source_id`. Solution: use the first symbol in the importing file as `source_id` (file anchor), and store `target_name=local_binding_name`. This lets `_build_import_map` correctly construct `{(file, "importedFunc"): "util.js"}`, which Phase 2 uses to resolve `importedFunc()` call edges.

## Bug Fix — BUG-001: Aliased Import Resolution

**Fixed in:** `src/loom/store/models.py`, `src/loom/store/db.py`, `src/loom/indexer/pipeline.py`

Root cause: for `import { getProduct as fetchProduct } from './product.js'`, the pipeline stored
`target_name="fetchProduct"` (local alias) in the import edge but discarded the original exported
name `"getProduct"`. Strategy 2 then looked up `"fetchProduct"` in `product.js` and found nothing.

**Fix:**
1. Added `original_name: str | None` field to `Edge` dataclass in `models.py`.
2. Added `original_name TEXT` column to the `edges` table schema in `db.py`.
3. Updated all SELECT queries in `db.py` to include `original_name` (8th column) and updated
   `_row_to_edge` to read it.
4. In `_parse_all_files()` (pipeline.py): when storing import edges, set
   `original_name=exported_name` if it differs from `local_name`; `None` for non-aliased imports.
5. Changed `_build_import_map()` return type from `dict[tuple[str,str], str]` to
   `dict[tuple[str,str], tuple[str, str|None]]` — values are now `(target_file, original_name)`.
6. Updated `_resolve_single_edge()` Strategy 2: after failing to find the local alias name in
   the resolved file, falls back to looking up `original_name` if present. This lets
   `fetchProduct(1)` resolve to `getProduct` at confidence 0.95.

**Tests added/updated:**
- `test_aliased_import_resolution` (renamed from `test_aliased_import_resolution_fails`):
  assertion flipped from `== 0` (bug present) to `>= 1` (bug fixed); verifies confidence 0.95
  and correct `target_id` pointing to `getProduct`.
- `test_import_edge_target_name_is_local_binding`: updated to verify import map value is now a
  tuple and `original_name == "getProduct"`.
- `test_import_edge_original_name_stored_in_db` (new): verifies DB column stores exported name.
- `test_non_aliased_import_original_name_is_null` (new): verifies non-aliased imports have
  `original_name IS NULL`.

## Test Coverage

- **272 tests**, all passing
- **Coverage: 92.82%** (threshold: 85%)
- Test files: `test_db.py`, `test_engine.py`, `test_parser.py`, `test_pipeline.py`, `test_server.py`, `test_embedder.py`, `test_watcher.py`, `test_qa_foundation_data_model.py`
- All external deps mocked (embedding model, file I/O via tmp dirs); real SQLite used for integration tests

| File | Coverage |
|------|----------|
| `store/models.py` | 100% |
| `store/db.py` | 96% |
| `indexer/parser.py` | 96% |
| `server.py` | 94% |
| `search/engine.py` | 93% |
| `indexer/watcher.py` | 92% |
| `indexer/pipeline.py` | 88% |
| `indexer/embedder.py` | 79% |

## Runbook

```bash
# All tests
uv run pytest

# Lint
uv run ruff check src/ tests/

# Format
uv run ruff format src/ tests/

# Type check
uv run mypy src/
```

All four gates pass clean. No errors, no warnings (except the `asyncio_mode` config which is inert).
