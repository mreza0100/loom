"""Adversarial QA tests for the adapter-arch refactor.

Tests the LanguageAdapter Protocol, AdapterRegistry, and all integration points:
  - Protocol conformance (structural subtyping)
  - Registry edge cases (duplicate registration, unknown extension, empty registry)
  - Import chain / no circular imports
  - Pipeline integration (module resolution through adapter layer)
  - Config defaults (computed from registry)
  - Watcher module-level constants (computed at import time)
  - Public API contracts preserved (parse_file signature, importable helpers)
  - Stale global state between tests
"""

from __future__ import annotations

from pathlib import Path
from unittest.mock import MagicMock

from loom.indexer.adapters.base import AdapterRegistry, LanguageAdapter
from loom.store.models import ParsedEdge, Symbol

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


class _MinimalAdapter:
    """Minimal conforming LanguageAdapter stub."""

    extensions: frozenset[str] = frozenset({".min"})
    language_name: str = "minimal"
    excluded_dirs: frozenset[str] = frozenset({"__minimal_dist__"})

    def parse(self, source: bytes, file_path: str) -> tuple[list[Symbol], list[ParsedEdge]]:
        return [], []

    def resolve_module_path(self, import_path: str, source_file: str, known_files: set[str]) -> str:
        return import_path


class _OverlappingAdapter:
    """Adapter claiming .js (overlaps with JavaScriptAdapter)."""

    extensions: frozenset[str] = frozenset({".js", ".fakeext"})
    language_name: str = "fake"
    excluded_dirs: frozenset[str] = frozenset()

    def parse(self, source: bytes, file_path: str) -> tuple[list[Symbol], list[ParsedEdge]]:
        return [], []

    def resolve_module_path(self, import_path: str, source_file: str, known_files: set[str]) -> str:
        return "FAKE"


# ---------------------------------------------------------------------------
# 1. Protocol Conformance
# ---------------------------------------------------------------------------


class TestLanguageAdapterProtocol:
    def test_minimal_adapter_satisfies_protocol(self) -> None:
        """Structural subtype check — no inheritance required."""
        adapter = _MinimalAdapter()
        assert isinstance(adapter, LanguageAdapter)

    def test_missing_extension_attribute_fails_protocol(self) -> None:
        """Object without 'extensions' is NOT a LanguageAdapter."""

        class _Bad:
            language_name: str = "bad"
            excluded_dirs: frozenset[str] = frozenset()

            def parse(self, source: bytes, file_path: str) -> tuple[list[Symbol], list[ParsedEdge]]:
                return [], []

            def resolve_module_path(
                self, import_path: str, source_file: str, known_files: set[str]
            ) -> str:
                return import_path

        assert not isinstance(_Bad(), LanguageAdapter)

    def test_missing_parse_method_fails_protocol(self) -> None:
        """Object without 'parse' is NOT a LanguageAdapter."""

        class _NoParse:
            extensions: frozenset[str] = frozenset({".x"})
            language_name: str = "x"
            excluded_dirs: frozenset[str] = frozenset()

            def resolve_module_path(
                self, import_path: str, source_file: str, known_files: set[str]
            ) -> str:
                return import_path

        assert not isinstance(_NoParse(), LanguageAdapter)

    def test_missing_resolve_module_path_fails_protocol(self) -> None:
        """Object without 'resolve_module_path' is NOT a LanguageAdapter."""

        class _NoResolve:
            extensions: frozenset[str] = frozenset({".x"})
            language_name: str = "x"
            excluded_dirs: frozenset[str] = frozenset()

            def parse(self, source: bytes, file_path: str) -> tuple[list[Symbol], list[ParsedEdge]]:
                return [], []

        assert not isinstance(_NoResolve(), LanguageAdapter)

    def test_javascript_adapter_satisfies_protocol(self) -> None:
        """The shipped JavaScriptAdapter must satisfy the Protocol."""
        from loom.indexer.adapters.javascript import JavaScriptAdapter

        adapter = JavaScriptAdapter()
        assert isinstance(adapter, LanguageAdapter)


