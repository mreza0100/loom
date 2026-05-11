# Bug Report — foundation-data-model

**Pipeline:** foundation-data-model
**QA Mode:** PRE-MERGE (re-check after fix)
**Test files:**
- `tests/test_qa_foundation_data_model.py` (original)
- `tests/test_qa_bug001_recheck.py` (re-check adversarial tests)
**Suite result:** 286 passed, 0 failed
**Coverage:** 93.58% (threshold 85%)

---

## BUG-001: Aliased Import Resolution Fails

**Severity:** HIGH
**Status:** FIXED and VERIFIED

**Location:** `src/loom/store/models.py`, `src/loom/store/db.py`, `src/loom/indexer/pipeline.py`

**Reproduction:**
```javascript
// src/order.js
import { getProduct as fetchProduct } from './product.js';
function processOrder() { fetchProduct(1); }

// src/product.js
export function getProduct(id) { return id; }
```

After `full_index()`, the call edge `processOrder → fetchProduct` was **never resolved** (target_id stayed NULL, confidence 0.0). Expected: resolved to `getProduct` at confidence 0.95.

**Fix applied:**
1. `original_name: str | None` field added to `Edge` dataclass in `models.py`.
2. `original_name TEXT` column added to the `edges` table schema in `db.py`.
3. All SELECT queries in `db.py` updated to include `original_name` (8th column); `_row_to_edge` reads it.
4. `_parse_all_files()` (pipeline.py): import edges now set `original_name=exported_name` when it differs from `local_name`; `None` for non-aliased imports.
5. `_build_import_map()` return type changed from `dict[tuple[str,str], str]` to `dict[tuple[str,str], tuple[str, str|None]]` — values are `(target_file, original_name)`.
6. `_resolve_single_edge()` Strategy 2: after failing to find the local alias name in the resolved file, falls back to looking up `original_name` when present.

**Verification (re-check):** `test_aliased_import_resolution` in `test_qa_foundation_data_model.py` — assertion flipped from `== 0` (bug present) to `>= 1` (bug fixed). PASSES.

---

## Re-check: Adversarial Edge Cases — All Pass

Added `tests/test_qa_bug001_recheck.py` with 14 new tests targeting fix edge cases:

| Test class | Scenario verified | Result |
|---|---|---|
| `TestMultipleAliasedImports` | Two aliased imports from same module both resolve | PASS |
| `TestMultipleAliasedImports` | Mixed aliased + non-aliased imports from same module | PASS |
| `TestAliasNameCollision` | Alias name accidentally matches a different symbol in target file | PASS |
| `TestImportMapKeyCollision` | Two files use same local alias name — map key includes file prefix, no collision | PASS |
| `TestDottedCallOnAliasedImport` | `alias.method()` where alias is an aliased import | PASS |
| `TestReindexPreservesAliasedResolution` | `remove_file` + re-index cycle preserves aliased resolution | PASS |
| `TestImportMapReturnType` | All import map values are 2-tuples `(target_file, original_name)` | PASS |
| `TestImportMapReturnType` | Aliased import map value has non-None `original_name` | PASS |
| `TestImportMapReturnType` | Non-aliased import map value has `original_name=None` | PASS |
| `TestRowToEdgeOriginalName` | `original_name` column round-trips via `get_unresolved_edges()` | PASS |
| `TestRowToEdgeOriginalName` | Non-aliased import edge has `original_name=None` after DB read | PASS |
| `TestStrategy2NonAliasedFallback` | Non-aliased Strategy 2 does not crash (no `original_name`) | PASS |
| `TestStrategy2NonAliasedFallback` | Manually injected import edge with `original_name=None` handled | PASS |
| `TestBug001RegressionFull` | Exact BUG-001 scenario end-to-end: confidence 0.95, correct target_id | PASS |

No remaining edge cases found. The fix is correct and complete.

---

## No Other Bugs Found

All behaviors verified correct (original tests, unchanged):

- FK constraints enforced on insert (IntegrityError for bad source_id/target_id) ✓
- ON DELETE CASCADE removes outgoing edges when source symbol deleted ✓
- ON DELETE SET NULL nullifies target_id when target symbol deleted ✓
- `remove_file()` nullifies multiple incoming edges before CASCADE ✓
- `get_unresolved_edges()` only returns IS NULL, not resolved edges ✓
- `update_edge_target()` on non-existent edge_id is a no-op (no crash) ✓
- `PRAGMA foreign_keys = ON` is set after `executescript()` ✓
- Schema DROP TABLE IF EXISTS edges on reconnect wipes edges, keeps symbols ✓
- `ParsedEdge` is immutable NamedTuple ✓
- `Edge` requires `source_id`, defaults `target_id=None`, `confidence=0.0` ✓
- Full dotted call expressions stored verbatim (Phase 3) ✓
- `console.*` filtered, `logger.*` not filtered ✓
- `this.method()` stored as `"this.method"` ✓
- `callee.split(".")[-1]` removal confirmed: `db.query()` → `"db.query"` not `"query"` ✓
- Self-call guard `foo()` inside `foo()` = 0 edges ✓
- `Foo.init()` inside `Foo()` NOT filtered (dotted self-reference is not recursion) ✓
- Strategy 4b resolves `compile()` to `Compiler.compile` at 0.8 confidence ✓
- Strategy 5 fails when name is ambiguous (2+ symbols) ✓
- Strategy 5 confidence = 0.6 ✓
- Strategy 2 confidence = 0.95 (non-aliased import) ✓
- Strategy 1 confidence = 1.0 (exact file+name match) ✓
- File with only imports and no symbols: no crash ✓
- Empty JS file: indexed with 0 symbols, no crash ✓
- `_resolve_all_edges()` on empty DB returns 0 ✓
- Second `_resolve_all_edges()` call is a no-op (0 resolved) ✓
- Circular imports (A→B→A) do not crash or loop ✓
- `impact()` with all-unresolved outgoing edges: no crash, returns [] ✓
- `impact()` filters unresolved callers where source is a generic name ✓
- `related()` with all unresolved outgoing edges: no crash ✓
- Empty `target_name` edge: stored and retrievable ✓
- Confidence 0.0 and 1.0 boundary values round-trip correctly ✓

---

## Compliance

- **BUG-RAW-PRINT:** None — no `print()` in `src/` ✓
- **BUG-MOCK-VIOLATION:** None — only `Embedder` (external model) is mocked; LoomDB, SearchEngine, IndexPipeline, parse_file all use real implementations ✓
- **BUG-COVERAGE:** Not triggered — 93.58% > 85% threshold ✓ (improved from 92.82%)
- **ruff check src/:** All checks passed ✓
- **mypy src/:** No issues in 14 source files ✓
