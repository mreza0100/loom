"""Tests for loom.store.graph — SymbolGraph wrapping NetworkX DiGraph."""

import pytest

from loom.store.db import LoomDB
from loom.store.graph import SymbolGraph
from loom.store.models import Edge, Symbol


def _insert_sym(db: LoomDB, name: str, file: str = "app.js") -> int:
    sym_id = db.insert_symbol(
        Symbol(name=name, kind="function", file=file, line=1, end_line=10, language="javascript")
    )
    db.insert_embedding(sym_id, [0.1] * 768)
    return sym_id


def _insert_resolved_edge(
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


class TestBuildFromDB:
    def test_graph_build_from_resolved_edges(self, db: LoomDB) -> None:
        ids = [_insert_sym(db, f"sym{i}") for i in range(5)]
        # 5 resolved edges: 0→1, 1→2, 2→3, 3→4, 4→0
        pairs = [(0, 1), (1, 2), (2, 3), (3, 4), (4, 0)]
        for src, tgt in pairs:
            _insert_resolved_edge(db, ids[src], ids[tgt])
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        assert g._g.number_of_nodes() == 5
        assert g._g.number_of_edges() == 5

    def test_graph_ignores_unresolved_edges(self, db: LoomDB) -> None:
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        _insert_resolved_edge(db, a, b)
        # Unresolved edge (target_id=None)
        db.insert_edge(
            Edge(
                source_id=a,
                target_name="Ghost",
                target_id=None,
                relationship="calls",
                confidence=0.0,
            )
        )
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        # Only the resolved edge (a→b) should appear
        assert g._g.number_of_edges() == 1
        assert g._g.has_edge(a, b)

    def test_build_clears_existing_graph(self, db: LoomDB) -> None:
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        _insert_resolved_edge(db, a, b)
        db.commit()

        g = SymbolGraph()
        # First build
        g.build_from_db(db)
        assert g._g.number_of_nodes() == 2

        # Second build should replace, not accumulate
        g.build_from_db(db)
        assert g._g.number_of_nodes() == 2
        assert g._g.number_of_edges() == 1

    def test_duplicate_edges_keep_highest_confidence(self, db: LoomDB) -> None:
        """When A→B appears twice (calls + imports), keep highest confidence."""
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        db.insert_edge(
            Edge(source_id=a, target_name="B", target_id=b, relationship="calls", confidence=0.6)
        )
        db.insert_edge(
            Edge(source_id=a, target_name="B", target_id=b, relationship="imports", confidence=0.95)
        )
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        # DiGraph: only one edge per (a, b) pair
        assert g._g.number_of_edges() == 1
        assert g._g[a][b]["confidence"] == pytest.approx(0.95)


class TestTransitiveTraversal:
    def test_transitive_dependents(self, db: LoomDB) -> None:
        """A→B→C: dependents(C) = [B@depth1, A@depth2]."""
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        c = _insert_sym(db, "C")
        _insert_resolved_edge(db, a, b)
        _insert_resolved_edge(db, b, c)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        result = g.dependents(c)

        sym_ids = {entry[0] for entry in result}
        assert b in sym_ids
        assert a in sym_ids

        by_id = {entry[0]: entry for entry in result}
        assert by_id[b][1] == 1  # depth
        assert by_id[a][1] == 2

    def test_transitive_dependencies(self, db: LoomDB) -> None:
        """A→B→C: dependencies(A) = [B@depth1, C@depth2]."""
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        c = _insert_sym(db, "C")
        _insert_resolved_edge(db, a, b)
        _insert_resolved_edge(db, b, c)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        result = g.dependencies(a)

        by_id = {entry[0]: entry for entry in result}
        assert b in by_id
        assert c in by_id
        assert by_id[b][1] == 1
        assert by_id[c][1] == 2

    def test_dependents_max_depth(self, db: LoomDB) -> None:
        """A→B→C: dependents(C, max_depth=1) returns only B, not A."""
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        c = _insert_sym(db, "C")
        _insert_resolved_edge(db, a, b)
        _insert_resolved_edge(db, b, c)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        result = g.dependents(c, max_depth=1)

        sym_ids = {entry[0] for entry in result}
        assert b in sym_ids
        assert a not in sym_ids

    def test_dependents_excludes_self(self, db: LoomDB) -> None:
        """The queried symbol should never appear in its own dependents list."""
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        _insert_resolved_edge(db, a, b)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        result_a = g.dependents(a)
        assert a not in {e[0] for e in result_a}

        result_b = g.dependents(b)
        assert b not in {e[0] for e in result_b}


class TestShortestPath:
    def test_shortest_path(self, db: LoomDB) -> None:
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        c = _insert_sym(db, "C")
        _insert_resolved_edge(db, a, b)
        _insert_resolved_edge(db, b, c)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        path = g.shortest_path(a, c)
        assert path == [a, b, c]

    def test_shortest_path_no_path(self, db: LoomDB) -> None:
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        db.commit()  # No edge inserted

        g = SymbolGraph()
        g.build_from_db(db)

        assert g.shortest_path(a, b) is None


class TestImpactRadius:
    def test_impact_radius_decay(self, db: LoomDB) -> None:
        """depth-1 node scores conf×1.0; depth-2 node scores conf×0.5."""
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        c = _insert_sym(db, "C")
        _insert_resolved_edge(db, a, b, confidence=1.0)
        _insert_resolved_edge(db, b, c, confidence=1.0)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        # impact_radius(c) → a@depth2 scores 0.5, b@depth1 scores 1.0
        hits = g.impact_radius(c, max_depth=2)

        by_id = dict(hits)
        assert by_id[b] == pytest.approx(1.0)  # depth 1, conf 1.0
        assert by_id[a] == pytest.approx(0.5)  # depth 2, conf 1.0

    def test_impact_radius_sorted_descending(self, db: LoomDB) -> None:
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        c = _insert_sym(db, "C")
        _insert_resolved_edge(db, a, b, confidence=1.0)
        _insert_resolved_edge(db, b, c, confidence=1.0)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        hits = g.impact_radius(c, max_depth=2)
        scores = [s for _, s in hits]
        assert scores == sorted(scores, reverse=True)

    def test_impact_radius_uses_confidence(self, db: LoomDB) -> None:
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        _insert_resolved_edge(db, a, b, confidence=0.8)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        hits = g.impact_radius(b, max_depth=1)

        by_id = dict(hits)
        assert by_id[a] == pytest.approx(0.8)  # depth 1, conf 0.8


class TestCentrality:
    def test_centrality_ranking(self, db: LoomDB) -> None:
        """Hub node (many inbound edges) should rank higher than leaf."""
        hub = _insert_sym(db, "hub")
        leaf1 = _insert_sym(db, "leaf1")
        leaf2 = _insert_sym(db, "leaf2")
        leaf3 = _insert_sym(db, "leaf3")
        _insert_resolved_edge(db, leaf1, hub)
        _insert_resolved_edge(db, leaf2, hub)
        _insert_resolved_edge(db, leaf3, hub)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        ranks = g.centrality(top_n=10)

        sym_ids = [r[0] for r in ranks]
        # hub should appear and be ranked higher (lower index) than leaves
        assert hub in sym_ids
        hub_idx = sym_ids.index(hub)
        for leaf in (leaf1, leaf2, leaf3):
            if leaf in sym_ids:
                assert hub_idx < sym_ids.index(leaf)

    def test_centrality_top_n_respected(self, db: LoomDB) -> None:
        for i in range(10):
            _insert_sym(db, f"sym{i}")
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)
        ranks = g.centrality(top_n=3)
        assert len(ranks) <= 3


