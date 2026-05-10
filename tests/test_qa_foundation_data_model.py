"""QA adversarial tests — foundation-data-model pipeline.

Covers unhappy paths, edge cases, boundary conditions, and data integrity
for Phases 1 (ID-Based Edges), 2 (Two-Phase Indexing), and 3 (Full Call Expressions).
"""

import sqlite3
from pathlib import Path
from unittest.mock import MagicMock

import pytest

from loom.config import LoomConfig
from loom.indexer.parser import parse_file
from loom.indexer.pipeline import IndexPipeline
from loom.store.db import LoomDB
from loom.store.models import Edge, ParsedEdge, Symbol
from tests.conftest import make_js_file

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_sym(db: LoomDB, name: str, file: str = "a.js", kind: str = "function") -> Symbol:
    sym_id = db.insert_symbol(
        Symbol(name=name, kind=kind, file=file, line=1, end_line=5, language="javascript")
    )
    sym = db.get_symbol_by_id(sym_id)
    assert sym is not None
    return sym


def _mock_embedder() -> MagicMock:
    e = MagicMock()
    e.embed.return_value = [[0.1] * 768]
    e.embed_single.return_value = [0.1] * 768
    e.build_symbol_text.return_value = "fn\ncode"
    return e


# ---------------------------------------------------------------------------
# Phase 1 — ID-Based Edge Model: DB layer adversarial tests
# ---------------------------------------------------------------------------


