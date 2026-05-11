# Code Auditor Report — lang-adapters Pipeline

**Scope:** src/loom/indexer/adapters/ — python.py, go.py, java.py, rust.py, csharp.py, __init__.py
**Date:** 2026-05-11
**Verdict:** NEEDS A SWEEP

## Summary

| Category | Findings | Critical | Actionable |
|----------|----------|----------|------------|
| Dead Code | 2 | 0 | 2 |
| Stale Dependencies | 0 | 0 | 0 |
| Architectural Smells | 2 | 0 | 2 |
| Type Safety Gaps | 1 | 0 | 1 |
| Naming Inconsistencies | 1 | 0 | 1 |
| Code Quality | 2 | 0 | 2 |
| 7A Info Leakage | 0 | 0 | 0 |
| 7B Injection | 0 | 0 | 0 |
| 7C LLM & Embedding | 0 | 0 | 0 |
| 7D Crypto & Secrets | 0 | 0 | 0 |
| 7E Supply Chain | 0 | 0 | 0 |
| Edge Extraction | 4 | 0 | 4 |
| **Total** | **12** | **0** | **12** |

---

## Findings

### Dead Code

**DEAD-1:** `_children_by_type` defined in all 5 adapters but never called
- Type: unused export / duplicated dead helper
- Files: `python.py:115`, `go.py:101`, `java.py:105`, `rust.py:129`, `csharp.py:91`
- Safe to remove: yes — it's defined but has zero call sites across all 5 files. If future code needs it, it's trivial to restore.

**DEAD-2:** `is_class: bool = True` parameter on `_extract_base_list` in `csharp.py`
- Type: unused function parameter (dead code within function body)
- File: `csharp.py:222`
- The parameter is declared and passed at call sites (lines 283, 318, 353) but never read inside the function. The docstring mentions it ("C# base_list is flat") but the logic doesn't branch on it.
- Safe to remove: yes — drop the parameter and update the 3 call sites.

---

### Architectural Smells

**SMELL-1:** `Parser` instantiated per `parse()` call across all 5 adapters
- Where: `python.py:39`, `go.py:37`, `java.py:37`, `rust.py:37`, `csharp.py:37`
- What: `Parser(LANGUAGE)` is created on every file parse. The language constant is module-level but the Parser object is not reused.
- Impact: Unnecessary allocation on every file during indexing. For large codebases (10k+ files) this is measurable overhead.
- Fix: Move `parser = Parser(LANGUAGE)` to a module-level constant alongside `*_LANGUAGE`. Parser is thread-safe once configured; reuse is safe.

**SMELL-2:** Grammar initialization failures escape `__init__.py`'s error guard
- Where: `__init__.py:14-54` (all 6 try/except blocks)
- What: The module-level constants (`PY_LANGUAGE = Language(tspy.language())`, etc.) execute at import time. If the tree-sitter grammar binding raises `RuntimeError` (ABI mismatch) instead of `ImportError`, the `except ImportError` guard won't catch it, and the server crashes instead of degrading gracefully.
- Impact: Any grammar ABI incompatibility (common during tree-sitter upgrades) kills the whole server instead of just disabling that language.
- Fix: Change each guard to `except (ImportError, Exception)` with a log.exception() — or more surgically, `except (ImportError, RuntimeError, OSError)`.

---

### Type Safety Gaps

**TYPE-GAP-1:** `Symbol.kind` is `str` — `kind="macro"` introduced by Rust adapter is undocumented
- Code: `rust.py:593` — `kind="macro"` (only occurrence across all adapters)
- Risk: Server docstrings (`server.py:105, 123, 141`) document kind as `"function" | "class" | "method" | "variable"`. MCP clients filtering by kind will never see macro symbols. No runtime error, but semantic inconsistency.
- Fix: Either add `"macro"` to the documented kind set in server.py, or map macros to `"function"` (macros ARE callable in Rust). If the codebase adds a `KindLiteral` type, include `"macro"`.

---

### Naming Inconsistencies

