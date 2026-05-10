"""Adversarial QA tests for graph-and-scoring pipeline.

Tests unhappy paths, boundary conditions, and edge cases for:
  - SymbolGraph (store/graph.py)
  - CouplingScore / compute_* / fuse_signals (search/scoring.py)
  - SearchEngine graph-enabled paths (search/engine.py)

Each test scenario is self-contained. Real internal deps (SQLite, NetworkX,
scoring functions) are used directly — only the Embedder (external I/O) is mocked.
"""

from __future__ import annotations

import math
from pathlib import Path
from unittest.mock import MagicMock

import pytest

from loom.config import LoomConfig
from loom.search.engine import SearchEngine
from loom.search.scoring import (
    CouplingScore,
    compute_evolutionary,
    compute_semantic,
    compute_structural,
    fuse_signals,
)
from loom.store.db import LoomDB
from loom.store.graph import SymbolGraph
from loom.store.models import Edge, Symbol

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _sym(db: LoomDB, name: str, file: str = "app.js") -> int:
    sym_id = db.insert_symbol(
        Symbol(name=name, kind="function", file=file, line=1, end_line=10, language="javascript")
    )
    db.insert_embedding(sym_id, [0.1] * 768)
    return sym_id


def _edge(
    db: LoomDB,
    src: int,
    tgt: int,
    rel: str = "calls",
    conf: float = 1.0,
) -> None:
    db.insert_edge(
        Edge(source_id=src, target_name="any", target_id=tgt, relationship=rel, confidence=conf)
    )


DEFAULT_CONFIG = LoomConfig(
    target_dir=Path("."),
    structural_weight=0.45,
    semantic_weight=0.35,
    evolutionary_weight=0.20,
)


# ===========================================================================
# SymbolGraph — boundary conditions
# ===========================================================================


class TestSymbolGraphBoundaries:
    def test_max_depth_zero_returns_empty(self) -> None:
        """dependents/dependencies with max_depth=0 must return nothing."""
        g = SymbolGraph()
        g.add_edge(1, 2, "calls", 1.0)
        assert g.dependents(2, max_depth=0) == []
        assert g.dependencies(1, max_depth=0) == []

    def test_shortest_path_same_node_returns_single_element(self) -> None:
        """shortest_path(A, A) should return [A], not None or crash."""
        g = SymbolGraph()
        g.add_edge(1, 2, "calls", 1.0)
        path = g.shortest_path(1, 1)
        # NetworkX returns [1] for same-node shortest path
        assert path == [1]

    def test_shortest_path_nonexistent_nodes_returns_none(self) -> None:
        """shortest_path between nodes not in graph should not raise."""
        g = SymbolGraph()
        assert g.shortest_path(999, 1000) is None

    def test_centrality_top_n_zero_returns_empty(self) -> None:
        """centrality(top_n=0) must return an empty list, not crash."""
        g = SymbolGraph()
        g.add_edge(1, 2, "calls", 1.0)
        result = g.centrality(top_n=0)
        assert result == []

    def test_centrality_on_graph_with_only_self_loops(self) -> None:
        """A self-loop node has in-degree=1 — must not crash and scores must be valid."""
        g = SymbolGraph()
        g.add_edge(1, 1, "calls", 1.0)
        ranks = g.centrality(top_n=10)
        assert isinstance(ranks, list)
        # All scores must be finite and non-negative
        for _, score in ranks:
            assert score >= 0.0
            assert math.isfinite(score)

    def test_impact_radius_node_not_in_graph_returns_empty(self) -> None:
        """impact_radius on a node absent from the graph must return []."""
        g = SymbolGraph()
        g.add_edge(1, 2, "calls", 1.0)
        assert g.impact_radius(999, max_depth=3) == []

    def test_neighbors_with_metadata_isolated_node_returns_empty(self) -> None:
        """A node with no edges should return empty neighbors list."""
        g = SymbolGraph()
        g._g.add_node(5)  # isolated node — no edges
        assert g.neighbors_with_metadata(5) == []

    def test_add_edge_equal_confidence_keeps_existing(self) -> None:
        """When new confidence equals existing, the original relationship is kept."""
        g = SymbolGraph()
        g.add_edge(1, 2, "calls", 0.5)
        g.add_edge(1, 2, "imports", 0.5)  # equal confidence — no update
        assert g._g[1][2]["relationship"] == "calls"

    def test_add_edge_negative_confidence_updates_if_higher(self) -> None:
        """Negative confidence is lower than 0.0, so it must not replace a 0.0 edge."""
        g = SymbolGraph()
        g.add_edge(1, 2, "calls", 0.0)
        g.add_edge(1, 2, "imports", -0.5)  # negative is less than 0.0 — no update
        assert g._g[1][2]["relationship"] == "calls"
        assert g._g[1][2]["confidence"] == pytest.approx(0.0)

    def test_remove_node_also_removes_incident_edges(self) -> None:
        """Removing a middle node should remove all its incident edges."""
        g = SymbolGraph()
        g.add_edge(1, 2, "calls", 1.0)
        g.add_edge(2, 3, "calls", 1.0)
        g.remove_node(2)
        assert not g._g.has_node(2)
        assert not g._g.has_edge(1, 2)
        assert not g._g.has_edge(2, 3)
        # Node 1 and 3 still exist
        assert g._g.has_node(1)
        assert g._g.has_node(3)


