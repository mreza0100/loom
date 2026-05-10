"""Tests for loom.search.engine — hybrid search, RRF, coupling."""

from unittest.mock import MagicMock, patch

import pytest

from loom.search.engine import (
    KIND_BOOST,
    MAX_BOOST,
    MAX_STRUCTURAL_RESULTS,
    RRF_K,
    THEORETICAL_MAX_RRF,
    SearchEngine,
    _GENERIC_CALL_TARGETS,
    _normalize_scores,
    _rrf_score,
)
from loom.store.db import LoomDB
from loom.store.models import CoupledSymbol, Edge, SearchResult, Symbol


class TestRRFScore:
    def test_rank_zero(self) -> None:
        assert _rrf_score(0) == pytest.approx(1.0 / RRF_K)

    def test_rank_one(self) -> None:
        assert _rrf_score(1) == pytest.approx(1.0 / (RRF_K + 1))

    def test_monotonically_decreasing(self) -> None:
        scores = [_rrf_score(i) for i in range(10)]
        for i in range(len(scores) - 1):
            assert scores[i] > scores[i + 1]


class TestNormalizeScores:
    def test_empty_list(self) -> None:
        assert _normalize_scores([]) == []

    def test_single_element(self) -> None:
        result = _normalize_scores([(1, 0.5)])
        assert len(result) == 1
        assert result[0][0] == 1
        assert 0 < result[0][1] <= 1.0

    def test_scores_capped_at_one(self) -> None:
        result = _normalize_scores([(1, 100.0)])
        assert result[0][1] <= 1.0

    def test_zero_max_score(self) -> None:
        result = _normalize_scores([(1, 0.0), (2, 0.0)])
        assert result == [(1, 0.0), (2, 0.0)]

    def test_uses_theoretical_max(self) -> None:
        small_score = THEORETICAL_MAX_RRF / 10
        result = _normalize_scores([(1, small_score)])
        assert result[0][1] < 1.0


class TestTheoreticalMax:
    def test_theoretical_max_positive(self) -> None:
        assert THEORETICAL_MAX_RRF > 0

    def test_max_boost_correct(self) -> None:
        assert MAX_BOOST == max(KIND_BOOST.values())


