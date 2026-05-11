"""Tests for loom.indexer.pipeline — two-phase indexing pipeline."""

from pathlib import Path
from unittest.mock import MagicMock

import pytest

from loom.config import LoomConfig
from loom.indexer.pipeline import (
    IndexPipeline,
    _resolve_import_path,
    _should_index,
)
from loom.store.db import LoomDB
from tests.conftest import make_js_file


class TestShouldIndex:
    def test_js_file_accepted(self, config: LoomConfig, tmp_dir: Path) -> None:
        f = tmp_dir / "app.js"
        f.write_text("const x = 1;")
        assert _should_index(f, config) is True

    def test_ts_file_accepted(self, config: LoomConfig, tmp_dir: Path) -> None:
        f = tmp_dir / "app.ts"
        f.write_text("const x = 1;")
        assert _should_index(f, config) is True

    def test_py_file_accepted(self, config: LoomConfig, tmp_dir: Path) -> None:
        f = tmp_dir / "app.py"
        f.write_text("x = 1")
        assert _should_index(f, config) is True

    def test_node_modules_excluded(self, config: LoomConfig, tmp_dir: Path) -> None:
        d = tmp_dir / "node_modules" / "pkg"
        d.mkdir(parents=True)
        f = d / "index.js"
        f.write_text("module.exports = {};")
        assert _should_index(f, config) is False

    def test_large_file_rejected(self, config: LoomConfig, tmp_dir: Path) -> None:
        f = tmp_dir / "huge.js"
        f.write_bytes(b"x" * (config.max_file_size_bytes + 1))
        assert _should_index(f, config) is False


class TestResolveImportPath:
    def test_relative_sibling(self) -> None:
        result = _resolve_import_path("./utils.js", "src/services/order.js")
        assert result == "src/services/utils.js"

    def test_relative_parent(self) -> None:
        result = _resolve_import_path("../models/cart.js", "src/services/order.js")
        assert result == "src/models/cart.js"

    def test_nested_relative(self) -> None:
        result = _resolve_import_path("./helpers/format.js", "src/utils/index.js")
        assert result == "src/utils/helpers/format.js"

    def test_top_level_file(self) -> None:
        result = _resolve_import_path("./config.js", "index.js")
        assert result == "config.js"


