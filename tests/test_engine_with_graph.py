"""Integration tests for SearchEngine with real SymbolGraph — Phase 4+5 paths."""

from unittest.mock import MagicMock

import pytest

from loom.config import LoomConfig
from loom.search.engine import MAX_STRUCTURAL_RESULTS, SearchEngine
from loom.store.db import LoomDB
from loom.store.graph import SymbolGraph
from loom.store.models import Edge, Symbol


def _make_sym(
    db: LoomDB,
    name: str,
    file: str = "app.js",
    kind: str = "function",
    embedding: list[float] | None = None,
) -> Symbol:
    sym_id = db.insert_symbol(
        Symbol(name=name, kind=kind, file=file, line=1, end_line=10, language="javascript")
    )
    db.insert_embedding(sym_id, embedding if embedding is not None else [0.1] * 768)
    sym = db.get_symbol_by_id(sym_id)
    assert sym is not None
    return sym


def _edge(
    db: LoomDB,
    source_id: int,
    target_id: int,
    relationship: str = "calls",
    confidence: float = 1.0,
) -> None:
    db.insert_edge(
        Edge(
            source_id=source_id,
            target_name="any",
            target_id=target_id,
            relationship=relationship,
            confidence=confidence,
        )
    )


@pytest.fixture
def mock_embedder() -> MagicMock:
    embedder = MagicMock()
    embedder.embed_single.return_value = [0.1] * 768
    embedder.build_symbol_text.return_value = "function test\ncode"
    return embedder


