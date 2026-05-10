"""Tests for loom.server — MCP tool definitions and formatting helpers."""

from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

import loom.server as server_module
from loom.server import (
    _format_coupled,
    _format_results,
    _format_symbol,
    impact,
    initialize,
    neighborhood,
    reindex,
    related,
    search,
    status,
)
from loom.store.models import CoupledSymbol, SearchResult, Symbol


def _make_symbol(
    name: str = "testFunc",
    kind: str = "function",
    file: str = "test.js",
    line: int = 1,
    end_line: int = 10,
    language: str = "javascript",
) -> Symbol:
    return Symbol(
        name=name,
        kind=kind,
        file=file,
        line=line,
        end_line=end_line,
        language=language,
        context="function testFunc() {}",
        id=1,
    )


class TestFormatSymbol:
    def test_format_symbol_fields(self) -> None:
        sym = _make_symbol()
        result = _format_symbol(sym)
        assert result["name"] == "testFunc"
        assert result["kind"] == "function"
        assert result["file"] == "test.js"
        assert result["line"] == 1
        assert result["end_line"] == 10
        assert result["language"] == "javascript"
        # context and id are NOT included
        assert "context" not in result
        assert "id" not in result


class TestFormatCoupled:
    def test_empty_list(self) -> None:
        assert _format_coupled([]) == []

    def test_coupled_fields(self) -> None:
        sym = _make_symbol()
        coupled = [CoupledSymbol(symbol=sym, score=0.75, reason="calls (structural)")]
        result = _format_coupled(coupled)
        assert len(result) == 1
        assert result[0]["score"] == pytest.approx(0.75, abs=0.001)
        assert result[0]["reason"] == "calls (structural)"
        assert result[0]["symbol"]["name"] == "testFunc"

    def test_score_rounded(self) -> None:
        sym = _make_symbol()
        coupled = [CoupledSymbol(symbol=sym, score=0.123456789, reason="test")]
        result = _format_coupled(coupled)
        assert result[0]["score"] == pytest.approx(0.123, abs=0.001)


class TestFormatResults:
    def test_empty_list(self) -> None:
        assert _format_results([]) == []

    def test_result_fields(self) -> None:
        sym = _make_symbol()
        result = SearchResult(symbol=sym, score=0.85, coupled=[])
        formatted = _format_results([result])
        assert len(formatted) == 1
        assert formatted[0]["symbol"]["name"] == "testFunc"
        assert formatted[0]["score"] == pytest.approx(0.85, abs=0.001)
        assert formatted[0]["coupled"] == []


class TestServerUninitialized:
    """All MCP tools return error when engine/db not initialized."""

    def setup_method(self) -> None:
        # Reset module globals
        server_module._engine = None
        server_module._db = None
        server_module._pipeline = None
        server_module._config = None
        server_module._embedder = None

    def test_search_returns_error(self) -> None:
        result = search("query")
        assert isinstance(result, list)
        assert result[0].get("error") is not None

    def test_related_returns_error(self) -> None:
        result = related("symbol")
        assert isinstance(result, list)
        assert result[0].get("error") is not None

    def test_impact_returns_error(self) -> None:
        result = impact("symbol")
        assert isinstance(result, list)
        assert result[0].get("error") is not None

    def test_neighborhood_returns_error(self) -> None:
        result = neighborhood("file.js", 1)
        assert isinstance(result, dict)
        assert "error" in result

    def test_reindex_returns_error(self) -> None:
        result = reindex()
        assert isinstance(result, dict)
        assert "error" in result

    def test_status_returns_error(self) -> None:
        result = status()
        assert isinstance(result, dict)
        assert "error" in result