# ---------------------------------------------------------------------------
# 2. AdapterRegistry — edge cases
# ---------------------------------------------------------------------------


class TestAdapterRegistryEdgeCases:
    def _fresh_registry(self) -> AdapterRegistry:
        return AdapterRegistry()

    def test_empty_registry_get_adapter_returns_none(self) -> None:
        """Fresh registry: any extension lookup returns None."""
        reg = self._fresh_registry()
        assert reg.get_adapter(".js") is None
        assert reg.get_adapter("") is None

    def test_empty_registry_all_extensions_is_empty_frozenset(self) -> None:
        reg = self._fresh_registry()
        result = reg.get_all_extensions()
        assert result == frozenset()

    def test_empty_registry_all_excluded_dirs_is_empty_frozenset(self) -> None:
        reg = self._fresh_registry()
        result = reg.get_all_excluded_dirs()
        assert result == frozenset()

    def test_register_and_retrieve_adapter(self) -> None:
        reg = self._fresh_registry()
        adapter = _MinimalAdapter()
        reg.register(adapter)
        assert reg.get_adapter(".min") is adapter

    def test_get_adapter_unknown_extension_returns_none(self) -> None:
        reg = self._fresh_registry()
        reg.register(_MinimalAdapter())
        assert reg.get_adapter(".unknown") is None

    def test_get_adapter_empty_extension_returns_none(self) -> None:
        reg = self._fresh_registry()
        reg.register(_MinimalAdapter())
        assert reg.get_adapter("") is None

    def test_double_register_same_adapter_idempotent(self) -> None:
        """Registering the same adapter object twice must not duplicate it."""
        reg = self._fresh_registry()
        adapter = _MinimalAdapter()
        reg.register(adapter)
        reg.register(adapter)
        # _adapters should contain it exactly once
        assert reg._adapters.count(adapter) == 1

    def test_extension_overwrite_with_different_adapter(self) -> None:
        """Registering a second adapter for the same extension overwrites the first."""
        reg = self._fresh_registry()
        adapter1 = _MinimalAdapter()
        reg.register(adapter1)

        # Second adapter claims the same .min extension
        class _SecondAdapter(_MinimalAdapter):
            language_name: str = "second"

        adapter2 = _SecondAdapter()
        reg.register(adapter2)

        # The extension now points to the second adapter
        assert reg.get_adapter(".min") is adapter2

    def test_get_all_extensions_union(self) -> None:
        reg = self._fresh_registry()
        reg.register(_MinimalAdapter())
        reg.register(_OverlappingAdapter())
        exts = reg.get_all_extensions()
        assert ".min" in exts
        assert ".js" in exts
        assert ".fakeext" in exts

    def test_get_all_excluded_dirs_union(self) -> None:
        reg = self._fresh_registry()
        reg.register(_MinimalAdapter())
        reg.register(_OverlappingAdapter())
        dirs = reg.get_all_excluded_dirs()
        assert "__minimal_dist__" in dirs
        # _OverlappingAdapter has empty excluded_dirs — no pollution
        assert len(dirs) == 1

    def test_registry_does_not_include_git_or_pycache(self) -> None:
        """Registry itself must NOT include always-excluded dirs."""
        from loom.indexer.adapters import REGISTRY

        excluded = REGISTRY.get_all_excluded_dirs()
        # .git and __pycache__ are consumer-layer concerns, not adapter concerns
        assert ".git" not in excluded
        assert "__pycache__" not in excluded


# ---------------------------------------------------------------------------
# 3. No circular imports
# ---------------------------------------------------------------------------


