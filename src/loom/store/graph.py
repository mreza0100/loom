"""In-memory NetworkX DiGraph over resolved symbol relationships."""

import logging

import networkx as nx

from loom.store.db import LoomDB

log = logging.getLogger(__name__)


class SymbolGraph:
    """Directed graph of resolved symbol relationships.

    Nodes are integer symbol IDs.  Each edge carries two attributes:
        relationship (str)  — e.g. "calls", "imports", "extends"
        confidence   (float) — resolution confidence from Phase 2

    Duplicate (source, target) pairs with different relationships are
    collapsed to the single edge with the highest confidence value.
    """

    def __init__(self) -> None:
        self._g: nx.DiGraph = nx.DiGraph()

    # ------------------------------------------------------------------
    # Mutation
    # ------------------------------------------------------------------

    def build_from_db(self, db: LoomDB) -> None:
        """Rebuild the entire graph from resolved edges in the database.

        Clears the current graph first, then loads every edge where
        target_id IS NOT NULL.  Duplicate (source, target) pairs are
        resolved by keeping the highest-confidence edge.
        """
        self._g.clear()
        rows = db.conn.execute(
            "SELECT source_id, target_id, relationship, confidence "
            "FROM edges WHERE target_id IS NOT NULL",
        ).fetchall()
        for source_id, target_id, relationship, confidence in rows:
            self.add_edge(source_id, target_id, relationship, confidence)
        log.debug(
            "SymbolGraph built: %d nodes, %d edges",
            self._g.number_of_nodes(),
            self._g.number_of_edges(),
        )

    def add_edge(
        self,
        source_id: int,
        target_id: int,
        relationship: str,
        confidence: float,
    ) -> None:
        """Add or update a directed edge.

        If an edge (source_id → target_id) already exists and the new
        confidence is higher, the edge attributes are replaced.  If the
        existing edge has equal or higher confidence, this call is a no-op.
        """
        if self._g.has_edge(source_id, target_id):
            existing_conf: float = self._g[source_id][target_id].get("confidence", 0.0)
            if confidence <= existing_conf:
                return
        self._g.add_edge(source_id, target_id, relationship=relationship, confidence=confidence)

    def remove_node(self, symbol_id: int) -> None:
        """Remove a node and all its incident edges.  No-op if absent."""
        if self._g.has_node(symbol_id):
            self._g.remove_node(symbol_id)

    # ------------------------------------------------------------------
    # Traversal
    # ------------------------------------------------------------------

    def dependents(
        self,
        symbol_id: int,
        max_depth: int = 3,
    ) -> list[tuple[int, int, str, float]]:
        """Return nodes that (transitively) depend ON symbol_id.

        Traverses the REVERSE graph (incoming edges) up to max_depth hops.
        Returns list of (sym_id, depth, relationship, confidence).
        The queried symbol_id itself is never included.
        """
        if not self._g.has_node(symbol_id):
            return []
        return self._bfs(self._g.reverse(copy=False), symbol_id, max_depth)

    def dependencies(
        self,
        symbol_id: int,
        max_depth: int = 3,
    ) -> list[tuple[int, int, str, float]]:
        """Return nodes that symbol_id (transitively) depends ON.

        Traverses the FORWARD graph (outgoing edges) up to max_depth hops.
        Returns list of (sym_id, depth, relationship, confidence).
        """
        if not self._g.has_node(symbol_id):
            return []
        return self._bfs(self._g, symbol_id, max_depth)

    def shortest_path(self, source_id: int, target_id: int) -> list[int] | None:
        """Return the shortest directed path between two nodes, or None."""
        try:
            path: list[int] = nx.shortest_path(self._g, source_id, target_id)
            return path
        except (nx.NetworkXNoPath, nx.NodeNotFound):
            return None

    def impact_radius(
        self,
        symbol_id: int,
        max_depth: int = 3,
    ) -> list[tuple[int, float]]:
        """Blast-radius scores for nodes that depend on symbol_id.

        Score formula: confidence × (1 / 2^(depth−1))
          depth 1, confidence 1.0 → 1.0
          depth 2, confidence 1.0 → 0.5
          depth 3, confidence 0.8 → 0.2

        Returns list of (sym_id, score) sorted by score descending.
        """
        entries = self.dependents(symbol_id, max_depth=max_depth)
        scored: list[tuple[int, float]] = []
        for sym_id, depth, _relationship, confidence in entries:
            decay = 1.0 / (2 ** (depth - 1))
            score = confidence * decay
            scored.append((sym_id, score))
        scored.sort(key=lambda x: x[1], reverse=True)
        return scored

    def centrality(self, top_n: int = 20) -> list[tuple[int, float]]:
        """Return top-N symbols by in-degree centrality (descending score).

        Uses in-degree as a lightweight proxy for PageRank to avoid the scipy
        dependency that networkx.pagerank requires in recent releases.
        In-degree correlates strongly with PageRank for typical code graphs
        (high in-degree = heavily depended-on = high centrality).
        """
        if self._g.number_of_nodes() == 0:
            return []
        g_no_selfloops = self._g.copy()
        g_no_selfloops.remove_edges_from(nx.selfloop_edges(g_no_selfloops))
        degree_centrality: dict[int, float] = dict(nx.in_degree_centrality(g_no_selfloops))
        sorted_ranks = sorted(degree_centrality.items(), key=lambda x: x[1], reverse=True)
        return sorted_ranks[:top_n]

    def neighbors_with_metadata(
        self,
        symbol_id: int,
        max_depth: int = 2,
    ) -> list[tuple[int, int, str, float]]:
        """Merge dependents and dependencies, deduplicated by sym_id.

        Returns list of (sym_id, depth, relationship, confidence).
        When a node appears in both directions, the entry with higher
        confidence is kept (dependents win on tie).
        """
        deps_in = self.dependents(symbol_id, max_depth=max_depth)
        deps_out = self.dependencies(symbol_id, max_depth=max_depth)

        # Deduplicate: keep highest-confidence entry per sym_id
        best: dict[int, tuple[int, int, str, float]] = {}
        for entry in deps_in:
            sym_id, depth, relationship, confidence = entry
            if sym_id not in best or confidence > best[sym_id][3]:
                best[sym_id] = entry
        for entry in deps_out:
            sym_id, depth, relationship, confidence = entry
            if sym_id not in best or confidence > best[sym_id][3]:
                best[sym_id] = entry

        return list(best.values())

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _bfs(
        self,
        graph: nx.DiGraph,
        source: int,
        max_depth: int,
    ) -> list[tuple[int, int, str, float]]:
        """BFS traversal using nx.bfs_edges with manual depth tracking.

        Returns list of (sym_id, depth, relationship, confidence).
        The source node itself is excluded from results.
        """
        results: list[tuple[int, int, str, float]] = []
        depths: dict[int, int] = {source: 0}

        for u, v in nx.bfs_edges(graph, source, depth_limit=max_depth):
            depth = depths[u] + 1
            depths[v] = depth
            edge_data = graph[u][v]
            relationship: str = edge_data.get("relationship", "unknown")
            confidence: float = edge_data.get("confidence", 0.0)
            results.append((v, depth, relationship, confidence))

        return results
