# Loom Dogfood Report — 2026-05-10

**Mode:** Adversarial (360-driven)
**Baseline:** 18 files, 107 symbols, 378 edges, 107 vectors
**After:** 21 files, 135 symbols, 450 edges, 135 vectors

---

## Previous Report Regression Check

| Bug ID | Description | Previous | Current | Verdict |
|--------|-------------|----------|---------|---------|
| BUG-001 | `impact("authenticate")` returned `registerRoutes` 10x | BROKEN | Returns once | **FIXED** |
| BUG-002 | Search scores flat at 0.028-0.033 | BROKEN | Scores now 0.5-1.0 range for real queries | **IMPROVED** (but see BUG-N05) |
| BUG-003 | `neighborhood()` didn't indicate anchor symbol | BROKEN | Returns `anchor` field with full symbol info | **FIXED** |
| BUG-004 | Noisy variable-level search results | BROKEN | `kind` filter now exists; `search("cart", kind="function")` eliminates noise | **PARTIALLY FIXED** |

### Previous "What's Missing" Addressed

| Request | Status |
|---------|--------|
| Re-index trigger | **DONE** — `reindex()` tool exists |
| Filtering/faceting | **PARTIALLY DONE** — `kind` filter on search/related/impact |
| Last-indexed timestamp | **DONE** — `status()` returns `last_indexed` |
| Stale file count | **DONE** — `status()` returns `stale_files` |
| Cross-function data flow | NOT ADDRESSED |
| "What imports this module?" | NOT ADDRESSED |
| Inverse neighborhood | NOT ADDRESSED |
| Export/import chain visualization | NOT ADDRESSED |

---

## Preflight Results

| Test | Input | Expected | Actual | Verdict |
|------|-------|----------|--------|---------|
| Empty query | `search("")` | Graceful empty/error | `fts5: syntax error near ""` | **FAIL** |
| Pure miss | `search("xyzzy_nonexistent_symbol_42")` | Empty or low-score results | 10 results, scores 0.86-1.0, all irrelevant | **FAIL** |
| Common name | `related("constructor")` | Results for constructor methods | Empty `[]` | **DEGRADE** |
| Ubiquitous symbol | `impact("log")` | Manageable list of dependents | 9 `log` variables, all "semantically similar" to themselves | **DEGRADE** |
| Bad path | `neighborhood("nonexistent/file.js", 1)` | Error message | `{anchor: null, coupled: []}` — silent empty | **DEGRADE** |
| Idempotent reindex | `reindex()` x2 | Same counts | Both returned 0, counts unchanged | **PASS** |
| Hyphen in query | `search("not-really-js")` | Results or graceful empty | `no such column: really` — FTS5 crash | **FAIL** |

---

## Task 1 — Ambiguity Attack: Duplicate `getProductById`

**Goal:** Create `src/services/inventory.js` with a `getProductById` wrapping the model version. Test disambiguation.

### Discovery (Loom-only)

| # | Tool | Args | Result | Grade | Notes |
|---|------|------|--------|-------|-------|
| 1 | search | `"getProductById"` | Both versions found, product.js rank 1, inventory.js rank 2 | A | Good discovery |
| 2 | related | `"getProductById", file="inventory.js"` | Shows `imports` edge to product.js version | A | Correct structural link |
| 3 | related | `"getProductById", file="inventory.js"` | Lists `createOrder`, `addToCart`, etc. as `called_by` | **F** | FALSE POSITIVE — these call product.js version, not inventory.js |
| 4 | impact | `"getProductById", file="inventory.js"` | Returns 7 callers including `createOrder`, `addToCart` | **F** | Same name-collision bug — phantom callers |
| 5 | related | `"getProductById", file="product.js"` | Now shows `inventory.js/getProductById` as `called_by` | B | Correct edge, but also shows phantom edges from other callers to inventory version |

### Findings

**CRITICAL BUG (BUG-N01):** Loom treats same-named functions as structurally equivalent for caller/callee resolution. When module A calls `getProductById` (from `product.js`), Loom records edges to ALL `getProductById` symbols regardless of which file they're in. This means:
- `inventory.js/getProductById` falsely claims to be called by `createOrder`, `addToCart`, `updateStock`, `deleteProduct`, `registerRoutes`
- Every function that shows `calls getProductById` now lists BOTH versions
- The `file` parameter correctly selects the START symbol but doesn't filter the edge resolution