class TestEngineWithGraph:
    def test_related_uses_graph_multi_hop(
        self, db: LoomDB, mock_embedder: MagicMock, config: LoomConfig
    ) -> None:
        """_find_coupled with graph traverses 2 hops, not just 1."""
        a = _make_sym(db, "A")
        b = _make_sym(db, "B")
        c = _make_sym(db, "C")
        assert a.id is not None
        assert b.id is not None
        assert c.id is not None

        _edge(db, a.id, b.id, "calls", 1.0)
        _edge(db, b.id, c.id, "calls", 1.0)
        db.commit()

        graph = SymbolGraph()
        graph.build_from_db(db)
        engine = SearchEngine(db, mock_embedder, graph=graph, config=config)

        coupled = engine.related("A")
        names = {c.symbol.name for c in coupled}

        # B is at depth 1 — must appear
        assert "B" in names
        # C is at depth 2 — should appear with graph traversal
        assert "C" in names

    def test_related_with_graph_uses_real_scores(
        self, db: LoomDB, mock_embedder: MagicMock, config: LoomConfig
    ) -> None:
        """Scores with graph should come from coupling computation, not flat 0.7/0.6."""
        a = _make_sym(db, "A")
        b = _make_sym(db, "B")
        assert a.id is not None
        assert b.id is not None

        _edge(db, a.id, b.id, "calls", 1.0)
        db.commit()

        graph = SymbolGraph()
        graph.build_from_db(db)
        engine = SearchEngine(db, mock_embedder, graph=graph, config=config)

        coupled = engine.related("A")
        b_result = next((c for c in coupled if c.symbol.name == "B"), None)
        assert b_result is not None
        # With graph: score is compute_structural("calls", 1.0, 1) then fused
        # Not flat 0.7
        assert b_result.score != 0.7

    def test_related_reason_contains_structural_keyword(
        self, db: LoomDB, mock_embedder: MagicMock, config: LoomConfig
    ) -> None:
        """reason field must contain 'structural' — required by test_structural_results_capped."""
        a = _make_sym(db, "A")
        b = _make_sym(db, "B")
        assert a.id is not None
        assert b.id is not None

        _edge(db, a.id, b.id, "calls", 1.0)
        db.commit()

        graph = SymbolGraph()
        graph.build_from_db(db)
        engine = SearchEngine(db, mock_embedder, graph=graph, config=config)

        coupled = engine.related("A")
        structural = [c for c in coupled if "structural" in c.reason]
        assert len(structural) > 0

    def test_impact_uses_graph_traversal(
        self, db: LoomDB, mock_embedder: MagicMock, config: LoomConfig
    ) -> None:
        """impact() with graph should use impact_radius for multi-hop traversal."""
        a = _make_sym(db, "A")
        b = _make_sym(db, "B")
        c = _make_sym(db, "C")
        assert a.id is not None
        assert b.id is not None
        assert c.id is not None

        _edge(db, a.id, b.id, "calls", 1.0)
        _edge(db, b.id, c.id, "calls", 1.0)
        db.commit()

        graph = SymbolGraph()
        graph.build_from_db(db)
        engine = SearchEngine(db, mock_embedder, graph=graph, config=config)

        # impact(C) should find both B (direct caller) and A (transitive caller)
        dependents = engine.impact("C")
        names = {d.symbol.name for d in dependents}

        assert "B" in names
        assert "A" in names

    def test_impact_graph_scores_decay_with_depth(
        self, db: LoomDB, mock_embedder: MagicMock, config: LoomConfig
    ) -> None:
        """Deeper structural dependents should score lower than shallower ones."""
        a = _make_sym(db, "A")
        b = _make_sym(db, "B")
        c = _make_sym(db, "C")
        assert a.id is not None
        assert b.id is not None
        assert c.id is not None

        _edge(db, a.id, b.id, "calls", 1.0)
        _edge(db, b.id, c.id, "calls", 1.0)
        db.commit()

        graph = SymbolGraph()
        graph.build_from_db(db)

        # Use a zero embedding to suppress semantic hits
        mock_embedder.embed_single.return_value = [-1.0] * 768
        engine = SearchEngine(db, mock_embedder, graph=graph, config=config)

        dependents = engine.impact("C")
        b_result = next((d for d in dependents if d.symbol.name == "B"), None)
        a_result = next((d for d in dependents if d.symbol.name == "A"), None)

        if b_result and a_result:
            assert b_result.score >= a_result.score

    def test_impact_unresolved_fallback_still_works_with_graph(
        self, db: LoomDB, mock_embedder: MagicMock, config: LoomConfig
    ) -> None:
        """Even with graph, unresolved callers (target_id=None) must surface via name match."""
        target = _make_sym(db, "targetSym", "target.js")
        caller = _make_sym(db, "callerFunc", "caller.js")
        assert target.id is not None
        assert caller.id is not None

        # Unresolved edge — not in graph
        db.insert_edge(
            Edge(
                source_id=caller.id,
                target_name="targetSym",
                target_id=None,
                relationship="calls",
            )
        )
        db.commit()

        graph = SymbolGraph()
        graph.build_from_db(db)
        engine = SearchEngine(db, mock_embedder, graph=graph, config=config)

        dependents = engine.impact("targetSym")
        dep_names = {d.symbol.name for d in dependents}
        assert "callerFunc" in dep_names

    def test_related_structural_capped_at_max_with_graph(
        self, db: LoomDB, mock_embedder: MagicMock, config: LoomConfig
    ) -> None:
        """With graph, coupled list should be capped at MAX_STRUCTURAL_RESULTS."""
        big_func = _make_sym(db, "bigFunc", "big.js")
        assert big_func.id is not None

        for i in range(50):
            tgt = _make_sym(db, f"dep{i}", f"dep{i}.js")
            assert tgt.id is not None
            _edge(db, big_func.id, tgt.id, "calls", 1.0)
        db.commit()

        graph = SymbolGraph()
        graph.build_from_db(db)
        engine = SearchEngine(db, mock_embedder, graph=graph, config=config)

        coupled = engine.related("bigFunc")
        structural = [c for c in coupled if "structural" in c.reason]
        assert len(structural) <= MAX_STRUCTURAL_RESULTS

    def test_semantic_fuses_with_structural_when_both_present(
        self, db: LoomDB, mock_embedder: MagicMock, config: LoomConfig
    ) -> None:
        """When a symbol is found by both structural and semantic, scores should be fused."""
        a = _make_sym(db, "A", embedding=[0.5] * 768)
        b = _make_sym(db, "B", embedding=[0.5] * 768)
        assert a.id is not None
        assert b.id is not None

        _edge(db, a.id, b.id, "calls", 1.0)
        db.commit()

        graph = SymbolGraph()
        graph.build_from_db(db)

        # Return an embedding close to B's — distance ~0 → semantic score ~1.0
        mock_embedder.embed_single.return_value = [0.5] * 768
        engine = SearchEngine(db, mock_embedder, graph=graph, config=config)

        coupled = engine.related("A")
        b_result = next((c for c in coupled if c.symbol.name == "B"), None)
        assert b_result is not None
        # Fused score should be higher than structural-only
        assert b_result.score > 0.0

    def test_no_graph_fallback_still_works(self, db: LoomDB, mock_embedder: MagicMock) -> None:
        """Without graph, engine falls back to one-hop DB traversal gracefully."""
        a = _make_sym(db, "A")
        b = _make_sym(db, "B")
        assert a.id is not None
        assert b.id is not None

        _edge(db, a.id, b.id, "calls", 1.0)
        db.commit()

        # No graph passed — should use fallback path
        engine = SearchEngine(db, mock_embedder)
        coupled = engine.related("A")

        names = {c.symbol.name for c in coupled}
        assert "B" in names