class TestNoCircularImports:
    def test_import_adapters_base(self) -> None:
        import importlib

        mod = importlib.import_module("loom.indexer.adapters.base")
        assert hasattr(mod, "LanguageAdapter")
        assert hasattr(mod, "AdapterRegistry")

    def test_import_adapters_init(self) -> None:
        import importlib

        mod = importlib.import_module("loom.indexer.adapters")
        assert hasattr(mod, "REGISTRY")

    def test_import_adapters_javascript(self) -> None:
        import importlib

        mod = importlib.import_module("loom.indexer.adapters.javascript")
        assert hasattr(mod, "JavaScriptAdapter")

    def test_import_parser(self) -> None:
        import importlib

        mod = importlib.import_module("loom.indexer.parser")
        assert hasattr(mod, "parse_file")

    def test_import_pipeline(self) -> None:
        import importlib

        mod = importlib.import_module("loom.indexer.pipeline")
        assert hasattr(mod, "IndexPipeline")
        assert hasattr(mod, "_should_index")
        assert hasattr(mod, "_resolve_import_path")

    def test_import_watcher(self) -> None:
        import importlib

        mod = importlib.import_module("loom.indexer.watcher")
        assert hasattr(mod, "_is_excluded")
        assert hasattr(mod, "WATCH_EXTENSIONS")
        assert hasattr(mod, "EXCLUDED_DIRS")

    def test_import_config(self) -> None:
        import importlib

        mod = importlib.import_module("loom.config")
        assert hasattr(mod, "LoomConfig")


# ---------------------------------------------------------------------------
# 4. Public contract preservation
# ---------------------------------------------------------------------------


class TestPublicContractPreservation:
    def test_parse_file_signature_path_only(self, tmp_path: Path) -> None:
        """parse_file(Path) with no source kwarg reads from disk."""
        from loom.indexer.parser import parse_file

        f = tmp_path / "sample.js"
        f.write_text("function hello() {}")
        symbols, edges = parse_file(f)  # no source argument
        assert any(s.name == "hello" for s in symbols)

    def test_parse_file_signature_with_source_bytes(self) -> None:
        """parse_file(Path, source=bytes) must accept bytes source."""
        from loom.indexer.parser import parse_file

        symbols, _ = parse_file(Path("inline.js"), source=b"function inline() {}")
        assert any(s.name == "inline" for s in symbols)

    def test_parse_file_unknown_extension_returns_empty(self) -> None:
        """Unknown extensions must return ([], []), not raise."""
        from loom.indexer.parser import parse_file

        symbols, edges = parse_file(Path("test.rb"), source=b"def ruby_fn; end")
        assert symbols == []
        assert edges == []

    def test_parse_file_extension_no_dot_returns_empty(self) -> None:
        """A path with no suffix must gracefully return ([], [])."""
        from loom.indexer.parser import parse_file

        symbols, edges = parse_file(Path("Makefile"), source=b"all: build")
        assert symbols == []
        assert edges == []

    def test_should_index_importable_from_pipeline(self) -> None:
        from loom.indexer.pipeline import _should_index

        assert callable(_should_index)

    def test_resolve_import_path_importable_from_pipeline(self) -> None:
        from loom.indexer.pipeline import _resolve_import_path

        assert callable(_resolve_import_path)

    def test_is_excluded_importable_from_watcher(self) -> None:
        from loom.indexer.watcher import _is_excluded

        assert callable(_is_excluded)


# ---------------------------------------------------------------------------
# 5. JavaScriptAdapter.parse() — adversarial inputs
# ---------------------------------------------------------------------------


