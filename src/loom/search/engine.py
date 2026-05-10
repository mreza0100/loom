"""Hybrid search engine — FTS5 + vector search fused via Reciprocal Rank Fusion."""

import logging

from loom.indexer.embedder import Embedder
from loom.store.db import LoomDB
from loom.store.models import CoupledSymbol, SearchResult, Symbol

log = logging.getLogger(__name__)

RRF_K = 60

_GENERIC_CALL_TARGETS = frozenset(
    {
        "map",
        "filter",
        "reduce",
        "forEach",
        "find",
        "some",
        "every",
        "includes",
        "indexOf",
        "flat",
        "flatMap",
        "concat",
        "slice",
        "splice",
        "push",
        "pop",
        "shift",
        "unshift",
        "join",
        "sort",
        "reverse",
        "keys",
        "values",
        "entries",
        "assign",
        "freeze",
        "hasOwnProperty",
        "isArray",
        "from",
        "has",
        "get",
        "set",
        "delete",
        "add",
        "clear",
        "then",
        "catch",
        "finally",
        "call",
        "apply",
        "bind",
        "toString",
        "valueOf",
        "log",
        "warn",
        "error",
        "info",
        "debug",
        "now",
        "parse",
        "stringify",
        "setTimeout",
        "setInterval",
        "clearTimeout",
        "clearInterval",
        "require",
        "callback",
        "next",
        "done",
    },
)

MAX_STRUCTURAL_RESULTS = 30

KIND_BOOST = {"function": 1.5, "class": 1.5, "method": 1.3, "variable": 0.5}


def _rrf_score(rank: int) -> float:
    return 1.0 / (RRF_K + rank)


MAX_BOOST = max(KIND_BOOST.values())
THEORETICAL_MAX_RRF = (1.0 / RRF_K) * MAX_BOOST + (1.0 / RRF_K)


def _normalize_scores(results: list[tuple[int, float]]) -> list[tuple[int, float]]:
    if not results:
        return results
    max_score = max(s for _, s in results)
    if max_score <= 0:
        return results
    divisor = max(max_score, THEORETICAL_MAX_RRF)
    return [(sym_id, min(1.0, score / divisor)) for sym_id, score in results]


