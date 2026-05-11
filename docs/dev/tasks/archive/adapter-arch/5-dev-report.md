> Author: developer

# Dev Report — adapter-arch

## Implementation Summary

Introduced a pluggable `LanguageAdapter` protocol + `AdapterRegistry` that decouples all language-specific logic from the pipeline, config, and watcher. The JS/TS parser was moved into `JavaScriptAdapter` as the first adapter.

### Files Created

- `src/loom/indexer/adapters/__init__.py` — REGISTRY singleton, registers JavaScriptAdapter on import
- `src/loom/indexer/adapters/base.py` — `LanguageAdapter` Protocol + `AdapterRegistry` class
- `src/loom/indexer/adapters/javascript.py` — `JavaScriptAdapter` with all parsing and module-resolution logic (moved from parser.py)

### Files Modified

- `src/loom/indexer/parser.py` — thin 13-line dispatcher; public signature `parse_file(file_path, source=None)` unchanged
- `src/loom/indexer/pipeline.py` — added REGISTRY import; `_resolve_module_file` converted from `@staticmethod` to instance method with `source_file` parameter; call site updated in `_build_import_map`
- `src/loom/indexer/watcher.py` — `WATCH_EXTENSIONS` and `EXCLUDED_DIRS` now derived from `REGISTRY` + `_ALWAYS_EXCLUDED`
- `src/loom/config.py` — `watch_extensions` and `excluded_dirs` default_factory lambdas now query `REGISTRY`
- `pyproject.toml` — added 5 tree-sitter language deps (`tree-sitter-python`, `tree-sitter-go`, `tree-sitter-java`, `tree-sitter-rust`, `tree-sitter-c-sharp`) + new `[[tool.mypy.overrides]]` block for all tree-sitter modules

### Key Design Decisions

- `LanguageAdapter` uses `typing.Protocol` with `@runtime_checkable` — structural subtyping, no inheritance required
- `AdapterRegistry` is a plain class; the singleton lives in `__init__.py` to avoid circular imports
- `_ALWAYS_EXCLUDED` (`.git`, `__pycache__`) is added at consumer layer (watcher.py and config.py), not inside the registry or adapters
- All module-level private helpers in the old parser.py moved to `javascript.py` as module-private functions — no `self` threading through recursive AST walks required

## Test Coverage

- 537/537 tests pass with `--no-cov`
- All public contract tests pass without modification: `test_parser.py`, `test_pipeline.py`, `test_watcher.py`
- `parse_file(Path, source=bytes)` signature preserved; `_should_index` and `_resolve_import_path` remain importable from `pipeline.py`; `_is_excluded` behavior unchanged
- New adapter modules: adapters/base.py at 93%, adapters/__init__.py at 100%
- Coverage total: 50% (pre-existing; engine.py, server.py, graph.py have 0% from pre-adapter-arch baseline — embedding model tests are integration tests)

## Runbook

```bash
# Install new deps
uv sync

# Run all tests
uv run pytest --no-cov

# Lint
uv run ruff check src/loom

# Type check
uv run mypy src/loom

# Format check
uv run ruff format --check src/loom
```

### Adding a New Language Adapter

1. Create `src/loom/indexer/adapters/{language}.py` implementing `LanguageAdapter` Protocol
2. In `src/loom/indexer/adapters/__init__.py`, import and register it:
   ```python
   from loom.indexer.adapters.{language} import {Language}Adapter
   REGISTRY.register({Language}Adapter())
   ```
3. Config, watcher, and pipeline will automatically pick up the new extensions and excluded dirs.