class TestJavaScriptAdapterParseEdgeCases:
    def _adapter(self):  # type: ignore[return]
        from loom.indexer.adapters.javascript import JavaScriptAdapter

        return JavaScriptAdapter()

    def test_parse_empty_bytes(self) -> None:
        adapter = self._adapter()
        symbols, edges = adapter.parse(b"", "test.js")
        assert isinstance(symbols, list)
        assert isinstance(edges, list)

    def test_parse_binary_garbage(self) -> None:
        """Feeding raw binary bytes must not crash the parser."""
        adapter = self._adapter()
        garbage = bytes(range(256)) * 4
        symbols, edges = adapter.parse(garbage, "test.js")
        assert isinstance(symbols, list)
        assert isinstance(edges, list)

    def test_parse_null_bytes_in_source(self) -> None:
        adapter = self._adapter()
        source = b"function foo() {}\x00\x00\x00"
        symbols, edges = adapter.parse(source, "test.js")
        assert isinstance(symbols, list)

    def test_parse_extension_not_in_map_returns_empty(self) -> None:
        """Adapter called with a file extension it doesn't handle returns ([], [])."""
        adapter = self._adapter()
        # .rb is not in _EXTENSION_TO_LANG
        symbols, edges = adapter.parse(b"function test() {}", "test.rb")
        assert symbols == []
        assert edges == []

    def test_parse_no_extension_returns_empty(self) -> None:
        adapter = self._adapter()
        symbols, edges = adapter.parse(b"function test() {}", "Makefile")
        assert symbols == []
        assert edges == []

    def test_parse_very_large_source(self) -> None:
        """Parser must not hang or crash on large source."""
        adapter = self._adapter()
        # Generate a valid JS file with 1000 functions
        big_source = "\n".join(f"function fn_{i}() {{ return {i}; }}" for i in range(1000))
        symbols, edges = adapter.parse(big_source.encode(), "big.js")
        assert len(symbols) == 1000

    def test_parse_tsx_extension_treated_as_typescript(self) -> None:
        adapter = self._adapter()
        symbols, _ = adapter.parse(b"function Comp() {}", "comp.tsx")
        assert any(s.language == "typescript" for s in symbols)

    def test_parse_mjs_extension_treated_as_javascript(self) -> None:
        adapter = self._adapter()
        symbols, _ = adapter.parse(b"function esm() {}", "mod.mjs")
        assert any(s.language == "javascript" for s in symbols)

    def test_parse_unicode_identifiers(self) -> None:
        """Unicode in string literals must not crash (identifiers are ASCII in JS)."""
        adapter = self._adapter()
        source = 'function greet() { return "こんにちは"; }'.encode()
        symbols, edges = adapter.parse(source, "test.js")
        assert any(s.name == "greet" for s in symbols)

    def test_parse_deeply_nested_functions(self) -> None:
        """Deeply nested AST must not stack-overflow."""
        adapter = self._adapter()
        depth = 50
        nested = "function f0() {" + " ".join(f"function f{i}() {{" for i in range(1, depth))
        nested += "}" * depth
        symbols, edges = adapter.parse(nested.encode(), "deep.js")
        assert isinstance(symbols, list)

    def test_parse_returns_symbol_with_correct_file_path(self) -> None:
        adapter = self._adapter()
        symbols, _ = adapter.parse(b"function check() {}", "src/utils/check.js")
        assert all(s.file == "src/utils/check.js" for s in symbols)


# ---------------------------------------------------------------------------
# 6. JavaScriptAdapter.resolve_module_path() — adversarial inputs
# ---------------------------------------------------------------------------