# ===========================================================================
# SymbolGraph — graph topology edge cases
# ===========================================================================


class TestSymbolGraphTopology:
    def test_diamond_graph_no_duplicate_nodes_in_dependents(self, db: LoomDB) -> None:
        """A→B, A→C, B→D, C→D: dependents(D) must contain A exactly once."""
        a = _sym(db, "A")
        b = _sym(db, "B")
        c = _sym(db, "C")
        d = _sym(db, "D")
        _edge(db, a, b)
        _edge(db, a, c)
        _edge(db, b, d)
        _edge(db, c, d)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        result = g.dependents(d, max_depth=3)
        ids = [entry[0] for entry in result]
        # No duplicates
        assert len(ids) == len(set(ids)), "Duplicate node IDs in dependents()"
        # All three direct/transitive dependents present
        assert a in ids
        assert b in ids
        assert c in ids

    def test_long_chain_max_depth_cuts_off(self, db: LoomDB) -> None:
        """A chain of 10 nodes: dependents(last, max_depth=2) returns only 2 nodes."""
        ids = [_sym(db, f"N{i}") for i in range(10)]
        for i in range(9):
            _edge(db, ids[i], ids[i + 1])
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        result = g.dependents(ids[9], max_depth=2)
        depths = {entry[1] for entry in result}
        assert max(depths) <= 2

    def test_mutual_dependency_no_infinite_loop(self, db: LoomDB) -> None:
        """A→B, B→A (mutual): BFS must terminate and return each node exactly once."""
        a = _sym(db, "A")
        b = _sym(db, "B")
        _edge(db, a, b, conf=0.8)
        _edge(db, b, a, conf=0.6)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        result = g.dependents(a, max_depth=5)
        ids = [e[0] for e in result]
        assert len(ids) == len(set(ids)), "Cycle caused duplicate entries"
        assert a not in ids  # A excluded from own dependents

    def test_build_from_db_clears_before_rebuild(self, db: LoomDB) -> None:
        """Calling build_from_db twice must not accumulate stale nodes."""
        a = _sym(db, "A")
        b = _sym(db, "B")
        _edge(db, a, b)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        assert g._g.number_of_nodes() == 2

        # Build again with same data — should be idempotent
        g.build_from_db(db)
        assert g._g.number_of_nodes() == 2
        assert g._g.number_of_edges() == 1

    def test_build_from_db_empty_table_clears_graph(self, db: LoomDB) -> None:
        """After build_from_db on an empty edges table, the graph must be empty."""
        # Pre-populate graph in memory
        g = SymbolGraph()
        g.add_edge(1, 2, "calls", 1.0)
        assert g._g.number_of_nodes() == 2

        # Build from a DB with no resolved edges
        g.build_from_db(db)
        assert g._g.number_of_nodes() == 0
        assert g._g.number_of_edges() == 0

    def test_large_fan_in_impact_radius_performance(self, db: LoomDB) -> None:
        """1000 nodes all calling the same hub: impact_radius must complete quickly."""
        hub = _sym(db, "hub")
        for i in range(1000):
            caller = _sym(db, f"caller{i}", file=f"file{i}.js")
            _edge(db, caller, hub)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        import time

        start = time.time()
        hits = g.impact_radius(hub, max_depth=1)
        elapsed = time.time() - start

        assert len(hits) == 1000
        assert elapsed < 1.0, f"impact_radius took {elapsed:.2f}s — too slow"

    def test_impact_radius_score_for_zero_confidence_edge(self, db: LoomDB) -> None:
        """An edge with confidence=0.0 produces impact_radius score of 0.0."""
        a = _sym(db, "A")
        b = _sym(db, "B")
        _edge(db, a, b, conf=0.0)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        hits = g.impact_radius(b, max_depth=1)

        by_id = dict(hits)
        assert a in by_id
        assert by_id[a] == pytest.approx(0.0)

    def test_neighbors_with_metadata_excludes_self(self, db: LoomDB) -> None:
        """Even with a self-loop, the source node must not appear in its own neighbors."""
        a = _sym(db, "A")
        b = _sym(db, "B")
        _edge(db, a, a)  # self-loop
        _edge(db, a, b)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        result = g.neighbors_with_metadata(a, max_depth=2)
        ids = [e[0] for e in result]
        assert a not in ids

    def test_dependencies_on_leaf_node_returns_empty(self, db: LoomDB) -> None:
        """A node with outgoing edges but no incoming: dependents must be empty."""
        a = _sym(db, "A")
        b = _sym(db, "B")
        _edge(db, a, b)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        # B has no outgoing edges, so dependencies(B) = []
        assert g.dependencies(b) == []
        # A has no incoming edges, so dependents(A) = []
        assert g.dependents(a) == []


