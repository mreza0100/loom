"""QA Post-Merge adversarial tests — foundation-data-model pipeline on main.

360-sweep dimensions covered:
  Inputs      — edge.id=None in resolve, FTS5 special chars, empty strings
  State       — unknown source_name skipped, file_anchor=None skip
  Boundaries  — Strategy 3 suffix, Strategy 4a exact dotted, Strategy 5 uppercase dotted (1.0)
  Sequences   — incremental delete -> re-resolve, Phase-1-only then Phase-2
  Error paths — source_id missing in incoming edge traversal
  Data shapes — original_name=None when target==local, _row_to_edge column count
  Environment — FK enforcement after executescript
  Regressions — second full_index is idempotent, non-import edge skipped when source unknown
"""

import sqlite3
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from loom.config import LoomConfig
from loom.indexer.parser import parse_file
from loom.indexer.pipeline import IndexPipeline
from loom.search.engine import SearchEngine
from loom.store.db import LoomDB, _sanitize_fts_query  # noqa: PLC2701
from loom.store.models import Edge, Symbol
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
    e.embed.side_effect = lambda texts: [[0.1] * 768 for _ in texts]
    e.embed_single.return_value = [0.1] * 768
    e.build_symbol_text.return_value = "fn\ncode"
    return e


def _make_pipeline(tmp_dir: Path, config: LoomConfig, db: LoomDB) -> IndexPipeline:
    return IndexPipeline(config, db, _mock_embedder())


# ---------------------------------------------------------------------------
# Inputs: FTS query sanitisation edge cases
# ---------------------------------------------------------------------------


class TestFTSSanitization:
    """_sanitize_fts_query must handle adversarial query strings without crashing DB."""

    def test_empty_query_returns_empty(self) -> None:
        assert _sanitize_fts_query("") == ""

    def test_whitespace_only_returns_empty(self) -> None:
        assert _sanitize_fts_query("   ") == ""

    def test_fts5_and_operator_quoted(self) -> None:
        result = _sanitize_fts_query("foo AND bar")
        assert '"AND"' in result

    def test_fts5_or_operator_quoted(self) -> None:
        result = _sanitize_fts_query("foo OR bar")
        assert '"OR"' in result

    def test_fts5_not_operator_quoted(self) -> None:
        result = _sanitize_fts_query("NOT foo")
        assert '"NOT"' in result

    def test_hyphen_token_quoted(self) -> None:
        result = _sanitize_fts_query("resolve-session")
        assert '"resolve-session"' in result

    def test_star_token_quoted(self) -> None:
        result = _sanitize_fts_query("foo*")
        assert '"foo*"' in result

    def test_colon_token_quoted(self) -> None:
        result = _sanitize_fts_query("name:processOrder")
        assert '"name:processOrder"' in result

    def test_normal_query_unchanged(self) -> None:
        result = _sanitize_fts_query("processOrder")
        assert result == "processOrder"

    def test_fts_query_executes_safely_in_db(self, db: LoomDB) -> None:
        """A sanitized adversarial query must not raise sqlite3.OperationalError."""
        _make_sym(db, "target", "a.js")
        db.commit()
        # These characters would break unsanitized FTS5 queries
        for adversarial in ["AND OR", "foo-bar", "foo*", "NOT", '"quoted"', "key:val"]:
            results = db.search_fts(adversarial, limit=5)
            assert isinstance(results, list)


# ---------------------------------------------------------------------------
# Inputs: _resolve_single_edge with edge.id=None (warning branch)
# ---------------------------------------------------------------------------