class TestNeighborsWithMetadata:
    def test_neighbors_with_metadata_bidirectional(self, db: LoomDB) -> None:
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        c = _insert_sym(db, "C")
        _insert_resolved_edge(db, a, b)  # A→B (A depends on B)
        _insert_resolved_edge(db, c, a)  # C→A (C depends on A)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        # neighbors_with_metadata(a) should include both B (dependency) and C (dependent)
        result = g.neighbors_with_metadata(a, max_depth=1)
        sym_ids = {entry[0] for entry in result}
        assert b in sym_ids
        assert c in sym_ids

    def test_neighbors_with_metadata_deduplication(self, db: LoomDB) -> None:
        """A node reachable from both directions appears only once."""
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        # A→B and B→A creates mutual dependency
        _insert_resolved_edge(db, a, b, confidence=0.9)
        _insert_resolved_edge(db, b, a, confidence=0.7)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        result = g.neighbors_with_metadata(a, max_depth=1)
        # B should appear exactly once
        b_entries = [e for e in result if e[0] == b]
        assert len(b_entries) == 1
        # Keep higher confidence
        assert b_entries[0][3] == pytest.approx(0.9)


class TestIncrementalMutation:
    def test_incremental_add_edge(self) -> None:
        g = SymbolGraph()
        g.add_edge(1, 2, "calls", 1.0)

        assert g._g.has_node(1)
        assert g._g.has_node(2)
        assert g._g.has_edge(1, 2)
        assert g._g[1][2]["confidence"] == pytest.approx(1.0)

    def test_incremental_remove_node(self) -> None:
        g = SymbolGraph()
        g.add_edge(1, 2, "calls", 1.0)
        g.remove_node(1)

        assert not g._g.has_node(1)
        assert not g._g.has_edge(1, 2)

    def test_incremental_remove_nonexistent_node(self) -> None:
        """remove_node on absent node must not raise."""
        g = SymbolGraph()
        g.remove_node(999)  # Should not raise

    def test_add_edge_updates_to_higher_confidence(self) -> None:
        g = SymbolGraph()
        g.add_edge(1, 2, "calls", 0.5)
        g.add_edge(1, 2, "imports", 0.9)

        assert g._g[1][2]["confidence"] == pytest.approx(0.9)
        assert g._g[1][2]["relationship"] == "imports"

    def test_add_edge_ignores_lower_confidence(self) -> None:
        g = SymbolGraph()
        g.add_edge(1, 2, "calls", 0.9)
        g.add_edge(1, 2, "imports", 0.5)

        assert g._g[1][2]["confidence"] == pytest.approx(0.9)
        assert g._g[1][2]["relationship"] == "calls"