class TestEdgeModelInvariants:
    """Invariant checks on the new ID-based edge schema."""

    def test_foreign_keys_on_at_connect(self, db: LoomDB) -> None:
        """PRAGMA foreign_keys must be 1 (ON) after connect()."""
        result = db.conn.execute("PRAGMA foreign_keys").fetchone()
        assert result[0] == 1, "Foreign key enforcement disabled — cascade/set-null won't work"

    def test_insert_edge_with_invalid_source_id_raises(self, db: LoomDB) -> None:
        """Inserting an edge with a non-existent source_id fails due to FK constraint."""
        edge = Edge(source_id=99999, target_name="something", relationship="calls")
        with pytest.raises(sqlite3.IntegrityError):
            db.insert_edge(edge)

    def test_insert_edge_with_invalid_target_id_raises(self, db: LoomDB) -> None:
        """Inserting an edge with a non-existent target_id fails due to FK constraint."""
        src = _make_sym(db, "src")
        assert src.id is not None
        edge = Edge(source_id=src.id, target_name="foo", target_id=99999, relationship="calls")
        with pytest.raises(sqlite3.IntegrityError):
            db.insert_edge(edge)

    def test_cascade_delete_removes_source_edges(self, db: LoomDB) -> None:
        """Deleting a symbol must CASCADE delete its outgoing edges."""
        src = _make_sym(db, "src", "src.js")
        tgt = _make_sym(db, "tgt", "tgt.js")
        assert src.id is not None
        assert tgt.id is not None
        edge_id = db.insert_edge(
            Edge(source_id=src.id, target_name="tgt", target_id=tgt.id, relationship="calls")
        )
        db.commit()

        db.conn.execute("DELETE FROM symbols WHERE id = ?", (src.id,))
        db.commit()

        row = db.conn.execute("SELECT id FROM edges WHERE id = ?", (edge_id,)).fetchone()
        assert row is None, "Edge should have been CASCADE deleted with source symbol"

    def test_delete_target_symbol_sets_null(self, db: LoomDB) -> None:
        """Deleting the target symbol must SET target_id NULL (not delete the edge)."""
        src = _make_sym(db, "src", "src.js")
        tgt = _make_sym(db, "tgt", "tgt.js")
        assert src.id is not None
        assert tgt.id is not None
        edge_id = db.insert_edge(
            Edge(source_id=src.id, target_name="tgt", target_id=tgt.id, relationship="calls")
        )
        db.commit()

        db.conn.execute("DELETE FROM symbols WHERE id = ?", (tgt.id,))
        db.commit()

        row = db.conn.execute("SELECT target_id FROM edges WHERE id = ?", (edge_id,)).fetchone()
        assert row is not None, "Edge should still exist after target deleted"
        assert row[0] is None, "target_id should be NULL after ON DELETE SET NULL"

    def test_get_unresolved_edges_excludes_resolved(self, db: LoomDB) -> None:
        """get_unresolved_edges must only return edges with IS NULL, not resolved ones."""
        src = _make_sym(db, "src", "src.js")
        tgt = _make_sym(db, "tgt", "tgt.js")
        assert src.id is not None
        assert tgt.id is not None
        db.insert_edge(
            Edge(source_id=src.id, target_name="tgt", target_id=tgt.id, relationship="calls")
        )
        db.commit()

        unresolved = db.get_unresolved_edges()
        assert all(e.target_id is None for e in unresolved)

    def test_confidence_boundary_zero(self, db: LoomDB) -> None:
        """Edges with confidence=0.0 (unresolved) round-trip correctly."""
        src = _make_sym(db, "src")
        assert src.id is not None
        db.insert_edge(
            Edge(source_id=src.id, target_name="unknown", relationship="calls", confidence=0.0)
        )
        db.commit()
        unresolved = db.get_unresolved_edges()
        assert any(e.confidence == pytest.approx(0.0) for e in unresolved)

    def test_confidence_boundary_one(self, db: LoomDB) -> None:
        """Edges with confidence=1.0 (exact match) round-trip correctly."""
        src = _make_sym(db, "src")
        tgt = _make_sym(db, "tgt")
        assert src.id is not None
        assert tgt.id is not None
        db.insert_edge(
            Edge(
                source_id=src.id,
                target_name="tgt",
                target_id=tgt.id,
                relationship="calls",
                confidence=1.0,
            )
        )
        db.commit()
        edges = db.get_edges_from(src.id)
        assert edges[0].confidence == pytest.approx(1.0)

    def test_get_edges_to_by_name_returns_both_resolved_and_unresolved(self, db: LoomDB) -> None:
        """get_edges_to_by_name must return edges regardless of target_id status."""
        src1 = _make_sym(db, "caller1", "c1.js")
        src2 = _make_sym(db, "caller2", "c2.js")
        tgt = _make_sym(db, "targetFunc", "t.js")
        assert src1.id is not None
        assert src2.id is not None
        assert tgt.id is not None

        db.insert_edge(
            Edge(
                source_id=src1.id,
                target_name="targetFunc",
                target_id=tgt.id,
                relationship="calls",
                confidence=1.0,
            )
        )
        db.insert_edge(
            Edge(
                source_id=src2.id,
                target_name="targetFunc",
                target_id=None,
                relationship="calls",
            )
        )
        db.commit()

        results = db.get_edges_to_by_name("targetFunc")
        assert len(results) == 2
        resolved = [e for e in results if e.target_id is not None]
        unresolved = [e for e in results if e.target_id is None]
        assert len(resolved) == 1
        assert len(unresolved) == 1

    def test_update_edge_target_nonexistent_is_noop(self, db: LoomDB) -> None:
        """update_edge_target on a non-existent edge_id silently does nothing."""
        tgt = _make_sym(db, "tgt")
        assert tgt.id is not None
        db.update_edge_target(99999, tgt.id, 0.95)
        db.commit()

    def test_remove_file_handles_no_symbols(self, db: LoomDB) -> None:
        """remove_file on a path with no symbols must not crash."""
        db.set_file_hash("ghost.js", "abc")
        db.commit()
        db.remove_file("ghost.js")
        db.commit()
        assert db.get_file_hash("ghost.js") is None

    def test_remove_file_cascade_with_multiple_outgoing(self, db: LoomDB) -> None:
        """Removing a file with multiple outgoing edges must cascade-delete all of them."""
        src = _make_sym(db, "bigFunc", "src.js")
        assert src.id is not None
        tgt1 = _make_sym(db, "helperA", "other.js")
        tgt2 = _make_sym(db, "helperB", "other.js")
        assert tgt1.id is not None
        assert tgt2.id is not None

        db.insert_edge(
            Edge(source_id=src.id, target_name="helperA", target_id=tgt1.id, relationship="calls")
        )
        db.insert_edge(
            Edge(source_id=src.id, target_name="helperB", target_id=tgt2.id, relationship="calls")
        )
        db.set_file_hash("src.js", "h1")
        db.commit()

        db.remove_file("src.js")
        db.commit()

        assert db.get_symbol_by_name("bigFunc") == []
        assert len(db.get_symbol_by_name("helperA")) == 1

    def test_remove_file_nullifies_multiple_incoming_edges(self, db: LoomDB) -> None:
        """Removing a file nullifies ALL incoming edges pointing to its symbols."""
        tgt1 = db.insert_symbol(
            Symbol(
                name="fn1", kind="function", file="a.js", line=1, end_line=2, language="javascript"
            )
        )
        tgt2 = db.insert_symbol(
            Symbol(
                name="fn2", kind="function", file="a.js", line=3, end_line=4, language="javascript"
            )
        )
        caller = _make_sym(db, "caller", "b.js")
        assert caller.id is not None

        edge_id1 = db.insert_edge(
            Edge(
                source_id=caller.id,
                target_name="fn1",
                target_id=tgt1,
                relationship="calls",
                confidence=1.0,
            )
        )
        edge_id2 = db.insert_edge(
            Edge(
                source_id=caller.id,
                target_name="fn2",
                target_id=tgt2,
                relationship="calls",
                confidence=1.0,
            )
        )
        db.commit()

        db.remove_file("a.js")
        db.commit()

        unresolved = db.get_unresolved_edges()
        unresolved_ids = {e.id for e in unresolved}
        assert edge_id1 in unresolved_ids
        assert edge_id2 in unresolved_ids