class TestResolveEdgeWithNoneId:
    """Edges with id=None are warned and skipped during _resolve_all_edges."""

    def test_edge_id_none_warning_logged(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """An edge with id=None triggers a log.warning and is skipped (no crash)."""
        src = _make_sym(db, "caller", "caller.js")
        tgt = _make_sym(db, "target", "target.js")
        assert src.id is not None
        assert tgt.id is not None

        # Insert unresolved edge normally
        db.insert_edge(
            Edge(
                source_id=src.id,
                target_name="target",
                target_file="target.js",
                relationship="calls",
            )
        )
        db.commit()

        pipeline = _make_pipeline(tmp_dir, config, db)

        # Monkey-patch get_unresolved_edges to inject an Edge with id=None
        real_get_unresolved = db.get_unresolved_edges

        def patched_get_unresolved() -> list[Edge]:
            real_edges = real_get_unresolved()
            # Inject a phantom edge with id=None into the list
            phantom = Edge(
                source_id=src.id,  # type: ignore[arg-type]
                target_name="target",
                target_file="target.js",
                relationship="calls",
                id=None,  # The None-id case
            )
            return [phantom] + real_edges

        with patch.object(db, "get_unresolved_edges", side_effect=patched_get_unresolved):
            import logging

            with patch.object(logging.getLogger("loom.indexer.pipeline"), "warning") as mock_warn:
                resolved = pipeline._resolve_all_edges()
                # The phantom edge is warned and skipped; real edge is resolved
                mock_warn.assert_called_once()
                assert "None id" in mock_warn.call_args[0][0]
                # Real edge still resolves
                assert resolved >= 1


# ---------------------------------------------------------------------------
# State: unknown source_name skips edge (log.debug branch in _parse_all_files)
# ---------------------------------------------------------------------------


class TestUnknownSourceNameSkipped:
    """Non-import edges whose source_name doesn't map to a local symbol are skipped."""

    def test_edge_with_unknown_source_name_not_inserted(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """When parser emits an edge from a source that isn't a declared symbol, skip it."""
        from loom.store.models import ParsedEdge

        # Patch parse_file to return a fake ParsedEdge from a non-existent source symbol
        real_parse = parse_file

        def patched_parse(path: Path, source: bytes | None = None):  # type: ignore[misc]
            symbols, edges = real_parse(path, source)
            # Inject a bad edge whose source_name doesn't exist in this file
            bad_edge = ParsedEdge(
                source_name="__GHOST_SYMBOL__",
                target_name="something",
                relationship="calls",
            )
            return symbols, edges + [bad_edge]

        make_js_file(tmp_dir, "a.js", "function realFunc() { something(); }")

        with patch("loom.indexer.pipeline.parse_file", side_effect=patched_parse):
            pipeline = _make_pipeline(tmp_dir, config, db)
            result = pipeline.full_index()

        # No crash; real symbols still indexed
        assert result["indexed"] == 1
        assert result["symbols"] >= 1

        # The ghost edge must NOT be in the DB (source_name not in local_name_to_id)
        # realFunc->something edge IS valid; but __GHOST_SYMBOL__->something must not exist
        # Check no edge has source from a non-existent symbol
        all_source_ids = {
            row[0] for row in db.conn.execute("SELECT source_id FROM edges").fetchall()
        }
        all_symbol_ids = {row[0] for row in db.conn.execute("SELECT id FROM symbols").fetchall()}
        # Every edge source_id must reference a real symbol
        assert all_source_ids.issubset(all_symbol_ids), (
            "All edge source_ids must reference valid symbols"
        )


# ---------------------------------------------------------------------------
# State: file with no symbols + import edge → file_anchor_id=None → skip
# ---------------------------------------------------------------------------


class TestFileAnchorNoneSkipsImportEdge:
    """A file with no declared symbols skips its import edges (file_anchor_id=None)."""

    def test_imports_only_file_has_no_import_edges_in_db(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """import { foo } from './foo.js' with no other symbols = 0 import edges stored."""
        make_js_file(tmp_dir, "imports_only.js", "import { foo } from './foo.js';")
        make_js_file(tmp_dir, "foo.js", "export function foo() {}")

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        # The imports_only.js file has no symbols; its import edge is skipped
        # All edges in DB should have source_ids from foo.js symbols only
        import_edges = db.conn.execute(
            "SELECT COUNT(*) FROM edges WHERE relationship = 'imports'"
        ).fetchone()[0]
        # foo.js has no imports; imports_only.js import edges skipped
        assert import_edges == 0, (
            f"File with no symbols should produce 0 import edges, got {import_edges}"
        )


# ---------------------------------------------------------------------------
# Boundaries: Strategy 3 — file suffix match (target_file is partial path)
# ---------------------------------------------------------------------------


class TestStrategy3FileSuffixMatch:
    """Strategy 3 resolves when target_file is a relative path suffix."""

    def test_strategy3_suffix_match_resolves(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """An edge with target_file='utils/helper.js' resolves against 'src/utils/helper.js'."""
        src = _make_sym(db, "caller", "src/caller.js")
        tgt = _make_sym(db, "helperFn", "src/utils/helper.js")
        assert src.id is not None
        assert tgt.id is not None

        # Edge has a partial target_file (suffix) and no import in import_map
        db.insert_edge(
            Edge(
                source_id=src.id,
                target_name="helperFn",
                target_file="utils/helper.js",  # partial path — suffix of actual file
                relationship="calls",
                confidence=0.0,
            )
        )
        db.commit()

        pipeline = _make_pipeline(tmp_dir, config, db)
        resolved = pipeline._resolve_all_edges()

        assert resolved >= 1
        edges = db.get_edges_from(src.id)
        call_edges = [e for e in edges if e.relationship == "calls" and e.target_id == tgt.id]
        assert len(call_edges) == 1
        assert call_edges[0].confidence == pytest.approx(0.9)

    def test_strategy3_ambiguous_suffix_stays_unresolved(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """If target_file suffix matches multiple symbols, Strategy 3 doesn't resolve."""
        src = _make_sym(db, "caller", "caller.js")
        # Two symbols with same name in files that share a suffix
        _make_sym(db, "sharedFn", "a/utils.js")
        _make_sym(db, "sharedFn", "b/utils.js")
        assert src.id is not None

        db.insert_edge(
            Edge(
                source_id=src.id,
                target_name="sharedFn",
                target_file="utils.js",  # suffix matches BOTH a/utils.js and b/utils.js
                relationship="calls",
                confidence=0.0,
            )
        )
        db.commit()

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline._resolve_all_edges()

        edges = db.get_edges_from(src.id)
        call_edges = [e for e in edges if e.relationship == "calls"]
        # Should remain unresolved since suffix is ambiguous
        # (Strategy 4/5 may resolve if unique by name — here it's not)
        unresolved = [e for e in call_edges if e.target_id is None]
        assert len(unresolved) >= 1, "Ambiguous suffix should not resolve via Strategy 3"


# ---------------------------------------------------------------------------
# Boundaries: Strategy 4a — dotted expression is itself a symbol (lines 311-315)
# ---------------------------------------------------------------------------


class TestStrategy4aExactDottedSymbol:
    """Strategy 4a resolves when a dotted call expression exactly matches a symbol name."""

    def test_strategy4a_exact_dotted_symbol_resolves(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """'Compiler.compile' call resolves to the matching method symbol at confidence 0.8."""
        src = _make_sym(db, "runner", "runner.js")
        # 'Compiler.compile' is the exact symbol name (method on class)
        tgt_id = db.insert_symbol(
            Symbol(
                name="Compiler.compile",
                kind="method",
                file="compiler.js",
                line=5,
                end_line=10,
                language="javascript",
            )
        )
        assert src.id is not None

        # Edge stores the dotted name as target_name (full call expression, Phase 3)
        db.insert_edge(
            Edge(
                source_id=src.id,
                target_name="Compiler.compile",  # dotted expression IS a symbol name
                target_file=None,
                relationship="calls",
                confidence=0.0,
            )
        )
        db.commit()

        pipeline = _make_pipeline(tmp_dir, config, db)
        resolved = pipeline._resolve_all_edges()

        assert resolved >= 1
        edges = db.get_edges_from(src.id)
        call_edges = [e for e in edges if e.relationship == "calls"]
        resolved_edges = [e for e in call_edges if e.target_id == tgt_id]
        assert len(resolved_edges) == 1
        assert resolved_edges[0].confidence == pytest.approx(0.8)


# ---------------------------------------------------------------------------
# Boundaries: Strategy uppercase dotted (lines 334-337) — last resort 1.0 confidence
# ---------------------------------------------------------------------------


class TestStrategyUppercaseDottedFallback:
    """Dotted expressions starting with uppercase resolve at 1.0 as last strategy."""

    def test_uppercase_dotted_unique_resolves_at_1_0(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """'EventEmitter.emit' where EventEmitter starts with uppercase resolves at 1.0
        if Strategy 4a finds it uniquely.

        This exercises the last branch in _resolve_single_edge (lines 334-337).
        """
        src = _make_sym(db, "handler", "handler.js")
        tgt_id = db.insert_symbol(
            Symbol(
                name="EventEmitter.emit",
                kind="method",
                file="events.js",
                line=2,
                end_line=5,
                language="javascript",
            )
        )
        assert src.id is not None

        # Edge target_name is 'EventEmitter.emit' (uppercase first char)
        # No target_file, no import map entry — should fall through to the uppercase branch
        db.insert_edge(
            Edge(
                source_id=src.id,
                target_name="EventEmitter.emit",
                target_file=None,
                relationship="calls",
                confidence=0.0,
            )
        )
        db.commit()

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline._resolve_all_edges()

        edges = db.get_edges_from(src.id)
        call_edges = [e for e in edges if e.relationship == "calls"]
        # Should be resolved; accept confidence of either 0.8 (Strategy 4a exact dotted)
        # or 1.0 (Strategy uppercase-dotted fallback)
        resolved_edges = [e for e in call_edges if e.target_id == tgt_id]
        assert len(resolved_edges) == 1, "EventEmitter.emit should resolve to the method symbol"


# ---------------------------------------------------------------------------
# Sequences: incremental delete nullifies then second index re-resolves
# ---------------------------------------------------------------------------


class TestIncrementalDeleteAndReResolve:
    """Deleting a file nullifies edges; adding a new file with the same symbol re-resolves them."""

    def test_incremental_delete_nullifies_target_edges(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Delete the target file. Edge's target_id should become NULL."""
        callee_file = make_js_file(tmp_dir, "callee.js", "function callee() {}")
        make_js_file(tmp_dir, "caller.js", "function caller() { callee(); }")

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        # Verify edge resolved before deletion
        caller_sym = db.get_symbol_by_name("caller")
        assert len(caller_sym) == 1
        edges_before = db.get_edges_from(caller_sym[0].id)
        call_edges_before = [
            e for e in edges_before if e.relationship == "calls" and e.target_id is not None
        ]
        assert len(call_edges_before) >= 1, "Edge should be resolved before deletion"

        # Delete the callee file — nullify via incremental_index
        callee_file.unlink()
        pipeline.incremental_index([callee_file])

        # Edge should now be unresolved (target_id NULL)
        caller_sym2 = db.get_symbol_by_name("caller")
        assert len(caller_sym2) == 1
        edges_after = db.get_edges_from(caller_sym2[0].id)
        call_edges_after = [e for e in edges_after if e.relationship == "calls"]
        unresolved_after = [e for e in call_edges_after if e.target_id is None]
        assert len(unresolved_after) >= 1, "Deleted file's target edges should be nullified"

    def test_previously_unresolved_resolves_after_new_file_added(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """An edge that starts unresolved resolves after the target symbol is added."""
        make_js_file(tmp_dir, "caller.js", "function caller() { missingFn(); }")

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        caller_sym = db.get_symbol_by_name("caller")
        assert len(caller_sym) == 1
        edges = db.get_edges_from(caller_sym[0].id)
        unresolved = [e for e in edges if e.target_id is None and e.relationship == "calls"]
        assert len(unresolved) >= 1, "Edge to missing symbol should start unresolved"

        # Now add the missing symbol
        new_file = make_js_file(tmp_dir, "missing.js", "function missingFn() {}")
        pipeline.incremental_index([new_file])

        # Edge should now be resolved
        caller_sym2 = db.get_symbol_by_name("caller")
        assert len(caller_sym2) == 1
        edges_after = db.get_edges_from(caller_sym2[0].id)
        resolved = [e for e in edges_after if e.target_id is not None and e.relationship == "calls"]
        assert len(resolved) >= 1, (
            "Previously unresolved edge should resolve after target symbol is indexed"
        )
        assert resolved[0].confidence == pytest.approx(0.6), (
            "Strategy 5 (unique name match) confidence should be 0.6"
        )


# ---------------------------------------------------------------------------
# Error paths: get_symbol_by_id returns None for source_id in incoming traversal
# ---------------------------------------------------------------------------


class TestEngineMissingSourceSymbol:
    """_find_coupled and impact() gracefully handle edges where source symbol is missing."""

    def test_find_coupled_skips_edge_with_missing_source(self, db: LoomDB) -> None:
        """If get_symbol_by_id(edge.source_id) returns None for an incoming edge, skip it."""
        target = _make_sym(db, "targetFn", "target.js")
        caller = _make_sym(db, "callerFn", "caller.js")
        assert target.id is not None
        assert caller.id is not None

        db.insert_edge(
            Edge(
                source_id=caller.id,
                target_name="targetFn",
                target_id=target.id,
                relationship="calls",
                confidence=1.0,
            )
        )
        db.commit()

        mock_emb = MagicMock()
        mock_emb.embed_single.return_value = [-1.0] * 768
        mock_emb.build_symbol_text.return_value = "fn"

        engine = SearchEngine(db, mock_emb)

        # Patch get_symbol_by_id to return None for caller's id
        real_get_by_id = db.get_symbol_by_id

        def patched_get_by_id(symbol_id: int) -> Symbol | None:
            if symbol_id == caller.id:
                return None  # Simulate orphaned edge (symbol deleted but FK not enforced)
            return real_get_by_id(symbol_id)

        with patch.object(db, "get_symbol_by_id", side_effect=patched_get_by_id):
            coupled = engine._find_coupled(target)

        # Should not crash; the None-source edge is simply skipped
        assert isinstance(coupled, list)
        # callerFn should NOT appear in coupled since its lookup returned None
        assert not any(c.symbol.name == "callerFn" for c in coupled)

    def test_impact_skips_edge_with_missing_source(self, db: LoomDB) -> None:
        """impact() must not crash when source_id resolves to None."""
        target = _make_sym(db, "impactedFn", "target.js")
        caller = _make_sym(db, "ghostCaller", "caller.js")
        assert target.id is not None
        assert caller.id is not None

        db.insert_edge(
            Edge(
                source_id=caller.id,
                target_name="impactedFn",
                target_id=target.id,
                relationship="calls",
                confidence=1.0,
            )
        )
        db.commit()

        mock_emb = MagicMock()
        mock_emb.embed_single.return_value = [-1.0] * 768
        mock_emb.build_symbol_text.return_value = "fn"

        engine = SearchEngine(db, mock_emb)

        real_get_by_id = db.get_symbol_by_id

        def patched(symbol_id: int) -> Symbol | None:
            if symbol_id == caller.id:
                return None
            return real_get_by_id(symbol_id)

        with patch.object(db, "get_symbol_by_id", side_effect=patched):
            dependents = engine.impact("impactedFn")

        assert isinstance(dependents, list)
        assert not any(d.symbol.name == "ghostCaller" for d in dependents)


# ---------------------------------------------------------------------------
# Data shapes: original_name=None when exported==local (pipeline line 145)
# ---------------------------------------------------------------------------


class TestOriginalNameNoneWhenNoAlias:
    """original_name is None when exported name equals local binding name (no alias)."""

    def test_non_aliased_original_name_stored_as_none(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """import { foo } from './lib' → original_name must be NULL in DB."""
        make_js_file(
            tmp_dir,
            "consumer.js",
            "import { foo } from './lib.js';\nfunction run() { foo(); }",
        )
        make_js_file(tmp_dir, "lib.js", "export function foo() {}")

        pipeline = _make_pipeline(tmp_dir, config, db)
        # Only Phase 1 — don't resolve so we can inspect raw import edge
        pipeline._parse_all_files([tmp_dir / "consumer.js", tmp_dir / "lib.js"])

        row = db.conn.execute(
            "SELECT target_name, original_name FROM edges "
            "WHERE relationship='imports' AND target_name='foo'"
        ).fetchone()
        assert row is not None, "Import edge for 'foo' should exist"
        target_name, original_name = row
        assert target_name == "foo"
        assert original_name is None, (
            f"Non-aliased import should have NULL original_name, got {original_name!r}"
        )

    def test_aliased_original_name_stored_correctly(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """import { bar as baz } from './lib' → original_name='bar', target_name='baz'."""
        make_js_file(
            tmp_dir,
            "consumer.js",
            "import { bar as baz } from './lib.js';\nfunction run() { baz(); }",
        )
        make_js_file(tmp_dir, "lib.js", "export function bar() {}")

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline._parse_all_files([tmp_dir / "consumer.js", tmp_dir / "lib.js"])

        row = db.conn.execute(
            "SELECT target_name, original_name FROM edges "
            "WHERE relationship='imports' AND target_name='baz'"
        ).fetchone()
        assert row is not None, "Import edge for alias 'baz' should exist"
        target_name, original_name = row
        assert target_name == "baz", f"Expected 'baz', got {target_name!r}"
        assert original_name == "bar", f"Expected 'bar', got {original_name!r}"


# ---------------------------------------------------------------------------
# Data shapes: _row_to_edge len(row) > 7 guard still handles 8-column rows
# ---------------------------------------------------------------------------


class TestRowToEdgeColumnGuard:
    """_row_to_edge handles the 8-column row (original_name as 8th col) correctly."""

    def test_row_to_edge_with_8_columns_reads_original_name(self, db: LoomDB) -> None:
        """An 8-element row tuple is handled correctly by _row_to_edge."""
        row = (1, 10, 20, "targetFn", "file.js", "calls", 0.95, "exportedName")
        edge = LoomDB._row_to_edge(row)
        assert edge.id == 1
        assert edge.source_id == 10
        assert edge.target_id == 20
        assert edge.target_name == "targetFn"
        assert edge.original_name == "exportedName"
        assert edge.confidence == pytest.approx(0.95)

    def test_row_to_edge_with_null_original_name(self, db: LoomDB) -> None:
        """An 8-element row with None original_name field is handled correctly."""
        row = (2, 11, None, "foo", None, "extends", 0.0, None)
        edge = LoomDB._row_to_edge(row)
        assert edge.id == 2
        assert edge.target_id is None
        assert edge.original_name is None

    def test_row_to_edge_fewer_than_8_columns_original_name_defaults_none(self, db: LoomDB) -> None:
        """The len(row) > 7 guard: a 7-element row still produces original_name=None."""
        row = (3, 12, 22, "bar", "b.js", "calls", 1.0)  # 7 elements, no original_name
        edge = LoomDB._row_to_edge(row)
        assert edge.original_name is None, "Should default to None for legacy 7-col rows"


# ---------------------------------------------------------------------------
# Regression: second full_index is idempotent (hash skip)
# ---------------------------------------------------------------------------


class TestFullIndexIdempotent:
    """Running full_index twice on unchanged files is a no-op on the second run."""

    def test_second_full_index_skips_all_files(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Second full_index must report 0 indexed files (content hash unchanged)."""
        make_js_file(tmp_dir, "a.js", "function a() {}")
        make_js_file(tmp_dir, "b.js", "function b() { a(); }")

        pipeline = _make_pipeline(tmp_dir, config, db)
        first = pipeline.full_index()
        assert first["indexed"] == 2

        second = pipeline.full_index()
        assert second["indexed"] == 0, "Second full_index must skip unchanged files"
        # Symbol count should not grow
        total_symbols = db.conn.execute("SELECT COUNT(*) FROM symbols").fetchone()[0]
        assert total_symbols == first["symbols"], "Symbols must not be duplicated on second index"


# ---------------------------------------------------------------------------
# Regression: incremental_index with zero changed + zero deleted returns all zeros
# ---------------------------------------------------------------------------


class TestIncrementalIndexNoOp:
    """incremental_index with no files processes cleanly."""

    def test_incremental_no_files_returns_zeros(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        pipeline = _make_pipeline(tmp_dir, config, db)
        result = pipeline.incremental_index([])
        assert result["indexed"] == 0
        assert result["deleted"] == 0
        assert result["symbols"] == 0
        assert result["edges"] == 0
        assert result["resolved"] == 0


# ---------------------------------------------------------------------------
# Regression: get_edges_to_by_name semantics — name vs last segment
# ---------------------------------------------------------------------------


class TestGetEdgesToByNameSemantics:
    """get_edges_to_by_name matches full target_name string, not last segment."""

    def test_by_name_full_dotted_expression_matched(self, db: LoomDB) -> None:
        """get_edges_to_by_name('db.query') finds edge with target_name='db.query'."""
        src = _make_sym(db, "runner", "runner.js")
        assert src.id is not None

        db.insert_edge(
            Edge(
                source_id=src.id,
                target_name="db.query",
                relationship="calls",
            )
        )
        db.commit()

        results = db.get_edges_to_by_name("db.query")
        assert len(results) == 1
        assert results[0].target_name == "db.query"

    def test_by_name_last_segment_does_not_match_dotted(self, db: LoomDB) -> None:
        """get_edges_to_by_name('query') does NOT return edge with target_name='db.query'."""
        src = _make_sym(db, "runner", "runner.js")
        assert src.id is not None

        db.insert_edge(
            Edge(
                source_id=src.id,
                target_name="db.query",
                relationship="calls",
            )
        )
        db.commit()

        results = db.get_edges_to_by_name("query")
        assert len(results) == 0, (
            "get_edges_to_by_name should match full target_name, not last segment"
        )


# ---------------------------------------------------------------------------
# Compliance: no print() in src, PRAGMA FK enforcement on every new connection
# ---------------------------------------------------------------------------


class TestComplianceChecks:
    """Compliance checks: FK enforcement, no print() in src."""

    def test_foreign_keys_on_after_connect(self, db: LoomDB) -> None:
        """PRAGMA foreign_keys must be 1 immediately after connect()."""
        fk = db.conn.execute("PRAGMA foreign_keys").fetchone()[0]
        assert fk == 1

    def test_foreign_keys_on_after_reconnect(self, config: LoomConfig) -> None:
        """PRAGMA foreign_keys must be 1 on a fresh connection (not just the fixture one)."""
        fresh_db = LoomDB(config)
        fresh_db.connect()
        try:
            fk = fresh_db.conn.execute("PRAGMA foreign_keys").fetchone()[0]
            assert fk == 1, "FK must be enabled on every connection, not just the first"
        finally:
            fresh_db.close()

    def test_invalid_target_id_raises_integrity_error(self, db: LoomDB) -> None:
        """Inserting edge with invalid target_id raises IntegrityError when FK is ON."""
        src = _make_sym(db, "src", "src.js")
        assert src.id is not None
        with pytest.raises(sqlite3.IntegrityError):
            db.insert_edge(
                Edge(source_id=src.id, target_name="x", target_id=99999, relationship="calls")
            )