**Reproduction:** Create any function with the same name as an existing one in a different module. Query `related()` for the new one — it inherits all callers of the old one.

---

## Task 2 — Cross-cutting: Add `currency` parameter to `validatePrice`

**Goal:** Modify `validatePrice(price)` → `validatePrice(price, currency='USD')`. Test `impact()` accuracy.

### Discovery

| # | Tool | Args | Result | Grade | Notes |
|---|------|------|--------|-------|-------|
| 1 | impact | `"validatePrice"` | `createProduct`, `updateProduct` — exactly 2 callers | A | Perfect |
| 2 | neighborhood | `"validator.js", 9` | Anchored on `validatePrice`, showed co-located validators + 2 callers | A | Useful context |
| 3 | search | `"CURRENCY_DECIMALS"` | Found at rank 2, score 0.61 | B | Indexed correctly |
| 4 | related | `"CURRENCY_DECIMALS"` | Empty `[]` | D | No structural edge to `validatePrice` despite being used inside it |
| 5 | search | `"CURRENCY_DECIMALS"` | Also surfaced `getCartTotal`, `applyCoupon`, `getOrderTotalsByUser` semantically | A | Good semantic grouping of price/money functions |

### Findings

`impact()` was perfectly accurate — the two callers it identified are exactly right. Watcher picked up the edit within 3 seconds. Semantic search for the new constant surfaced conceptually related price/money functions across the codebase — genuinely useful.

Minor gap: `CURRENCY_DECIMALS` has no structural relationship to `validatePrice` even though it's consumed inside it. Loom tracks inter-function calls but not intra-function variable references.

---

## Task 3 — Edge Case: Class hierarchy with inheritance

**Goal:** Create `PricingStrategy` base class with `PercentageDiscount`, `FixedDiscount`, `TieredDiscount` subclasses.

### Discovery

| # | Tool | Args | Result | Grade | Notes |
|---|------|------|--------|-------|-------|
| 1 | search | `"PricingStrategy"` | All 4 classes, all methods, factory function found | A | Good discovery |
| 2 | related | `"PricingStrategy"` | Empty `[]` | **F** | Base class has ZERO relationships. No subclass edges |
| 3 | related | `"PercentageDiscount"` | `FixedDiscount` (0.603 semantic), `TieredDiscount` (0.386 semantic) | C | Semantic similarity helps, but no structural `extends` edge |
| 4 | impact | `"PricingStrategy"` | Empty `[]` | **F** | Can't determine blast radius of base class change |
| 5 | related | `"createStrategy"` | Empty `[]` | D | Factory creating instances via `new` not tracked |

### Findings

**HIGH BUG (BUG-N02):** Loom does not extract `extends` relationships from class declarations. The CLAUDE.md architecture doc explicitly lists "Inheritance — A extends/implements B" as a signal, but it's not implemented. Consequences:
- `related("PricingStrategy")` returns nothing — a developer changing the base class gets zero guidance
- `impact("PricingStrategy")` returns nothing — blast radius analysis is blind to inheritance
- `createStrategy()` factory function shows no edges despite `new PercentageDiscount()` etc.

Semantic similarity partially fills the gap: `PercentageDiscount` → `FixedDiscount` at 0.603. But the base class is invisible because it has a generic name that doesn't embed well against its subclasses.

---

## Task 4 — Chaos Monkey: File lifecycle + malformed files

### File lifecycle (create → delete → recreate)

| Step | Action | Expected | Actual | Verdict |
|------|--------|----------|--------|---------|
| 1 | Create `temp-feature.js` with `tempFunction`, `anotherTemp` | Indexed within 5s | Indexed in <3s, +2 symbols | **PASS** |
| 2 | Delete `temp-feature.js` | Symbols removed | Symbols removed in <3s, counts correct | **PASS** |
| 3 | Recreate with `completelyDifferentFunction`, `processItems` | New symbols, no ghosts | New symbols indexed, `tempFunction` fully gone | **PASS** |

### Malformed .js file (JSON content)

| Test | Expected | Actual | Verdict |
|------|----------|--------|---------|
| Create `not-really-js.js` with JSON | No crash, 0 symbols | File counted, 0 symbols extracted | **PASS** |

### Circular imports (A→B→C→A)