class TestSearchEngine:
    @pytest.fixture()
    def mock_embedder(self) -> MagicMock:
        embedder = MagicMock()
        embedder.embed_single.return_value = [0.1] * 768
        embedder.build_symbol_text.return_value = "function test\ncode"
        return embedder

    def test_search_basic(self, populated_db: LoomDB, mock_embedder: MagicMock) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        results = engine.search("processOrder")
        assert len(results) > 0
        assert any(r.symbol.name == "processOrder" for r in results)

    def test_search_returns_scores(self, populated_db: LoomDB, mock_embedder: MagicMock) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        results = engine.search("processOrder")
        for r in results:
            assert 0 < r.score <= 1.0

    def test_search_with_kind_filter(
        self,
        populated_db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        results = engine.search("Cart", kind="class")
        for r in results:
            assert r.symbol.kind == "class"

    def test_search_respects_limit(
        self,
        populated_db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        results = engine.search("function", limit=2)
        assert len(results) <= 2

    def test_search_no_results(self, populated_db: LoomDB, mock_embedder: MagicMock) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        results = engine.search("xyzzy_nonexistent_42")
        # May still get vec results but shouldn't crash
        assert isinstance(results, list)

    def test_search_includes_coupled(
        self,
        populated_db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        results = engine.search("processOrder")
        order_result = next((r for r in results if r.symbol.name == "processOrder"), None)
        if order_result:
            assert isinstance(order_result.coupled, list)

    def test_related(self, populated_db: LoomDB, mock_embedder: MagicMock) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        coupled = engine.related("processOrder")
        assert isinstance(coupled, list)
        names = {c.symbol.name for c in coupled}
        assert "validateCart" in names or len(coupled) > 0

    def test_related_with_file(self, populated_db: LoomDB, mock_embedder: MagicMock) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        coupled = engine.related("getProduct", file="src/services/product.js")
        assert isinstance(coupled, list)

    def test_related_fuzzy_method_suffix(
        self,
        populated_db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        coupled = engine.related("addItem")
        assert isinstance(coupled, list)

    def test_related_nonexistent(self, populated_db: LoomDB, mock_embedder: MagicMock) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        coupled = engine.related("nonexistent_symbol")
        assert coupled == []

    def test_related_with_kind_filter(
        self,
        populated_db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        coupled = engine.related("processOrder", kind="function")
        for c in coupled:
            assert c.symbol.kind == "function"

    def test_impact(self, populated_db: LoomDB, mock_embedder: MagicMock) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        dependents = engine.impact("validateCart")
        assert isinstance(dependents, list)

    def test_impact_nonexistent(self, populated_db: LoomDB, mock_embedder: MagicMock) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        assert engine.impact("nonexistent") == []

    def test_impact_with_kind_filter(
        self,
        populated_db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        dependents = engine.impact("validateCart", kind="function")
        for d in dependents:
            assert d.symbol.kind == "function"

    def test_impact_sorted_by_score(
        self,
        populated_db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        dependents = engine.impact("validateCart")
        for i in range(len(dependents) - 1):
            assert dependents[i].score >= dependents[i + 1].score


class TestNeighborhood:
    @pytest.fixture()
    def mock_embedder(self) -> MagicMock:
        embedder = MagicMock()
        embedder.embed_single.return_value = [0.1] * 768
        embedder.build_symbol_text.return_value = "function test\ncode"
        return embedder

    def test_finds_anchor_by_line(self, populated_db: LoomDB, mock_embedder: MagicMock) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        anchor, coupled = engine.neighborhood("src/services/order.js", 15)
        assert anchor is not None
        assert anchor.name == "processOrder"

    def test_nearest_anchor_fallback(
        self,
        populated_db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        anchor, coupled = engine.neighborhood("src/services/order.js", 100)
        assert anchor is not None

    def test_empty_file(self, populated_db: LoomDB, mock_embedder: MagicMock) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        anchor, coupled = engine.neighborhood("nonexistent.js", 1)
        assert anchor is None
        assert coupled == []

    def test_coupled_includes_colocated(
        self,
        populated_db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        anchor, coupled = engine.neighborhood("src/models/cart.js", 5)
        if anchor and anchor.name == "Cart":
            colocated_names = {c.symbol.name for c in coupled}
            assert "Cart.addItem" in colocated_names

    def test_coupled_sorted_by_score(
        self,
        populated_db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        engine = SearchEngine(populated_db, mock_embedder)
        _, coupled = engine.neighborhood("src/models/cart.js", 5)
        for i in range(len(coupled) - 1):
            assert coupled[i].score >= coupled[i + 1].score


class TestBuiltinFiltering:
    @pytest.fixture()
    def mock_embedder(self) -> MagicMock:
        embedder = MagicMock()
        embedder.embed_single.return_value = [0.1] * 768
        embedder.build_symbol_text.return_value = "function test\ncode"
        return embedder

    def test_generic_targets_filtered_from_coupled(
        self,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        db.insert_symbol(
            Symbol(
                name="myFunc",
                kind="function",
                file="app.js",
                line=1,
                end_line=10,
                language="javascript",
            )
        )
        db.insert_embedding(1, [0.1] * 768)
        db.insert_symbol(
            Symbol(
                name="realHelper",
                kind="function",
                file="helper.js",
                line=1,
                end_line=5,
                language="javascript",
            )
        )
        db.insert_embedding(2, [0.9] * 768)
        db.insert_edge(
            Edge(
                source_name="myFunc",
                source_file="app.js",
                target_name="push",
                target_file=None,
                relationship="calls",
            )
        )
        db.insert_edge(
            Edge(
                source_name="myFunc",
                source_file="app.js",
                target_name="map",
                target_file=None,
                relationship="calls",
            )
        )
        db.insert_edge(
            Edge(
                source_name="myFunc",
                source_file="app.js",
                target_name="realHelper",
                target_file="helper.js",
                relationship="calls",
            )
        )
        db.commit()

        engine = SearchEngine(db, mock_embedder)
        coupled = engine.related("myFunc")
        coupled_names = {c.symbol.name for c in coupled}
        assert "realHelper" in coupled_names
        assert "push" not in coupled_names
        assert "map" not in coupled_names

    def test_generic_sources_filtered_from_impact(
        self,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        db.insert_symbol(
            Symbol(
                name="targetFunc",
                kind="function",
                file="target.js",
                line=1,
                end_line=10,
                language="javascript",
            )
        )
        db.insert_embedding(1, [0.1] * 768)
        db.insert_symbol(
            Symbol(
                name="realCaller",
                kind="function",
                file="caller.js",
                line=1,
                end_line=5,
                language="javascript",
            )
        )
        db.insert_embedding(2, [0.9] * 768)
        db.insert_edge(
            Edge(
                source_name="callback",
                source_file="generic.js",
                target_name="targetFunc",
                target_file="target.js",
                relationship="calls",
            )
        )
        db.insert_edge(
            Edge(
                source_name="realCaller",
                source_file="caller.js",
                target_name="targetFunc",
                target_file="target.js",
                relationship="calls",
            )
        )
        db.commit()

        engine = SearchEngine(db, mock_embedder)
        dependents = engine.impact("targetFunc")
        dep_names = {d.symbol.name for d in dependents}
        assert "realCaller" in dep_names
        assert "callback" not in dep_names

    def test_null_target_file_edges_excluded_with_file(self, db: LoomDB) -> None:
        db.insert_edge(
            Edge(
                source_name="callerA",
                source_file="a.js",
                target_name="create",
                target_file="factory.js",
                relationship="calls",
            )
        )
        db.insert_edge(
            Edge(
                source_name="callerB",
                source_file="b.js",
                target_name="create",
                target_file=None,
                relationship="calls",
            )
        )
        db.insert_edge(
            Edge(
                source_name="callerC",
                source_file="c.js",
                target_name="create",
                target_file=None,
                relationship="calls",
            )
        )
        db.commit()

        scoped = db.get_edges_to("create", file="factory.js")
        assert len(scoped) == 1
        assert scoped[0].source_name == "callerA"

    def test_structural_results_capped(
        self,
        db: LoomDB,
        mock_embedder: MagicMock,
    ) -> None:
        db.insert_symbol(
            Symbol(
                name="bigFunc",
                kind="function",
                file="big.js",
                line=1,
                end_line=100,
                language="javascript",
            )
        )
        db.insert_embedding(1, [0.1] * 768)

        for i in range(50):
            target_name = f"dep{i}"
            db.insert_symbol(
                Symbol(
                    name=target_name,
                    kind="function",
                    file=f"dep{i}.js",
                    line=1,
                    end_line=5,
                    language="javascript",
                )
            )
            db.insert_embedding(i + 2, [0.9] * 768)
            db.insert_edge(
                Edge(
                    source_name="bigFunc",
                    source_file="big.js",
                    target_name=target_name,
                    target_file=f"dep{i}.js",
                    relationship="calls",
                )
            )
        db.commit()

        engine = SearchEngine(db, mock_embedder)
        coupled = engine.related("bigFunc")
        structural = [c for c in coupled if "structural" in c.reason]
        assert len(structural) <= MAX_STRUCTURAL_RESULTS
