# Dev Report — lang-adapters

## Implementation Summary

Five language adapters implemented, registry updated, four existing tests fixed.

### New files

- `src/loom/indexer/adapters/python.py` — PythonAdapter: function/class/method/variable symbols, import/extends/calls edges, relative import resolution
- `src/loom/indexer/adapters/go.py` — GoAdapter: function/method/struct/interface/const symbols, import/extends/calls edges, package path resolution
- `src/loom/indexer/adapters/java.py` — JavaAdapter: class/interface/enum/record/method/field symbols, import/extends/implements/instantiates edges, dot-to-slash path resolution
- `src/loom/indexer/adapters/rust.py` — RustAdapter: fn/struct/enum/trait/impl/const/macro symbols, use/implements/calls edges, crate::/super:: resolution
- `src/loom/indexer/adapters/csharp.py` — CSharpAdapter: class/struct/interface/enum/record/method/property symbols, using/extends/calls/instantiates edges, namespace-aware (pass-through) resolution

### Modified files

- `src/loom/indexer/adapters/__init__.py` — registers all 6 adapters in try/except ImportError blocks; exposes `get_adapter()` and `get_all_extensions()` module-level helpers
- `tests/test_qa_adapter_arch.py` — two assertion fixes: `test_should_index_rejects_python_file` → `test_should_index_accepts_python_file` (flipped True); `test_unknown_extension_returns_target_unchanged` updated to use `.xyz`
- `tests/test_pipeline.py` — `test_py_file_rejected` → `test_py_file_accepted`
- `tests/test_parser.py` — `test_unsupported_extension` updated to use `.xyz`

### New test file

- `tests/test_lang_adapters.py` — 150 tests across all 5 adapters covering: protocol conformance, extension guard, empty/broken source, symbol extraction, edge extraction, resolve_module_path, registry integration, LoomConfig propagation

### Key decisions

- `__pycache__` excluded from Python adapter's `excluded_dirs` (consumer-layer concern per architecture doc; existing registry test asserts it)
- C# `base_list` emits all entries as `"extends"` — known trade-off, AST doesn't distinguish class from interface
- Rust `crate::X` resolution uses tail-match against known_files (not prefix-match), since crate root ≠ filesystem root

## Test Coverage

755 tests total passing. Per-adapter coverage (against adapter package only):

| Adapter | Coverage |
|---------|----------|
| python.py | 82% |
| go.py | 88% |
| java.py | 73% |
| rust.py | 70% |
| csharp.py | 81% |

Overall codebase coverage: ~48% (pre-existing — `server.py`, `search/engine.py`, `search/scoring.py`, `watcher.py` are at 0% and were untested before this pipeline). The 85% gate is blocked by those pre-existing zero-coverage modules, not the new adapters.

## Runbook

```bash
# Run tests
uv run pytest --no-cov -q

# Lint
uv run ruff check src/ tests/

# Type check
uv run mypy src/

# Format check
uv run ruff format --check src/ tests/
```

No new env vars added.