class TestEdgeCases:
    def test_empty_graph_no_crash(self) -> None:
        g = SymbolGraph()
        assert g.dependents(1) == []
        assert g.dependencies(1) == []
        assert g.shortest_path(1, 2) is None
        assert g.impact_radius(1) == []
        assert g.centrality() == []
        assert g.neighbors_with_metadata(1) == []

    def test_self_loop_handling(self, db: LoomDB) -> None:
        """A→A self-loop must not cause infinite traversal or include A in its own dependents."""
        a = _insert_sym(db, "A")
        _insert_resolved_edge(db, a, a)  # self-loop
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        # dependents(A) must not include A itself and must terminate
        result = g.dependents(a)
        assert a not in {e[0] for e in result}

        # dependencies(A) same
        result = g.dependencies(a)
        assert a not in {e[0] for e in result}

    def test_node_with_no_connections(self, db: LoomDB) -> None:
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        _insert_resolved_edge(db, a, b)
        # C has no edges
        c = _insert_sym(db, "C")
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        # C is not in the graph (no edges involving C)
        assert g.dependents(c) == []
        assert g.dependencies(c) == []

    def test_relationship_metadata_preserved(self, db: LoomDB) -> None:
        a = _insert_sym(db, "A")
        b = _insert_sym(db, "B")
        _insert_resolved_edge(db, a, b, relationship="extends", confidence=0.75)
        db.commit()

        g = SymbolGraph()
        g.build_from_db(db)

        # dependencies(a) should carry relationship and confidence from edge
        result = g.dependencies(a)
        assert len(result) == 1
        sym_id, depth, relationship, confidence = result[0]
        assert sym_id == b
        assert depth == 1
        assert relationship == "extends"
        assert confidence == pytest.approx(0.75)
