> Author: qa

# Bug Report — adapter-arch

## Test File

`tests/test_qa_adapter_arch.py` — 82 adversarial tests across 13 categories.

## Summary

**Result: PASS — no bugs found.**

All 619 tests pass (537 pre-existing + 82 new). Coverage: 91.25% (threshold: 85%). Lint and type-check clean.

## Compliance Checks

| Check | Result |
|-------|--------|
| Raw `print()` in `src/` | PASS — none found |
| Mock violations (internal deps mocked) | PASS — none found |
| Coverage >= 85% | PASS — 91.25% |
| `uv run ruff check` | PASS |
| `uv run mypy` | PASS (note: unused `[[tool.mypy.overrides]]` sections for future adapters — expected) |

## 360° Coverage

All 10 dimensions swept for the adapter-arch refactor:

1. **Inputs** — Unknown/empty extensions, no-suffix paths, binary garbage, null bytes, very large sources, Unicode identifiers.
2. **State** — Fresh (empty) registry, singleton registry, fresh vs singleton isolation.
3. **Boundaries** — Empty `known_files`, single-entry `known_files`, extension not in map, empty extension string.
4. **Sequences** — Double-register same adapter, extension overwrite by second adapter, register → retrieve.
5. **Timing** — WATCH_EXTENSIONS/EXCLUDED_DIRS computed at import time (module-level constants stable after import).
6. **Error paths** — Unknown extension to `parse_file`, no-suffix path to adapter `parse()`, `_resolve_module_file` with unregistered extension.
7. **Data shapes** — Protocol conformance via `isinstance`, minimal stub (all 3 required attrs + 2 methods), missing-attribute stubs rejected correctly.
8. **Environment** — WATCH_EXTENSIONS populated at import time; `.git`/`__pycache__` are consumer-layer (not registry) concerns verified.
9. **Auth/Authz** — N/A.
10. **Regressions** — `parse_file` signature unchanged, `_should_index`/`_resolve_import_path` importable from `pipeline.py`, `_is_excluded` importable from `watcher.py`, node_modules/dist/.git/.next/coverage still excluded.

## Key Findings (No Bugs — Design Validated)

- **Protocol correctness**: `LanguageAdapter` Protocol with `@runtime_checkable` correctly rejects objects missing any of the 3 required attributes (`extensions`, `language_name`, `excluded_dirs`) or either method (`parse`, `resolve_module_path`). `JavaScriptAdapter` satisfies the protocol.

- **Registry idempotence**: Double-registering the same adapter object (by identity) is correctly deduplicated in `_adapters`. Extension map still updated (idempotent overwrite).

- **Extension overwrite semantics**: Registering a second adapter with an overlapping extension correctly overwrites the first in `_by_extension`. Second adapter also enters `_adapters` as a separate entry. This is an intentional last-write-wins design — no bug, but callers should be aware.

- **Consumer-layer separation**: `.git` and `__pycache__` correctly absent from `REGISTRY.get_all_excluded_dirs()`. Config and watcher both add them at their layer via `_ALWAYS_EXCLUDED` constants.

- **Fallback in `_resolve_module_file`**: When source file has no registered adapter (e.g., `.py`, no extension), returns `target_file` unchanged. Clean and safe.

- **Stale constants**: `WATCH_EXTENSIONS` and `EXCLUDED_DIRS` in `watcher.py` are computed once at module import time. If adapters are registered after import, these module-level constants won't update. This is by design (registry is sealed at startup) — not a bug, but worth documenting for future adapter authors.

- **`mypy` note**: `pyproject.toml` contains unused `[[tool.mypy.overrides]]` sections for `tree_sitter_python`, `tree_sitter_go`, `tree_sitter_java`, `tree_sitter_rust`, `tree_sitter_c_sharp`. These are stubs for future adapters. mypy reports them as "unused section(s)" but does not fail.