# ---------------------------------------------------------------------------
# Phase 1 — ParsedEdge model validation
# ---------------------------------------------------------------------------


class TestParsedEdgeModel:
    """Tests for the ParsedEdge NamedTuple introduced in Phase 1."""

    def test_parsed_edge_immutable(self) -> None:
        """ParsedEdge is a NamedTuple (immutable)."""
        pe = ParsedEdge(source_name="foo", target_name="bar", relationship="calls")
        with pytest.raises((AttributeError, TypeError)):
            pe.source_name = "changed"  # type: ignore[misc]

    def test_parsed_edge_optional_target_file(self) -> None:
        """target_file defaults to None."""
        pe = ParsedEdge(source_name="a", target_name="b", relationship="calls")
        assert pe.target_file is None

    def test_edge_dataclass_source_id_required(self) -> None:
        """Edge requires source_id — missing it must raise TypeError."""
        with pytest.raises(TypeError):
            Edge(target_name="foo", relationship="calls")  # type: ignore[call-arg]

    def test_edge_dataclass_target_id_defaults_none(self) -> None:
        """Edge target_id defaults to None (unresolved)."""
        edge = Edge(source_id=1, target_name="foo", relationship="calls")
        assert edge.target_id is None

    def test_edge_dataclass_confidence_defaults_zero(self) -> None:
        """Edge confidence defaults to 0.0."""
        edge = Edge(source_id=1, target_name="foo", relationship="calls")
        assert edge.confidence == 0.0


# ---------------------------------------------------------------------------
# Phase 3 — Full Call Expressions: parser adversarial tests
# ---------------------------------------------------------------------------


