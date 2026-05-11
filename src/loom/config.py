"""Loom configuration."""

from dataclasses import dataclass, field
from pathlib import Path

from loom.indexer.adapters import REGISTRY

_ALWAYS_EXCLUDED_CONFIG: frozenset[str] = frozenset({".git", "__pycache__", ".loom"})


@dataclass(frozen=True)
class LoomConfig:
    target_dir: Path
    db_path: Path = Path(".loom/loom.db")
    watch_extensions: frozenset[str] = field(
        default_factory=lambda: REGISTRY.get_all_extensions(),
    )
    debounce_seconds: float = 2.0
    embedding_model: str = "jinaai/jina-embeddings-v2-base-code"
    embedding_dimensions: int = 768
    max_file_size_bytes: int = 512_000
    excluded_dirs: frozenset[str] = field(
        default_factory=lambda: REGISTRY.get_all_excluded_dirs() | _ALWAYS_EXCLUDED_CONFIG,
    )
    # Coupling signal weights — must sum to 1.0 when all signals active
    structural_weight: float = 0.45
    semantic_weight: float = 0.35
    evolutionary_weight: float = 0.20
    # Git co-change analysis (evolutionary coupling)
    enable_git_analysis: bool = True
    git_max_commits: int = 500
    git_max_files_per_commit: int = 20

    def resolve_db_path(self) -> Path:
        resolved = self.target_dir / self.db_path
        resolved.parent.mkdir(parents=True, exist_ok=True)
        return resolved