class TestJavaScriptAdapterResolveModulePath:
    def _adapter(self):  # type: ignore[return]
        from loom.indexer.adapters.javascript import JavaScriptAdapter

        return JavaScriptAdapter()

    def test_exact_match_in_known_files(self) -> None:
        adapter = self._adapter()
        known = {"src/utils.js"}
        assert adapter.resolve_module_path("src/utils.js", "src/app.js", known) == "src/utils.js"

    def test_extension_candidate_resolution(self) -> None:
        adapter = self._adapter()
        known = {"src/utils.ts"}
        result = adapter.resolve_module_path("src/utils", "src/app.ts", known)
        assert result == "src/utils.ts"

    def test_index_file_resolution(self) -> None:
        adapter = self._adapter()
        known = {"src/utils/index.js"}
        result = adapter.resolve_module_path("src/utils", "src/app.js", known)
        assert result == "src/utils/index.js"

    def test_no_match_returns_import_path_unchanged(self) -> None:
        adapter = self._adapter()
        result = adapter.resolve_module_path("react", "src/app.js", set())
        assert result == "react"

    def test_empty_known_files_returns_import_path(self) -> None:
        adapter = self._adapter()
        result = adapter.resolve_module_path("./helper", "src/app.js", set())
        assert result == "./helper"

    def test_bare_npm_module_not_in_known_files(self) -> None:
        """Bare npm module specifiers (no ./) should be returned unchanged."""
        adapter = self._adapter()
        known: set[str] = set()
        result = adapter.resolve_module_path("lodash", "src/app.js", known)
        assert result == "lodash"

    def test_import_path_already_has_extension(self) -> None:
        adapter = self._adapter()
        known = {"src/helper.js"}
        result = adapter.resolve_module_path("src/helper.js", "src/app.js", known)
        assert result == "src/helper.js"

    def test_index_ts_resolution_priority(self) -> None:
        """index.ts should also be tried when index.js isn't present."""
        adapter = self._adapter()
        known = {"src/utils/index.ts"}
        result = adapter.resolve_module_path("src/utils", "src/app.ts", known)
        assert result == "src/utils/index.ts"

    def test_resolution_with_many_candidates(self) -> None:
        """If multiple extensions exist, the first match in extension order wins."""
        adapter = self._adapter()
        # Both .js and .ts variants exist — .js comes first in the extension list
        known = {"src/utils.ts", "src/utils.js"}
        result = adapter.resolve_module_path("src/utils", "src/app.js", known)
        # Should match .js first (.js before .jsx, .ts, .tsx, .mjs, .cjs in the list)
        assert result == "src/utils.js"


# ---------------------------------------------------------------------------
# 7. Config defaults computed from registry
# ---------------------------------------------------------------------------