| Test | Expected | Actual | Verdict |
|------|----------|--------|---------|
| `related("getA")` | Cycle doesn't crash | Correct: calls `getB`, called_by `getC`, `processB` | **PASS** |
| `related("processB")` | Full cycle visible | Correct: calls `processC`, `getA`, called_by `processA` | **PASS** |
| `impact("getC")` | Shows dependents through cycle | `processA`, `getB` (structural), `getA` (semantic) | **PASS** |

### Input sanitization

| Test | Expected | Actual | Verdict |
|------|----------|--------|---------|
| `search("not-really-js")` | Graceful results | `no such column: really` — FTS5 treats `-` as NOT | **FAIL** |

---

## Task 5 — Completeness Probe: Wishlist feature + Loom-vs-Grep audit

**Goal:** Add `src/services/wishlist.js` with 6 exported functions, wire into `api.js` routes. Compare Loom graph vs manual grep.

### Discovery

| # | Tool | Args | Result | Grade | Notes |
|---|------|------|--------|-------|-------|
| 1 | search | `"wishlist user favorites"` (pre-implementation) | `getNotifications`, `getOrdersByUser`, `getCart` — user-scoped collection patterns | A | Impressive semantic discovery of analogous patterns |
| 2 | related | `"addToWishlist"` | calls: `getProductById`, `getWishlist`; called_by: `registerRoutes`; semantic: `removeFromWishlist` | B | Correct except phantom edge to `inventory.js/getProductById` (BUG-N01) |
| 3 | related | `"getWishlist"` | calls: `getUserById`; called_by: 4 wishlist functions + `registerRoutes` | A | Complete and accurate |
| 4 | related | `"moveToCart"` | calls: `removeFromWishlist`; called_by: `registerRoutes` | B | Correct but misses `addToCartFn` callback parameter call |
| 5 | related | `"notifyWishlistPriceDrop"` | calls: `getWishlist`, `sendNotification` | A | Both outgoing calls found |
| 6 | impact | `"getWishlist"` | 5 callers: `addToWishlist`, `removeFromWishlist`, `getWishlistAvailability`, `notifyWishlistPriceDrop`, `registerRoutes` | A | Complete blast radius |

### Loom-vs-Grep Comparison

| Symbol | Loom found | Grep found | Loom missed | Grep missed |
|--------|-----------|-----------|-------------|-------------|
| `addToWishlist` | `getProductById` (calls), `getWishlist` (calls), `registerRoutes` (called_by), `removeFromWishlist` (semantic) | `api.js:10` (import), `api.js:96` (call), `wishlist.js:18` (def), `wishlist.js:20` (calls getWishlist) | Nothing material | Semantic link to `removeFromWishlist`; dependency graph of `getProductById` |
| `getWishlist` | `getUserById` (calls), 4 wishlist callers, `registerRoutes` (called_by) | `api.js:10` (import), `api.js:92` (call), `wishlist.js:12` (def), 4 internal calls | Nothing | All call relationship context |
| `moveToCart` | `removeFromWishlist` (calls), `registerRoutes` (called_by) | `api.js:10` (import), `api.js:104` (call with `addToCart` arg), `wishlist.js:47` (def) | Callback `addToCartFn` not tracked | Connection to `addToCart` (passed as argument) |
| `notifyWishlistPriceDrop` | `getWishlist` (calls), `sendNotification` (calls) | `wishlist.js:58` (def only, no external callers) | Nothing | Both call targets (grep just finds the string) |

**Summary:** Loom captured 100% of static structural call relationships that grep could identify. Loom additionally surfaced semantic connections and full dependency graphs that grep cannot. The one gap is function-as-parameter passing (`addToCart` passed to `moveToCart`), which is inherently hard for static analysis.

---

## Watcher Test Results

| Operation | Expected | Actual | Verdict |
|-----------|----------|--------|---------|
| Create file (3 functions) | Indexed in <5s | Indexed in <3s, +3 symbols, +1 file | **PASS** |
| Modify file (add function) | Reindexed in <5s | Reindexed in <3s, +1 symbol, +1 edge (call) | **PASS** |
| Delete file | Symbols removed | -4 symbols, -1 file, no orphans | **PASS** |
| Rapid create (3 files in <1s) | All indexed | All 3 indexed within 5s, +3 symbols, +3 files | **PASS** |
| No-op save (touch) | No reindex | `last_indexed` unchanged, no count changes | **PASS** |

The watcher is solid. Fast, reliable, handles edge cases (rapid creates, deletions, no-op saves). Content-hash based indexing correctly skips unchanged files.

---

## Bugs