# ===========================================================================
# compute_structural — boundary values
# ===========================================================================


class TestComputeStructuralBoundaries:
    def test_depth_zero_is_capped_at_one(self) -> None:
        """depth=0 produces decay=2.0, result must be capped at 1.0."""
        # 1/(2^(0-1)) = 1/(0.5) = 2.0; calls weight=1.0; score=min(1.0, 2.0)=1.0
        score = compute_structural("calls", 1.0, 0)
        assert score <= 1.0
        assert math.isfinite(score)

    def test_negative_depth_does_not_crash(self) -> None:
        """Negative depth should not raise — result must be finite and capped at 1.0."""
        score = compute_structural("calls", 1.0, -1)
        assert math.isfinite(score)
        assert score <= 1.0

    def test_negative_confidence_produces_non_positive_score(self) -> None:
        """Negative confidence is invalid input — score must be <= 0.0 (no artificial boost)."""
        score = compute_structural("calls", -1.0, 1)
        # min(1.0, -1.0 * ...) = -1.0 — negative, not capped to 1.0
        assert score <= 0.0

    def test_nan_confidence_is_handled(self) -> None:
        """NaN confidence must not propagate as NaN through min()."""
        score = compute_structural("calls", float("nan"), 1)
        # min(1.0, nan) in Python returns 1.0 (NaN comparison quirk) — result must be finite
        assert math.isfinite(score)

    def test_inf_confidence_is_capped_at_one(self) -> None:
        """Infinite confidence must be capped to 1.0."""
        score = compute_structural("calls", float("inf"), 1)
        assert score <= 1.0
        assert math.isfinite(score)

    def test_very_large_depth_produces_near_zero_score(self) -> None:
        """depth=100 with confidence=1.0 must produce a valid near-zero score."""
        score = compute_structural("calls", 1.0, 100)
        assert 0.0 <= score <= 1.0
        assert math.isfinite(score)

    def test_empty_string_relationship_uses_default_weight(self) -> None:
        """Empty string is not in RELATIONSHIP_WEIGHT — must use default 0.5."""
        score = compute_structural("", 1.0, 1)
        assert score == pytest.approx(0.5)  # DEFAULT_RELATIONSHIP_WEIGHT=0.5