class TestFullCallExpressionEdgeCases:
    """Adversarial tests for Phase 3 — full dotted expression storage."""

    def _parse(self, code: str, filename: str = "test.js") -> tuple[list, list]:
        return parse_file(Path(filename), source=code.encode())

    def test_deeply_nested_chain_stored(self) -> None:
        """A deeply nested chain like a.b.c.d.e() is stored verbatim."""
        code = "function run() { a.b.c.d.e(); }"
        _, edges = self._parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert any(e.target_name == "a.b.c.d.e" for e in calls)

    def test_console_warn_still_filtered(self) -> None:
        """console.warn() is filtered by the console. prefix check."""
        code = "function f() { console.warn('x'); }"
        _, edges = self._parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert len(calls) == 0

    def test_console_error_still_filtered(self) -> None:
        """console.error() is filtered."""
        code = "function f() { console.error('x'); }"
        _, edges = self._parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert len(calls) == 0

    def test_non_console_dotted_not_filtered(self) -> None:
        """logger.error() is NOT filtered (not console.*)."""
        code = "function f() { logger.error('x'); }"
        _, edges = self._parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert any(e.target_name == "logger.error" for e in calls)

    def test_this_method_call_stored(self) -> None:
        """this.validate() stores 'this.validate' as target_name."""
        code = "function handler() { this.validate(); }"
        _, edges = self._parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert any(e.target_name == "this.validate" for e in calls)

    def test_source_name_preserved_in_parsed_edge(self) -> None:
        """ParsedEdge source_name is the function that contains the call."""
        code = "function processItems() { db.save(); }"
        _, edges = self._parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert len(calls) == 1
        assert calls[0].source_name == "processItems"
        assert calls[0].target_name == "db.save"

    def test_multiple_dotted_calls_in_one_function(self) -> None:
        """Multiple dotted method calls all produce separate edges with full expression."""
        code = "function setup() {\n  db.connect();\n  cache.init();\n  fs.mkdirSync('/tmp');\n}"
        _, edges = self._parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        target_names = {e.target_name for e in calls}
        assert "db.connect" in target_names
        assert "cache.init" in target_names
        assert "fs.mkdirSync" in target_names

    def test_no_target_name_stripping(self) -> None:
        """Verify old behavior (stripping to last segment) is GONE.
        db.query() must produce 'db.query', NOT 'query'."""
        code = "function run() { db.query('SELECT 1'); }"
        _, edges = self._parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert len(calls) == 1
        assert calls[0].target_name != "query", "Old callee.split('.')[-1] behavior must be removed"
        assert calls[0].target_name == "db.query"

    def test_self_call_exact_name_guard(self) -> None:
        """foo() inside foo() is filtered because callee == caller_name exactly."""
        code = "function foo() { foo(); }"
        _, edges = self._parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert len(calls) == 0

    def test_dotted_self_reference_not_filtered(self) -> None:
        """Foo.init() inside Foo() is NOT filtered — callee != caller_name.
        This is correct: a static method call on the class is not self-recursion."""
        code = "function Foo() { Foo.init(); }"
        _, edges = self._parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert any(e.target_name == "Foo.init" for e in calls)


# ---------------------------------------------------------------------------
# Phase 2 — Two-Phase Indexing: resolution strategy adversarial tests
# ---------------------------------------------------------------------------