class TestServerTools:
    """MCP tools with mocked engine/db."""

    def setup_method(self) -> None:

        server_module._engine = MagicMock()
        server_module._db = MagicMock()
        server_module._pipeline = MagicMock()
        server_module._config = MagicMock()
        server_module._embedder = MagicMock()

    def teardown_method(self) -> None:
        server_module._engine = None
        server_module._db = None
        server_module._pipeline = None
        server_module._config = None
        server_module._embedder = None

    def test_search_calls_engine(self) -> None:
        sym = _make_symbol()
        server_module._engine.search.return_value = [
            SearchResult(symbol=sym, score=0.9, coupled=[]),
        ]
        result = search("processOrder", limit=5)
        server_module._engine.search.assert_called_once_with("processOrder", limit=5, kind=None)
        assert len(result) == 1
        assert result[0]["symbol"]["name"] == "testFunc"

    def test_search_with_kind_filter(self) -> None:
        server_module._engine.search.return_value = []
        search("query", kind="function")
        server_module._engine.search.assert_called_once_with("query", limit=10, kind="function")

    def test_related_calls_engine(self) -> None:
        sym = _make_symbol()
        server_module._engine.related.return_value = [
            CoupledSymbol(symbol=sym, score=0.7, reason="calls (structural)"),
        ]
        result = related("processOrder", file="order.js", kind="function")
        server_module._engine.related.assert_called_once_with(
            "processOrder",
            file="order.js",
            kind="function",
        )
        assert len(result) == 1

    def test_impact_calls_engine(self) -> None:
        server_module._engine.impact.return_value = []
        result = impact("validateCart")
        server_module._engine.impact.assert_called_once_with(
            "validateCart",
            file=None,
            kind=None,
        )
        assert result == []

    def test_neighborhood_no_symbols(self) -> None:
        server_module._db.get_colocated_symbols.return_value = []
        result = neighborhood("missing.js", 1)
        assert "error" in result
        assert "missing.js" in result["error"]

    def test_neighborhood_with_anchor(self) -> None:
        sym = _make_symbol()
        server_module._db.get_colocated_symbols.return_value = [sym]
        server_module._engine.neighborhood.return_value = (sym, [])
        result = neighborhood("test.js", 5)
        assert "anchor" in result
        assert "coupled" in result
        assert result["anchor"]["name"] == "testFunc"

    def test_neighborhood_anchor_none(self) -> None:
        sym = _make_symbol()
        server_module._db.get_colocated_symbols.return_value = [sym]
        server_module._engine.neighborhood.return_value = (None, [])
        result = neighborhood("test.js", 999)
        assert result["anchor"] is None

    def test_reindex_calls_pipeline(self) -> None:
        server_module._pipeline.full_index.return_value = {
            "indexed": 3,
            "symbols": 10,
            "edges": 5,
        }
        result = reindex()
        assert result["status"] == "reindex complete"
        assert result["indexed"] == 3

    def test_status_returns_stats(self) -> None:
        server_module._db.get_stats.return_value = {
            "symbols": 100,
            "edges": 50,
            "files": 10,
            "vectors": 100,
            "last_indexed": "2025-01-01",
            "stale_files": 0,
        }
        server_module._config.target_dir = Path("/tmp/project")  # noqa: S108
        server_module._config.resolve_db_path.return_value = Path("/tmp/project/.loom.db")  # noqa: S108
        result = status()
        assert result["symbols"] == 100
        assert "target" in result
        assert "db_path" in result


class TestInitialize:
    def teardown_method(self) -> None:
        server_module._engine = None
        server_module._db = None
        server_module._pipeline = None
        server_module._config = None
        server_module._embedder = None

    def test_initialize_sets_globals(self, tmp_path: Path) -> None:
        with (
            patch("loom.server.LoomDB") as mock_db_cls,
            patch("loom.server.Embedder"),
            patch("loom.server.IndexPipeline") as mock_pipeline_cls,
            patch("loom.server.SearchEngine"),
            patch("loom.server.start_watcher"),
        ):
            mock_db = MagicMock()
            mock_db_cls.return_value = mock_db
            mock_pipeline = MagicMock()
            mock_pipeline.full_index.return_value = {"indexed": 0, "symbols": 0, "edges": 0}
            mock_pipeline_cls.return_value = mock_pipeline

            initialize(tmp_path)

            assert server_module._config is not None
            assert server_module._db is not None
            assert server_module._engine is not None
            assert server_module._pipeline is not None
            mock_db.connect.assert_called_once()
            mock_pipeline.full_index.assert_called_once()