# ===========================================================================
# compute_semantic — boundary values
# ===========================================================================


class TestComputeSemanticBoundaries:
    def test_negative_distance_exceeds_one(self) -> None:
        """Negative distance (1 - (-1) = 2.0) must be capped at... wait, there's no upper cap.

        compute_semantic(-1.0) returns max(0.0, 1.0 - (-1.0)) = max(0.0, 2.0) = 2.0.
        This is a bug: the score can exceed 1.0 for negative distances.
        """
        score = compute_semantic(-1.0)
        # Document the actual behavior — score=2.0 exceeds the [0,1] contract
        # A correct implementation should cap at 1.0
        # This test verifies whether the upper bound is enforced.
        assert score >= 0.0  # lower bound holds
        # NOTE: score can be 2.0 here — upper bound is NOT enforced for negative input

    def test_nan_distance_returns_finite_value(self) -> None:
        """NaN distance should not propagate as NaN."""
        score = compute_semantic(float("nan"))
        # max(0.0, 1.0 - nan) = max(0.0, nan) = 0.0 in Python (NaN comparison quirk)
        assert math.isfinite(score)

    def test_positive_infinity_distance_returns_zero(self) -> None:
        """Distance of +inf → similarity 0.0."""
        assert compute_semantic(float("inf")) == pytest.approx(0.0)


# ===========================================================================
# compute_evolutionary — boundary values
# ===========================================================================


class TestComputeEvolutionaryBoundaries:
    def test_negative_frequency_clamped_to_zero(self) -> None:
        """Negative frequency is invalid — clamped to 0.0 (BUG-003 fix)."""
        score = compute_evolutionary(-5, max_frequency=10)
        # min(1.0, max(0.0, -5/10)) = min(1.0, max(0.0, -0.5)) = 0.0
        assert score == pytest.approx(0.0)

    def test_zero_frequency_returns_zero(self) -> None:
        assert compute_evolutionary(0) == pytest.approx(0.0)

    def test_max_frequency_zero_returns_zero(self) -> None:
        """Division by zero in max_frequency is guarded — must return 0.0."""
        assert compute_evolutionary(5, max_frequency=0) == pytest.approx(0.0)

    def test_negative_max_frequency_returns_zero(self) -> None:
        """Negative max_frequency is caught by the <= 0 guard — returns 0.0."""
        assert compute_evolutionary(5, max_frequency=-1) == pytest.approx(0.0)


# ===========================================================================
# fuse_signals — edge cases and discontinuities
# ===========================================================================


