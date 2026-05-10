"""Loom MCP server — contextual code co-occurrence engine."""

import logging
from pathlib import Path
from typing import Any

from fastmcp import FastMCP

from loom.config import LoomConfig
from loom.indexer.embedder import Embedder
from loom.indexer.pipeline import IndexPipeline
from loom.indexer.watcher import start_watcher
from loom.search.engine import SearchEngine
from loom.store.db import LoomDB
from loom.store.models import CoupledSymbol, SearchResult, Symbol

log = logging.getLogger(__name__)

mcp = FastMCP("loom")

_config: LoomConfig | None = None
_db: LoomDB | None = None
_embedder: Embedder | None = None
_pipeline: IndexPipeline | None = None
_engine: SearchEngine | None = None


def _format_symbol(sym: Symbol) -> dict[str, Any]:
    return {
        "name": sym.name,
        "kind": sym.kind,
        "file": sym.file,
        "line": sym.line,
        "end_line": sym.end_line,
        "language": sym.language,
    }


def _format_coupled(coupled: list[CoupledSymbol]) -> list[dict[str, Any]]:
    return [
        {
            "symbol": _format_symbol(c.symbol),
            "score": round(c.score, 3),
            "reason": c.reason,
        }
        for c in coupled
    ]


def _format_results(results: list[SearchResult]) -> list[dict[str, Any]]:
    return [
        {
            "symbol": _format_symbol(r.symbol),
            "score": round(r.score, 3),
            "coupled": _format_coupled(r.coupled),
        }
        for r in results
    ]


def initialize(target_dir: Path) -> None:
    global _config, _db, _embedder, _pipeline, _engine

    _config = LoomConfig(target_dir=target_dir.resolve())
    _db = LoomDB(_config)
    _db.connect()
    _embedder = Embedder(_config)
    _pipeline = IndexPipeline(_config, _db, _embedder)
    _engine = SearchEngine(_db, _embedder)

    log.info("Loom initialized for %s", target_dir)

    result = _pipeline.full_index()
    log.info("Initial index: %s", result)

    def on_changes(changed: list[Path]) -> None:
        if _pipeline is None:
            log.error("on_changes called but pipeline is None — skipping incremental reindex")
            return
        inc_result = _pipeline.incremental_index(changed)
        log.info("Incremental reindex: %s", inc_result)

    start_watcher(
        target_dir,
        on_changes,
        debounce_sec=_config.debounce_seconds,
        extensions=_config.watch_extensions,
    )
    log.info("File watcher active")


@mcp.tool()
def search(query: str, limit: int = 10, kind: str | None = None) -> list[dict[str, Any]]:
    """Hybrid search: keyword + semantic, fused via Reciprocal Rank Fusion.

    Each result is expanded with its top coupled symbols — you search for A,
    you get A plus its neighborhood.

    Args:
        query: Search query (symbol name, keyword, or natural language).
        limit: Max results to return.
        kind: Filter by symbol kind — "function", "class", "method", or "variable".
    """
    if _engine is None:
        return [{"error": "Loom not initialized"}]
    results = _engine.search(query, limit=limit, kind=kind)
    return _format_results(results)


@mcp.tool()
def related(symbol: str, file: str | None = None, kind: str | None = None) -> list[dict[str, Any]]:
    """Find all symbols coupled to the given symbol.

    Returns structural (call graph, imports), semantic (embedding similarity),
    and co-location relationships with scores.

    Args:
        symbol: Symbol name to find relationships for.
        file: Optional file path to disambiguate symbols with the same name.
        kind: Filter results by symbol kind — "function", "class", "method", or "variable".
    """
    if _engine is None:
        return [{"error": "Loom not initialized"}]
    coupled = _engine.related(symbol, file=file, kind=kind)
    return _format_coupled(coupled)


@mcp.tool()
def impact(symbol: str, file: str | None = None, kind: str | None = None) -> list[dict[str, Any]]:
    """Blast radius analysis — what breaks if this symbol changes?

    Combines structural dependents (who calls this?) with semantic neighbors
    (what looks like this?). Results are deduplicated by symbol.

    Args:
        symbol: Symbol name to analyze impact for.
        file: Optional file path to disambiguate.
        kind: Filter results by symbol kind.
    """
    if _engine is None:
        return [{"error": "Loom not initialized"}]
    dependents = _engine.impact(symbol, file=file, kind=kind)
    return _format_coupled(dependents)


@mcp.tool()
def neighborhood(file: str, line: int) -> dict[str, Any]:
    """The coupling neighborhood of a code location.

    Given a file and line number, find the symbol at that position and return
    the anchor symbol plus everything related to it.
    """
    if _engine is None or _db is None:
        return {"error": "Loom not initialized"}
    colocated = _db.get_colocated_symbols(file)
    if not colocated:
        return {"error": f"No symbols found in '{file}'. File may not exist or is not indexed."}
    anchor, coupled = _engine.neighborhood(file, line)
    return {
        "anchor": _format_symbol(anchor) if anchor else None,
        "coupled": _format_coupled(coupled),
    }


@mcp.tool()
def reindex() -> dict[str, Any]:
    """Force a full reindex of the target directory.

    Use this after creating new files or when the watcher might have missed changes.
    """
    if _pipeline is None or _config is None:
        return {"error": "Loom not initialized"}
    result = _pipeline.full_index()
    return {"status": "reindex complete", **result}


@mcp.tool()
def status() -> dict[str, Any]:
    """Index health: total files, symbols, edges, vectors, timestamps, and target directory."""
    if _db is None or _config is None:
        return {"error": "Loom not initialized"}
    stats = _db.get_stats()
    return {
        "target": str(_config.target_dir),
        "db_path": str(_config.resolve_db_path()),
        **stats,
    }