class TestConfigDefaults:
    def test_watch_extensions_contains_js_extensions(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert ".js" in config.watch_extensions
        assert ".ts" in config.watch_extensions
        assert ".tsx" in config.watch_extensions
        assert ".jsx" in config.watch_extensions
        assert ".mjs" in config.watch_extensions
        assert ".cjs" in config.watch_extensions

    def test_excluded_dirs_contains_node_modules(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert "node_modules" in config.excluded_dirs

    def test_excluded_dirs_contains_always_excluded(self, tmp_path: Path) -> None:
        """Consumer layer must always add .git and __pycache__."""
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert ".git" in config.excluded_dirs
        assert "__pycache__" in config.excluded_dirs

    def test_excluded_dirs_contains_dist(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert "dist" in config.excluded_dirs

    def test_config_watch_extensions_is_frozenset(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert isinstance(config.watch_extensions, frozenset)

    def test_config_excluded_dirs_is_frozenset(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert isinstance(config.excluded_dirs, frozenset)


# ---------------------------------------------------------------------------
# 8. Watcher module-level constants
# ---------------------------------------------------------------------------


class TestWatcherModuleLevelConstants:
    def test_watch_extensions_populated_at_import(self) -> None:
        from loom.indexer.watcher import WATCH_EXTENSIONS

        assert ".js" in WATCH_EXTENSIONS
        assert ".ts" in WATCH_EXTENSIONS

    def test_excluded_dirs_contains_node_modules(self) -> None:
        from loom.indexer.watcher import EXCLUDED_DIRS

        assert "node_modules" in EXCLUDED_DIRS

    def test_excluded_dirs_contains_always_excluded(self) -> None:
        from loom.indexer.watcher import EXCLUDED_DIRS

        assert ".git" in EXCLUDED_DIRS
        assert "__pycache__" in EXCLUDED_DIRS

    def test_is_excluded_node_modules(self) -> None:
        from loom.indexer.watcher import _is_excluded

        assert _is_excluded(Path("project/node_modules/lodash/index.js")) is True

    def test_is_excluded_git_dir(self) -> None:
        from loom.indexer.watcher import _is_excluded

        assert _is_excluded(Path("project/.git/config")) is True

    def test_is_excluded_dist_dir(self) -> None:
        from loom.indexer.watcher import _is_excluded

        assert _is_excluded(Path("project/dist/bundle.js")) is True

    def test_is_excluded_normal_src_path(self) -> None:
        from loom.indexer.watcher import _is_excluded

        assert _is_excluded(Path("src/app.js")) is False

    def test_is_excluded_next_dir(self) -> None:
        """Adapter-declared .next exclusion must propagate to watcher."""
        from loom.indexer.watcher import _is_excluded

        assert _is_excluded(Path("project/.next/server/app.js")) is True

    def test_is_excluded_coverage_dir(self) -> None:
        """Adapter-declared coverage exclusion must propagate to watcher."""
        from loom.indexer.watcher import _is_excluded

        assert _is_excluded(Path("project/coverage/lcov.info")) is True


# ---------------------------------------------------------------------------
# 9. Pipeline — _resolve_module_file falls back for unknown extension
# ---------------------------------------------------------------------------


class TestPipelineResolveModuleFileFallback:
    def test_unknown_extension_returns_target_unchanged(self, tmp_path: Path) -> None:
        """_resolve_module_file with a non-registered extension returns target unchanged."""
        from loom.config import LoomConfig
        from loom.indexer.pipeline import IndexPipeline

        config = LoomConfig(target_dir=tmp_path)
        pipeline = IndexPipeline(
            config=config,
            db=MagicMock(),
            embedder=MagicMock(),
            graph=None,
        )
        result = pipeline._resolve_module_file(
            "some/target/module",
            {"some/target/module.py"},
            "src/main.py",  # .py has no registered adapter
        )
        # Must return unchanged — no adapter, no modification
        assert result == "some/target/module"

    def test_js_extension_delegates_to_adapter(self, tmp_path: Path) -> None:
        """_resolve_module_file with .js extension uses JavaScriptAdapter logic."""
        from loom.config import LoomConfig
        from loom.indexer.pipeline import IndexPipeline

        config = LoomConfig(target_dir=tmp_path)
        pipeline = IndexPipeline(
            config=config,
            db=MagicMock(),
            embedder=MagicMock(),
            graph=None,
        )
        known = {"src/helper.ts"}
        result = pipeline._resolve_module_file(
            "src/helper",
            known,
            "src/main.js",  # .js → JavaScriptAdapter
        )
        # Adapter should resolve src/helper → src/helper.ts
        assert result == "src/helper.ts"

    def test_empty_source_file_extension_returns_target_unchanged(self, tmp_path: Path) -> None:
        """Source file with no extension falls back gracefully."""
        from loom.config import LoomConfig
        from loom.indexer.pipeline import IndexPipeline

        config = LoomConfig(target_dir=tmp_path)
        pipeline = IndexPipeline(
            config=config,
            db=MagicMock(),
            embedder=MagicMock(),
            graph=None,
        )
        result = pipeline._resolve_module_file(
            "some/module",
            {"some/module.js"},
            "Makefile",  # no extension
        )
        # No adapter for "" → return unchanged
        assert result == "some/module"


# ---------------------------------------------------------------------------
# 10. REGISTRY singleton integrity
# ---------------------------------------------------------------------------


class TestRegistrySingletonIntegrity:
    def test_registry_singleton_is_same_object(self) -> None:
        """Importing REGISTRY twice must yield the same object."""
        from loom.indexer import adapters as adapters_mod1
        from loom.indexer import adapters as adapters_mod2

        assert adapters_mod1.REGISTRY is adapters_mod2.REGISTRY

    def test_registry_has_js_adapter_registered(self) -> None:
        from loom.indexer.adapters import REGISTRY
        from loom.indexer.adapters.javascript import JavaScriptAdapter

        js_adapter = REGISTRY.get_adapter(".js")
        assert js_adapter is not None
        assert isinstance(js_adapter, JavaScriptAdapter)

    def test_registry_all_js_extensions_registered(self) -> None:
        from loom.indexer.adapters import REGISTRY

        expected = {".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs"}
        registered = REGISTRY.get_all_extensions()
        assert expected.issubset(registered)

    def test_registry_adapter_for_ts_is_javascript_adapter(self) -> None:
        """All JS/TS extensions must map to the same JavaScriptAdapter."""
        from loom.indexer.adapters import REGISTRY
        from loom.indexer.adapters.javascript import JavaScriptAdapter

        for ext in [".ts", ".tsx", ".jsx", ".mjs", ".cjs"]:
            adapter = REGISTRY.get_adapter(ext)
            assert adapter is not None, f"No adapter for {ext}"
            assert isinstance(adapter, JavaScriptAdapter), f"Wrong adapter for {ext}"


# ---------------------------------------------------------------------------
# 11. Edge case: parse_file with None-suffix (no dot in filename)
# ---------------------------------------------------------------------------


class TestParsFileNonSuffixPaths:
    def test_parse_file_makefile_no_suffix(self) -> None:
        from loom.indexer.parser import parse_file

        symbols, edges = parse_file(Path("Makefile"), source=b"all:\n\tmake build")
        assert symbols == []
        assert edges == []

    def test_parse_file_dot_prefix_only(self) -> None:
        """Dotfiles like .eslintrc have suffix '' or '.eslintrc' depending on platform."""
        from loom.indexer.parser import parse_file

        # .eslintrc has suffix "" on most platforms — should silently skip
        symbols, edges = parse_file(Path(".eslintrc"), source=b'{"extends": "airbnb"}')
        assert symbols == []
        assert edges == []


# ---------------------------------------------------------------------------
# 12. Global state isolation — REGISTRY must not be polluted between tests
# ---------------------------------------------------------------------------


class TestRegistryStateIsolation:
    def test_registry_extensions_stable_across_calls(self) -> None:
        """Repeated calls to get_all_extensions must return identical results."""
        from loom.indexer.adapters import REGISTRY

        first_call = REGISTRY.get_all_extensions()
        second_call = REGISTRY.get_all_extensions()
        assert first_call == second_call

    def test_fresh_registry_unaffected_by_singleton(self) -> None:
        """A fresh AdapterRegistry instance is independent of the singleton."""
        from loom.indexer.adapters import REGISTRY

        fresh = AdapterRegistry()
        # Fresh has no adapters
        assert fresh.get_all_extensions() == frozenset()
        # Singleton still has its adapters
        assert ".js" in REGISTRY.get_all_extensions()


# ---------------------------------------------------------------------------
# 13. Regression: all previously-testable behaviors still work
# ---------------------------------------------------------------------------


class TestRegressions:
    def test_parse_file_js_produces_symbols(self) -> None:
        from loom.indexer.parser import parse_file

        symbols, _ = parse_file(Path("test.js"), source=b"function legacy() {}")
        assert any(s.name == "legacy" for s in symbols)

    def test_should_index_respects_excluded_dirs(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig
        from loom.indexer.pipeline import _should_index

        config = LoomConfig(target_dir=tmp_path)
        nm = tmp_path / "node_modules" / "pkg"
        nm.mkdir(parents=True)
        f = nm / "index.js"
        f.write_text("module.exports = {}")
        assert _should_index(f, config) is False

    def test_should_index_accepts_js_file(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig
        from loom.indexer.pipeline import _should_index

        config = LoomConfig(target_dir=tmp_path)
        f = tmp_path / "app.js"
        f.write_text("const x = 1;")
        assert _should_index(f, config) is True

    def test_should_index_rejects_python_file(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig
        from loom.indexer.pipeline import _should_index

        config = LoomConfig(target_dir=tmp_path)
        f = tmp_path / "main.py"
        f.write_text("def foo(): pass")
        assert _should_index(f, config) is False

    def test_is_excluded_does_not_false_positive_src(self) -> None:
        from loom.indexer.watcher import _is_excluded

        # Ensure 'src' is not accidentally in EXCLUDED_DIRS
        assert _is_excluded(Path("src/index.js")) is False

    def test_resolve_import_path_stable(self) -> None:
        from loom.indexer.pipeline import _resolve_import_path

        result = _resolve_import_path("./utils.js", "src/services/order.js")
        assert result == "src/services/utils.js"