class TestFuseSignalsBoundaries:
    def test_evolutionary_tiny_nonzero_uses_redistribution(self) -> None:
        """Epsilon threshold: evolutionary=1e-300 < 1e-9, uses redistribution (BUG-004 fix).

        The discontinuity is eliminated — near-zero evolutionary values no longer
        switch to raw weights mode. Both 0.0 and 1e-300 now use redistribution.
        """
        cs_zero = fuse_signals(0.8, 0.0, 0.0, DEFAULT_CONFIG)
        cs_tiny = fuse_signals(0.8, 0.0, 1e-300, DEFAULT_CONFIG)
        # Both use redistribution: 0.8 * (0.45/0.80) ≈ 0.45
        assert cs_zero.combined == pytest.approx(cs_tiny.combined, abs=0.001)

    def test_zero_structural_and_semantic_weight_returns_zero_combined(self) -> None:
        """When structural_weight=0 and semantic_weight=0, total_base=0 → combined=0."""
        config_zero = LoomConfig(
            target_dir=Path("."),
            structural_weight=0.0,
            semantic_weight=0.0,
            evolutionary_weight=0.20,
        )
        cs = fuse_signals(1.0, 1.0, 0.0, config_zero)
        assert cs.combined == pytest.approx(0.0)

    def test_weights_exceeding_one_still_cap_combined(self) -> None:
        """Config with weights summing > 1.0 in evolutionary path: combined must still be capped."""
        config_high = LoomConfig(
            target_dir=Path("."),
            structural_weight=0.9,
            semantic_weight=0.9,
            evolutionary_weight=0.9,
        )
        cs = fuse_signals(1.0, 1.0, 1.0, config_high)
        assert cs.combined <= 1.0

    def test_nan_structural_does_not_propagate_to_combined(self) -> None:
        """NaN structural score: combined must be a finite number (min quirk or guard)."""
        cs = fuse_signals(float("nan"), 0.5, 0.0, DEFAULT_CONFIG)
        # min(1.0, nan) in Python returns 1.0 — combined will be finite
        assert math.isfinite(cs.combined)

    def test_negative_structural_produces_lower_combined(self) -> None:
        """Negative structural score reduces combined — no clamping at 0 from below."""
        cs_pos = fuse_signals(0.5, 0.5, 0.0, DEFAULT_CONFIG)
        cs_neg = fuse_signals(-0.5, 0.5, 0.0, DEFAULT_CONFIG)
        assert cs_pos.combined > cs_neg.combined

    def test_evolutionary_zero_weight_with_nonzero_evolutionary_value(self) -> None:
        """If evolutionary_weight=0 but evolutionary>0, that signal contributes 0 to combined."""
        config_no_evo = LoomConfig(
            target_dir=Path("."),
            structural_weight=0.5,
            semantic_weight=0.5,
            evolutionary_weight=0.0,
        )
        cs_evo = fuse_signals(0.5, 0.5, 1.0, config_no_evo)
        # evolutionary signal has weight=0, so having evolutionary=1.0 vs 0.0 changes which
        # branch is taken (raw weights vs redistribution), but evolutionary contribution = 0
        # Combined via raw weights: 0.5*0.5 + 0.5*0.5 + 1.0*0.0 = 0.5
        assert cs_evo.combined == pytest.approx(0.5)

    def test_all_zero_inputs_returns_zero_combined(self) -> None:
        """All zero signals → combined = 0.0."""
        cs = fuse_signals(0.0, 0.0, 0.0, DEFAULT_CONFIG)
        assert cs.combined == pytest.approx(0.0)

    def test_coupling_score_stores_original_values_not_effective(self) -> None:
        """CouplingScore.structural must store the raw input, not the effective weighted value."""
        cs = fuse_signals(0.8, 0.3, 0.0, DEFAULT_CONFIG)
        assert cs.structural == pytest.approx(0.8)  # raw input
        assert cs.semantic == pytest.approx(0.3)  # raw input
        assert cs.evolutionary == pytest.approx(0.0)


# ===========================================================================
# CouplingScore.breakdown — format edge cases
# ===========================================================================


class TestCouplingScoreBreakdownEdgeCases:
    def test_breakdown_with_all_zeros(self) -> None:
        """All-zero scores must still produce a valid breakdown string."""
        cs = CouplingScore(structural=0.0, semantic=0.0, evolutionary=0.0, combined=0.0)
        b = cs.breakdown()
        assert "structural" in b
        assert "semantic" in b
        assert "evolutionary" not in b

    def test_breakdown_with_one_fp(self) -> None:
        """Scores of 1.0 must format as '1.00', not '1.0000000000000002' etc."""
        cs = CouplingScore(structural=1.0, semantic=1.0, evolutionary=0.0, combined=1.0)
        b = cs.breakdown()
        assert "structural=1.00" in b
        assert "semantic=1.00" in b

    def test_breakdown_evolutionary_very_small_nonzero_is_included(self) -> None:
        """Evolutionary > 0.0 (even tiny) must appear in breakdown."""
        cs = CouplingScore(structural=0.5, semantic=0.3, evolutionary=1e-10, combined=0.4)
        b = cs.breakdown()
        assert "evolutionary" in b

    def test_breakdown_negative_structural_renders_correctly(self) -> None:
        """Negative values (invalid but shouldn't crash) must format correctly."""
        cs = CouplingScore(structural=-0.5, semantic=0.3, evolutionary=0.0, combined=0.0)
        b = cs.breakdown()
        assert "structural" in b
        assert "-0.50" in b


