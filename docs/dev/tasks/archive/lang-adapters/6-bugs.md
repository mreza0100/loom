# Bug Report — lang-adapters QA

**Pipeline:** lang-adapters
**Mode:** PRE-MERGE
**Date:** 2026-05-11
**Test file:** `tests/test_qa_lang_adapters.py` (100 tests)

---

## Summary

| # | ID | Severity | Status |
|---|---|---|---|
| 1 | BUG-RUST-USE-AS | Medium | INLINE-FIXED |

Coverage gate: PASS (91.79% — up from 84.99% pre-suite, 41.38% adapter-only run)

---

## Bugs Found

### BUG-RUST-USE-AS — Rust adapter silently drops `use X as Y` imports

**File:** `src/loom/indexer/adapters/rust.py` — `_handle_use_declaration()`

**Description:** Top-level `use X as Y;` statements produce zero edges. The `_handle_use_declaration` function dispatches on child node type but had no branch for `use_as_clause` at the declaration level. The `use_as_clause` handler only existed inside `_handle_use_list` (for grouped imports like `use foo::{A as B}`), leaving the common `use std::collections::HashMap as Map;` pattern silently unindexed.

**Root cause:** The tree-sitter AST for `use std::collections::HashMap as Map;` is:
```
use_declaration
  use_as_clause
    scoped_identifier: std::collections::HashMap
    identifier: Map
```
`_handle_use_declaration` matched `scoped_identifier` and `scoped_use_list` but not `use_as_clause` directly.

**Impact:** Any Rust codebase using `use X as Y` aliasing (extremely common — e.g., `use std::collections::HashMap as Map`) would silently miss those import edges, breaking structural coupling for aliased imports.

**Fix (INLINE-FIXED):** Added a `use_as_clause` branch in `_handle_use_declaration()` in `src/loom/indexer/adapters/rust.py`. Extracts the original path from the first child, emits an `imports` edge using the last `::` segment as both `source_name` and `target_name`.

```python
elif child.type == "use_as_clause":
    orig_node = child.children[0] if child.children else None
    if orig_node:
        orig = _get_text(orig_node, source)
        last_part = orig.split("::")[-1]
        edges.append(
            ParsedEdge(
                source_name=last_part,
                target_name=last_part,
                target_file=orig,
                relationship="imports",
            )
        )
```

**Change size:** 11 lines, single function, zero logic change in other paths. Qualifies as inline fix.

---

## Compliance Checks

- **BUG-RAW-PRINT:** None found. All 5 adapters use `log.*` exclusively. Verified by AST walk in test class `TestCompliancePrint`.
- **BUG-MOCK-VIOLATION:** None. Tests use real tree-sitter parsers (internal deps), no mocking.
- **BUG-COVERAGE:** Not triggered. Coverage at 91.79% after QA tests added, exceeding the 70% threshold.

---

## Coverage Deltas (adapter-specific, before → after QA tests)

| Adapter | Before | After |
|---------|--------|-------|
| python.py | 82% | 92% |
| go.py | 88% | 94% |
| java.py | 73% | 96% |
| rust.py | 70% | 89% |
| csharp.py | 81% | 95% |
| **Overall** | **84.99%** | **91.79%** |

The Rust adapter coverage jump (70% → 89%) was the critical gate blocker and is now resolved.

---

## Test Scenarios Added (`tests/test_qa_lang_adapters.py`)

**100 tests total across these classes:**

- `TestRegistryHelpers` — module-level `get_adapter()` / `get_all_extensions()` wrappers, `get_all_excluded_dirs()` union
- `TestPythonAdapterAdversarial` — null bytes, unicode identifiers, BOM, aliased imports (`import X as Y`, `from X import Y as Z`), wildcard `from X import *`, relative imports (`..module`, `.sibling`, `.sub/__init__.py`), stacked decorators, nested classes, `self.method()` call edges, `MyClass()` instantiation edges, very long names
- `TestGoAdapterAdversarial` — type alias, plain type definition, var declaration, grouped consts, call edges, selector expression calls, direct-match resolution, multi-level tail resolution, `extended_by` embedding edge, pointer receiver method kind, language attribute, package-only source
- `TestJavaAdapterAdversarial` — static import, inner class qualified name, inner class method name, constructor extraction, field as variable, record kind, `new Foo()` instantiates edge, method call edge, no-dot import tail match, interface-extends-interface, enum with body methods
- `TestRustAdapterAdversarial` — static item, type alias, `use X as Y` edge (the bug), grouped use list, glob-in-list skipped, `self.helper()` call, `super::` resolution, `crate::` to `mod.rs` resolution, bare mod name, trait method stubs, multiple impl blocks, enum variant kind, symbol language, generic impl no-crash, bare use identifier
- `TestCSharpAdapterAdversarial` — `using static`, `using Alias = Type`, `new Foo()` instantiates, object method call, simple bare call, struct with interface base list, nested class qualified name, constructor, interface-extends-interface, deep namespace traversal, field declaration, extension guards
- `TestCrossAdapterNamespace` — same function name in Python+Go, same class name in Java+C# (verifies language fields don't bleed)
- `TestCompliancePrint` — AST-based check for `print()` in all 5 adapters