class SearchEngine:
    def __init__(self, db: LoomDB, embedder: Embedder) -> None:
        self._db = db
        self._embedder = embedder

    def search(
        self,
        query: str,
        limit: int = 10,
        kind: str | None = None,
    ) -> list[SearchResult]:
        fts_results = self._db.search_fts(query, limit=limit * 3)

        embedding = self._embedder.embed_single(query)
        vec_results = self._db.search_vec(embedding, limit=limit * 3)

        scores: dict[int, float] = {}
        symbol_map: dict[int, Symbol] = {}

        for rank, sym in enumerate(fts_results):
            if sym.id is None:
                log.warning("FTS result has None id, skipping: %s", sym.name)
                continue
            boost = KIND_BOOST.get(sym.kind, 1.0)
            scores[sym.id] = scores.get(sym.id, 0) + _rrf_score(rank) * boost
            symbol_map[sym.id] = sym

        for rank, (sym_id, _distance) in enumerate(vec_results):
            scores[sym_id] = scores.get(sym_id, 0) + _rrf_score(rank)
            if sym_id not in symbol_map:
                vec_sym = self._db.get_symbol_by_id(sym_id)
                if vec_sym:
                    boost = KIND_BOOST.get(vec_sym.kind, 1.0)
                    scores[sym_id] = scores.get(sym_id, 0) + _rrf_score(rank) * (boost - 1.0)
                    symbol_map[sym_id] = vec_sym

        if kind:
            scores = {
                sid: s
                for sid, s in scores.items()
                if symbol_map.get(sid) and symbol_map[sid].kind == kind
            }

        normalized = _normalize_scores(list(scores.items()))
        ranked = sorted(normalized, key=lambda x: x[1], reverse=True)[:limit]

        results: list[SearchResult] = []
        for sym_id, score in ranked:
            result_sym = symbol_map.get(sym_id)
            if not result_sym:
                continue
            coupled = self._find_coupled(result_sym)
            results.append(SearchResult(symbol=result_sym, score=score, coupled=coupled))

        return results

    def related(
        self,
        symbol_name: str,
        file: str | None = None,
        kind: str | None = None,
    ) -> list[CoupledSymbol]:
        symbols = self._db.get_symbol_by_name_fuzzy(symbol_name, file)
        if not symbols:
            return []

        target = symbols[0]
        coupled = self._find_coupled(target)

        if kind:
            coupled = [c for c in coupled if c.symbol.kind == kind]

        return coupled

    def impact(
        self,
        symbol_name: str,
        file: str | None = None,
        kind: str | None = None,
    ) -> list[CoupledSymbol]:
        symbols = self._db.get_symbol_by_name_fuzzy(symbol_name, file)
        if not symbols:
            return []

        target = symbols[0]
        dependents: list[CoupledSymbol] = []
        seen: set[int | None] = {target.id}

        # Resolved incoming edges: use ID-based lookup
        if target.id is not None:
            resolved_incoming = self._db.get_edges_to(target.id)
            for edge in resolved_incoming:
                source_sym = self._db.get_symbol_by_id(edge.source_id)
                if source_sym is None:
                    continue
                # Filter generic source names
                if source_sym.name.split(".")[-1] in _GENERIC_CALL_TARGETS:
                    continue
                if source_sym.id in seen:
                    continue
                seen.add(source_sym.id)
                dependents.append(
                    CoupledSymbol(
                        symbol=source_sym,
                        score=0.8,
                        reason=f"{edge.relationship} (structural)",
                    ),
                )

        # Unresolved incoming edges: find by target_name match
        # These are callers that haven't been Phase-2-resolved yet
        unresolved_by_name = self._db.get_edges_to_by_name(target.name)
        for edge in unresolved_by_name:
            # Skip if resolved to a different symbol (false positive by name)
            if edge.target_id is not None and edge.target_id != target.id:
                continue
            # Skip already-resolved edges (already handled above)
            if edge.target_id == target.id:
                continue
            source_sym = self._db.get_symbol_by_id(edge.source_id)
            if source_sym is None:
                continue
            if source_sym.name.split(".")[-1] in _GENERIC_CALL_TARGETS:
                continue
            if source_sym.id in seen:
                continue
            seen.add(source_sym.id)
            dependents.append(
                CoupledSymbol(
                    symbol=source_sym,
                    score=0.8,
                    reason=f"{edge.relationship} (structural)",
                ),
            )

        if target.id is not None:
            sym_text = self._embedder.build_symbol_text(target.name, target.kind, target.context)
            embedding = self._embedder.embed_single(sym_text)
            vec_hits = self._db.search_vec(embedding, limit=10)
            for sym_id, distance in vec_hits:
                if sym_id in seen:
                    continue
                seen.add(sym_id)
                sym = self._db.get_symbol_by_id(sym_id)
                if sym:
                    sim = max(0.0, 1.0 - distance)
                    if sim > 0.3:
                        dependents.append(
                            CoupledSymbol(
                                symbol=sym,
                                score=sim,
                                reason="semantically similar",
                            ),
                        )

        if kind:
            dependents = [d for d in dependents if d.symbol.kind == kind]

        dependents.sort(key=lambda c: c.score, reverse=True)
        return dependents

    def neighborhood(
        self,
        file: str,
        line: int,
    ) -> tuple[Symbol | None, list[CoupledSymbol]]:
        colocated = self._db.get_colocated_symbols(file)

        anchor: Symbol | None = None
        for sym in colocated:
            if sym.line <= line <= sym.end_line:
                anchor = sym
                break

        if not anchor:
            anchor = min(colocated, key=lambda s: abs(s.line - line), default=None)

        if not anchor:
            return None, [
                CoupledSymbol(symbol=s, score=0.5, reason="co-located") for s in colocated
            ]

        coupled = self._find_coupled(anchor)
        for sym in colocated:
            if sym.id != anchor.id and sym.id not in {c.symbol.id for c in coupled}:
                coupled.append(CoupledSymbol(symbol=sym, score=0.4, reason="co-located"))

        coupled.sort(key=lambda c: c.score, reverse=True)
        return anchor, coupled

    def _find_coupled(self, target: Symbol) -> list[CoupledSymbol]:
        """Find all symbols structurally/semantically coupled to target.

        Uses ID-based edge traversal. Unresolved edges (target_id=None) are skipped
        for outgoing traversal — only resolved edges are followed.
        """
        coupled: list[CoupledSymbol] = []
        seen: set[int | None] = {target.id}

        if target.id is not None:
            # Outgoing edges: only follow resolved ones (target_id is not None)
            outgoing = self._db.get_edges_from(target.id)
            for edge in outgoing:
                # Phase 3 compat: check last segment of dotted expression against filter
                if edge.target_name.split(".")[-1] in _GENERIC_CALL_TARGETS:
                    continue
                if edge.target_id is None:
                    continue  # unresolved — skip
                sym = self._db.get_symbol_by_id(edge.target_id)
                if sym and sym.id not in seen:
                    seen.add(sym.id)
                    coupled.append(
                        CoupledSymbol(
                            symbol=sym,
                            score=0.7,
                            reason=f"{edge.relationship} (structural)",
                        ),
                    )
                if len(coupled) >= MAX_STRUCTURAL_RESULTS:
                    break

            # Incoming edges: source is always resolved (source_id always set)
            incoming = self._db.get_edges_to(target.id)
            for edge in incoming:
                source_sym = self._db.get_symbol_by_id(edge.source_id)
                if source_sym is None:
                    continue
                # Filter generic source names
                if source_sym.name.split(".")[-1] in _GENERIC_CALL_TARGETS:
                    continue
                if source_sym.id not in seen:
                    seen.add(source_sym.id)
                    coupled.append(
                        CoupledSymbol(
                            symbol=source_sym,
                            score=0.6,
                            reason="called_by (structural)",
                        ),
                    )
                if len(coupled) >= MAX_STRUCTURAL_RESULTS:
                    break

        # Semantic coupling via vector search
        if target.id is not None:
            sym_text = self._embedder.build_symbol_text(target.name, target.kind, target.context)
            embedding = self._embedder.embed_single(sym_text)
            vec_hits = self._db.search_vec(embedding, limit=10)
            for sym_id, distance in vec_hits:
                if sym_id in seen:
                    continue
                seen.add(sym_id)
                sym = self._db.get_symbol_by_id(sym_id)
                if sym:
                    sim = max(0.0, 1.0 - distance)
                    if sim > 0.3:
                        coupled.append(
                            CoupledSymbol(
                                symbol=sym,
                                score=sim,
                                reason="semantically similar",
                            ),
                        )

        coupled.sort(key=lambda c: c.score, reverse=True)
        return coupled
