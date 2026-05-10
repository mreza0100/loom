"""Data models for Loom storage."""

from dataclasses import dataclass, field
from typing import NamedTuple


@dataclass
class Symbol:
    name: str
    kind: str
    file: str
    line: int
    end_line: int
    language: str
    context: str = ""
    id: int | None = None


class ParsedEdge(NamedTuple):
    """Intermediate edge produced by the parser before source_id resolution."""

    source_name: str
    target_name: str
    relationship: str
    target_file: str | None = None


@dataclass
class Edge:
    source_id: int
    target_name: str  # preserved always — diagnostic + unresolved fallback
    relationship: str
    confidence: float = 0.0
    target_id: int | None = None
    # resolution hint from parser; not authoritative after Phase 2 resolution
    target_file: str | None = None
    id: int | None = None  # DB row id; needed by Phase 2 update_edge_target
    # For aliased imports: original exported name in the target module.
    # target_name stores the local binding (import map key); original_name stores
    # what the target module actually exports so Strategy 2 can resolve it.
    original_name: str | None = None


@dataclass
class FileState:
    path: str
    content_hash: str
    last_indexed: str


@dataclass
class CoupledSymbol:
    symbol: Symbol
    score: float
    reason: str


@dataclass
class SearchResult:
    symbol: Symbol
    score: float
    coupled: list[CoupledSymbol] = field(default_factory=list)