| ID | Severity | Tool | Description | Repro |
|----|----------|------|-------------|-------|
| BUG-N01 | **critical** | related, impact, search | Name-collision disambiguation failure: same-named functions in different modules share caller/callee edges. `inventory.js/getProductById` inherits all callers of `product.js/getProductById`. | Create `getProductById` in a new file, call `related()` with `file` param — phantom callers appear |
| BUG-N02 | **high** | related, impact | No inheritance edge detection. `extends` keyword not parsed. `related("PricingStrategy")` and `impact("PricingStrategy")` return empty despite 3 subclasses. | Create class hierarchy with `extends`, query base class |
| BUG-N03 | **high** | search | Empty query crash: `search("")` → `fts5: syntax error near ""` | Call `search("")` |
| BUG-N04 | **high** | search | Hyphen in query crash: `search("not-really-js")` → `no such column: really`. FTS5 interprets `-` as NOT operator. | Call `search("kebab-case-name")` |
| BUG-N05 | **medium** | search | No-match queries return high scores (0.86-1.0) for irrelevant results. `search("xyzzy_nonexistent_symbol_42")` returns `NotFoundError` at score 1.0. No way to distinguish "good match" from "no match". | Search for any non-existent symbol |
| BUG-N06 | **medium** | related, impact | Constructor calls via `new` not tracked. `createStrategy()` calls `new PercentageDiscount()` etc. but no structural edges. | Create factory function using `new`, query `related()` |
| BUG-N07 | **low** | neighborhood | Nonexistent file returns silent empty (`{anchor: null, coupled: []}`) instead of error. | `neighborhood("fake.js", 1)` |
| BUG-N08 | **low** | related | Module-scope constants used inside functions have no structural edges. `CURRENCY_DECIMALS` used in `validatePrice` shows empty `related()`. | Add constant used in a function, query `related()` on the constant |

---

## Ratings

| Tool | Score | Trend | Notes |
|------|-------|-------|-------|
| search | 5/10 | -- | Two crash bugs (empty string, hyphens). No-match queries return high scores. `kind` filter is a nice improvement but search is unreliable for edge-case inputs. |
| related | 6/10 | ↓ | Name-collision bug is critical — false edges poison results in any codebase with common names. No inheritance edges. When it works (unique names), it's accurate and useful. |
| impact | 6/10 | ↑ | Dedup bug fixed (was 10x, now 1x). Accurate for unique symbols. Blind to inheritance hierarchies. Name-collision bug applies here too. |
| neighborhood | 7/10 | ↑ | Anchor field added (previous report request). Good for understanding a file's shape before editing. Could warn on nonexistent files. |
| status | 9/10 | ↑ | Now includes `last_indexed` and `stale_files` (both previously requested). Clean, fast, informative. |
| reindex | 8/10 | NEW | Idempotent, fast. Return value could indicate "already up-to-date" vs "reindexed N files" more clearly when watcher already handled it. |
| **watcher** | 9/10 | ↑ | All 5 stress tests passed. Fast (<3s detection), content-hash based (skips no-ops), handles rapid creates, clean deletion. Rock solid. |

### Overall: 6/10 (previous: 6.4/10)

Score dipped slightly despite real improvements (dedup fix, anchor field, kind filter, reindex tool, stale_files) because this round exposed critical bugs the previous round didn't test for. The name-collision disambiguation failure (BUG-N01) is severe — it will produce false edges in any real codebase where two modules export functions with the same name (which is extremely common: `init`, `create`, `get`, `validate`, etc.). The missing inheritance support (BUG-N02) means class-heavy codebases lose significant structural signal. And the FTS5 input sanitization bugs (BUG-N03, N04) mean search crashes on common inputs.

**Top 3 issues to fix, by impact on developer experience:**

1. **Name-collision disambiguation (BUG-N01)** — This silently produces wrong results without any warning. A developer trusting `impact()` to scope a change would get phantom dependencies. Fix: edge resolution must track which specific symbol (by file path) is the target of each call, not just match by name.

2. **FTS5 input sanitization (BUG-N03, N04)** — Search is the most-used tool and it crashes on empty strings and hyphens. Both are trivial to fix: escape/quote FTS5 special characters in the query string before passing to the FTS engine.

3. **Inheritance edges (BUG-N02)** — Class hierarchies are fundamental to JavaScript/TypeScript codebases. Tree-sitter already parses `extends` — Loom just needs to extract it and create edges. This would make `impact()` on base classes immediately useful.