**NAMING-1:** Inconsistent `source_name` semantics for import edges across adapters
- Python (import X): `source_name=module_name`, `target_name=module_name` — module path is both
- Python (from X import Y): `source_name=Y`, `target_name=Y` — local binding name
- Go: `source_name=path`, `target_name=path` — full import path
- Java: `source_name=import_text`, `target_name=import_text` — full dotted path
- Rust: `source_name=last_part`, `target_name=last_part` — last segment only
- C#: `source_name=namespace`, `target_name=namespace` — namespace string
- The pipeline (`pipeline.py:163`) uses `source_name` as the local binding name and `target_name` as the exported name. Rust's `last_part` is correct for this contract. Java and Go's full-path `source_name` will fail import map lookups when the caller uses a short alias.
- Convention: `source_name` should be the local alias/binding, `target_name` should be the exported symbol name in the target module.
- Fix: Java and Go import edges should set `source_name` to the last segment (package/class name used in code), not the full path. Full path goes in `target_file`.

---

### Code Quality

**QUALITY-1:** `callee[0].isupper()` in `python.py` — fragile empty-string guard
- Where: `python.py:415`
- What: `callee = _get_text(func_node, source)` then immediately `callee[0].isupper()` with no guard for empty string. Tree-sitter identifier nodes are never zero-length in valid source, but malformed/synthesized ASTs (e.g., error recovery nodes) could produce an empty decode.
- Impact: `IndexError` on rare malformed input — uncaught, would crash the parse for that file.
- Fix: `if callee and callee[0].isupper():` — one-character fix.

**QUALITY-2:** `_extract_base_list` in `csharp.py` uses redundant punctuation guard
- Where: `csharp.py:237-239`
- What: `if base_name in (":", ","):` after already filtering `if child.type in ("identifier", "qualified_name", "generic_name"):`. The `:` and `,` tokens have types `":"` and `","` in tree-sitter, not `"identifier"` — so the inner check is unreachable dead logic. Suggests the author wasn't sure what the grammar emitted.
- Impact: Harmless but misleading — implies `:` can appear as an identifier.
- Fix: Remove the inner guard. Add a comment explaining that tree-sitter C# `base_list` uses typed separator tokens.

---

### Edge Extraction Completeness

**EDGE-1:** Java interface `extends` misses `extended_by` reverse edge
- Where: `java.py:297-303` (inside `_handle_interface_declaration`)
- What: `interface B extends A` emits `B extends A` but NOT `A extended_by B`. Compare with `_handle_class_declaration` which correctly emits both directions (lines 225-234).
- Impact: Impact analysis (`impact()` MCP tool) can't traverse upward from `A` to find `B` depends on it. Breaks blast radius computation for interfaces.
- Fix: Add mirror `extended_by` edge after each `extends` edge in `_handle_interface_declaration`.

**EDGE-2:** Java `implements` edges miss `implemented_by` reverse
- Where: `java.py:244-252` (inside `_handle_class_declaration`)
- What: `class C implements I` emits `C implements I` but NOT `I implemented_by C`. Rust correctly emits both (`rust.py:513, 521`).
- Impact: Can't find all implementors of an interface via `impact()` or `related()`. A design change to interface `I` can't compute its full blast radius.
- Fix: Add `implemented_by` reverse edge after each `implements` edge.

**EDGE-3:** Go struct embedding misses package-qualified embedded types
- Where: `go.py:356-370` (`_check_embedding`)
- What: `_check_embedding` looks for `type_identifier` and `pointer_type → type_identifier` children of a `field_declaration`. Embedded types from other packages (e.g., `type S struct { io.Reader }`) produce a `qualified_type` node (package.TypeName), not a bare `type_identifier`. These are silently skipped.
- Impact: Cross-package struct embedding relationships missing from the graph. Particularly relevant for stdlib embeds (`sync.Mutex`, `io.Reader`, `http.Handler`).
- Fix: Add `qualified_type` handling to `_check_embedding` — extract the type identifier from the `qualified_type`'s second child.