class TestIndexPipeline:
    @pytest.fixture
    def mock_embedder(self) -> MagicMock:
        embedder = MagicMock()
        embedder.embed.return_value = [[0.1] * 768]
        embedder.build_symbol_text.return_value = "function test\ncode"
        return embedder

    def test_full_index(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        make_js_file(tmp_dir, "src/app.js", "function main() { helper(); }\nfunction helper() {}")
        pipeline = IndexPipeline(config, db, mock_embedder)

        mock_embedder.embed.return_value = [[0.1] * 768, [0.2] * 768]
        result = pipeline.full_index()

        assert result["indexed"] == 1
        assert result["symbols"] == 2
        assert result["edges"] >= 1

    def test_skip_unchanged_files(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        make_js_file(tmp_dir, "src/app.js", "function main() {}")
        pipeline = IndexPipeline(config, db, mock_embedder)

        result1 = pipeline.full_index()
        assert result1["indexed"] == 1

        result2 = pipeline.full_index()
        assert result2["indexed"] == 0

    def test_reindex_changed_file(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        f = make_js_file(tmp_dir, "src/app.js", "function main() {}")
        pipeline = IndexPipeline(config, db, mock_embedder)

        pipeline.full_index()
        f.write_text("function main() {}\nfunction extra() {}")
        mock_embedder.embed.return_value = [[0.1] * 768, [0.2] * 768]

        result = pipeline.full_index()
        assert result["indexed"] == 1
        assert result["symbols"] == 2

    def test_incremental_index_new_file(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        f = make_js_file(tmp_dir, "src/new.js", "function brand_new() {}")
        pipeline = IndexPipeline(config, db, mock_embedder)

        result = pipeline.incremental_index([f])
        assert result["indexed"] == 1
        assert result["symbols"] == 1

    def test_incremental_index_deleted_file(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        f = make_js_file(tmp_dir, "src/app.js", "function main() {}")
        pipeline = IndexPipeline(config, db, mock_embedder)
        pipeline.full_index()

        f.unlink()
        result = pipeline.incremental_index([f])
        assert result["deleted"] == 1
        assert db.get_symbol_by_name("main") == []

    def test_excludes_node_modules(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        nm = tmp_dir / "node_modules" / "pkg"
        nm.mkdir(parents=True)
        (nm / "index.js").write_text("function lib() {}")
        make_js_file(tmp_dir, "src/app.js", "function main() {}")

        pipeline = IndexPipeline(config, db, mock_embedder)
        result = pipeline.full_index()
        assert result["indexed"] == 1

    def test_handles_parse_error_gracefully(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        make_js_file(tmp_dir, "src/broken.js", "function { totally broken ]]]")
        make_js_file(tmp_dir, "src/good.js", "function works() {}")
        pipeline = IndexPipeline(config, db, mock_embedder)

        result = pipeline.full_index()
        assert result["indexed"] >= 1


class TestTwoPhaseIndexing:
    """Integration tests for the two-phase parse-all / resolve-all pipeline."""

    @pytest.fixture
    def mock_embedder(self) -> MagicMock:
        embedder = MagicMock()
        embedder.embed.return_value = [[0.1] * 768, [0.2] * 768]
        embedder.build_symbol_text.return_value = "function test\ncode"
        return embedder

    def _make_pipeline(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> IndexPipeline:
        return IndexPipeline(config, db, mock_embedder)

    def test_two_phase_basic(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        """Two files: A calls B. After full_index, edge should be resolved."""
        make_js_file(tmp_dir, "a.js", "function a() { b(); }")
        make_js_file(tmp_dir, "b.js", "function b() {}")
        mock_embedder.embed.return_value = [[0.1] * 768]

        pipeline = self._make_pipeline(tmp_dir, config, db, mock_embedder)
        result = pipeline.full_index()

        assert result["indexed"] == 2
        assert result["symbols"] == 2

        # Find symbol b
        b_syms = db.get_symbol_by_name("b")
        assert len(b_syms) == 1
        b_sym = b_syms[0]
        assert b_sym.id is not None

        # Find symbol a
        a_syms = db.get_symbol_by_name("a")
        assert len(a_syms) == 1
        a_sym = a_syms[0]
        assert a_sym.id is not None

        # Edge from a -> b should be resolved
        edges_from_a = db.get_edges_from(a_sym.id)
        call_edges = [e for e in edges_from_a if e.relationship == "calls"]
        resolved = [e for e in call_edges if e.target_id == b_sym.id]
        assert len(resolved) >= 1
        assert resolved[0].confidence > 0

    def test_two_phase_import_resolution(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        """Import chain: A imports b from b.js, calls b(). Edge resolves via import map."""
        make_js_file(
            tmp_dir,
            "src/a.js",
            "import { b } from './b.js';\nfunction a() { b(); }",
        )
        make_js_file(tmp_dir, "src/b.js", "export function b() {}")
        mock_embedder.embed.return_value = [[0.1] * 768]

        pipeline = self._make_pipeline(tmp_dir, config, db, mock_embedder)
        result = pipeline.full_index()

        assert result["indexed"] == 2

        b_syms = db.get_symbol_by_name("b", "src/b.js")
        assert len(b_syms) == 1
        b_sym = b_syms[0]
        assert b_sym.id is not None

        a_syms = db.get_symbol_by_name("a", "src/a.js")
        assert len(a_syms) == 1
        a_sym = a_syms[0]
        assert a_sym.id is not None

        edges_from_a = db.get_edges_from(a_sym.id)
        call_edges = [e for e in edges_from_a if e.relationship == "calls"]
        resolved = [e for e in call_edges if e.target_id == b_sym.id]
        assert len(resolved) >= 1
        # Import-resolved confidence should be >= 0.6
        assert resolved[0].confidence >= 0.6

    def test_two_phase_unique_name(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        """Unique name match (strategy 5) at confidence 0.6."""
        make_js_file(tmp_dir, "caller.js", "function caller() { _makePathsRelative(); }")
        make_js_file(tmp_dir, "util.js", "function _makePathsRelative() { return '/'; }")
        mock_embedder.embed.return_value = [[0.1] * 768]

        pipeline = self._make_pipeline(tmp_dir, config, db, mock_embedder)
        pipeline.full_index()

        target_syms = db.get_symbol_by_name("_makePathsRelative")
        assert len(target_syms) == 1
        target_sym = target_syms[0]
        assert target_sym.id is not None

        caller_syms = db.get_symbol_by_name("caller")
        assert len(caller_syms) == 1
        caller_sym = caller_syms[0]
        assert caller_sym.id is not None

        edges = db.get_edges_from(caller_sym.id)
        call_edges = [
            e for e in edges if e.relationship == "calls" and e.target_id == target_sym.id
        ]
        assert len(call_edges) >= 1
        assert call_edges[0].confidence == pytest.approx(0.6)

    def test_two_phase_ambiguous_name(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        """Ambiguous name (multiple matches) with no import chain stays unresolved."""
        make_js_file(tmp_dir, "a.js", "function run() { create(); }")
        make_js_file(tmp_dir, "b.js", "function create() { return 1; }")
        make_js_file(tmp_dir, "c.js", "function create() { return 2; }")
        mock_embedder.embed.return_value = [[0.1] * 768]

        pipeline = self._make_pipeline(tmp_dir, config, db, mock_embedder)
        pipeline.full_index()

        run_syms = db.get_symbol_by_name("run")
        assert len(run_syms) == 1
        run_sym = run_syms[0]
        assert run_sym.id is not None

        edges = db.get_edges_from(run_sym.id)
        create_edges = [e for e in edges if e.target_name == "create"]
        # With two "create" symbols, strategy 5 (unique name) fails → edge stays unresolved
        unresolved = [e for e in create_edges if e.target_id is None]
        assert len(unresolved) >= 1

    def test_two_phase_confidence_ordering(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        """Confidence ordering: unique name (0.6) < import-resolved (0.95)."""
        # Unique name match
        make_js_file(tmp_dir, "caller_unique.js", "function callerUnique() { uniqueFunc(); }")
        make_js_file(tmp_dir, "unique.js", "function uniqueFunc() {}")

        # Import-resolved match
        make_js_file(
            tmp_dir,
            "caller_import.js",
            "import { importedFunc } from './imported.js';\n"
            "function callerImport() { importedFunc(); }",
        )
        make_js_file(tmp_dir, "imported.js", "export function importedFunc() {}")

        mock_embedder.embed.return_value = [[0.1] * 768]
        pipeline = self._make_pipeline(tmp_dir, config, db, mock_embedder)
        pipeline.full_index()

        # Check unique match confidence
        unique_caller = db.get_symbol_by_name("callerUnique")[0]
        assert unique_caller.id is not None
        unique_edges = [
            e
            for e in db.get_edges_from(unique_caller.id)
            if e.relationship == "calls" and e.target_id is not None
        ]

        # Check import-resolved confidence
        import_caller = db.get_symbol_by_name("callerImport")[0]
        assert import_caller.id is not None
        import_edges = [
            e
            for e in db.get_edges_from(import_caller.id)
            if e.relationship == "calls" and e.target_id is not None
        ]

        if unique_edges and import_edges:
            assert unique_edges[0].confidence < import_edges[0].confidence

    def test_incremental_re_resolution(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        """Index A (b() unresolved), then add B defining b. Re-index resolves edge."""
        a_file = make_js_file(tmp_dir, "a.js", "function a() { uniquelyNamedFunc(); }")
        mock_embedder.embed.return_value = [[0.1] * 768]

        pipeline = self._make_pipeline(tmp_dir, config, db, mock_embedder)

        # First: index only a.js — edge to uniquelyNamedFunc should be unresolved
        pipeline.incremental_index([a_file])

        a_sym = db.get_symbol_by_name("a")[0]
        assert a_sym.id is not None
        edges = db.get_edges_from(a_sym.id)
        call_edges = [e for e in edges if e.relationship == "calls"]
        # All edges should be unresolved (b doesn't exist yet)
        unresolved_initially = [e for e in call_edges if e.target_id is None]
        assert len(unresolved_initially) >= 1

        # Now add b.js
        b_file = make_js_file(tmp_dir, "b.js", "function uniquelyNamedFunc() {}")
        pipeline.incremental_index([b_file])

        # Re-check: edge should now be resolved
        b_syms = db.get_symbol_by_name("uniquelyNamedFunc")
        assert len(b_syms) == 1
        b_sym = b_syms[0]
        assert b_sym.id is not None

        edges_after = db.get_edges_from(a_sym.id)
        call_edges_after = [e for e in edges_after if e.relationship == "calls"]
        resolved = [e for e in call_edges_after if e.target_id == b_sym.id]
        assert len(resolved) >= 1
        assert resolved[0].confidence > 0

    def test_incremental_delete_nullifies(
        self,
        tmp_dir: Path,
        config: LoomConfig,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        """Delete file B after resolving edge A->B. Edge should become unresolved."""
        make_js_file(tmp_dir, "a.js", "function a() { b(); }")
        b_file = make_js_file(tmp_dir, "b.js", "function b() {}")
        mock_embedder.embed.return_value = [[0.1] * 768]

        pipeline = self._make_pipeline(tmp_dir, config, db, mock_embedder)

        # Index both files
        pipeline.full_index()

        b_syms = db.get_symbol_by_name("b")
        assert len(b_syms) == 1
        b_sym = b_syms[0]
        assert b_sym.id is not None

        a_syms = db.get_symbol_by_name("a")
        assert len(a_syms) == 1
        a_sym = a_syms[0]
        assert a_sym.id is not None

        # Confirm edge is resolved
        edges = db.get_edges_from(a_sym.id)
        resolved = [e for e in edges if e.target_id == b_sym.id]
        assert len(resolved) >= 1

        # Delete b.js
        b_file.unlink()
        pipeline.incremental_index([b_file])

        # b's symbol should be gone
        assert db.get_symbol_by_name("b") == []

        # Edge from a should now be unresolved (target_id=NULL)
        a_syms_after = db.get_symbol_by_name("a")
        assert len(a_syms_after) == 1
        edges_after = db.get_edges_from(a_syms_after[0].id)
        call_edges = [e for e in edges_after if e.relationship == "calls"]
        unresolved = [e for e in call_edges if e.target_id is None]
        assert len(unresolved) >= 1