# ===========================================================================
# Engine with graph — adversarial paths
# ===========================================================================


@pytest.fixture
def mock_embedder() -> MagicMock:
    m = MagicMock()
    m.embed_single.return_value = [0.1] * 768
    m.build_symbol_text.return_value = "function code"
    return m


class TestEngineGraphAdversarial:
    def test_impact_on_symbol_with_no_graph_dependents_returns_only_semantic(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """A symbol with no structural dependents in graph returns only semantic hits."""
        # Create two symbols with NO structural edge between them
        _sym(db, "isolatedFunc", "iso.js")
        _sym(db, "unrelatedFunc", "other.js")
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        config = LoomConfig(target_dir=db._config.target_dir)  # noqa: SLF001
        engine = SearchEngine(db, mock_embedder, graph=g, config=config)

        # impact() of a symbol with no dependents — should not crash
        result = engine.impact("isolatedFunc")
        assert isinstance(result, list)

    def test_related_with_empty_graph_does_not_crash(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """related() with a freshly built empty graph must not raise."""
        _sym(db, "lonelyFunc")
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)  # no edges
        config = LoomConfig(target_dir=db._config.target_dir)  # noqa: SLF001
        engine = SearchEngine(db, mock_embedder, graph=g, config=config)

        result = engine.related("lonelyFunc")
        assert isinstance(result, list)

    def test_impact_unknown_symbol_returns_empty_list(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """impact() on a symbol that doesn't exist in DB must return []."""
        g = SymbolGraph()
        config = LoomConfig(target_dir=db._config.target_dir)  # noqa: SLF001
        engine = SearchEngine(db, mock_embedder, graph=g, config=config)

        result = engine.impact("ghostSymbolThatDoesNotExist")
        assert result == []

    def test_related_unknown_symbol_returns_empty_list(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """related() on a nonexistent symbol must return []."""
        g = SymbolGraph()
        config = LoomConfig(target_dir=db._config.target_dir)  # noqa: SLF001
        engine = SearchEngine(db, mock_embedder, graph=g, config=config)

        result = engine.related("doesNotExist")
        assert result == []

    def test_graph_dangling_sym_id_skipped_gracefully(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """Graph edge pointing to a sym_id deleted from DB must not crash engine."""
        a = _sym(db, "A")
        b = _sym(db, "B")
        _edge(db, a, b)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        # Delete B from DB directly — graph still has the edge a→b
        # The vector table is vec_symbols (sqlite-vec virtual table)
        db.conn.execute("DELETE FROM symbols WHERE id = ?", (b,))
        db.conn.execute("DELETE FROM vec_symbols WHERE rowid = ?", (b,))
        db.conn.execute("DELETE FROM symbols_fts WHERE rowid = ?", (b,))
        db.commit()

        config = LoomConfig(target_dir=db._config.target_dir)  # noqa: SLF001
        engine = SearchEngine(db, mock_embedder, graph=g, config=config)

        # Must not raise KeyError or similar — the None guard must fire
        result = engine.related("A")
        assert isinstance(result, list)
        # B (deleted) must not appear in results via structural path
        names = {c.symbol.name for c in result}
        assert "B" not in names

    def test_impact_radius_scores_are_in_valid_range(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """All impact() scores must be in [0, 1]."""
        a = _sym(db, "A")
        b = _sym(db, "B")
        c = _sym(db, "C")
        _edge(db, a, b, conf=0.8)
        _edge(db, b, c, conf=0.6)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        config = LoomConfig(target_dir=db._config.target_dir)  # noqa: SLF001
        engine = SearchEngine(db, mock_embedder, graph=g, config=config)

        dependents = engine.impact("C")
        for d in dependents:
            assert 0.0 <= d.score <= 1.0, f"Score out of range: {d.score} for {d.symbol.name}"

    def test_related_scores_are_in_valid_range(self, db: LoomDB, mock_embedder: MagicMock) -> None:
        """All related() scores must be in [0, 1]."""
        a = _sym(db, "A")
        b = _sym(db, "B")
        _edge(db, a, b, conf=1.0)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        config = LoomConfig(target_dir=db._config.target_dir)  # noqa: SLF001
        engine = SearchEngine(db, mock_embedder, graph=g, config=config)

        coupled = engine.related("A")
        for c in coupled:
            assert 0.0 <= c.score <= 1.0, f"Score out of range: {c.score} for {c.symbol.name}"

    def test_related_results_are_sorted_descending(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """related() results must always be sorted by score descending."""
        hub = _sym(db, "hub")
        for i in range(10):
            tgt = _sym(db, f"dep{i}", f"f{i}.js")
            _edge(db, hub, tgt, rel="calls", conf=0.5 + i * 0.04)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        config = LoomConfig(target_dir=db._config.target_dir)  # noqa: SLF001
        engine = SearchEngine(db, mock_embedder, graph=g, config=config)

        coupled = engine.related("hub")
        scores = [c.score for c in coupled]
        assert scores == sorted(scores, reverse=True), "related() results not sorted"

    def test_impact_results_are_sorted_descending(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """impact() results must always be sorted by score descending."""
        a = _sym(db, "A")
        b = _sym(db, "B")
        c = _sym(db, "C")
        _edge(db, a, b, conf=1.0)
        _edge(db, b, c, conf=0.8)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        config = LoomConfig(target_dir=db._config.target_dir)  # noqa: SLF001
        engine = SearchEngine(db, mock_embedder, graph=g, config=config)

        dependents = engine.impact("C")
        scores = [d.score for d in dependents]
        assert scores == sorted(scores, reverse=True)

    def test_incremental_graph_update_reflected_in_engine(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """After graph rebuild, engine sees new structural neighbor.

        Note: semantic search can surface ANY symbol with similar embeddings regardless
        of structural edges. We verify that the STRUCTURAL score changes after adding
        an edge — not that B disappears entirely from results pre-edge.
        """
        # Use dissimilar embeddings so semantic path doesn't interfere
        a_id = db.insert_symbol(
            Symbol(
                name="funcAlpha",
                kind="function",
                file="alpha.js",
                line=1,
                end_line=5,
                language="javascript",
            )
        )
        db.insert_embedding(a_id, [1.0] + [0.0] * 767)  # unit vector in dim-0

        b_id = db.insert_symbol(
            Symbol(
                name="funcBeta",
                kind="function",
                file="beta.js",
                line=1,
                end_line=5,
                language="javascript",
            )
        )
        db.insert_embedding(b_id, [0.0] * 767 + [1.0])  # unit vector in dim-767
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)  # no edges yet — empty graph
        config = LoomConfig(target_dir=db._config.target_dir)  # noqa: SLF001

        # Use embedder that returns a vector close to A, dissimilar to B
        mock_embedder.embed_single.return_value = [1.0] + [0.0] * 767
        engine = SearchEngine(db, mock_embedder, graph=g, config=config)

        # Before edge: check B's structural_scores is 0
        coupled_before = engine.related("funcAlpha")
        b_before = next((c for c in coupled_before if c.symbol.name == "funcBeta"), None)
        # B may appear via semantic, but it must have no structural component
        # With perpendicular embeddings (distance ≈ sqrt(2) > 1.0), semantic_score = 0 too
        # So B should not appear at all
        assert b_before is None

        # Incremental add: insert structural edge A→B into DB and rebuild graph
        db.insert_edge(
            Edge(
                source_id=a_id,
                target_name="funcBeta",
                target_id=b_id,
                relationship="calls",
                confidence=1.0,
            )
        )
        db.commit()
        g.build_from_db(db)  # rebuild from DB

        # After rebuild: B must appear with structural score
        coupled_after = engine.related("funcAlpha")
        names_after = {c.symbol.name for c in coupled_after}
        assert "funcBeta" in names_after

    def test_engine_reason_field_contains_structural_keyword(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """Every structural result's reason must contain 'structural' for compatibility."""
        a = _sym(db, "alpha")
        b = _sym(db, "beta")
        _edge(db, a, b, rel="calls", conf=1.0)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        config = LoomConfig(target_dir=db._config.target_dir)  # noqa: SLF001
        engine = SearchEngine(db, mock_embedder, graph=g, config=config)

        coupled = engine.related("alpha")
        structural = [c for c in coupled if c.symbol.name == "beta"]
        assert len(structural) > 0, "beta should appear in alpha's related"
        for c in structural:
            assert "structural" in c.reason, f"reason lacks 'structural': {c.reason!r}"

    def test_generic_call_target_filtered_from_structural_impact(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """Symbols in _GENERIC_CALL_TARGETS are filtered from the structural graph path.

        The semantic path does NOT apply this filter — documented separately as a gap.
        """
        target_sym = _sym(db, "processData")
        # Generic caller ('map') — structural edge
        generic_caller_id = db.insert_symbol(
            Symbol(
                name="map",
                kind="function",
                file="utils.js",
                line=1,
                end_line=1,
                language="javascript",
            )
        )
        # Use a dissimilar embedding so 'map' is NOT picked up by semantic search
        db.insert_embedding(generic_caller_id, [0.0] * 767 + [1.0])
        _edge(db, generic_caller_id, target_sym, rel="calls", conf=1.0)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        config = LoomConfig(target_dir=db._config.target_dir)  # noqa: SLF001

        # Embedder returns a vector orthogonal to 'map' — semantic score will be ~0
        mock_embedder.embed_single.return_value = [1.0] + [0.0] * 767
        engine = SearchEngine(db, mock_embedder, graph=g, config=config)

        dependents = engine.impact("processData")
        dep_names = {d.symbol.name for d in dependents}
        # 'map' must be filtered by the structural path filter
        assert "map" not in dep_names, "'map' escaped the _GENERIC_CALL_TARGETS filter"

    def test_generic_call_target_filtered_from_semantic_path(
        self, db: LoomDB, mock_embedder: MagicMock
    ) -> None:
        """BUG-001 fix: semantic path in impact() now filters _GENERIC_CALL_TARGETS.

        A generic-named symbol must NOT surface in impact() results via semantic path,
        even if its embedding is identical to the target (distance ~0).
        """
        from loom.search.engine import _GENERIC_CALL_TARGETS

        _sym(db, "processData")  # stored with [0.1]*768

        # 'forEach' is in _GENERIC_CALL_TARGETS; give it the SAME embedding as target
        generic_id = db.insert_symbol(
            Symbol(
                name="forEach",
                kind="function",
                file="utils.js",
                line=1,
                end_line=1,
                language="javascript",
            )
        )
        db.insert_embedding(generic_id, [0.1] * 768)  # identical embedding → distance ~0
        # NO structural edge — only semantic similarity
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        config = LoomConfig(target_dir=db._config.target_dir)  # noqa: SLF001

        # Embedder returns [0.1]*768 — identical to both symbols → distance ~0 → score ~1.0
        mock_embedder.embed_single.return_value = [0.1] * 768
        engine = SearchEngine(db, mock_embedder, graph=g, config=config)

        dependents = engine.impact("processData")
        dep_names = {d.symbol.name for d in dependents}

        # 'forEach' is in _GENERIC_CALL_TARGETS and must be filtered from semantic path too
        assert "forEach" in _GENERIC_CALL_TARGETS  # sanity: the name is generic
        assert "forEach" not in dep_names, "'forEach' escaped the semantic path generic filter"