class TestResolutionStrategyEdgeCases:
    """Adversarial tests for _resolve_single_edge strategies."""

    def _make_pipeline(self, tmp_dir: Path, config: LoomConfig, db: LoomDB) -> IndexPipeline:
        return IndexPipeline(config, db, _mock_embedder())

    def test_strategy4b_qualified_name_resolves_simple_call(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Strategy 4b: compile() call resolves to Compiler.compile if only LIKE match."""
        make_js_file(tmp_dir, "compiler.js", "class Compiler {\n  compile() {}\n}")
        make_js_file(tmp_dir, "runner.js", "function run() { compile(); }")

        pipeline = self._make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        run_sym = db.get_symbol_by_name("run")
        assert len(run_sym) == 1
        assert run_sym[0].id is not None

        edges = db.get_edges_from(run_sym[0].id)
        call_edges = [e for e in edges if e.relationship == "calls"]
        resolved = [e for e in call_edges if e.target_id is not None]
        assert len(resolved) >= 1
        assert resolved[0].confidence == pytest.approx(0.8)

    def test_strategy_1_exact_file_match(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Strategy 1: exact file+name match gives confidence 1.0."""
        src = _make_sym(db, "caller", "caller.js")
        tgt = _make_sym(db, "uniqueFn", "target.js")
        assert src.id is not None
        assert tgt.id is not None

        db.insert_edge(
            Edge(
                source_id=src.id,
                target_name="uniqueFn",
                target_file="target.js",
                relationship="calls",
                confidence=0.0,
            )
        )
        db.commit()

        pipeline = self._make_pipeline(tmp_dir, config, db)
        resolved_count = pipeline._resolve_all_edges()

        assert resolved_count >= 1
        edges = db.get_edges_from(src.id)
        call_edges = [e for e in edges if e.relationship == "calls"]
        resolved = [e for e in call_edges if e.target_id == tgt.id]
        assert len(resolved) == 1
        assert resolved[0].confidence == pytest.approx(1.0)

    def test_strategy_5_fails_when_ambiguous(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Strategy 5 must NOT resolve when multiple symbols with same name exist."""
        make_js_file(tmp_dir, "a.js", "function run() { create(); }")
        make_js_file(tmp_dir, "b.js", "function create() { return 1; }")
        make_js_file(tmp_dir, "c.js", "function create() { return 2; }")

        pipeline = self._make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        run_sym = db.get_symbol_by_name("run")
        assert len(run_sym) == 1
        edges = db.get_edges_from(run_sym[0].id)
        create_edges = [e for e in edges if e.target_name == "create"]
        unresolved = [e for e in create_edges if e.target_id is None]
        assert len(unresolved) >= 1

    def test_aliased_import_resolution(self, tmp_dir: Path, config: LoomConfig, db: LoomDB) -> None:
        """FIX BUG-001: Aliased import resolution now works correctly.

        import { getProduct as fetchProduct } from './product.js'
        function processOrder() { fetchProduct(1); }

        Strategy 2 now stores the original exported name ('getProduct') in
        the edge's original_name field and falls back to it when the local alias
        ('fetchProduct') is not found in the target file.

        The call edge processOrder -> fetchProduct must resolve to getProduct at 0.95.
        """
        make_js_file(
            tmp_dir,
            "src/order.js",
            "import { getProduct as fetchProduct } from './product.js';\n"
            "function processOrder() { fetchProduct(1); }",
        )
        make_js_file(tmp_dir, "src/product.js", "export function getProduct(id) { return id; }")

        pipeline = self._make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        process_sym = db.get_symbol_by_name("processOrder")
        assert len(process_sym) == 1
        assert process_sym[0].id is not None

        edges = db.get_edges_from(process_sym[0].id)
        call_edges = [e for e in edges if e.relationship == "calls"]
        resolved = [e for e in call_edges if e.target_id is not None]
        # BUG-001 FIXED: aliased imports now resolve via original exported name
        assert len(resolved) >= 1, (
            "Aliased import should resolve: processOrder -> fetchProduct -> getProduct"
        )
        # Verify the resolved target is getProduct at 0.95 confidence
        assert resolved[0].confidence == 0.95
        get_product_sym = db.get_symbol_by_name("getProduct")
        assert len(get_product_sym) == 1
        assert resolved[0].target_id == get_product_sym[0].id

    def test_file_with_only_imports_no_symbols_skips_gracefully(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """A file with only imports and no declared symbols must not crash."""
        make_js_file(tmp_dir, "imports_only.js", "import { foo } from './foo.js';")
        make_js_file(tmp_dir, "foo.js", "export function foo() {}")

        pipeline = self._make_pipeline(tmp_dir, config, db)
        result = pipeline.full_index()
        assert result["indexed"] >= 1

    def test_empty_file_indexed_gracefully(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """A completely empty JS file must be indexed without crashing."""
        make_js_file(tmp_dir, "empty.js", "")

        pipeline = self._make_pipeline(tmp_dir, config, db)
        result = pipeline.full_index()
        assert result["indexed"] == 1
        assert result["symbols"] == 0

    def test_resolve_all_edges_empty_db(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """_resolve_all_edges on empty DB returns 0, no crash."""
        pipeline = self._make_pipeline(tmp_dir, config, db)
        result = pipeline._resolve_all_edges()
        assert result == 0

    def test_resolve_all_edges_already_resolved_are_skipped(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """After resolution, running _resolve_all_edges again is a no-op."""
        make_js_file(tmp_dir, "a.js", "function a() { b(); }")
        make_js_file(tmp_dir, "b.js", "function b() {}")

        pipeline = self._make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        count = pipeline._resolve_all_edges()
        assert count == 0

    def test_build_import_map_returns_dict(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """_build_import_map on any state returns a dict without crashing."""
        make_js_file(tmp_dir, "app.js", "import React from 'react';\nfunction App() {}")

        pipeline = self._make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        import_map = pipeline._build_import_map()
        assert isinstance(import_map, dict)

    def test_incremental_index_no_changed_files_returns_zeros(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """incremental_index with no changed and no deleted files returns all zeros."""
        pipeline = self._make_pipeline(tmp_dir, config, db)
        result = pipeline.incremental_index([])
        assert result["indexed"] == 0
        assert result["deleted"] == 0
        assert result["resolved"] == 0


# ---------------------------------------------------------------------------
# Phase 2 — Two-Phase: import edge source_id anchoring
# ---------------------------------------------------------------------------


class TestImportEdgeAnchoring:
    """Tests for the import edge file-anchor design."""

    def test_import_edge_uses_file_anchor_as_source(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Import edge source_id must point to a real symbol (the file anchor)."""
        make_js_file(
            tmp_dir,
            "main.js",
            "import { helper } from './util.js';\nfunction main() { helper(); }",
        )
        make_js_file(tmp_dir, "util.js", "export function helper() {}")

        pipeline = IndexPipeline(config, db, _mock_embedder())
        pipeline.full_index()

        import_edges = db.conn.execute(
            "SELECT source_id, target_name, target_file FROM edges WHERE relationship='imports'"
        ).fetchall()
        assert len(import_edges) >= 1
        for source_id, _target_name, _target_file in import_edges:
            sym = db.get_symbol_by_id(source_id)
            assert sym is not None, f"Import edge source_id={source_id} has no symbol"

    def test_import_edge_target_name_is_local_binding(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """For aliased imports, import edge target_name is the local binding name (for map key)
        and original_name carries the exported name from the target module.
        """
        make_js_file(
            tmp_dir,
            "a.js",
            "import { getProduct as fetchProduct } from './product.js';\n"
            "function run() { fetchProduct(1); }",
        )
        make_js_file(tmp_dir, "product.js", "export function getProduct(id) {}")

        pipeline = IndexPipeline(config, db, _mock_embedder())
        pipeline.full_index()

        import_map = pipeline._build_import_map()
        matching_keys = [(f, n) for (f, n) in import_map if n == "fetchProduct"]
        assert len(matching_keys) >= 1, (
            "Import map should have key for local binding 'fetchProduct'"
        )
        # Verify original_name is stored in the import map value tuple
        source_file, local_name = matching_keys[0]
        target_file, original_name = import_map[(source_file, local_name)]
        assert original_name == "getProduct", (
            "Import map value should carry original exported name 'getProduct'"
        )
        assert target_file is not None

    def test_import_edge_original_name_stored_in_db(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """For aliased imports, original_name column stores the exported name in DB."""
        make_js_file(
            tmp_dir,
            "order.js",
            "import { getProduct as fetchProduct } from './product.js';\n"
            "function run() { fetchProduct(1); }",
        )
        make_js_file(tmp_dir, "product.js", "export function getProduct(id) {}")

        pipeline = IndexPipeline(config, db, _mock_embedder())
        pipeline.full_index()

        row = db.conn.execute(
            "SELECT target_name, original_name FROM edges WHERE relationship = 'imports'",
        ).fetchone()
        assert row is not None
        target_name, original_name = row
        assert target_name == "fetchProduct", "target_name should be the local binding alias"
        assert original_name == "getProduct", "original_name should be the exported name"

    def test_non_aliased_import_original_name_is_null(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """For non-aliased imports, original_name is NULL (no alias present)."""
        make_js_file(
            tmp_dir,
            "main.js",
            "import { helper } from './util.js';\nfunction run() { helper(); }",
        )
        make_js_file(tmp_dir, "util.js", "export function helper() {}")

        pipeline = IndexPipeline(config, db, _mock_embedder())
        pipeline.full_index()

        row = db.conn.execute(
            "SELECT target_name, original_name FROM edges WHERE relationship = 'imports'",
        ).fetchone()
        assert row is not None
        target_name, original_name = row
        assert target_name == "helper"
        assert original_name is None, "Non-aliased imports should have NULL original_name"

    def test_circular_import_does_not_crash(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Circular imports (A imports B, B imports A) must not cause infinite loops."""
        make_js_file(
            tmp_dir,
            "a.js",
            "import { funcB } from './b.js';\nfunction funcA() { funcB(); }",
        )
        make_js_file(
            tmp_dir,
            "b.js",
            "import { funcA } from './a.js';\nfunction funcB() { funcA(); }",
        )

        pipeline = IndexPipeline(config, db, _mock_embedder())
        result = pipeline.full_index()
        assert result["indexed"] == 2


# ---------------------------------------------------------------------------
# Phase 1 — Engine: unresolved edge handling in impact() and related()
# ---------------------------------------------------------------------------


class TestEngineUnresolvedEdgeHandling:
    """Adversarial tests for search engine with unresolved edges."""

    @pytest.fixture
    def mock_embedder(self) -> MagicMock:
        e = MagicMock()
        e.embed_single.return_value = [-1.0] * 768
        e.build_symbol_text.return_value = "fn\ncode"
        return e

    def test_related_with_all_unresolved_outgoing_no_crash(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """related() must not crash when all outgoing edges are unresolved."""
        from loom.search.engine import SearchEngine

        sym = _make_sym(db, "myFunc", "f.js")
        assert sym.id is not None

        for i in range(5):
            db.insert_edge(
                Edge(
                    source_id=sym.id,
                    target_name=f"unknownTarget{i}",
                    target_id=None,
                    relationship="calls",
                )
            )
        db.commit()

        engine = SearchEngine(db, mock_embedder)
        coupled = engine.related("myFunc")
        assert isinstance(coupled, list)

    def test_impact_nonexistent_symbol_returns_empty(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """impact() on a symbol that does not exist returns []."""
        from loom.search.engine import SearchEngine

        engine = SearchEngine(db, mock_embedder)
        result = engine.impact("totallyNonExistent_xyzzy")
        assert result == []

    def test_impact_unresolved_edge_source_generic_filtered(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """impact() must filter unresolved incoming edges where source is generic."""
        from loom.search.engine import SearchEngine

        target = _make_sym(db, "targetFn", "target.js")
        generic_caller = _make_sym(db, "callback", "generic.js")
        real_caller = _make_sym(db, "realCaller", "caller.js")
        assert target.id is not None
        assert generic_caller.id is not None
        assert real_caller.id is not None

        db.insert_edge(
            Edge(
                source_id=generic_caller.id,
                target_name="targetFn",
                target_id=None,
                relationship="calls",
            )
        )
        db.insert_edge(
            Edge(
                source_id=real_caller.id,
                target_name="targetFn",
                target_id=None,
                relationship="calls",
            )
        )
        db.commit()

        engine = SearchEngine(db, mock_embedder)
        dependents = engine.impact("targetFn")
        dep_names = {d.symbol.name for d in dependents}
        assert "callback" not in dep_names
        assert "realCaller" in dep_names


# ---------------------------------------------------------------------------
# Data integrity: reconnect drops and recreates edges table
# ---------------------------------------------------------------------------


class TestSchemaDropRecreate:
    """Verify DROP TABLE IF EXISTS edges behavior on reconnect."""

    def test_reconnect_drops_edges_table(self, config: LoomConfig) -> None:
        """On reconnect, the edges table is dropped and recreated."""
        db = LoomDB(config)
        db.connect()

        sym_id = db.insert_symbol(
            Symbol(
                name="fn1",
                kind="function",
                file="a.js",
                line=1,
                end_line=5,
                language="javascript",
            )
        )
        sym_id2 = db.insert_symbol(
            Symbol(
                name="fn2",
                kind="function",
                file="b.js",
                line=1,
                end_line=5,
                language="javascript",
            )
        )
        db.insert_edge(
            Edge(source_id=sym_id, target_name="fn2", target_id=sym_id2, relationship="calls")
        )
        db.commit()

        edges_before = db.conn.execute("SELECT COUNT(*) FROM edges").fetchone()[0]
        assert edges_before == 1

        db.close()
        db.connect()

        edges_after = db.conn.execute("SELECT COUNT(*) FROM edges").fetchone()[0]
        assert edges_after == 0, (
            "Edges table should be empty after reconnect (DROP TABLE IF EXISTS)"
        )

        syms_after = db.conn.execute("SELECT COUNT(*) FROM symbols").fetchone()[0]
        assert syms_after == 2, "Symbols table must persist across reconnects"

        db.close()


# ---------------------------------------------------------------------------
# Boundary: empty target_name edge
# ---------------------------------------------------------------------------


class TestEmptyTargetName:
    """Test edge behavior with empty target_name."""

    def test_empty_target_name_stored_and_retrieved(self, db: LoomDB) -> None:
        """An edge with empty target_name must be storable (DB doesn't reject it)."""
        src = _make_sym(db, "src", "src.js")
        assert src.id is not None
        edge_id = db.insert_edge(Edge(source_id=src.id, target_name="", relationship="calls"))
        db.commit()
        assert edge_id > 0

        unresolved = db.get_unresolved_edges()
        assert any(e.target_name == "" for e in unresolved)

    def test_get_edges_to_by_name_empty_string(self, db: LoomDB) -> None:
        """get_edges_to_by_name('') must return edges with empty target_name."""
        src = _make_sym(db, "src", "src.js")
        assert src.id is not None
        db.insert_edge(Edge(source_id=src.id, target_name="", relationship="calls"))
        db.commit()

        results = db.get_edges_to_by_name("")
        assert len(results) >= 1


# ---------------------------------------------------------------------------
# Phase 2 — Resolution: confidence correctness
# ---------------------------------------------------------------------------


class TestResolutionConfidence:
    """Verify confidence values match spec across strategies."""

    def _make_pipeline(self, tmp_dir: Path, config: LoomConfig, db: LoomDB) -> IndexPipeline:
        return IndexPipeline(config, db, _mock_embedder())

    def test_import_resolved_confidence_095(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Import-resolved edge (strategy 2) must have confidence ~0.95."""
        make_js_file(
            tmp_dir,
            "src/main.js",
            "import { uniqueImported } from './lib.js';\nfunction main() { uniqueImported(); }",
        )
        make_js_file(tmp_dir, "src/lib.js", "export function uniqueImported() {}")

        pipeline = self._make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        main_sym = db.get_symbol_by_name("main")
        assert len(main_sym) == 1
        assert main_sym[0].id is not None

        edges = db.get_edges_from(main_sym[0].id)
        call_edges = [e for e in edges if e.relationship == "calls" and e.target_id is not None]
        assert len(call_edges) >= 1
        assert call_edges[0].confidence == pytest.approx(0.95)

    def test_unique_name_confidence_060(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Unique global name match (strategy 5) must have confidence ~0.6."""
        make_js_file(tmp_dir, "caller.js", "function runIt() { veryUniqueFunction9999(); }")
        make_js_file(tmp_dir, "target.js", "function veryUniqueFunction9999() {}")

        pipeline = self._make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        run_sym = db.get_symbol_by_name("runIt")
        assert len(run_sym) == 1
        assert run_sym[0].id is not None

        edges = db.get_edges_from(run_sym[0].id)
        call_edges = [e for e in edges if e.relationship == "calls" and e.target_id is not None]
        assert len(call_edges) >= 1
        assert call_edges[0].confidence == pytest.approx(0.6)

    def test_import_confidence_exceeds_unique_name_confidence(self) -> None:
        """Import-resolved (0.95) must always exceed unique-name (0.6). Spec invariant."""
        assert 0.95 > 0.6

    def test_exact_file_match_confidence_100(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Manually injected edge with target_file set resolves at confidence 1.0."""
        src = _make_sym(db, "caller", "caller.js")
        tgt = _make_sym(db, "myTarget", "target.js")
        assert src.id is not None
        assert tgt.id is not None

        db.insert_edge(
            Edge(
                source_id=src.id,
                target_name="myTarget",
                target_file="target.js",
                relationship="calls",
                confidence=0.0,
            )
        )
        db.commit()

        pipeline = self._make_pipeline(tmp_dir, config, db)
        pipeline._resolve_all_edges()

        edges = db.get_edges_from(src.id)
        call_edges = [e for e in edges if e.target_id == tgt.id]
        assert len(call_edges) == 1
        assert call_edges[0].confidence == pytest.approx(1.0)