**EDGE-4:** Go and Java interfaces don't extract method signatures as symbols
- Where: Go `_process_type_spec:316-327` (interface branch); Java `_handle_interface_declaration`
- What: Interface/trait method stubs are indexed as symbols in Rust (`_handle_function_signature`) but silently dropped in Go and Java. The interface itself is a `class` symbol, but its methods don't appear in the symbol table.
- Impact: `search("interface method name")` won't find Go/Java interface method definitions. Vector search blind spot for interface contracts.
- Fix: After indexing the interface symbol, iterate its method specs (Go: `method_elem` in `interface_type`'s `interface_body`; Java: `method_declaration` in `interface_body`) and emit them as `kind="method"` symbols.

---

### Security Deep Scan

#### 7A — Info Leakage & Error Exposure
No injection vectors found — adapters operate on bytes and return structured data. No user-controlled data reaches error messages or responses.

#### 7B — Injection Attacks
No injection vectors found. `_get_text` decodes bytes with `errors="replace"`. No eval/exec/subprocess. No SQL in adapters. Module path resolution returns from a pre-validated `known_files` set or returns the input unchanged — no file system access.

#### 7C — LLM & Embedding Security
No injection vectors found — the embedder is properly caged.

#### 7D — Cryptographic Failures & Secrets
No secrets, no crypto, no hardcoded credentials found across all 6 files.

#### 7E — Supply Chain & Dependencies
No new transitive dependencies introduced. All 5 grammar packages (`tree-sitter-python`, `tree-sitter-go`, `tree-sitter-java`, `tree-sitter-rust`, `tree-sitter-c-sharp`) are well-established PyPI packages. No pinning concerns beyond what's in `uv.lock`.

---

## Quick Wins (fix in < 5 minutes each)

1. Add `if callee and callee[0].isupper():` guard — `python.py:415` (1 char)
2. Remove `_children_by_type` from all 5 adapters — 5 × 2 lines deleted
3. Remove `is_class` parameter from `_extract_base_list` and its 3 call sites — `csharp.py`
4. Remove redundant `if base_name in (":", ","):` guard — `csharp.py:237-239`
5. Add `kind="macro"` to server.py docstrings for `search()`, `related()`, `impact()`

## Recommended `/jc` Fixes

1. **EDGE-1 + EDGE-2:** Add `extended_by` and `implemented_by` reverse edges in Java adapter — `java.py:297-303, 244-252` — targeted 4-line additions
2. **DEAD-1:** Remove `_children_by_type` across all adapters (coordinated multi-file cleanup)
3. **QUALITY-1:** Add `if callee` guard — `python.py:415`
4. **SMELL-2:** Broaden `except ImportError` to include `RuntimeError` in `__init__.py`

## Recommended `/build` Tasks

1. **NAMING-1:** Audit and fix Java + Go import edge `source_name` semantics — requires verifying pipeline behavior, adding tests for import map resolution
2. **EDGE-3:** Go cross-package embedded type extraction — needs tree-sitter node type verification + test fixture
3. **EDGE-4:** Go + Java interface method extraction — larger feature addition, needs test coverage
4. **SMELL-1:** Promote Parser to module-level constant in all adapters — simple but needs benchmarking to justify

## The Verdict

The adapters are structurally solid — consistent layout, clean error handling (no swallowed exceptions), good use of the Protocol contract, and a correct `__init__.py` registry pattern. The security posture is clean: no injection vectors, no credential exposure, no eval. The code reads like it was written by one brain in one sitting — which is a compliment.

The rough edges are in edge graph completeness: Java is missing `extended_by` and `implemented_by` reverse edges, Go silently drops cross-package struct embeddings, and neither Go nor Java extracts interface method signatures as symbols. These aren't correctness bugs — the server won't crash — but they're accuracy gaps that directly affect the `impact()` and `related()` tools for Java and Go codebases. The north-star metric suffers.

The `_children_by_type` dead helper defined in all 5 files (but never called in any of them) is the clearest sign that copy-paste was used to bootstrap the new adapters. That's fine — just sweep up after yourself.
